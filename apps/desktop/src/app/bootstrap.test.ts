import { IPC_VERSION, IpcCallError } from '@blocks2cpp/ipc-types';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { type AppRuntime, createAppRuntime } from './bootstrap';
import { createCommandRegistry } from './commands';
import { setCore } from './core';
import { createDialogQueue } from './dialogs';
import { setEditorHandle } from './editor-types';
import type { BusEvent } from './events';
import type { FeatureContext } from './features';
import { createScreenRegistry } from './screens';
import { resetAppStore, useAppStore } from './store';
import {
  appInfoFixture,
  createFakeIpc,
  type FakeIpc,
  settingsFixture,
  toolchainFixture,
} from './testing/fixtures';

let runtime: AppRuntime | null = null;

/** A runtime over a fake backend, with spies for what it installs. */
function setUp(options: { withoutBackend?: boolean } = {}) {
  const ipc = createFakeIpc();
  const commands = createCommandRegistry();
  const uninstallFeatures = vi.fn();
  const installFeatures = vi.fn<(ctx: FeatureContext) => () => void>(() => uninstallFeatures);
  const uninstallBlocklyDialogs = vi.fn();
  const installBlocklyDialogs = vi.fn(() => uninstallBlocklyDialogs);
  runtime = createAppRuntime({
    ipc,
    store: useAppStore,
    commands,
    screens: createScreenRegistry(),
    dialogs: createDialogQueue(),
    installFeatures,
    installBlocklyDialogs,
    window,
    ...options,
  });
  return {
    runtime,
    ipc,
    commands,
    installFeatures,
    uninstallFeatures,
    installBlocklyDialogs,
    uninstallBlocklyDialogs,
  };
}

/** Records every event of `kinds` that reaches the bus. */
function recordEvents(ctx: FeatureContext, kinds: BusEvent['kind'][]): BusEvent[] {
  const seen: BusEvent[] = [];
  for (const kind of kinds) {
    ctx.events.on(kind, (event) => seen.push(event));
  }
  return seen;
}

beforeEach(() => {
  resetAppStore();
});

afterEach(() => {
  runtime?.stop();
  runtime = null;
});

