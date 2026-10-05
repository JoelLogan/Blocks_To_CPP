/**
 * The app's in-window dialogs (docs/spec/04-user-interface.md §4.8): alerts, confirmations, text
 * prompts and choices such as *Save*, *Don't save* or *Cancel*. Requests wait in a queue and are
 * shown one at a time by `DialogHost`, which renders them with Radix's accessible, focus-trapped
 * dialog. Every request settles exactly once; dismissing a dialog (Escape, or a click outside it)
 * gives its cancel result.
 *
 * Text is always rendered as React text, never as HTML.
 */
import { createStore, type StoreApi } from 'zustand/vanilla';

/** The most requests that can wait at once; more are cancelled at once (and logged). */
export const MAX_PENDING_DIALOGS = 8;

/** The default and the largest number of characters a prompt accepts. */
export const DEFAULT_PROMPT_MAX_LENGTH = 1000;

/** What every dialog shows. */
export interface DialogTextOptions {
  /** The heading. Without one, the message is the heading. */
  title?: string;
  /** What the dialog says or asks. */
  message: string;
  /**
   * Called with the dialog's element once it is open; the returned function, if any, is called
   * when it closes. Lets the Blockly workspace hand its keyboard focus to the dialog.
   */
  onOpen?: (content: HTMLElement) => (() => void) | undefined;
}

/** An alert: a message and an OK button. */
export interface AlertOptions extends DialogTextOptions {
  okLabel?: string;
}

/** A yes-or-no question. */
export interface ConfirmOptions extends DialogTextOptions {
  confirmLabel?: string;
  cancelLabel?: string;
  /** The confirm action loses data: the cancel button gets the initial focus. */
  destructive?: boolean;
}

/** A question answered with a line of text. */
export interface PromptOptions extends DialogTextOptions {
  /** The text the field starts with (selected, so typing replaces it). */
  defaultValue?: string;
  okLabel?: string;
  cancelLabel?: string;
  /** At most this many characters (at most {@link DEFAULT_PROMPT_MAX_LENGTH}). */
  maxLength?: number;
  /** Returns why `value` cannot be accepted, or `null` when it can. */
  validate?: (value: string) => string | null;
}

/** One button of a choice dialog. */
export interface DialogChoice<T extends string> {
  id: T;
  label: string;
  /** The suggested action: it gets the initial focus and the primary style. */
  primary?: boolean;
  /** The action loses data: it gets the warning style. */
  destructive?: boolean;
}

/** A question with several answers, such as *Save*, *Don't save* or *Cancel*. */
export interface ChoiceOptions<T extends string> extends DialogTextOptions {
  /** The buttons, in display order. */
  choices: readonly DialogChoice<T>[];
  /** The answer when the dialog is dismissed; it must be one of the choices. */
  cancel: NoInfer<T>;
}

/** The in-window dialogs every feature uses. */
export interface DialogService {
  /** Shows a message; resolves when it is closed. */
  alert(options: AlertOptions): Promise<void>;
  /** Asks a yes-or-no question; resolves `true` for yes, `false` for no or dismissal. */
  confirm(options: ConfirmOptions): Promise<boolean>;
  /** Asks for a line of text; resolves with it, or `null` when cancelled or dismissed. */
  prompt(options: PromptOptions): Promise<string | null>;
  /** Asks a question with several answers; resolves with the chosen ID. */
  choose<T extends string>(options: ChoiceOptions<T>): Promise<T>;
}

/** A request waiting in the queue or being shown. */
export type DialogRequest =
  | { kind: 'alert'; id: number; options: AlertOptions; settle: () => void }
  | { kind: 'confirm'; id: number; options: ConfirmOptions; settle: (result: boolean) => void }
  | {
      kind: 'prompt';
      id: number;
      options: PromptOptions;
      settle: (result: string | null) => void;
    }
  | {
      kind: 'choice';
      id: number;
      options: ChoiceOptions<string>;
      settle: (result: string) => void;
    };

/** The queue's state: the first request is the one on screen. */
export interface DialogQueueState {
  queue: readonly DialogRequest[];
}

/** The dialog service plus the queue `DialogHost` renders. */
export interface DialogQueue extends DialogService {
  /** The queue; `DialogHost` subscribes to it. */
  readonly store: StoreApi<DialogQueueState>;
  /** Settles every request with its cancel result and empties the queue. */
  cancelAll(): void;
}

/** Settles `request` with the result it gets when it is dismissed. */
export function settleCancelled(request: DialogRequest): void {
  switch (request.kind) {
    case 'alert':
      request.settle();
      break;
    case 'confirm':
      request.settle(false);
      break;
    case 'prompt':
      request.settle(null);
      break;
    case 'choice':
      request.settle(request.options.cancel);
      break;
  }
}

/** Creates an empty queue. */
export function createDialogQueue(): DialogQueue {
  const store = createStore<DialogQueueState>()(() => ({ queue: [] }));
  let nextId = 1;

  /**
   * Adds a request. Its `settle` calls `done`, which removes it from the queue and returns whether
   * this is the first settlement (later ones are ignored).
   */
  function enqueue(make: (id: number, done: () => boolean) => DialogRequest): void {
    const id = nextId++;
    let settled = false;
    const request = make(id, () => {
      if (settled) {
        return false;
      }
      settled = true;
      store.setState((state) => ({ queue: state.queue.filter((queued) => queued.id !== id) }));
      return true;
    });
    if (store.getState().queue.length >= MAX_PENDING_DIALOGS) {
      console.warn(`Too many dialogs at once; "${request.kind}" was cancelled`);
      settleCancelled(request);
      return;
    }
    store.setState((state) => ({ queue: [...state.queue, request] }));
  }

  return {
    store,

    alert(options) {
      return new Promise<void>((resolve) => {
        enqueue((id, done) => ({
          kind: 'alert',
          id,
          options,
          settle: () => {
            if (done()) {
              resolve();
            }
          },
        }));
      });
    },

    confirm(options) {
      return new Promise<boolean>((resolve) => {
        enqueue((id, done) => ({
          kind: 'confirm',
          id,
          options,
          settle: (result) => {
            if (done()) {
              resolve(result);
            }
          },
        }));
      });
    },

    prompt(options) {
      return new Promise<string | null>((resolve) => {
        enqueue((id, done) => ({
          kind: 'prompt',
          id,
          options,
          settle: (result) => {
            if (done()) {
              resolve(result);
            }
          },
        }));
      });
    },

    choose<T extends string>(options: ChoiceOptions<T>) {
      if (!options.choices.some((choice) => choice.id === options.cancel)) {
        return Promise.reject(
          new Error('the cancel answer of a choice must be one of its choices'),
        );
      }
      return new Promise<T>((resolve) => {
        enqueue((id, done) => ({
          kind: 'choice',
          id,
          options,
          settle: (result) => {
            if (done()) {
              resolve(result as T);
            }
          },
        }));
      });
    },

    cancelAll() {
      for (const request of store.getState().queue) {
        settleCancelled(request);
      }
    },
  };
}
