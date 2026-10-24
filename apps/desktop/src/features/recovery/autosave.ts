/**
 * Autosave (docs/spec/04-user-interface.md §4.10, 05 §5.10): while the open project has unsaved
 * changes, its document is sent to the backend as a recovery snapshot (`recovery_save`) every
 * {@link AUTOSAVE_INTERVAL_MS} and whenever the window loses focus. The backend keeps the snapshot
 * in its own recovery folder, never next to the project, and deletes it on a clean save or close;
 * after a crash the start page offers it for restore.
 *
 * - **The canonical text, made when it is written.** The preview pipeline keeps only the hash of
 *   the project's canonical text (06 §6.13), so a snapshot asks the compiler core for the text of
 *   the project's document (`canonical()`) when it is due, not per edit.
 * - **Only while dirty.** The timer runs only while `project.dirty` is set, and a snapshot of text
 *   that is already in the latest snapshot (the same text hash) is not written again.
 * - **One call at a time.** A snapshot asked for while one is being written follows it, with the
 *   text as it is then, so the backend never receives them out of order.
 * - **Never across a save or a reload.** The backend deletes the snapshot when the project is
 *   saved or reloaded, so a snapshot written after that would offer older or discarded work as
 *   unsaved after a crash, and roll the backend's latest document back. No snapshot starts while a
 *   save or reload runs (`isPaused`), and both wait for the snapshot being written
 *   ({@link Autosave.idle}) before they send the project.
 * - **Quiet failures.** Autosave runs in the background: a failure is logged by its code (once per
 *   run of the same failure, never with project content) and retried at the next tick; the user
 *   is not interrupted every 30 seconds.
 */
import { type BdmDocument, type CoreWasm, MAX_DOCUMENT_BYTES } from '@blocks2cpp/b2c-core-wasm';
import type { Handle, IpcClient } from '@blocks2cpp/ipc-types';

import type { ProjectState, useAppStore } from '../../app/store';
import { errorCode, ipcErrorOf } from './text';

/** How often a snapshot is written while the project has unsaved changes (04 §4.10). */
export const AUTOSAVE_INTERVAL_MS = 30_000;

/** Where the window's `blur` event comes from: the window itself in the app. */
export type BlurSource = Pick<Window, 'addEventListener' | 'removeEventListener'>;

/** What autosave needs. */
export interface AutosaveDeps {
  /** The app's state; the canonical text of the open project's `document` is what a snapshot holds. */
  readonly store: typeof useAppStore;
  /** The backend (only `recovery_save` is used). */
  readonly ipc: Pick<IpcClient, 'recoverySave'>;
  /**
   * The running compiler core, which writes the document as canonical text (`canonical()`), or
   * `null` while none runs: then no snapshot is written until the next tick or blur.
   */
  readonly core: () => Pick<CoreWasm, 'canonical'> | null;
  /** The window whose `blur` writes a snapshot, or `null` for none. */
  readonly window: BlurSource | null;
  /** The time between snapshots, in milliseconds ({@link AUTOSAVE_INTERVAL_MS} by default). */
  readonly intervalMs?: number;
  /**
   * Whether snapshots wait for now: while the project lifecycle saves or reloads (the backend then
   * deletes the snapshot). A snapshot asked for meanwhile is not written; the next tick or blur writes it if
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

/** The latest snapshot the backend accepted: its project, document and the hash of its text. */
interface Written {
  readonly handle: Handle;
  /** The document it was written from (the store replaces the object whenever it changes). */
  readonly document: BdmDocument;
  /** The SHA-256 of its canonical text (the core's `textHash`). */
  readonly textHash: string;
}

/** A project's document as canonical text, to write as a snapshot, with the text's hash. */
interface SnapshotText {
  readonly text: string;
  readonly textHash: string;
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

/**
 * Whether two project states are the same save of the same project: the same handle, saved at the
 * same time with the same text (the same hash). A change of any of them (a save, which deletes the
 * snapshot, or another project) means the latest snapshot no longer counts as written.
 *
 * It compares three short fields, so the check costs nothing per store update even for a large
 * project.
 */
function sameSave(a: ProjectState | null, b: ProjectState | null): boolean {
  if (a === b) {
    return true;
  }
  return (
    a !== null &&
    b !== null &&
    a.handle === b.handle &&
    a.savedAt === b.savedAt &&
    a.savedTextHash === b.savedTextHash
  );
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

  /** Logs a failure by its code, once per run of the same failure. */
  function failed(code: string, message: string): void {
    if (code !== lastFailure) {
      console.warn(message, code);
    }
    lastFailure = code;
  }

  async function write(
    handle: Handle,
    document: BdmDocument,
    snapshot: SnapshotText,
  ): Promise<void> {
    try {
      await deps.ipc.recoverySave({ handle, document: snapshot.text });
      written = { handle, document, textHash: snapshot.textHash };
      lastFailure = null;
    } catch (error: unknown) {
      const code = errorCode(error);
      // `unknownHandle`: the project was closed meanwhile, and its snapshot with it.
      if (ipcErrorOf(error)?.code === 'unknownHandle') {
        lastFailure = code;
      } else {
        failed(code, 'Autosave could not write a recovery snapshot');
      }
    }
  }

  /**
   * The canonical text of `document` from the compiler core, or `null` (logged by code) when no
   * core runs or it cannot write the document.
   */
  function canonicalText(document: BdmDocument): SnapshotText | null {
    const core = deps.core();
    if (core === null) {
      failed('noCore', 'Autosave has no compiler core to write the snapshot with');
      return null;
    }
    try {
      const result = core.canonical(JSON.stringify(document));
      if (result.ok) {
        return { text: result.text, textHash: result.textHash };
      }
      // The store only holds documents the loader accepted (the pipeline commits only those, and
      // the editor stays on a module whose canvas does not load): a bug.
      failed(
        result.diagnostics.map((diagnostic) => diagnostic.code).join(', '),
        'Autosave could not write the project as a snapshot',
      );
    } catch (error: unknown) {
      // The core stopped (`CoreTrap`) or failed: logged by the error's name, never its text.
      failed(
        error instanceof Error ? error.name : typeof error,
        'Autosave could not write the project as a snapshot',
      );
    }
    return null;
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
    const latest = written?.handle === project.handle ? written : null;
    if (latest?.document === project.document) {
      return Promise.resolve();
    }
    const snapshot = canonicalText(project.document);
    if (snapshot === null) {
      return Promise.resolve();
    }
    if (latest?.textHash === snapshot.textHash) {
      // Another document object with the same text: nothing new to write.
      written = { ...latest, document: project.document };
      return Promise.resolve();
    }
    if (exceedsDocumentLimit(snapshot.text)) {
      failed('tooLarge', 'Autosave skipped a project larger than a project file may be');
      return Promise.resolve();
    }
    const current: Promise<void> = write(project.handle, project.document, snapshot).finally(() => {
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
    if (project === previous.project) {
      // Another slice changed (the analysis, the user interface, a build or run).
      return;
    }
    if (!sameSave(project, previous.project) || project?.dirty !== true) {
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
