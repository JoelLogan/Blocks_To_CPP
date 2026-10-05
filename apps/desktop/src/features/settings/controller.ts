/**
 * The settings feature's logic: reading the machine settings (`settings_get`), changing them with
 * partial updates (`settings_update`) and clearing the build cache (`build_cache_clear`)
 * (docs/spec/04-user-interface.md §4.12, 05 §5.9, 07 §7.5.1).
 *
 * Reads and updates run one after the other, in the order they were asked for, so an answer
 * never overwrites a newer one. Every answer goes into the app's store; the rest of the app
 * follows it from there (an indent change makes the live preview run again, the Run on errors
 * mode changes the run gate, the scrollback changes the console).
 */
import type { BuildCacheClearResponse, SettingsPatch } from '@blocks2cpp/ipc-types';
import { createStore, type StoreApi } from 'zustand/vanilla';

import type { FeatureContext } from '../../app/features';
import { type FailureCode, failureCode } from './page/ipcErrors';

/** Something the page asked the backend to do. */
export type SettingsAction = 'read' | 'update' | 'clearCache';

/** The page's own state, kept while the page is closed. */
export interface SettingsPageState {
  /** How many updates are on their way. */
  saving: number;
  /** Whether the build cache is being cleared. */
  clearing: boolean;
  /** What the last *Clear build cache* did, or `null`. */
  cacheResult: BuildCacheClearResponse | null;
  /** The last failure, or `null` (cleared by the next success of the same action). */
  error: { action: SettingsAction; code: FailureCode } | null;
  /** Whether the person closed the window banner about settings notices. */
  noticesDismissed: boolean;
}

/** The confirmation *Clear build cache* asks for (in the app; it is not a security decision). */
export const CLEAR_CACHE_CONFIRMATION = {
  title: 'Clear the build cache?',
  message:
    'This deletes the programs Blocks2Cpp has built and kept, except the ones in use right now. Your projects are not changed; the next build of each project just takes longer.',
  confirmLabel: 'Clear build cache',
  cancelLabel: 'Cancel',
  destructive: true,
} as const;

/** The settings feature's controller; one per installation. */
export class SettingsController {
  /** The page's state, as a store the page subscribes to. */
  readonly page: StoreApi<SettingsPageState>;

  private readonly ctx: FeatureContext;
  /** The end of the queue of reads and updates. */
  private queue: Promise<unknown> = Promise.resolve();
  private disposed = false;

  constructor(ctx: FeatureContext) {
    this.ctx = ctx;
    this.page = createStore<SettingsPageState>()(() => ({
      saving: 0,
      clearing: false,
      cacheResult: null,
      error: null,
      noticesDismissed: false,
    }));
  }

  /** Stops: answers that arrive later are ignored. */
  dispose(): void {
    this.disposed = true;
  }

  /** Reads the settings and their notices into the store (`settings_get`). */
  refresh(): Promise<boolean> {
    return this.enqueue(async () => {
      try {
        const response = await this.ctx.ipc.settingsGet();
        if (!this.isDisposed()) {
          this.ctx.store.getState().actions.setSettings({
            value: response.settings,
            notices: response.notices,
          });
          this.clearError('read');
        }
        return true;
      } catch (error: unknown) {
        this.fail('read', error);
        return false;
      }
    });
  }

  /**
   * Changes some settings (`settings_update`) and puts the settings the backend answers with
   * into the store. Resolves `false` when the change was refused or could not be saved; the
   * settings then stay as they were.
   */
  update(patch: SettingsPatch): Promise<boolean> {
    this.page.setState((state) => ({ saving: state.saving + 1 }));
    return this.enqueue(async () => {
      try {
        const response = await this.ctx.ipc.settingsUpdate(patch);
        if (!this.isDisposed()) {
          this.ctx.store.getState().actions.setSettings({ value: response.settings });
          this.clearError('update');
        }
        return true;
      } catch (error: unknown) {
        this.fail('update', error);
        return false;
      } finally {
        this.page.setState((state) => ({ saving: Math.max(0, state.saving - 1) }));
      }
    });
  }

  /**
   * *Clear build cache*: asks for confirmation in the app, then clears (`build_cache_clear`).
   * Resolves `false` when it was cancelled or failed.
   */
  async clearBuildCache(): Promise<boolean> {
    if (this.page.getState().clearing || this.disposed) {
      return false;
    }
    this.page.setState({ clearing: true });
    try {
      const confirmed = await this.ctx.dialogs.confirm({ ...CLEAR_CACHE_CONFIRMATION });
      if (!confirmed || this.isDisposed()) {
        return false;
      }
      const result = await this.ctx.ipc.buildCacheClear();
      this.page.setState({ cacheResult: result });
      this.clearError('clearCache');
      return true;
    } catch (error: unknown) {
      this.page.setState({ cacheResult: null });
      this.fail('clearCache', error);
      return false;
    } finally {
      this.page.setState({ clearing: false });
    }
  }

  /** Whether {@link dispose} was called (a method, so checks after an `await` are not narrowed). */
  private isDisposed(): boolean {
    return this.disposed;
  }

  /** Closes the window banner about settings notices for this session. */
  dismissNotices(): void {
    this.page.setState({ noticesDismissed: true });
  }

  /** Runs `task` after everything queued before it. */
  private enqueue<T>(task: () => Promise<T>): Promise<T> {
    const run = this.queue.then(task, task);
    this.queue = run.catch(() => undefined);
    return run;
  }

  private fail(action: SettingsAction, error: unknown): void {
    const code = failureCode(error);
    console.error(`The settings action ${action} failed`, code);
    if (!this.disposed) {
      this.page.setState({ error: { action, code } });
    }
  }

  private clearError(action: SettingsAction): void {
    if (this.page.getState().error?.action === action) {
      this.page.setState({ error: null });
    }
  }
}

/** What to say when a settings action failed. Only fixed texts: errors carry no text. */
export function settingsFailureText(action: SettingsAction, code: FailureCode): string {
  switch (action) {
    case 'read':
      return `The settings could not be read (${code}).`;
    case 'update':
      if (code === 'invalidRequest') {
        return 'That value is not allowed, so the setting was not changed.';
      }
      if (code === 'io') {
        return 'The settings file could not be written, so the setting was not changed.';
      }
      return `The setting could not be changed (${code}).`;
    case 'clearCache':
      return `The build cache could not be cleared (${code}). Some builds may be left.`;
  }
}
