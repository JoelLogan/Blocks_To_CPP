/**
 * Copying plain text from a panel (the code panel's *Copy all* and *Copy selection*, a program's
 * link). Only `text/plain` is written; the app has no clipboard plugin and no backend command for
 * it (M2 decision "Clipboard transport and API").
 */

/** The most text one copy writes (UTF-16 code units): 32 MiB, the largest document (05 §5.6). */
export const MAX_COPY_CHARS = 32 * 1024 * 1024;

/**
 * Writes `text` to the system clipboard as `text/plain`. Call it from a user action (a click or a
 * key press): browsers refuse clipboard writes at other times.
 *
 * It uses the asynchronous Clipboard API and falls back to a one-off DOM `copy` event, which the
 * webviews allow during a user action. The fallback's listener runs first and stops the event, so
 * the editor's own copy handling (blocks as `application/x-blocks2cpp+json`) never sees it.
 *
 * @returns whether the text was written. Text over {@link MAX_COPY_CHARS} is refused.
 */
export async function copyPlainText(text: string): Promise<boolean> {
  if (text.length > MAX_COPY_CHARS) {
    return false;
  }
  // `navigator.clipboard` is missing outside secure contexts, whatever the DOM typings say.
  const clipboard = navigator.clipboard as Clipboard | undefined;
  if (clipboard !== undefined) {
    try {
      await clipboard.writeText(text);
      return true;
    } catch {
      // Denied (no user activation or no permission): try the copy event below.
    }
  }
  return copyWithCopyEvent(text);
}

/** The DOM `copy` event route: `document.execCommand('copy')` with a listener that sets the data. */
function copyWithCopyEvent(text: string): boolean {
  let written = false;
  const onCopy = (event: ClipboardEvent) => {
    if (event.clipboardData === null) {
      return;
    }
    event.clipboardData.setData('text/plain', text);
    event.preventDefault();
    event.stopImmediatePropagation();
    written = true;
  };
  document.addEventListener('copy', onCopy, { capture: true });
  try {
    // `execCommand` is deprecated, but it is the only synchronous way to fill a DataTransfer.
    // eslint-disable-next-line @typescript-eslint/no-deprecated
    const accepted = typeof document.execCommand === 'function' && document.execCommand('copy');
    return accepted && written;
  } catch {
    return false;
  } finally {
    document.removeEventListener('copy', onCopy, { capture: true });
  }
}
