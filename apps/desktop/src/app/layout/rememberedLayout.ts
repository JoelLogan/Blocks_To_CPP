/**
 * Remembering the editor's layout between sessions (docs/spec/04-user-interface.md §4.1,
 * 05 §5.9 `ui.layout`): the toolbox's width, the docks' sizes and whether each dock is hidden.
 *
 * The layout (./EditorLayout.tsx) takes the sizes from the settings in the app's store once, when
 * they are first there, and tells the {@link LayoutSaver} each change the user makes. The settings
 * feature registers the saver, which writes a settled change to `settings.json` at once, and the
 * steps of a series (arrow keys on a splitter, a pointer drag while it lasts) together, half a
 * second after the last one: a window closed right after a change must not lose it. With no saver
 * registered, nothing is remembered. The shell itself never writes the settings.
 */
import type { LayoutPatch } from '@blocks2cpp/ipc-types';

/**
 * Takes a change of the layout to remember (only the values that changed). `settled` is true for a
 * change that ends what the user did (a dock shown or hidden, the end of a drag, `Home` or `End`),
 * and false for a step of a series that may go on (an arrow key, a drag while it lasts).
 */
export type LayoutSaver = (change: LayoutPatch, settled: boolean) => void;

/** The registered savers, oldest first; the newest one is used. */
const savers: LayoutSaver[] = [];

/**
 * Registers `saver` as the one that remembers the layout (the newest registration wins); returns
 * the function that removes it again.
 */
export function registerLayoutSaver(saver: LayoutSaver): () => void {
  savers.push(saver);
  let registered = true;
  return () => {
    if (!registered) {
      return;
    }
    registered = false;
    const index = savers.lastIndexOf(saver);
    if (index >= 0) {
      savers.splice(index, 1);
    }
  };
}

/**
 * Hands `change` to the newest saver ({@link LayoutSaver} says what `settled` means); nothing
 * happens while none is registered.
 */
export function rememberLayout(change: LayoutPatch, settled: boolean): void {
  savers.at(-1)?.(change, settled);
}
