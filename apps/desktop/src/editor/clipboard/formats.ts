/**
 * What the editor puts on the clipboard (docs/spec/05-project-format.md §5.12): the canonical
 * clipboard payload as `application/x-blocks2cpp+json`, and the copied blocks' C++ as
 * `text/plain`. Both travel through the `DataTransfer` of the webview's own `copy`, `cut` and
 * `paste` events; there is no clipboard plugin and no backend command (M2 decision "Clipboard
 * transport and API").
 *
 * Only the custom type is ever read back. Text from the clipboard is untrusted: it goes to the
 * compiler core's `pastePrepare`, which validates it like a project file, and is never parsed
 * here.
 */
import { MAX_DOCUMENT_BYTES } from '@blocks2cpp/b2c-core-wasm';

/** The clipboard type of the payload (05 §5.12). */
export const CLIPBOARD_MIME = 'application/x-blocks2cpp+json';

/** The clipboard type of the copied blocks' C++. */
export const PLAIN_TEXT_MIME = 'text/plain';

/**
 * The longest payload passed on to the compiler core, in UTF-16 code units: one over the 32 MiB
 * limit, so that the loader still reports an oversized payload (`B2C-E0101`) while nothing larger
 * is copied around.
 */
export const MAX_PAYLOAD_CHARS = MAX_DOCUMENT_BYTES + 1;

/** Copied blocks: what a copy puts on the clipboard and what the in-app copy keeps. */
export interface ClipboardData {
  /** The canonical clipboard payload (`application/x-blocks2cpp+json`), made by the core. */
  readonly payload: string;
  /** The blocks' C++ (`text/plain`), or `null` when they produce none (loose or disabled blocks). */
  readonly text: string | null;
}

/**
 * Writes copied blocks into a copy or cut event's `DataTransfer`. The caller cancels the event, so
 * the webview puts exactly this on the system clipboard.
 *
 * @returns whether the payload itself was written. An engine that refuses the custom type still
 *   gets the C++ as text; the in-app copy keeps the payload either way.
 */
export function writeTransfer(transfer: DataTransfer, data: ClipboardData): boolean {
  let written = false;
  try {
    transfer.setData(CLIPBOARD_MIME, data.payload);
    written = true;
  } catch (error: unknown) {
    console.warn('The webview refused the Blocks2Cpp clipboard type', error);
  }
  if (data.text !== null) {
    try {
      transfer.setData(PLAIN_TEXT_MIME, data.text);
    } catch (error: unknown) {
      console.warn('The webview refused the C++ text for the clipboard', error);
    }
  }
  return written;
}

/**
 * The Blocks2Cpp payload of a paste event's `DataTransfer`, or `null` when it holds none (empty, or
 * only other types). Text longer than {@link MAX_PAYLOAD_CHARS} is cut to that length; the core
 * refuses it as too large.
 */
export function readTransfer(transfer: DataTransfer | null): string | null {
  if (transfer === null) {
    return null;
  }
  let text: string;
  try {
    text = transfer.getData(CLIPBOARD_MIME);
  } catch {
    return null;
  }
  if (text === '') {
    return null;
  }
  return text.length > MAX_PAYLOAD_CHARS ? text.slice(0, MAX_PAYLOAD_CHARS) : text;
}