describe('starting the window', () => {
  it('checks the IPC version, subscribes once, reads the state and installs everything', async () => {
    const { runtime, ipc, commands, installFeatures, installBlocklyDialogs } = setUp();
    const toolchain = toolchainFixture();
    ipc.toolchainList.mockResolvedValue({ toolchains: [toolchain], discovering: true });
    ipc.settingsGet.mockResolvedValue({
      settings: settingsFixture({ run: { onErrors: 'showProblems' } }),
      notices: [{ key: 'console.scrollbackLines', reason: 'invalidValue' }],
    });
    expect(runtime.getPhase()).toEqual({ kind: 'starting' });

    const [first, second] = await Promise.all([runtime.start(), runtime.start()]);

    expect(first).toEqual({ kind: 'ready' });
    expect(second).toBe(first);
    expect(runtime.getPhase()).toEqual({ kind: 'ready' });
    expect(ipc.appInfo).toHaveBeenCalledTimes(1);
    expect(ipc.appSubscribe).toHaveBeenCalledTimes(1);
    const state = useAppStore.getState();
    expect(state.appInfo).toEqual(appInfoFixture());
    expect(state.settings.value?.run.onErrors).toBe('showProblems');
    expect(state.settings.notices).toHaveLength(1);
    expect(state.toolchains).toMatchObject({ list: [toolchain], discovering: true });
    expect(commands.hasCommand('problems.focusFirstError')).toBe(true);
    expect(installBlocklyDialogs).toHaveBeenCalledTimes(1);
    expect(installFeatures).toHaveBeenCalledTimes(1);
  });

  it('gives features everything they need', async () => {
    const { runtime, ipc, installFeatures } = setUp();
    await runtime.start();

    const ctx = installFeatures.mock.calls[0]?.[0];
    expect(ctx).toBe(runtime.context);
    expect(ctx?.ipc).toBe(ipc);
    expect(ctx?.store).toBe(useAppStore);
    expect(ctx?.dialogs).toBe(runtime.dialogs);

    expect(ctx?.core()).toBeNull();
    const core = {} as Parameters<typeof setCore>[0];
    setCore(core);
    expect(ctx?.core()).toBe(core);
    setCore(null);

    expect(ctx?.editor()).toBeNull();
    const editor = {} as Parameters<typeof setEditorHandle>[0];
    setEditorHandle(editor);
    expect(ctx?.editor()).toBe(editor);
    setEditorHandle(null);
  });

  it('stops with a version mismatch and installs nothing', async () => {
    const { runtime, ipc, installFeatures } = setUp();
    ipc.appInfo.mockResolvedValue(appInfoFixture({ ipcVersion: IPC_VERSION + 1 }));

    await expect(runtime.start()).resolves.toEqual({
      kind: 'versionMismatch',
      frontend: IPC_VERSION,
      backend: IPC_VERSION + 1,
    });
    expect(ipc.appSubscribe).not.toHaveBeenCalled();
    expect(installFeatures).not.toHaveBeenCalled();
    expect(useAppStore.getState().appInfo).toBeNull();
  });

  it('treats an answer without a usable version as a mismatch', async () => {
    const { runtime, ipc } = setUp();
    ipc.appInfo.mockResolvedValue({ appVersion: '9.0.0' } as unknown as ReturnType<
      typeof appInfoFixture
    >);
    await expect(runtime.start()).resolves.toMatchObject({
      kind: 'versionMismatch',
      backend: null,
    });

    const other = setUp();
    other.ipc.appInfo.mockResolvedValue(null as unknown as ReturnType<typeof appInfoFixture>);
    await expect(other.runtime.start()).resolves.toMatchObject({ backend: null });
  });

  it('fails with the error code when the backend does not answer', async () => {
    const { runtime, ipc, installFeatures } = setUp();
    ipc.appInfo.mockRejectedValue(
      new IpcCallError('app_info', { code: 'transport', message: 'no backend' }),
    );

    await expect(runtime.start()).resolves.toEqual({ kind: 'failed', code: 'transport' });
    expect(installFeatures).not.toHaveBeenCalled();
  });

  it('fails when the subscription is refused, so close requests are never missed', async () => {
    const { runtime, ipc, installFeatures } = setUp();
    ipc.appSubscribe.mockRejectedValue(new Error('denied'));

    await expect(runtime.start()).resolves.toEqual({ kind: 'failed', code: 'transport' });
    expect(installFeatures).not.toHaveBeenCalled();
  });

  it('starts anyway when the settings or the toolchains cannot be read', async () => {
    const { runtime, ipc } = setUp();
    const consoleError = vi.spyOn(console, 'error').mockImplementation(() => undefined);
    ipc.settingsGet.mockRejectedValue(new IpcCallError('settings_get', { code: 'internal' }));
    ipc.toolchainList.mockRejectedValue(new Error('gone'));

    await expect(runtime.start()).resolves.toEqual({ kind: 'ready' });
    expect(useAppStore.getState().settings.value).toBeNull();
    expect(useAppStore.getState().toolchains.discovering).toBe(false);
    expect(consoleError).toHaveBeenCalledWith('Could not read the settings', 'internal');
    expect(consoleError).toHaveBeenCalledWith('Could not read the toolchain list', 'transport');
  });

  it('starts without the backend only when told to (a browser)', async () => {
    const { runtime, ipc, installFeatures } = setUp({ withoutBackend: true });
    vi.spyOn(console, 'warn').mockImplementation(() => undefined);

    await expect(runtime.start()).resolves.toEqual({ kind: 'ready' });
    expect(ipc.appInfo).not.toHaveBeenCalled();
    expect(ipc.appSubscribe).not.toHaveBeenCalled();
    expect(installFeatures).toHaveBeenCalledTimes(1);
  });

  it('fails with "internal" when the start-up itself breaks', async () => {
    const { runtime, installFeatures } = setUp();
    const consoleError = vi.spyOn(console, 'error').mockImplementation(() => undefined);
    installFeatures.mockImplementation(() => {
      throw new Error('a bug');
    });

    await expect(runtime.start()).resolves.toEqual({ kind: 'failed', code: 'internal' });
    expect(runtime.getPhase()).toEqual({ kind: 'failed', code: 'internal' });
    expect(consoleError).toHaveBeenCalledWith('Blocks2Cpp failed to start', expect.any(Error));
  });

  it('tells subscribers about each phase change', async () => {
    const { runtime } = setUp();
    const listener = vi.fn();
    const unsubscribe = runtime.subscribe(listener);

    await runtime.start();
    expect(listener).toHaveBeenCalledTimes(1);
    unsubscribe();
  });

  it('uninstalls everything on stop, once', async () => {
    const { runtime, uninstallFeatures, uninstallBlocklyDialogs, commands } = setUp();
    await runtime.start();

    const pending = runtime.dialogs.confirm({ message: 'still open' });
    runtime.stop();
    runtime.stop();

    expect(uninstallFeatures).toHaveBeenCalledTimes(1);
    expect(uninstallBlocklyDialogs).toHaveBeenCalledTimes(1);
    expect(commands.hasCommand('problems.focusFirstError')).toBe(false);
    await expect(pending).resolves.toBe(false);
  });
});

