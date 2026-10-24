import { describe, expect, it } from 'vitest';

import {
  COMMAND_NAMES,
  IPC_ERROR_CODES,
  IPC_VERSION,
  IpcCallError,
  createIpcClient,
  isIpcError,
  type BuildEvent,
  type IpcClient,
  type IpcTransport,
  type RunEvent,
} from './index';

const HANDLE = 'ph_0123456789abcdef0123456789abcdef';
const BUILD_ID = 'bd_0123456789abcdef0123456789abcdef';

interface Invocation {
  cmd: string;
  args: Record<string, unknown>;
}

/** A transport that records every call and channel and answers with a fixed result. */
class FakeTransport implements IpcTransport {
  readonly invocations: Invocation[] = [];
  readonly channels: ((message: unknown) => void)[] = [];
  result: unknown = {};
  rejection: { reason: unknown } | undefined;

  invoke(cmd: string, args: Record<string, unknown>): Promise<unknown> {
    this.invocations.push({ cmd, args });
    if (this.rejection !== undefined) {
      // Tauri rejects with the command's serialised error, which is a plain object.
      // eslint-disable-next-line @typescript-eslint/prefer-promise-reject-errors
      return Promise.reject(this.rejection.reason);
    }
    return Promise.resolve(this.result);
  }

  channel(onMessage: (message: unknown) => void): unknown {
    this.channels.push(onMessage);
    return `__CHANNEL__:${String(this.channels.length)}`;
  }

  /** Delivers a message on the `index`-th channel created so far. */
  deliver(index: number, message: unknown): void {
    const channel = this.channels[index];
    if (channel === undefined) {
      throw new Error(`no channel ${String(index)}`);
    }
    channel(message);
  }
}

function camelCase(name: string): string {
  return name.replace(/_([a-z])/g, (_match, letter: string) => letter.toUpperCase());
}

type AnyMethod = (...args: unknown[]) => Promise<unknown>;

function method(client: IpcClient, command: string): AnyMethod {
  const value = (client as unknown as Record<string, unknown>)[camelCase(command)];
  if (typeof value !== 'function') {
    throw new Error(`no client method for ${command}`);
  }
  return value as AnyMethod;
}

describe('the command table', () => {
  it('carries the IPC version and every command once', () => {
    expect(IPC_VERSION).toBe(1);
    expect(COMMAND_NAMES).toHaveLength(36);
    expect(new Set(COMMAND_NAMES).size).toBe(COMMAND_NAMES.length);
    expect(COMMAND_NAMES).toContain('build_start');
    expect(COMMAND_NAMES).not.toContain('app_version');
  });

  it('has a client method for every command, which invokes that command', async () => {
    const transport = new FakeTransport();
    const client = createIpcClient(transport);
    for (const command of COMMAND_NAMES) {
      await method(client, command)(
        { some: 'request' },
        () => undefined,
        () => undefined,
      );
    }
    expect(transport.invocations.map((i) => i.cmd)).toEqual([...COMMAND_NAMES]);
    for (const { args } of transport.invocations) {
      for (const key of Object.keys(args)) {
        expect(['request', 'onEvent', 'onOutput']).toContain(key);
      }
    }
  });
});

describe('argument shapes', () => {
  it('sends {} for commands without arguments', async () => {
    const transport = new FakeTransport();
    transport.result = {
      appVersion: '0.1.0',
      ipcVersion: 1,
      platform: 'linux',
      catalogVersion: '1.0.0',
    };
    const info = await createIpcClient(transport).appInfo();
    expect(info.ipcVersion).toBe(1);
    expect(transport.invocations).toEqual([{ cmd: 'app_info', args: {} }]);
    expect(transport.channels).toHaveLength(0);
  });

  it('sends the request under the key `request`', async () => {
    const transport = new FakeTransport();
    const client = createIpcClient(transport);
    await client.projectSave({ handle: HANDLE, document: '{}' });
    await client.settingsUpdate({ codeStyle: { indentWidth: 2 } });
    expect(transport.invocations).toEqual([
      { cmd: 'project_save', args: { request: { handle: HANDLE, document: '{}' } } },
      { cmd: 'settings_update', args: { request: { codeStyle: { indentWidth: 2 } } } },
    ]);
  });

  it('creates one channel per channel argument', async () => {
    const transport = new FakeTransport();
    const client = createIpcClient(transport);
    await client.buildStart({ handle: HANDLE, document: '{}', config: 'debug' }, () => undefined);
    await client.runStart(
      { buildId: BUILD_ID, runOptions: { cols: 80, rows: 24 } },
      () => undefined,
      () => undefined,
    );
    expect(transport.invocations).toEqual([
      {
        cmd: 'build_start',
        args: {
          request: { handle: HANDLE, document: '{}', config: 'debug' },
          onEvent: '__CHANNEL__:1',
        },
      },
      {
        cmd: 'run_start',
        args: {
          request: { buildId: BUILD_ID, runOptions: { cols: 80, rows: 24 } },
          onOutput: '__CHANNEL__:2',
          onEvent: '__CHANNEL__:3',
        },
      },
    ]);
  });
});

