// The preview worker's client and host (src/worker/client.ts, src/worker/host.ts) without a build:
// the client against a scripted worker whose replies each test writes, and the host behind one end
// of Node's MessageChannel (real structured cloning, the `Module` included) with a fake core.
// test/worker.core.test.ts runs the two together with the real core.

import { MessageChannel, type MessagePort } from 'node:worker_threads';

import { afterEach, describe, expect, it, vi } from 'vitest';

import type { CoreVersion } from '../src/api';
import { createCore, type CoreHandle } from '../src/core';
import { CoreError, CoreTrap } from '../src/errors';
import {
  createCoreWorkerClient,
  type CoreWorkerClient,
  type CoreWorkerClientOptions,
  serveCoreWorker,
  type WorkerLike,
  type WorkerReply,
  type WorkerRequest,
} from '../src/worker';

/** `\0asm` version 1: the smallest valid module. */
const MODULE = new WebAssembly.Module(new Uint8Array([0, 0x61, 0x73, 0x6d, 1, 0, 0, 0]));

const VERSION: CoreVersion = {
  app: '0.1.0',
  catalog: '1.3.0',
  formatVersion: 2,
  sourceMapVersion: 1,
};
const VERSION_JSON = JSON.stringify(VERSION);

/** A minimal update answer for `text`. */
function answerFor(text: string): string {
  return JSON.stringify({ stage: 'generate', documentText: text });
}

/** A worker whose replies the test writes. */
class ScriptedWorker implements WorkerLike {
  onmessage: ((event: { readonly data: unknown }) => void) | null = null;
  onerror: ((event: unknown) => void) | null = null;
  onmessageerror: ((event: unknown) => void) | null = null;
  readonly sent: WorkerRequest[] = [];
  terminated = false;
  /** Throws `DataCloneError` for a `start`, as a webview that cannot post a `Module` does. */
  refuseModule = false;

  postMessage(message: WorkerRequest): void {
    if (this.refuseModule && message.kind === 'start') {
      throw new DOMException('could not be cloned', 'DataCloneError');
    }
    this.sent.push(message);
  }

  terminate(): void {
    this.terminated = true;
  }

  reply(data: unknown): void {
    this.onmessage?.({ data });
  }

  started(): void {
    this.reply({ p: 1, kind: 'started', versionJson: VERSION_JSON });
  }

  /** The last request of `kind`. */
  last(kind: WorkerRequest['kind']): WorkerRequest {
    const found = this.sent.filter((request) => request.kind === kind).at(-1);
    if (found === undefined) {
      throw new Error(`no ${kind} request`);
    }
    return found;
  }

  /** Answers the update in flight. */
  answer(updateJson?: string): void {
    const request = this.last('update');
    if (request.kind !== 'update') {
      throw new Error('not an update');
    }
    this.reply({
      p: 1,
      kind: 'updated',
      id: request.id,
      updateJson: updateJson ?? answerFor(request.documentJson),
      scopeJson: null,
      startedAt: 1_500,
      updateMs: 12,
      indexMs: 0,
    });
  }
}

/** A client over scripted workers; `workers` lists every one it spawned. */
function scripted(options: Partial<CoreWorkerClientOptions> = {}) {
  const workers: ScriptedWorker[] = [];
  const log = vi.fn<(message: string) => void>();
  const client = createCoreWorkerClient({
    spawn: () => {
      const worker = new ScriptedWorker();
      workers.push(worker);
      return worker;
    },
    module: () => Promise.resolve(MODULE),
    timeOrigin: 1_000,
    log,
    ...options,
  });
  const current = (): ScriptedWorker => {
    const worker = workers.at(-1);
    if (worker === undefined) {
      throw new Error('no worker');
    }
    return worker;
  };
  return { client, workers, current, log };
}

/** Lets the client's pending promises (the module, posting) run. */
async function flush(): Promise<void> {
  for (let turn = 0; turn < 5; turn += 1) {
    await Promise.resolve();
  }
}

async function startedClient(options: Partial<CoreWorkerClientOptions> = {}) {
  const made = scripted(options);
  await flush();
  made.current().started();
  await made.client.ready();
  return made;
}

afterEach(() => {
  vi.useRealTimers();
});

