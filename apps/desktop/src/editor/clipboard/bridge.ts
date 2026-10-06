/**
 * The DOM side of the clipboard (docs/spec/05-project-format.md §5.12): the webview's own `copy`,
 * `cut` and `paste` events, whose `DataTransfer` is the only way to put a custom type on the
 * system clipboard without a clipboard plugin.
 *
 * - **Keyboard** (`Ctrl+C`, `Ctrl+X`, `Ctrl+V` on the canvas): the editor's shortcut does the work
 *   during the key press (the copy is made, or the paste target noted) and *arms* the bridge
 *   without cancelling the key, so the webview goes on to fire its `copy`, `cut` or `paste` event
 *   in the same task. The armed copy is written into that event; the armed paste reads the custom
 *   type from it. The matching `beforecopy`, `beforecut` and `beforepaste` events are cancelled
 *   while armed, which is how WebKit enables the commands when no text is selected. An engine that
 *   fires no event leaves the system clipboard as it was: a paste then falls back to the in-app
 *   copy after one task, and a copy is still kept in the app.
 * - **Menus and commands** (no key press): {@link ClipboardBridge.writeNow} fires a one-off `copy`
 *   event with `document.execCommand('copy')` and writes into it; a paste can only use the in-app
 *   copy, since nothing may read the system clipboard outside a paste event.
 * - **Other events aimed at the canvas** (an engine's own menu, for example) copy, cut or paste at
 *   the focus. Events aimed anywhere else (the code panel, the console, a text field) are left
 *   alone.
 *
 * The listeners sit on the document in the capture phase, so no other handler sees an event the
 * editor handles.
 */
import { type ClipboardData, readTransfer, writeTransfer } from './formats';

/** Runs `run` after the current task; returns the function that cancels it. */
export type Scheduler = (run: () => void) => () => void;

/** The default {@link Scheduler}: a zero-delay timer. */
export const nextTask: Scheduler = (run) => {
  const timer = setTimeout(run, 0);
  return () => {
    clearTimeout(timer);
  };
};

/** What a bridge needs from the editor. */
export interface ClipboardBridgeOptions {
  /** The document the editor is in. */
  readonly document: Document;
  /** The canvas element (the workspace's injection div), or `null` while there is none. */
  readonly container: () => Element | null;
  /**
   * A copy or cut event aimed at the canvas that no key press armed: copies (for a cut, also
   * deletes) what has the focus. Returns what to write, or `null` to leave the event alone.
   */
  readonly copyFocused: (cut: boolean) => ClipboardData | null;
  /** A paste event aimed at the canvas that no key press armed: pastes `payload` at the focus. */
  readonly pasteFocused: (payload: string | null) => void;
  /** Runs fallbacks after the current task; {@link nextTask} by default. */
  readonly schedule?: Scheduler;
}

/** Pastes a payload read from a paste event, or with `null` the in-app copy. */
export type PasteRun = (payload: string | null) => void;

/** Whether `node` is (inside) an element that edits text: there, the webview's own copy applies. */
function isTextEditing(node: Node | null): boolean {
  const element = node instanceof Element ? node : (node?.parentElement ?? null);
  if (element === null) {
    return false;
  }
  if (element.closest('input, textarea, select') !== null) {
    return true;
  }
  return element instanceof HTMLElement && element.isContentEditable;
}

/** The DOM clipboard events of one editor (see the module comment). */
export class ClipboardBridge {
  private readonly options: ClipboardBridgeOptions;
  private readonly schedule: Scheduler;
  private pendingWrite: ClipboardData | null = null;
  private cancelWriteTimer: (() => void) | null = null;
  private pendingPaste: PasteRun | null = null;
  private cancelPasteTimer: (() => void) | null = null;
  /** Whether the last armed write went into an event. */
  private wrote = false;
  private disposed = false;

  constructor(options: ClipboardBridgeOptions) {
    this.options = options;
    this.schedule = options.schedule ?? nextTask;
    const doc = options.document;
    doc.addEventListener('beforecopy', this.onBefore, { capture: true });
    doc.addEventListener('beforecut', this.onBefore, { capture: true });
    doc.addEventListener('beforepaste', this.onBefore, { capture: true });
    doc.addEventListener('copy', this.onCopyOrCut, { capture: true });
    doc.addEventListener('cut', this.onCopyOrCut, { capture: true });
    doc.addEventListener('paste', this.onPaste, { capture: true });
  }

  /** Whether a key press armed a copy or a paste that has not happened yet. */
  get armed(): boolean {
    return this.pendingWrite !== null || this.pendingPaste !== null;
  }

  /**
   * Writes `data` into the next `copy` or `cut` event of this task (see the module comment). Until
   * then, or until the task ends, other events are not handled.
   */
  armWrite(data: ClipboardData): void {
    if (this.disposed) {
      return;
    }
    this.disarmWrite();
    this.pendingWrite = data;
    this.wrote = false;
    this.cancelWriteTimer = this.schedule(() => {
      this.cancelWriteTimer = null;
      this.pendingWrite = null;
    });
  }

