/**
 * Selecting from the app's store for a panel that can be hidden (a dock tab that is not chosen, a
 * collapsed dock): while the panel is hidden, it keeps what it selected when it was last shown, so
 * it neither renders again nor redoes its work on every edit, and it catches up once when it is
 * shown again (docs/plans/m2-polish-plan.md P-2). What it keeps belongs to one project: another
 * project, or none, is selected afresh even while the panel is hidden.
 */
import type { Handle } from '@blocks2cpp/ipc-types';
import { useRef } from 'react';
import { useShallow } from 'zustand/react/shallow';

import { type AppState, useAppStore } from './index';

/** What a hidden panel keeps: its last selection, and the project it was made for. */
interface Held<T> {
  readonly value: T;
  readonly project: Handle | null;
}

/**
 * `useAppStore(useShallow(selector))` while `shown`. While hidden, it gives what `selector` gave
 * when the panel was last shown (the first selection, if it has not been shown yet), so changes
 * to the store render nothing; the render that shows the panel again selects afresh.
 *
 * A change of project (another one opened, or the project closed) is selected afresh even while
 * hidden: the panel then shows the new project's state at once (an empty one, as the store resets
 * it), so it drops what it held of the old project (a code view with its selection and scroll
 * position, sorted and expanded problem rows) as it did before panels were held, and it follows
 * that project from then on. `selector` returns an object or array whose entries are compared one
 * by one (`useShallow`).
 */
export function useAppStoreWhileShown<T>(shown: boolean, selector: (state: AppState) => T): T {
  const held = useRef<Held<T> | null>(null);
  return useAppStore(
    useShallow((state: AppState) => {
      const project = state.project?.handle ?? null;
      if (!shown && held.current !== null && held.current.project === project) {
        return held.current.value;
      }
      const value = selector(state);
      held.current = { value, project };
      return value;
    }),
  );
}