describe('the client', () => {
  it('posts the page’s module and is ready once the worker has started', async () => {
    const { client, current } = scripted();
    expect(client.state).toBe('starting');
    expect(client.everStarted).toBe(false);
    await flush();
    expect(current().sent).toEqual([{ p: 1, kind: 'start', module: MODULE }]);
    current().started();
    await expect(client.ready()).resolves.toEqual(VERSION);
    expect(client.state).toBe('ready');
    expect(client.everStarted).toBe(true);
  });

  it('sends an update once started and resolves with the parsed answer on the page’s clock', async () => {
    const { client, current } = scripted();
    const answered = client.update('{"a":1}', { indentWidth: 2 });
    await flush();
    expect(current().sent.map((request) => request.kind)).toEqual(['start']);
    current().started();
    expect(current().last('update')).toEqual({
      p: 1,
      kind: 'update',
      id: 1,
      documentJson: '{"a":1}',
      optionsJson: '{"indentWidth":2}',
    });
    current().answer();
    await expect(answered).resolves.toEqual({
      result: { stage: 'generate', documentText: '{"a":1}' },
      scopeJson: null,
      startedAt: 500,
      updateMs: 12,
      indexMs: 0,
    });
  });

  it('sends only the known preview option and cuts a document over the size limit', async () => {
    const { client, current } = await startedClient();
    const huge = 'x'.repeat(32 * 1024 * 1024 + 10);
    void client.update(huge, { indentWidth: 4, extra: true } as never);
    const request = current().last('update');
    expect(request.kind === 'update' && request.documentJson.length).toBe(32 * 1024 * 1024 + 1);
    expect(request.kind === 'update' && request.optionsJson).toBe('{"indentWidth":4}');
  });

  it('runs one request at a time and keeps only the newest waiting', async () => {
    const { client, current } = await startedClient();
    const first = client.update('1', { indentWidth: 4 });
    const second = client.update('2', { indentWidth: 4 });
    const third = client.update('3', { indentWidth: 4 });
    await expect(second).resolves.toBeNull();
    expect(current().sent.filter((request) => request.kind === 'update')).toHaveLength(1);
    current().answer();
    expect((await first)?.result).toMatchObject({ documentText: '1' });
    const sent = current().last('update');
    expect(sent.kind === 'update' && sent.documentJson).toBe('3');
    expect(sent.kind === 'update' && sent.id).toBe(2);
    current().answer();
    expect((await third)?.result).toMatchObject({ documentText: '3' });
  });

  it('rejects an error envelope with the CoreError the main thread throws', async () => {
    const { client, current, workers } = await startedClient();
    const answered = client.update('{}', { indentWidth: 4 });
    current().answer('{"error":{"kind":"invalidOptions","message":"indentWidth must be 2 or 4"}}');
    const error: unknown = await answered.catch((caught: unknown) => caught);
    expect(error).toBeInstanceOf(CoreError);
    expect((error as CoreError).kind).toBe('invalidOptions');
    expect(workers).toHaveLength(1);
  });

  it('after a trap, rejects with CoreTrap, keeps the worker and sends the waiting request', async () => {
    const { client, current, workers } = await startedClient();
    const trapped = client.update('1', { indentWidth: 4 });
    const waiting = client.update('2', { indentWidth: 4 });
    const request = current().last('update');
    current().reply({
      p: 1,
      kind: 'trapped',
      id: request.kind === 'update' ? request.id : 0,
      message: 'x',
    });
    await expect(trapped).rejects.toBeInstanceOf(CoreTrap);
    expect(workers).toHaveLength(1);
    expect(current().terminated).toBe(false);
    const next = current().last('update');
    expect(next.kind === 'update' && next.documentJson).toBe('2');
    current().answer();
    expect((await waiting)?.result).toMatchObject({ documentText: '2' });
  });

  it('replaces a worker that raises an error and sends the waiting request to the new one', async () => {
    const { client, workers } = await startedClient();
    const running = client.update('1', { indentWidth: 4 });
    const waiting = client.update('2', { indentWidth: 4 });
    const old = workers[0];
    old?.onerror?.(new Event('error'));
    await expect(running).rejects.toBeInstanceOf(CoreTrap);
    expect(old?.terminated).toBe(true);
    expect(workers).toHaveLength(2);
    expect(client.state).toBe('starting');
    await flush();
    const fresh = workers[1];
    expect(fresh?.sent.map((request) => request.kind)).toEqual(['start']);
    fresh?.started();
    fresh?.answer();
    expect((await waiting)?.result).toMatchObject({ documentText: '2' });
    // A message from the old worker is not heard any more.
    old?.reply({ p: 1, kind: 'garbage' });
    expect(workers).toHaveLength(2);
  });

  it('replaces a worker on a messageerror, a reply for another request or a garbage reply', async () => {
    for (const misbehave of [
      (worker: ScriptedWorker) => worker.onmessageerror?.(new Event('messageerror')),
      (worker: ScriptedWorker) => {
        worker.answer();
        worker.reply({ ...worker.sent.at(-1), kind: 'updated' });
      },
      (worker: ScriptedWorker) => {
        worker.reply({
          p: 1,
          kind: 'updated',
          id: 99,
          updateJson: '{}',
          scopeJson: null,
          startedAt: 0,
          updateMs: 0,
          indexMs: 0,
        });
      },
      (worker: ScriptedWorker) => {
        worker.reply({ p: 2, kind: 'updated' });
      },
      (worker: ScriptedWorker) => {
        worker.reply('hello');
      },
      (worker: ScriptedWorker) => {
        worker.started();
      },
    ]) {
      const { client, current, workers } = await startedClient();
      const running = client.update('1', { indentWidth: 4 });
      running.catch(() => undefined);
      misbehave(current());
      expect(workers, misbehave.toString()).toHaveLength(2);
      expect(workers[0]?.terminated).toBe(true);
    }
  });

  it('replaces a worker that does not answer within the watchdog, without counting a failed start', async () => {
    vi.useFakeTimers();
    const { client, workers, log } = scripted({ updateTimeoutMs: 1_000, maxFailedStarts: 2 });
    for (let round = 0; round < 4; round += 1) {
      await flush();
      workers.at(-1)?.started();
      const hanging = client.update(String(round), { indentWidth: 4 });
      vi.advanceTimersByTime(999);
      expect(workers).toHaveLength(round + 1);
      vi.advanceTimersByTime(1);
      await expect(hanging).rejects.toBeInstanceOf(CoreTrap);
      expect(workers).toHaveLength(round + 2);
    }
    expect(client.state).toBe('starting');
    expect(log).toHaveBeenCalledWith('The preview worker did not answer an update in time.');
  });

  it('replaces a worker that does not start in time, counting it as a failed start', async () => {
    vi.useFakeTimers();
    const { client, workers } = scripted({ startTimeoutMs: 500, maxFailedStarts: 3 });
    const waiting = client.update('1', { indentWidth: 4 });
    vi.advanceTimersByTime(500);
    expect(workers).toHaveLength(2);
    vi.advanceTimersByTime(500);
    expect(workers).toHaveLength(3);
    vi.advanceTimersByTime(500);
    expect(client.state).toBe('failed');
    expect(workers).toHaveLength(3);
    expect(workers.every((worker) => worker.terminated)).toBe(true);
    const error: unknown = await waiting.catch((caught: unknown) => caught);
    expect(error).toBeInstanceOf(CoreError);
    expect((error as CoreError).kind).toBe('init');
  });

  it('fails after three failed starts in a row, and then refuses every request', async () => {
    const { client, current, workers } = scripted();
    const ready = client.ready();
    for (let attempt = 0; attempt < 3; attempt += 1) {
      await flush();
      current().reply({ p: 1, kind: 'startFailed', message: 'CompileError' });
    }
    expect(workers).toHaveLength(3);
    expect(client.state).toBe('failed');
    await expect(ready).rejects.toMatchObject({ kind: 'init' });
    await expect(client.update('{}', { indentWidth: 4 })).rejects.toMatchObject({ kind: 'init' });
    await expect(client.ready()).rejects.toMatchObject({ kind: 'init' });
    await expect(client.probe()).rejects.toMatchObject({ kind: 'init' });
  });

  it('does not count a worker replaced after it answered, and an answer resets the count', async () => {
    const { client, current, workers } = await startedClient({ maxFailedStarts: 2 });
    for (let round = 0; round < 5; round += 1) {
      const answered = client.update(String(round), { indentWidth: 4 });
      current().answer();
      await answered;
      current().onerror?.(new Event('error'));
      await flush();
      current().started();
    }
    expect(workers).toHaveLength(6);
    expect(client.state).toBe('ready');
    // One failed start (an error before any answer) is not yet two.
    current().onerror?.(new Event('error'));
    await flush();
    current().started();
    const answered = client.update('x', { indentWidth: 4 });
    current().answer();
    await answered;
    current().onerror?.(new Event('error'));
    await flush();
    current().onerror?.(new Event('error'));
    expect(client.state).toBe('starting');
  });

  it('fails at once when the webview cannot post a Module', async () => {
    const workers: ScriptedWorker[] = [];
    const client = createCoreWorkerClient({
      spawn: () => {
        const worker = new ScriptedWorker();
        worker.refuseModule = true;
        workers.push(worker);
        return worker;
      },
      module: () => Promise.resolve(MODULE),
      log: () => undefined,
    });
    const waiting = client.update('{}', { indentWidth: 4 });
    await expect(waiting).rejects.toMatchObject({ kind: 'init' });
    expect(client.state).toBe('failed');
    expect(workers).toHaveLength(1);
    expect(workers[0]?.terminated).toBe(true);
  });

  it('fails at once when the worker cannot be created or the page’s module does not compile', async () => {
    const throwing = createCoreWorkerClient({
      spawn: () => {
        throw new DOMException('refused', 'SecurityError');
      },
      module: () => Promise.resolve(MODULE),
      log: () => undefined,
    });
    expect(throwing.state).toBe('failed');
    const compileFails = createCoreWorkerClient({
      spawn: () => new ScriptedWorker(),
      module: () => Promise.reject(new CoreError('init', 'no')),
      log: () => undefined,
    });
    await expect(compileFails.ready()).rejects.toMatchObject({ kind: 'init' });
    expect(compileFails.state).toBe('failed');
  });

  it('refuses a worker whose core is another version', async () => {
    const { client, current, workers } = scripted({
      acceptVersion: (version) => version.app === '9.9.9',
    });
    for (let attempt = 0; attempt < 3; attempt += 1) {
      await flush();
      current().started();
    }
    expect(workers).toHaveLength(3);
    expect(client.state).toBe('failed');
    expect(client.everStarted).toBe(false);
  });

  it('treats an acceptVersion that throws as a refusal', async () => {
    vi.useFakeTimers();
    const { client, current, workers, log } = scripted({
      acceptVersion: () => {
        throw new CoreTrap('version: the compiler core stopped');
      },
      startTimeoutMs: 20_000,
    });
    for (let attempt = 0; attempt < 3; attempt += 1) {
      await flush();
      // Refused at once, with no time passing: the start timeout plays no part.
      expect(() => {
        current().started();
      }).not.toThrow();
      expect(workers[attempt]?.terminated).toBe(true);
    }
    expect(log).toHaveBeenCalledWith('The preview worker is replaced (protocol).');
    expect(workers).toHaveLength(3);
    expect(client.state).toBe('failed');
    expect(client.everStarted).toBe(false);
  });

  it('probes, one request at a time', async () => {
    const { client, current } = await startedClient();
    const running = client.update('1', { indentWidth: 4 });
    const probed = client.probe();
    expect(current().sent.filter((request) => request.kind === 'probe')).toHaveLength(0);
    current().answer();
    await running;
    const request = current().last('probe');
    const probe = { eval: 'blocked', request: 'failed', directives: [] };
    current().reply({ p: 1, kind: 'probed', id: request.kind === 'probe' ? request.id : 0, probe });
    await expect(probed).resolves.toEqual(probe);
  });

  it('kill() replaces the worker as an error does', async () => {
    const { client, workers } = await startedClient();
    client.kill();
    expect(workers).toHaveLength(2);
    expect(workers[0]?.terminated).toBe(true);
  });

  it('dispose() terminates the worker and resolves what waits with null', async () => {
    const { client, current } = await startedClient();
    const running = client.update('1', { indentWidth: 4 });
    const waiting = client.update('2', { indentWidth: 4 });
    const probed = client.probe();
    client.dispose();
    await expect(running).resolves.toBeNull();
    await expect(waiting).resolves.toBeNull();
    await expect(probed).rejects.toMatchObject({ kind: 'init' });
    expect(current().terminated).toBe(true);
    await expect(client.update('3', { indentWidth: 4 })).rejects.toMatchObject({ kind: 'init' });
    client.dispose();
  });
});