describe('channels', () => {
  it('passes JSON events through', async () => {
    const transport = new FakeTransport();
    const events: BuildEvent[] = [];
    await createIpcClient(transport).buildStart(
      { handle: HANDLE, document: '{}', config: 'release' },
      (event) => events.push(event),
    );
    const finished = { kind: 'finished', outcome: 'built', projectHash: null, elapsedMs: 5 };
    transport.deliver(0, { kind: 'progress', stage: 'compile', done: 1, total: 2 });
    transport.deliver(0, finished);
    expect(events).toEqual([{ kind: 'progress', stage: 'compile', done: 1, total: 2 }, finished]);
  });

  it('turns raw output into ArrayBuffers and ignores anything else', async () => {
    const transport = new FakeTransport();
    const output: ArrayBuffer[] = [];
    const events: RunEvent[] = [];
    await createIpcClient(transport).runStart(
      { buildId: BUILD_ID, runOptions: { cols: 80, rows: 24 } },
      (bytes) => output.push(bytes),
      (event) => events.push(event),
    );
    const buffer = new Uint8Array([104, 105]).buffer;
    transport.deliver(0, buffer);
    const view = new Uint8Array([0, 1, 2, 3, 4]).subarray(1, 3);
    transport.deliver(0, view);
    transport.deliver(0, 'not bytes');
    transport.deliver(0, [1, 2]);
    expect(output).toHaveLength(2);
    expect(output[0]).toBe(buffer);
    expect([...new Uint8Array(output[1] ?? new ArrayBuffer(0))]).toEqual([1, 2]);
    transport.deliver(1, { kind: 'started', containment: 'cgroup', mode: 'pty', ideHelpers: true });
    expect(events).toEqual([
      { kind: 'started', containment: 'cgroup', mode: 'pty', ideHelpers: true },
    ]);
  });
});

describe('errors', () => {
  async function failure(reason: unknown): Promise<IpcCallError> {
    const transport = new FakeTransport();
    transport.rejection = { reason };
    const promise = createIpcClient(transport).projectClose({ handle: HANDLE });
    await expect(promise).rejects.toBeInstanceOf(IpcCallError);
    return promise.then(
      () => {
        throw new Error('expected a rejection');
      },
      (error: unknown) => error as IpcCallError,
    );
  }

  it('wraps a typed error from the backend', async () => {
    const error = await failure({ code: 'unknownHandle' });
    expect(error.command).toBe('project_close');
    expect(error.error).toEqual({ code: 'unknownHandle' });
    expect(error.name).toBe('IpcCallError');
    expect(error.message).toBe('IPC command project_close failed: unknownHandle');
    expect(error).toBeInstanceOf(Error);
  });

  it('keeps the arguments of a typed error', async () => {
    const invalid = { code: 'invalidRequest', reason: 'badId', field: 'handle' };
    expect((await failure(invalid)).error).toEqual(invalid);
  });

  it('reports anything else as a transport failure', async () => {
    expect((await failure('command project_close not found')).error).toEqual({
      code: 'transport',
      message: 'command project_close not found',
    });
    expect((await failure(new Error('boom'))).error).toEqual({
      code: 'transport',
      message: 'boom',
    });
    for (const reason of [{ code: 'somethingElse' }, { code: 7 }, null, 42, undefined]) {
      expect((await failure(reason)).error).toEqual({
        code: 'transport',
        message: 'unknown transport failure',
      });
    }
  });

  it('recognises every IpcError code and nothing else', () => {
    for (const code of IPC_ERROR_CODES) {
      expect(isIpcError({ code })).toBe(true);
    }
    expect(IPC_ERROR_CODES).toHaveLength(25);
    for (const value of [null, undefined, 'internal', { code: 'transport' }, {}, { reason: 'x' }]) {
      expect(isIpcError(value)).toBe(false);
    }
  });
});
