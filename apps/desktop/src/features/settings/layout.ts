/**
 * Remembering the editor's layout (docs/spec/04-user-interface.md §4.1, 05 §5.9 `ui.layout`): the
 * layout hands each change the user makes to the saver this feature registers
 * (app/layout/rememberedLayout.ts), and a {@link LayoutWriter} writes it with one
 * `settings_update`. A settled change (a dock shown or hidden, the end of a splitter drag, `Home`
 * or `End`) is written at once, with any steps still waiting; the steps of a series (arrow keys, a
 * drag while it lasts) are put together and written {@link LAYOUT_WRITE_DELAY_MS} after the last
 * one, so that a drag writes `settings.json` once rather than at every step.
 *
 * Writing settled changes at once matters because nothing is written when the window closes: the
 * backend lets a window without unsaved changes close straight away, and a waiting write is lost
 * with the page.
 */
import type { LayoutPatch } from '@blocks2cpp/ipc-types';

/** How long after the last step of a series the layout is written, in milliseconds (05 §5.9). */
export const LAYOUT_WRITE_DELAY_MS = 500;

/**
 * Collects changes of the layout and writes them together: at once for a settled change, and
 * otherwise once none came for {@link LAYOUT_WRITE_DELAY_MS}. A later value of a key replaces an
 * earlier one.
 */
export class LayoutWriter {
  private readonly write: (layout: LayoutPatch) => void;
  private readonly delayMs: number;
  /** The changes not written yet, or `null` when there are none. */
  private pending: LayoutPatch | null = null;
  private timer: ReturnType<typeof setTimeout> | null = null;

  /** `write` is called with the changes put together. */
  constructor(write: (layout: LayoutPatch) => void, delayMs: number = LAYOUT_WRITE_DELAY_MS) {
    this.write = write;
    this.delayMs = delayMs;
  }

  /**
   * Adds `change` to the next write: writes at once when it is `settled`, and otherwise starts the
   * wait for the write again.
   */
  readonly change = (change: LayoutPatch, settled: boolean): void => {
    this.pending = { ...this.pending, ...change };
    if (settled) {
      this.flush();
      return;
    }
    if (this.timer !== null) {
      clearTimeout(this.timer);
    }
    this.timer = setTimeout(this.flush, this.delayMs);
  };

  /** Writes the changes not written yet at once (nothing when there are none). */
  readonly flush = (): void => {
    if (this.timer !== null) {
      clearTimeout(this.timer);
      this.timer = null;
    }
    const layout = this.pending;
    this.pending = null;
    if (layout !== null) {
      this.write(layout);
    }
  };
}