/** A fake core whose `update` answers with the document, traps on `TRAP` and counts instances. */
function fakeInstances() {
  let instances = 0;
  const make = (): Promise<CoreHandle> => {
    instances += 1;
    const instance = instances;
    return Promise.resolve(
      createCore({
        version: () => JSON.stringify({ ...VERSION, app: `instance ${String(instance)}` }),
        update: (documentJson, optionsJson) => {
          if (documentJson === 'TRAP') {
            throw new WebAssembly.RuntimeError('unreachable');
          }
          return JSON.stringify({ stage: 'generate', documentJson, optionsJson });
        },
        load: () => '{}',
        canonical: () => '{}',
        symbols_in_scope: () => 'null',
        scope_index: () => '{"index":1}',
        conversion_table: () => '[]',
        clipboard_make: () => '{}',
        paste_prepare: () => '{}',
      }),
    );
  };
  return { make, count: () => instances };
}

/** The host behind `port2`, the test's requests and replies at `port1`. */
function hostOn(options: Parameters<typeof serveCoreWorker>[1] = {}) {
  const { port1, port2 } = new MessageChannel();
  serveCoreWorker(
    {
      postMessage: (message) => {
        port2.postMessage(message);
      },
      set onmessage(handler: ((event: { readonly data: unknown }) => void) | null) {
        port2.removeAllListeners('message');
        if (handler !== null) {
          port2.on('message', (data: unknown) => {
            handler({ data });
          });
        }
      },
      get onmessage() {
        return null;
      },
    },
    options,
  );
  const replies: WorkerReply[] = [];
  let wake: (() => void) | null = null;
  port1.on('message', (data: unknown) => {
    replies.push(data as WorkerReply);
    wake?.();
  });
  const next = async (): Promise<WorkerReply> => {
    while (replies.length === 0) {
      await new Promise<void>((resolve) => {
        wake = resolve;
      });
    }
    const reply = replies.shift();
    if (reply === undefined) {
      throw new Error('no reply');
    }
    return reply;
  };
  return { port: port1, other: port2, next, replies };
}

