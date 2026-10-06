/**
 * Autosave (docs/spec/04-user-interface.md §4.10, 05 §5.10): while the open project has unsaved
 * changes, its document is sent to the backend as a recovery snapshot (`recovery_save`) every
 * {@link AUTOSAVE_INTERVAL_MS} and whenever the window loses focus. The backend keeps the snapshot
 * in its own recovery folder, never next to the project, and deletes it on a clean save or close;
 * after a crash the start page offers it for restore.
 *
 * - **Only while dirty.** The timer runs only while `project.dirty` is set, and a snapshot of text
 *   that is already in the latest snapshot is not written again.
 * - **One call at a time.** A snapshot asked for while one is being written follows it, with the
 *   text as it is then, so the backend never receives them out of order.
 * - **Never across a save.** The backend deletes the snapshot when the project is saved, so a
 *   snapshot written after that would offer older work as unsaved after a crash, and roll the
 *   backend's latest document back. No snapshot starts while a save runs (`isPaused`), and a save
 *   waits for the snapshot being written ({@link Autosave.idle}) before it sends the project.
 * - **Quiet failures.** Autosave runs in the background: a failure is logged by its code (once per
 *   run of the same failure, never with project content) and retried at the next tick; the user
 *   is not interrupted every 30 seconds.
 */
import { MAX_DOCUMENT_BYTES } from '@blocks2cpp/b2c-core-wasm';
import type { Handle, IpcClient } from '@blocks2cpp/ipc-types';

import type { ProjectState, useAppStore } from '../../app/store';
import { errorCode, ipcErrorOf } from './text';

/** How often a snapshot is written while the project has unsaved changes (04 §4.10). */
export const AUTOSAVE_INTERVAL_MS = 30_000;

/** Where the window's `blur` event comes from: the window itself in the app. */
export type BlurSource = Pick<Window, 'addEventListener' | 'removeEventListener'>;

/** What autosave needs. */
export interface AutosaveDeps {
  /** The app's state; the open project's `canonicalText` is what a snapshot holds. */
  readonly store: typeof useAppStore;
  /** The backend (only `recovery_save` is used). */
  readonly ipc: Pick<IpcClient, 'recoverySave'>;
  /** The window whose `blur` writes a snapshot, or `null` for none. */
  readonly window: BlurSource | null;
  /** The time between snapshots, in milliseconds ({@link AUTOSAVE_INTERVAL_MS} by default). */
  readonly intervalMs?: number;
  /**
   * Whether snapshots wait for now: while the project lifecycle saves (its write deletes the
   * snapshot). A snapshot asked for meanwhile is not written; the next tick or blur writes it if
   * the changes are still unsaved. Never paused by default.
   */
  readonly isPaused?: () => boolean;
}

/** A running autosave. */
export interface Autosave {
  /**
   * Writes a snapshot now when the project has unsaved changes that are not in the latest one.
   * Resolves when it (and any write it had to wait for) is done; never rejects.
   */
  snapshotNow(): Promise<void>;
  /**
   * Resolves once no snapshot is being written (at once when none is), including one asked for
   * meanwhile. Never rejects. A save awaits this before it sends the project.
   */
  idle(): Promise<void>;
  /** Stops the timer and the `blur` listener; a write in progress still finishes. */
  stop(): void;
}

/** The text of the latest snapshot the backend accepted, and its project. */
interface Written {
  readonly handle: Handle;
  readonly text: string;
}

/**
 * Whether `text` is larger in UTF-8 than a project may be (05 §5.6). UTF-8 never has fewer bytes
 * than UTF-16 has code units, and at most three per unit, so only text in between is encoded.
 */
export function exceedsDocumentLimit(text: string): boolean {
  if (text.length > MAX_DOCUMENT_BYTES) {
    return true;
  }
  if (text.length * 3 <= MAX_DOCUMENT_BYTES) {
    return false;
  }
  return new TextEncoder().encode(text).length > MAX_DOCUMENT_BYTES;
}

/** What decides the timer and what counts as already written, per project state. */
function savedMarker(project: ProjectState | null): string {
  if (project === null) {
    return '';
  }
  return `${project.handle}\u0000${project.savedAt ?? ''}\u0000${project.savedCanonicalText ?? ''}`;
}

