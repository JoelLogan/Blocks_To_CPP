import { COMMAND_NAMES, IpcCallError, type IpcClient } from '@blocks2cpp/ipc-types';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { createFakeIpc } from '../app/testing/fixtures';
import { ipc, setIpcForTests, tauriTransport } from './ipc';

/** Tauri's `invoke` and `Channel`, which outside the desktop app have no backend to talk to. */
const tauri = vi.hoisted(() => {
  class Channel {
    onmessage: (message: unknown) => void = () => undefined;
  }
  return {
    invoke: vi.fn<(command: string, args?: unknown) => Promise<unknown>>(),
    Channel,
  };
});
vi.mock('@tauri-apps/api/core', () => tauri);

type FakeChannel = InstanceType<typeof tauri.Channel>;

beforeEach(() => {
  tauri.invoke.mockResolvedValue({});
});

afterEach(() => {
  setIpcForTests(null);
});

describe('the Tauri client', () => {
  it('has one method per backend command', () => {
    expect(Object.keys(ipc)).toHaveLength(COMMAND_NAMES.length);
  });

  it('calls app_info with no arguments', async () => {
    const info = { appVersion: '0.1.0', ipcVersion: 1, platform: 'linux', catalogVersion: '1.0.0' };
    tauri.invoke.mockResolvedValue(info);

    await expect(ipc.appInfo()).resolves.toEqual(info);
    expect(tauri.invoke).toHaveBeenCalledWith('app_info', {});
  });

  it('wraps a request in the single "request" key', async () => {
    await ipc.projectSetDirty({ handle: 'ph_0123456789abcdef0123456789abcdef', dirty: true });

    expect(tauri.invoke).toHaveBeenCalledWith('project_set_dirty', {
      request: { handle: 'ph_0123456789abcdef0123456789abcdef', dirty: true },
    });
  });

  it('passes app events through a Tauri channel', async () => {
    const onEvent = vi.fn();
    await ipc.appSubscribe(onEvent);

    const [command, args] = tauri.invoke.mock.calls[0] as [string, { onEvent: FakeChannel }];
    expect(command).toBe('app_subscribe');
    expect(args.onEvent).toBeInstanceOf(tauri.Channel);
    args.onEvent.onmessage({ kind: 'closeRequested' });
    expect(onEvent).toHaveBeenCalledWith({ kind: 'closeRequested' });
  });

  it('turns a backend error into an IpcCallError with its typed error', async () => {
    tauri.invoke.mockRejectedValue({ code: 'unknownHandle' });

    const failure = ipc.trustGet({ handle: 'ph_0123456789abcdef0123456789abcdef' });
    await expect(failure).rejects.toBeInstanceOf(IpcCallError);
    await expect(failure).rejects.toMatchObject({
      command: 'trust_get',
      error: { code: 'unknownHandle' },
    });
  });

  it('reports a failure below the command as a transport error', async () => {
    tauri.invoke.mockRejectedValue(new Error('the isolation hook dropped the message'));

    await expect(ipc.appInfo()).rejects.toMatchObject({
      error: { code: 'transport', message: 'the isolation hook dropped the message' },
    });
  });

  it('creates channels whose messages reach the handler', () => {
    const handler = vi.fn();
    const channel = tauriTransport.channel(handler) as FakeChannel;

    channel.onmessage('hello');
    expect(handler).toHaveBeenCalledWith('hello');
  });
});

describe('setIpcForTests', () => {
  it('sends every call to the replacement, also through references kept before', async () => {
    const kept: IpcClient = ipc;
    const fake = createFakeIpc();
    setIpcForTests(fake);

    await kept.appInfo();
    expect(fake.appInfo).toHaveBeenCalledTimes(1);
    expect(tauri.invoke).not.toHaveBeenCalled();
  });

  it('restores the Tauri client with null', async () => {
    setIpcForTests(createFakeIpc());
    setIpcForTests(null);

    await ipc.recentList();
    expect(tauri.invoke).toHaveBeenCalledWith('recent_list', {});
  });
});