const channels: MessagePort[] = [];

afterEach(() => {
  for (const port of channels.splice(0)) {
    port.close();
  }
});

describe('the host', () => {
  it('starts from the posted module and answers updates with the core’s text', async () => {
    const fake = fakeInstances();
    const clock = {
      timeOrigin: 1_000_000,
      now: vi.fn().mockReturnValueOnce(10).mockReturnValueOnce(52).mockReturnValueOnce(55),
    };
    const { port, other, next } = hostOn({ instantiate: fake.make, clock });
    channels.push(port, other);
    port.postMessage({ p: 1, kind: 'start', module: MODULE });
    expect(await next()).toEqual({
      p: 1,
      kind: 'started',
      versionJson: JSON.stringify({ ...VERSION, app: 'instance 1' }),
    });
    port.postMessage({
      p: 1,
      kind: 'update',
      id: 4,
      documentJson: '{"x":1}',
      optionsJson: '{"indentWidth":2}',
    });
    expect(await next()).toEqual({
      p: 1,
      kind: 'updated',
      id: 4,
      updateJson: JSON.stringify({
        stage: 'generate',
        documentJson: '{"x":1}',
        optionsJson: '{"indentWidth":2}',
      }),
      scopeJson: '{"index":1}',
      startedAt: 1_000_010,
      updateMs: 42,
      indexMs: 3,
    });
  });

  it('after a trap, makes a fresh instance from its module and answers the next update', async () => {
    const fake = fakeInstances();
    const { port, other, next } = hostOn({ instantiate: fake.make });
    channels.push(port, other);
    port.postMessage({ p: 1, kind: 'start', module: MODULE });
    await next();
    port.postMessage({ p: 1, kind: 'update', id: 1, documentJson: 'TRAP', optionsJson: '{}' });
    port.postMessage({ p: 1, kind: 'update', id: 2, documentJson: '{}', optionsJson: '{}' });
    expect(await next()).toMatchObject({ kind: 'trapped', id: 1 });
    expect(fake.count()).toBe(2);
    expect(await next()).toMatchObject({ kind: 'updated', id: 2 });
  });

  it('reports a start that fails, and a fresh instance that cannot be made after a trap', async () => {
    let fail = true;
    const fake = fakeInstances();
    const { port, other, next } = hostOn({
      instantiate: () => (fail ? Promise.reject(new CoreError('init', 'no')) : fake.make()),
    });
    channels.push(port, other);
    port.postMessage({ p: 1, kind: 'start', module: MODULE });
    expect(await next()).toEqual({
      p: 1,
      kind: 'startFailed',
      message: 'CoreError: no',
    });
    const second = hostOn({
      instantiate: () => (fail ? Promise.reject(new CoreError('init', 'gone')) : fake.make()),
    });
    channels.push(second.port, second.other);
    fail = false;
    second.port.postMessage({ p: 1, kind: 'start', module: MODULE });
    await second.next();
    fail = true;
    second.port.postMessage({
      p: 1,
      kind: 'update',
      id: 1,
      documentJson: 'TRAP',
      optionsJson: '{}',
    });
    expect(await second.next()).toMatchObject({ kind: 'startFailed', message: 'CoreError: gone' });
  });

  it('ignores what is not of its protocol, a second start, an update before the start and a probe it cannot answer', async () => {
    const fake = fakeInstances();
    const errors = vi.spyOn(console, 'error').mockImplementation(() => undefined);
    const { port, other, next, replies } = hostOn({ instantiate: fake.make });
    channels.push(port, other);
    port.postMessage({ p: 1, kind: 'update', id: 1, documentJson: '{}', optionsJson: '{}' });
    port.postMessage('garbage');
    port.postMessage({ p: 2, kind: 'start', module: MODULE });
    port.postMessage({ p: 1, kind: 'start', module: MODULE });
    await next();
    port.postMessage({ p: 1, kind: 'start', module: MODULE });
    port.postMessage({ p: 1, kind: 'probe', id: 3 });
    port.postMessage({ p: 1, kind: 'update', id: 4, documentJson: '{}', optionsJson: '{}' });
    expect(await next()).toMatchObject({ kind: 'updated', id: 4 });
    expect(replies).toHaveLength(0);
    expect(fake.count()).toBe(1);
    expect(errors).toHaveBeenCalledTimes(5);
    for (const call of errors.mock.calls) {
      expect(call).toEqual(['The preview worker ignored a message that is not of its protocol.']);
    }
    errors.mockRestore();
  });

  it('answers a probe when it has one', async () => {
    const fake = fakeInstances();
    const probe = {
      eval: 'blocked' as const,
      request: 'blocked' as const,
      directives: ['connect-src'],
    };
    const { port, other, next } = hostOn({
      instantiate: fake.make,
      probe: () => Promise.resolve(probe),
    });
    channels.push(port, other);
    port.postMessage({ p: 1, kind: 'start', module: MODULE });
    await next();
    port.postMessage({ p: 1, kind: 'probe', id: 9 });
    expect(await next()).toEqual({ p: 1, kind: 'probed', id: 9, probe });
  });
});

