/**
 * What the block editor says to screen readers (docs/spec/04-user-interface.md §4.8): a polite
 * live region for what the keys do (where the cursor is, where a moved block would go), and the
 * description of the canvas's keys. Both live in a visually hidden element inside Blockly's
 * injection `div`, so they go away with the workspace.
 *
 * Full announcements of every block (its whole textual form) come in M5; in M2 the editor names
 * the block, field or place the keyboard reaches.
 */

/** The longest announcement, in UTF-16 code units; longer text is cut with `…`. */
export const MAX_ANNOUNCEMENT_CHARS = 300;

/** The class of the hidden container (styled in ./keyboard.css). */
export const ANNOUNCER_CLASS = 'b2c-keyboard-a11y';

let nextId = 1;

/** A hidden description and a polite live region for one workspace. */
export class Announcer {
  /** The ID of the element holding the canvas's description (for `aria-describedby`). */
  readonly descriptionId: string;
  /** The ID of the element holding the toolbox blocks' description. */
  readonly flyoutDescriptionId: string;
  private readonly container: HTMLElement;
  private readonly status: HTMLElement;
  /** Alternates, so that the same text twice in a row is still announced twice. */
  private toggle = false;

  /**
   * Adds the hidden elements to `parent` (Blockly's injection `div`), with the canvas's and the
   * flyout's descriptions.
   */
  constructor(parent: HTMLElement, descriptions: { canvas: string; flyout: string }) {
    const document = parent.ownerDocument;
    const id = nextId++;
    this.descriptionId = `b2c-canvas-keys-${String(id)}`;
    this.flyoutDescriptionId = `b2c-flyout-keys-${String(id)}`;

    this.container = document.createElement('div');
    this.container.className = ANNOUNCER_CLASS;
    this.container.dataset['testid'] = 'keyboard-announcer';

    const canvas = document.createElement('p');
    canvas.id = this.descriptionId;
    canvas.textContent = descriptions.canvas;
    const flyout = document.createElement('p');
    flyout.id = this.flyoutDescriptionId;
    flyout.textContent = descriptions.flyout;

    this.status = document.createElement('div');
    this.status.setAttribute('role', 'status');
    this.status.setAttribute('aria-live', 'polite');
    this.status.setAttribute('aria-atomic', 'true');

    this.container.append(canvas, flyout, this.status);
    parent.append(this.container);
  }

  /** The text announced last (for tests and the manual checklist's checks). */
  get last(): string {
    return this.status.textContent.trimEnd();
  }

  /**
   * Announces `text` (React-free: it is set as plain text, never as HTML). Text that is empty after
   * trimming clears the region.
   */
  announce(text: string): void {
    const trimmed = text.trim();
    const shown =
      trimmed.length > MAX_ANNOUNCEMENT_CHARS
        ? `${trimmed.slice(0, MAX_ANNOUNCEMENT_CHARS - 1)}…`
        : trimmed;
    this.toggle = !this.toggle;
    // A trailing no-break space on every other message makes a repeated text a change.
    this.status.textContent = shown === '' ? '' : this.toggle ? shown : `${shown}\u00a0`;
  }

  /** Removes the hidden elements. */
  dispose(): void {
    this.container.remove();
  }
}
