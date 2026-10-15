/**
 * Starting the window (docs/spec/02-architecture.md §2.5.7, §2.5.3):
 *
 * 1. `app_info`: the backend's IPC version must equal the one this frontend was generated with
 *    (`IPC_VERSION`), and in a development build the editor's compiler core must have the
 *    backend's block catalog version (`catalogVersion`, see `coreCatalogVersion`); otherwise the
 *    window shows a blocking error and nothing else runs.
 * 2. `app_subscribe`, exactly once: the backend's push events. The shell itself keeps the
 *    toolchain list and the settings notices in the store; every event then goes to the event bus.
 * 3. The settings and the toolchain list, so the toolbar and the status bar are right from the
 *    start (failures here are logged; the features ask again).
 * 4. The shell's default commands, Blockly's dialogs, the shortcuts and the features.
 */
import { type AppInfo, IPC_VERSION, IpcCallError, type IpcClient } from '@blocks2cpp/ipc-types';

import { installShellCommands } from './actions';
import type { CommandRegistry } from './commands';
import { getCore } from './core';
import type { DialogQueue, DialogService } from './dialogs';
import { getEditorHandle } from './editor-types';
import { type AppEventBus, createAppEventBus, isAppEvent } from './events';
import type { FeatureContext } from './features';
import type { ScreenRegistry } from './screens';
import { installShortcuts } from './shortcuts';
import type { useAppStore } from './store';

/** Where the start of the window is. */
export type BootPhase =
  | { kind: 'starting' }
  | { kind: 'ready' }
  /**
   * The backend speaks another IPC version: a broken installation. `backend` is `null` when its
   * answer had no usable version.
   */
  | { kind: 'versionMismatch'; frontend: number; backend: number | null }
  /**
   * The editor's compiler core (WebAssembly) was built with another block catalog than the
   * backend: a core older or newer than the app, which would preview other C++ than the backend
   * builds. `backend` is `null` when `app_info` named no catalog version.
   */
  | { kind: 'coreMismatch'; core: string; backend: string | null }
  /** The backend did not answer; `code` is the IPC error code or `transport`. */
  | { kind: 'failed'; code: string };

/** What the window needs to start. */
export interface AppRuntimeOptions {
  ipc: IpcClient;
  store: typeof useAppStore;
  commands: CommandRegistry;
  screens: ScreenRegistry;
  dialogs: DialogQueue;
  /** Installs the features (src/features/index.ts). */
  installFeatures: (ctx: FeatureContext) => () => void;
  /** Overrides Blockly's own dialogs (src/app/dialogs/blockly.ts). */
  installBlocklyDialogs: (service: DialogService) => () => void;
  /** The window the shortcuts listen on. */
  window: Window;
  /**
   * Reads the block catalog version of the editor's compiler core (`version().catalog` of
   * `@blocks2cpp/b2c-core-wasm`; reading it starts the core). When given, the start compares it
   * with the backend's `catalogVersion` right after the IPC version, before the subscription, and
   * a difference is the blocking `coreMismatch`. A core that cannot start does not stop the window:
   * the editor reports that when it needs the core.
   */
  coreCatalogVersion?: () => Promise<string>;
  /**
   * Start without the backend: only for the Vite dev server opened in a browser, where there is
   * none. The window then has no app information and no backend events.
   */
  withoutBackend?: boolean;
}

/** The running window: its start-up state and everything features get. */
export interface AppRuntime {
  /** Everything a feature gets. */
  readonly context: FeatureContext;
  /** The dialog queue, for the `DialogHost`. */
  readonly dialogs: DialogQueue;
  /** Where the start is. */
  getPhase: () => BootPhase;
  /** Calls `listener` when the phase changes. Returns the unsubscriber. */
  subscribe: (listener: () => void) => () => void;
  /** Starts the window. Calling it again returns the same promise: it starts only once. */
  start(): Promise<BootPhase>;
  /** Uninstalls the features, the shortcuts and the dialog overrides. */
  stop(): void;
}

/** The code of a failed call, without any text from the backend. */
function failureCode(error: unknown): string {
  return error instanceof IpcCallError ? error.error.code : 'transport';
}

/** The IPC version in an `app_info` answer, or `null` when it has none. */
function versionOf(info: unknown): number | null {
  if (typeof info !== 'object' || info === null) {
    return null;
  }
  const version: unknown = (info as { ipcVersion?: unknown }).ipcVersion;
  return typeof version === 'number' && Number.isSafeInteger(version) ? version : null;
}

/** The catalog version in an `app_info` answer, or `null` when it has none. */
function catalogVersionOf(info: AppInfo): string | null {
  const version: unknown = (info as { catalogVersion?: unknown }).catalogVersion;
  return typeof version === 'string' ? version : null;
}