describe('the client and the host', () => {
  it('work together over a MessageChannel, the Module cloned', async () => {
    const fake = fakeInstances();
    const ports: MessagePort[] = [];
    let client: CoreWorkerClient | null = null;
    try {
      client = createCoreWorkerClient({
        spawn: () => {
          const { port1, port2 } = new MessageChannel();
          ports.push(port1, port2);
          serveCoreWorker(
            {
              postMessage: (message) => {
                port2.postMessage(message);
              },
              set onmessage(handler: ((event: { readonly data: unknown }) => void) | null) {
                port2.removeAllListeners('message');
                if (handler !== null) {
                  port2.on('message', (data: unknown) => {
                    handler({ data });
                  });
                }
              },
              get onmessage() {
                return null;
              },
            },
            {
              instantiate: (module) => {
                expect(module).toBeInstanceOf(WebAssembly.Module);
                return fake.make();
              },
            },
          );
          const worker: WorkerLike = {
            postMessage: (message) => {
              port1.postMessage(message);
            },
            onmessage: null,
            onerror: null,
            onmessageerror: null,
            terminate: () => {
              port1.close();
            },
          };
          port1.on('message', (data: unknown) => worker.onmessage?.({ data }));
          return worker;
        },
        module: () => Promise.resolve(MODULE),
      });
      await expect(client.ready()).resolves.toMatchObject({ app: 'instance 1' });
      const answer = await client.update('{"doc":true}', { indentWidth: 2 });
      expect(answer?.result).toEqual({
        stage: 'generate',
        documentJson: '{"doc":true}',
        optionsJson: '{"indentWidth":2}',
      });
      await expect(client.update('TRAP', { indentWidth: 2 })).rejects.toBeInstanceOf(CoreTrap);
      expect((await client.update('{}', { indentWidth: 4 }))?.result).toMatchObject({
        stage: 'generate',
      });
      expect(fake.count()).toBe(2);
    } finally {
      client?.dispose();
      for (const port of ports) {
        port.close();
      }
    }
  });
});
