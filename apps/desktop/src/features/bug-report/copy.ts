/**
 * Puts text on the system clipboard from a user action, as the editor's clipboard does
 * (../../editor/clipboard/bridge.ts): through a `copy` event of the page, whose data the page may
 * set, then, if the webview fires none, `navigator.clipboard`. Nothing is read from the clipboard.
 */

/** Copies `text`; resolves whether it was put on the clipboard. */
export async function copyText(text: string, doc: Document = document): Promise<boolean> {
  const result = { copied: false };
  const listener = (event: ClipboardEvent): void => {
    if (event.clipboardData !== null) {
      event.clipboardData.setData('text/plain', text);
      event.preventDefault();
      event.stopImmediatePropagation();
      result.copied = true;
    }
  };
  doc.addEventListener('copy', listener, true);
  try {
    // A deprecated command, but the one way to fire a trusted copy event outside a key press.
    // eslint-disable-next-line @typescript-eslint/no-deprecated -- see above.
    doc.execCommand('copy');
  } catch {
    result.copied = false;
  } finally {
    doc.removeEventListener('copy', listener, true);
  }
  if (result.copied) {
    return true;
  }
  try {
    await doc.defaultView?.navigator.clipboard.writeText(text);
    return doc.defaultView?.navigator.clipboard !== undefined;
  } catch {
    return false;
  }
}
