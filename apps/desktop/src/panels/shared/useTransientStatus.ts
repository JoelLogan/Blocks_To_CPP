/**
 * A status message after an action such as a copy (docs/spec/04-user-interface.md §4.3, §4.8): a
 * success is a short confirmation that disappears after {@link TRANSIENT_STATUS_MS} and is
 * announced again each time the action succeeds again; a failure asks the person to act and stays
 * until the next attempt. Show the text in a `role="status"` element that is always rendered, so
 * screen readers hear every change.
 */
import { useCallback, useEffect, useRef, useState } from 'react';

/** How long a success stays shown, in milliseconds (04 §4.8: about 3 s). */
export const TRANSIENT_STATUS_MS = 3000;

/**
 * How long the status stays empty before a message replaces the one shown, in milliseconds: long
 * enough for the accessibility tree to see the empty live region, so that the same text repeated
 * ("Copied" after "Copied") is announced again.
 */
export const REANNOUNCE_MS = 100;

/** A short confirmation (`success`) or a message that stays until the next attempt (`failure`). */
export type StatusKind = 'success' | 'failure';

/** The message shown. */
export interface ShownStatus {
  readonly kind: StatusKind;
  readonly text: string;
}

/** What {@link useTransientStatus} returns. */
export interface TransientStatus {
  /** The message shown now, or `null`. */
  readonly shown: ShownStatus | null;
  /** Shows `text`, replacing what is shown; a `success` disappears by itself. */
  readonly show: (kind: StatusKind, text: string) => void;
  /** Removes what is shown (such as when its dialog closes). */
  readonly clear: () => void;
}

/** See the module documentation. Nothing changes once the component has unmounted. */
export function useTransientStatus(): TransientStatus {
  const [shown, setShown] = useState<ShownStatus | null>(null);
  const current = useRef<ShownStatus | null>(null);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const mounted = useRef(false);

  const set = useCallback((next: ShownStatus | null) => {
    current.current = next;
    setShown(next);
  }, []);

  const stopTimer = useCallback(() => {
    if (timer.current !== null) {
      clearTimeout(timer.current);
      timer.current = null;
    }
  }, []);

  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
      stopTimer();
    };
  }, [stopTimer]);

  const clear = useCallback(() => {
    if (!mounted.current) {
      return;
    }
    stopTimer();
    set(null);
  }, [set, stopTimer]);

  const show = useCallback(
    (kind: StatusKind, text: string) => {
      if (!mounted.current) {
        return;
      }
      stopTimer();
      const reveal = () => {
        timer.current = null;
        set({ kind, text });
        if (kind === 'success') {
          timer.current = setTimeout(() => {
            timer.current = null;
            set(null);
          }, TRANSIENT_STATUS_MS);
        }
      };
      if (current.current === null) {
        reveal();
      } else {
        set(null);
        timer.current = setTimeout(reveal, REANNOUNCE_MS);
      }
    },
    [set, stopTimer],
  );

  return { shown, show, clear };
}