describe('app events', () => {
  it('keeps the toolchain list and the settings notices in the store, and forwards every event', async () => {
    const { runtime, ipc } = setUp();
    await runtime.start();
    const seen = recordEvents(runtime.context, [
      'toolchainsUpdated',
      'settingsNotice',
      'projectChangedOnDisk',
      'closeRequested',
    ]);
    const toolchain = toolchainFixture();

    ipc.pushAppEvent({ kind: 'toolchainsUpdated', toolchains: [toolchain], discovering: false });
    ipc.pushAppEvent({
      kind: 'settingsNotice',
      notices: [{ key: 'codeStyle.indentWidth', reason: 'corruptFile' }],
    });
    ipc.pushAppEvent({
      kind: 'projectChangedOnDisk',
      handle: 'ph_0123456789abcdef0123456789abcdef',
      deleted: false,
    });
    ipc.pushAppEvent({ kind: 'closeRequested' });

    const state = useAppStore.getState();
    expect(state.toolchains).toMatchObject({ list: [toolchain], discovering: false });
    expect(state.settings.notices).toEqual([
      { key: 'codeStyle.indentWidth', reason: 'corruptFile' },
    ]);
    expect(seen.map((event) => event.kind)).toEqual([
      'toolchainsUpdated',
      'settingsNotice',
      'projectChangedOnDisk',
      'closeRequested',
    ]);
  });

  it('ignores messages of an unknown shape', async () => {
    const { runtime, ipc } = setUp();
    await runtime.start();
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    const seen = recordEvents(runtime.context, ['toolchainsUpdated']);

    ipc.pushAppEvent({ kind: 'toolchainsUpdated', toolchains: 'all of them' });
    ipc.pushAppEvent('closeRequested');

    expect(seen).toEqual([]);
    expect(useAppStore.getState().toolchains.list).toEqual([]);
    expect(warn).toHaveBeenCalledTimes(2);
  });

  it('prefers a toolchain event over an older list answer', async () => {
    const { runtime, ipc } = setUp();
    let answerList!: (value: Awaited<ReturnType<FakeIpc['toolchainList']>>) => void;
    ipc.toolchainList.mockReturnValue(
      new Promise((resolve) => {
        answerList = resolve;
      }),
    );
    const started = runtime.start();
    await vi.waitFor(() => {
      expect(ipc.toolchainList).toHaveBeenCalled();
    });
    expect(useAppStore.getState().toolchains.discovering).toBe(true);

    const fresh = toolchainFixture({ version: '15.2.0' });
    ipc.pushAppEvent({ kind: 'toolchainsUpdated', toolchains: [fresh], discovering: false });
    answerList({ toolchains: [], discovering: true });
    await started;

    expect(useAppStore.getState().toolchains).toMatchObject({
      list: [fresh],
      discovering: false,
    });
  });
});