/** Starts autosave for the store's open project; see the module comment. */
export function startAutosave(deps: AutosaveDeps): Autosave {
  const intervalMs = deps.intervalMs ?? AUTOSAVE_INTERVAL_MS;
  if (!Number.isFinite(intervalMs) || intervalMs < 1) {
    throw new RangeError('the autosave interval must be a positive number of milliseconds');
  }

  let timer: ReturnType<typeof setInterval> | null = null;
  /** The project the timer runs for. */
  let timerHandle: Handle | null = null;
  let written: Written | null = null;
  /** The write in progress. */
  let writing: Promise<void> | null = null;
  /** The snapshot asked for while one was being written: it follows that one. */
  let queued: Promise<void> | null = null;
  /** The code of the last failure, so a run of the same failure is logged once. */
  let lastFailure: string | null = null;
  let stopped = false;

  function clearTimer(): void {
    if (timer !== null) {
      clearInterval(timer);
      timer = null;
    }
    timerHandle = null;
  }

  /** Runs the timer exactly while the open project has unsaved changes. */
  function follow(project: ProjectState | null): void {
    if (stopped || project?.dirty !== true) {
      clearTimer();
      return;
    }
    if (timer !== null && timerHandle === project.handle) {
      return;
    }
    clearTimer();
    timerHandle = project.handle;
    timer = setInterval(() => {
      void snapshotNow();
    }, intervalMs);
  }

  async function write(handle: Handle, text: string): Promise<void> {
    try {
      await deps.ipc.recoverySave({ handle, document: text });
      written = { handle, text };
      lastFailure = null;
    } catch (error: unknown) {
      const code = errorCode(error);
      // `unknownHandle`: the project was closed meanwhile, and its snapshot with it.
      if (ipcErrorOf(error)?.code !== 'unknownHandle' && code !== lastFailure) {
        console.warn('Autosave could not write a recovery snapshot', code);
      }
      lastFailure = code;
    }
  }

  function snapshotNow(): Promise<void> {
    if (stopped) {
      return writing ?? Promise.resolve();
    }
    if (writing !== null) {
      // Taken after the write in progress, with the text as it is then.
      queued ??= writing.then(() => {
        queued = null;
        return snapshotNow();
      });
      return queued;
    }
    const project = deps.store.getState().project;
    if (project?.dirty !== true || deps.isPaused?.() === true) {
      return Promise.resolve();
    }
    const text = project.canonicalText;
    if (written?.handle === project.handle && written.text === text) {
      return Promise.resolve();
    }
    if (exceedsDocumentLimit(text)) {
      if (lastFailure !== 'tooLarge') {
        console.warn('Autosave skipped a project larger than a project file may be');
      }
      lastFailure = 'tooLarge';
      return Promise.resolve();
    }
    const current: Promise<void> = write(project.handle, text).finally(() => {
      writing = null;
    });
    writing = current;
    return current;
  }

  async function idle(): Promise<void> {
    // A snapshot that followed the one in progress may start another write: wait for each.
    for (let current = queued ?? writing; current !== null; current = queued ?? writing) {
      await current;
    }
  }

  const onBlur = (): void => {
    void snapshotNow();
  };

  follow(deps.store.getState().project);
  const unsubscribe = deps.store.subscribe((state, previous) => {
    const project = state.project;
    if (savedMarker(project) !== savedMarker(previous.project) || project?.dirty !== true) {
      // A save (which deletes the snapshot), another project, or no unsaved changes: the next
      // snapshot is written even when its text equals the last one.
      written = null;
    }
    if (
      project?.handle !== previous.project?.handle ||
      project?.dirty !== previous.project?.dirty
    ) {
      follow(project);
    }
  });
  deps.window?.addEventListener('blur', onBlur);

  return {
    snapshotNow,
    idle,
    stop() {
      if (stopped) {
        return;
      }
      stopped = true;
      clearTimer();
      unsubscribe();
      deps.window?.removeEventListener('blur', onBlur);
    },
  };
}
