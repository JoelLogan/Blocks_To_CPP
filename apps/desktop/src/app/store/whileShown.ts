/**
 * Selecting from the app's store for a panel that can be hidden (a dock tab that is not chosen, a
 * collapsed dock): while the panel is hidden, it keeps what it selected when it was last shown, so
 * it neither renders again nor redoes its work on every edit, and it catches up once when it is
 * shown again (docs/plans/m2-polish-plan.md P-2).
 */
import { useRef } from 'react';
import { useShallow } from 'zustand/react/shallow';

import { type AppState, useAppStore } from './index';

/**
 * `useAppStore(useShallow(selector))` while `shown`. While hidden, it gives what `selector` gave
 * when the panel was last shown (the first selection, if it has not been shown yet), so changes
 * to the store render nothing; the render that shows the panel again selects afresh. `selector`
 * returns an object or array whose entries are compared one by one (`useShallow`).
 */
export function useAppStoreWhileShown<T>(shown: boolean, selector: (state: AppState) => T): T {
  const held = useRef<{ readonly value: T } | null>(null);
  return useAppStore(
    useShallow((state: AppState) => {
      if (!shown && held.current !== null) {
        return held.current.value;
      }
      const value = selector(state);
      held.current = { value };
      return value;
    }),
  );
}
