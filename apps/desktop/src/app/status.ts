/**
 * The window's status region (docs/spec/04-user-interface.md §4.1, §4.8;
 * docs/plans/m3-contracts/w3-modules.md §2.11): one polite live region for the whole app, in the
 * status bar (./layout/StatusBar.tsx), where a feature says the outcome of what the person just did
 * (the module edits: *Module “utils” added.*). A message may carry one action, a button beside it
 * (*Undo delete*). A new message replaces the one shown; the same text said again is read out again
 * (each message has its own serial).
 *
 * Messages say what happened, and nothing a person must answer: a dialog asks, a banner warns.
 */
import { createStore, type StoreApi } from 'zustand/vanilla';

/** A button beside a message, which acts on what the message is about. */
export interface StatusAction {
  /** The button's words (*Undo delete*). */
  readonly label: string;
  /** What the button does. */
  readonly run: () => void;
  /** The button's `data-testid`, for the end-to-end flows. */
  readonly testId?: string;
}

/** The message the region shows. */
export interface StatusMessage {
  readonly text: string;
  /** Counts the messages, so that the same text said twice is read out twice. */
  readonly serial: number;
  /** The button beside the message, or `null`. */
  readonly action: StatusAction | null;
}

/** What the region shows: the last message, or `null` for none. */
export interface StatusRegionState {
  readonly message: StatusMessage | null;
}

/** The window's status region; see the module comment. */
export interface StatusRegion {
  /** What the region shows; the status bar reads it. */
  readonly state: StoreApi<StatusRegionState>;
  /** Shows `text` (with `action` beside it) in place of the last message; returns its serial. */
  show(text: string, action?: StatusAction | null): number;
  /**
   * Takes the message away: the message `serial` only while it is still shown (a feature takes
   * back only its own message), or whatever is shown without `serial`.
   */
  clear(serial?: number): void;
}

/** Creates an empty status region (the app has one, {@link appStatus}; tests make their own). */
export function createStatusRegion(): StatusRegion {
  const state = createStore<StatusRegionState>(() => ({ message: null }));
  let serial = 0;
  return {
    state,
    show(text, action = null) {
      serial += 1;
      state.setState({ message: { text, serial, action } });
      return serial;
    },
    clear(which) {
      const shown = state.getState().message;
      if (shown !== null && (which === undefined || shown.serial === which)) {
        state.setState({ message: null });
      }
    },
  };
}

/** The window's status region. */
export const appStatus: StatusRegion = createStatusRegion();