/** Creates the window's runtime; nothing happens until {@link AppRuntime.start}. */
export function createAppRuntime(options: AppRuntimeOptions): AppRuntime {
  const { ipc, store } = options;
  const events: AppEventBus = createAppEventBus();
  const context: FeatureContext = {
    ipc,
    store,
    commands: options.commands,
    screens: options.screens,
    dialogs: options.dialogs,
    events,
    core: getCore,
    editor: getEditorHandle,
  };

  let phase: BootPhase = { kind: 'starting' };
  const listeners = new Set<() => void>();
  const uninstallers: (() => void)[] = [];
  let started: Promise<BootPhase> | null = null;
  /** Whether a `toolchainsUpdated` event arrived, which is newer than any list asked for. */
  let toolchainEventSeen = false;

  function setPhase(next: BootPhase): BootPhase {
    phase = next;
    for (const listener of [...listeners]) {
      listener();
    }
    return next;
  }

  function onAppEvent(message: unknown): void {
    if (!isAppEvent(message)) {
      console.warn('Ignored an app event of an unknown shape');
      return;
    }
    const { actions } = store.getState();
    switch (message.kind) {
      case 'toolchainsUpdated':
        toolchainEventSeen = true;
        actions.setToolchains({ list: message.toolchains, discovering: message.discovering });
        break;
      case 'settingsNotice':
        actions.setSettings({ notices: message.notices });
        break;
      case 'projectChangedOnDisk':
      case 'closeRequested':
        break;
    }
    events.emit(message);
  }

  /** Reads the settings and the toolchain list; a failure leaves the defaults. */
  async function loadInitialState(): Promise<void> {
    const { actions } = store.getState();
    actions.setToolchains({ discovering: true });
    const [settings, toolchains] = await Promise.allSettled([
      ipc.settingsGet(),
      ipc.toolchainList(),
    ]);
    if (settings.status === 'fulfilled') {
      actions.setSettings({ value: settings.value.settings, notices: settings.value.notices });
    } else {
      console.error('Could not read the settings', failureCode(settings.reason));
    }
    if (toolchains.status === 'fulfilled') {
      if (!toolchainEventSeen) {
        actions.setToolchains({
          list: toolchains.value.toolchains,
          discovering: toolchains.value.discovering,
        });
      }
    } else {
      console.error('Could not read the toolchain list', failureCode(toolchains.reason));
      if (!toolchainEventSeen) {
        actions.setToolchains({ discovering: false });
      }
    }
  }

  /**
   * The `coreMismatch` phase when the compiler core's catalog version differs from the backend's,
   * or `null` when it is the same, not asked for, or unknown because the core did not start.
   */
  async function coreMismatch(info: AppInfo): Promise<BootPhase | null> {
    if (options.coreCatalogVersion === undefined) {
      return null;
    }
    let core: string;
    try {
      core = await options.coreCatalogVersion();
    } catch (error: unknown) {
      console.error('Could not start the compiler core to compare its catalog version', error);
      return null;
    }
    const backend = catalogVersionOf(info);
    return core === backend ? null : { kind: 'coreMismatch', core, backend };
  }

  /** Installs everything that runs in a ready window. */
  function install(): void {
    const shell = {
      store,
      commands: options.commands,
      screens: options.screens,
      editor: getEditorHandle,
    };
    uninstallers.push(installShellCommands(shell));
    uninstallers.push(options.installBlocklyDialogs(options.dialogs));
    uninstallers.push(installShortcuts(options.window, shell));
    uninstallers.push(options.installFeatures(context));
  }

  async function run(): Promise<BootPhase> {
    if (options.withoutBackend === true) {
      console.warn('Blocks2Cpp is running without its backend (a browser, not the desktop app)');
      install();
      return setPhase({ kind: 'ready' });
    }

    let info: AppInfo;
    try {
      info = await ipc.appInfo();
    } catch (error: unknown) {
      return setPhase({ kind: 'failed', code: failureCode(error) });
    }
    const backendVersion = versionOf(info);
    if (backendVersion !== IPC_VERSION) {
      return setPhase({ kind: 'versionMismatch', frontend: IPC_VERSION, backend: backendVersion });
    }
    // Before the subscription, like the IPC version: a window that shows a blocking error must not
    // take the backend's close requests, which only the features answer.
    const mismatch = await coreMismatch(info);
    if (mismatch !== null) {
      return setPhase(mismatch);
    }
    store.getState().actions.setAppInfo(info);

    try {
      await ipc.appSubscribe(onAppEvent);
    } catch (error: unknown) {
      // Without the subscription the window would miss close requests: do not go on.
      return setPhase({ kind: 'failed', code: failureCode(error) });
    }

    await loadInitialState();
    install();
    return setPhase({ kind: 'ready' });
  }

  return {
    context,
    dialogs: options.dialogs,
    getPhase: () => phase,
    subscribe: (listener) => {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
    start() {
      started ??= run().catch((error: unknown) => {
        // A bug in the start-up itself (not the backend's answer): show it instead of a window
        // that never finishes starting.
        console.error('Blocks2Cpp failed to start', error);
        return setPhase({ kind: 'failed', code: 'internal' });
      });
      return started;
    },
    stop() {
      for (const uninstall of uninstallers.splice(0).reverse()) {
        uninstall();
      }
      options.dialogs.cancelAll();
    },
  };
}