  /**
   * Pastes with `run` at the next `paste` event of this task, giving it the event's payload (or
   * `null` when the event has none); when no paste event comes, `run(null)` pastes the in-app copy
   * after the task. An earlier paste still waiting runs its fallback first.
   */
  armPaste(run: PasteRun): void {
    if (this.disposed) {
      return;
    }
    this.flushPaste();
    this.pendingPaste = run;
    this.cancelPasteTimer = this.schedule(() => {
      this.cancelPasteTimer = null;
      const waiting = this.pendingPaste;
      this.pendingPaste = null;
      waiting?.(null);
    });
  }

  /**
   * Puts `data` on the system clipboard from a menu or command, through a one-off `copy` event.
   * Call it during a user action: webviews refuse clipboard writes at other times.
   *
   * @returns whether the data was written; the in-app copy is the fallback either way.
   */
  writeNow(data: ClipboardData): boolean {
    if (this.disposed) {
      return false;
    }
    this.armWrite(data);
    let accepted = false;
    try {
      // `execCommand` is deprecated, but it is the only synchronous way to get a copy event's
      // DataTransfer outside a key press (see also panels/shared/clipboard.ts).
      const doc = this.options.document;
      // eslint-disable-next-line @typescript-eslint/no-deprecated
      accepted = typeof doc.execCommand === 'function' && doc.execCommand('copy');
    } catch (error: unknown) {
      console.warn('The webview refused to copy', error);
    }
    const written = this.wrote;
    this.disarmWrite();
    return accepted && written;
  }

  /** Stops listening and forgets anything armed (a waiting paste does not run). */
  dispose(): void {
    if (this.disposed) {
      return;
    }
    this.disposed = true;
    this.disarmWrite();
    this.cancelPasteTimer?.();
    this.cancelPasteTimer = null;
    this.pendingPaste = null;
    const doc = this.options.document;
    doc.removeEventListener('beforecopy', this.onBefore, { capture: true });
    doc.removeEventListener('beforecut', this.onBefore, { capture: true });
    doc.removeEventListener('beforepaste', this.onBefore, { capture: true });
    doc.removeEventListener('copy', this.onCopyOrCut, { capture: true });
    doc.removeEventListener('cut', this.onCopyOrCut, { capture: true });
    doc.removeEventListener('paste', this.onPaste, { capture: true });
  }

  private disarmWrite(): void {
    this.cancelWriteTimer?.();
    this.cancelWriteTimer = null;
    this.pendingWrite = null;
  }

  /** Runs a waiting paste's fallback now. */
  private flushPaste(): void {
    const waiting = this.pendingPaste;
    if (waiting === null) {
      return;
    }
    this.cancelPasteTimer?.();
    this.cancelPasteTimer = null;
    this.pendingPaste = null;
    waiting(null);
  }

  /** Whether an event that no key press armed is aimed at the canvas (and not at a text field). */
  private aimedAtCanvas(event: Event): boolean {
    const container = this.options.container();
    if (container === null) {
      return false;
    }
    const doc = this.options.document;
    const target = event.target instanceof Node ? event.target : null;
    // An engine without a selection may aim the event at the body: the focus says where it is.
    const unaimed =
      target === null || target === doc || target === doc.body || target === doc.documentElement;
    const aimed = unaimed ? doc.activeElement : target;
    return aimed !== null && !isTextEditing(aimed) && container.contains(aimed);
  }

  /** Writes `data` into the event and keeps the webview and other handlers from touching it. */
  private write(event: ClipboardEvent, data: ClipboardData): void {
    const transfer = event.clipboardData;
    if (transfer === null) {
      return;
    }
    writeTransfer(transfer, data);
    event.preventDefault();
    event.stopPropagation();
    this.wrote = true;
  }

  private readonly onBefore = (event: Event): void => {
    const armed = event.type === 'beforepaste' ? this.pendingPaste : this.pendingWrite;
    if (armed !== null) {
      // Tells WebKit (and Chromium) that the page handles the command, which enables it.
      event.preventDefault();
    }
  };

  private readonly onCopyOrCut = (event: Event): void => {
    if (!(event instanceof ClipboardEvent)) {
      return;
    }
    const armed = this.pendingWrite;
    if (armed !== null) {
      this.disarmWrite();
      this.write(event, armed);
      return;
    }
    if (event.defaultPrevented || !this.aimedAtCanvas(event)) {
      return;
    }
    const data = this.options.copyFocused(event.type === 'cut');
    if (data !== null) {
      this.write(event, data);
    }
  };

  private readonly onPaste = (event: Event): void => {
    if (!(event instanceof ClipboardEvent)) {
      return;
    }
    const armed = this.pendingPaste;
    if (armed !== null) {
      this.cancelPasteTimer?.();
      this.cancelPasteTimer = null;
      this.pendingPaste = null;
      event.preventDefault();
      event.stopPropagation();
      armed(readTransfer(event.clipboardData));
      return;
    }
    if (event.defaultPrevented || !this.aimedAtCanvas(event)) {
      return;
    }
    event.preventDefault();
    event.stopPropagation();
    this.options.pasteFocused(readTransfer(event.clipboardData));
  };
}
