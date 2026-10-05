/**
 * Telling the backend whether the open project has unsaved changes (`project_set_dirty`,
 * docs/spec/02-architecture.md §2.6), so that closing the window asks first. The store's `dirty`
 * flag is reported whenever it changes, one call at a time and always the latest value: calls on
 * Tauri's thread pool could otherwise arrive out of order and leave the backend with a stale flag.
 */
import type { Handle, IpcClient } from '@blocks2cpp/ipc-types';

import type { useAppStore } from '../../app/store';
import { errorCode } from './errors';

interface DirtyFlag {
  readonly handle: Handle;
  readonly dirty: boolean;
}

function same(a: DirtyFlag | null, b: DirtyFlag | null): boolean {
  return a?.handle === b?.handle && a?.dirty === b?.dirty;
}

/**
 * Reports the open project's `dirty` flag to the backend until the returned function is called.
 *
 * A newly opened handle starts clean in the backend (open, new, reload and save all reset it), so
 * only a change, or a project that is dirty from the start (a restored snapshot), is sent. A
 * failed call is logged by code and not retried; the next change sends the flag again.
 */
export function reportDirtyState(store: typeof useAppStore, ipc: IpcClient): () => void {
  /** What the backend was last told (or starts with) for the current handle. */
  let known: DirtyFlag | null = null;
  /** What it should know. */
  let wanted: DirtyFlag | null = null;
  let sending = false;
  let stopped = false;

  function pump(): void {
    if (stopped || sending || wanted === null || same(known, wanted)) {
      return;
    }
    const flag = wanted;
    sending = true;
    ipc
      .projectSetDirty({ handle: flag.handle, dirty: flag.dirty })
      .catch((error: unknown) => {
        // `unknownHandle`: the project was closed meanwhile; nothing left to tell.
        console.warn('Could not report unsaved changes to the backend', errorCode(error));
      })
      .finally(() => {
        known = flag;
        sending = false;
        pump();
      });
  }

  function follow(project: ReturnType<typeof store.getState>['project']): void {
    if (project === null) {
      wanted = null;
      return;
    }
    if (wanted?.handle !== project.handle) {
      // A new handle: the backend starts with "no unsaved changes".
      known = { handle: project.handle, dirty: false };
    }
    wanted = { handle: project.handle, dirty: project.dirty };
    pump();
  }

  follow(store.getState().project);
  const unsubscribe = store.subscribe((state, previous) => {
    if (
      state.project?.handle !== previous.project?.handle ||
      state.project?.dirty !== previous.project?.dirty
    ) {
      follow(state.project);
    }
  });
  return () => {
    stopped = true;
    unsubscribe();
  };
}
