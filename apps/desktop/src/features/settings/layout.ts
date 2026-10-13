/**
 * Remembering the editor's layout (docs/spec/04-user-interface.md §4.1, 05 §5.9 `ui.layout`): the
 * layout hands each change the user makes to the saver this feature registers
 * (app/layout/rememberedLayout.ts), and a {@link LayoutWriter} puts the changes together and
 * writes them with one `settings_update` {@link LAYOUT_WRITE_DELAY_MS} after the last one, so that
 * dragging a splitter writes `settings.json` once rather than at every step.
 */
import type { LayoutPatch } from '@blocks2cpp/ipc-types';

/** How long after the last change of the layout it is written, in milliseconds (05 §5.9). */
export const LAYOUT_WRITE_DELAY_MS = 500;

/**
 * Collects changes of the layout and writes them together once none came for
 * {@link LAYOUT_WRITE_DELAY_MS}; a later value of a key replaces an earlier one.
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

  /** Adds `change` to the next write and starts the wait for it again. */
  readonly change = (change: LayoutPatch): void => {
    this.pending = { ...this.pending, ...change };
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
