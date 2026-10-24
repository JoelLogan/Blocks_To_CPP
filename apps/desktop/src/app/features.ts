/**
 * Features are the app's vertical slices (project, build and run, toolchain, settings, trust,
 * recovery): each one registers its commands, screens and event handlers through a
 * {@link FeatureContext} when the app starts, and returns its clean-up function. The list of
 * features is in src/features/index.ts.
 */
import type { CoreWasm } from '@blocks2cpp/b2c-core-wasm';
import type { IpcClient } from '@blocks2cpp/ipc-types';

import type { CommandRegistry } from './commands';
import type { DialogService } from './dialogs';
import type { EditorHandle } from './editor-types';
import type { AppEventBus } from './events';
import type { ScreenRegistry } from './screens';
import type { useAppStore } from './store';

/** Everything a feature may use. */
export interface FeatureContext {
  /** The backend's commands. */
  ipc: IpcClient;
  /** The app's state. */
  store: typeof useAppStore;
  /** The command registry (`registerCommand`, `runCommand`). */
  commands: CommandRegistry;
  /** The screen registry (`registerScreen`). */
  screens: ScreenRegistry;
  /** The in-window dialogs. */
  dialogs: DialogService;
  /** The backend's push events and the frontend's local events. */
  events: AppEventBus;
  /**
   * The compiler core, or `null` before it has started. Call it each time and never keep the
   * instance: it is replaced after a WebAssembly trap (see `app/core.ts`).
   */
  core: () => CoreWasm | null;
  /** The block editor, or `null` while there is no workspace. */
  editor: () => EditorHandle | null;
}

/** A feature: installs itself and returns the function that uninstalls it. */
export type Feature = (ctx: FeatureContext) => () => void;

/**
 * Installs `features` in order and returns the function that uninstalls them in reverse order. A
 * feature that fails to install is logged and skipped, so one broken feature cannot keep the
 * window from starting; a failing clean-up is logged too.
 */
export function installAll(features: readonly Feature[], ctx: FeatureContext): () => void {
  const uninstallers: (() => void)[] = [];
  for (const feature of features) {
    try {
      uninstallers.push(feature(ctx));
    } catch (error: unknown) {
      console.error(`The feature ${feature.name || '(unnamed)'} failed to install`, error);
    }
  }
  let installed = true;
  return () => {
    if (!installed) {
      return;
    }
    installed = false;
    for (const uninstall of uninstallers.reverse()) {
      try {
        uninstall();
      } catch (error: unknown) {
        console.error('A feature failed to uninstall', error);
      }
    }
  };
}
