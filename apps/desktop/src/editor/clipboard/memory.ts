/**
 * The in-app copy (docs/spec/05-project-format.md §5.12): the last blocks copied or cut, kept in
 * memory so that pasting within the app works whatever the webview does with the custom clipboard
 * type. It is shared by every editor of the window, so it survives opening another project and
 * re-creating the workspace.
 */
import type { ClipboardData } from './formats';

/** Holds the last copied blocks. */
export class ClipboardMemory {
  private data: ClipboardData | null = null;

  /** The last copied blocks, or `null` when nothing was copied (or it was cleared). */
  get(): ClipboardData | null {
    return this.data;
  }

  /** Replaces the kept blocks. */
  set(data: ClipboardData): void {
    this.data = { payload: data.payload, text: data.text };
  }

  /** Forgets the kept blocks. */
  clear(): void {
    this.data = null;
  }
}

/** The app's in-app copy, shared by every editor of the window. */
export const appClipboardMemory = new ClipboardMemory();
