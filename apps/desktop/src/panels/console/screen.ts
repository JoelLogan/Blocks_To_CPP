/**
 * What the console's terminal holds ({@link ConsoleScreen}): its active buffer as text, with the
 * viewport and the cursor. The end-to-end tests read it (`consoleScreen()`, src/e2e/contract.ts)
 * to check what the person sees, such as a second run below the first, rather than the bytes the
 * console was given.
 */
import type { IBuffer } from '@xterm/xterm';

/** A snapshot of the terminal's active buffer. */
export interface ConsoleScreen {
  /** The buffer shown: the normal one, with the scrollback, or a program's alternate screen. */
  readonly buffer: 'normal' | 'alternate';
  /**
   * Every row of the buffer, scrollback first, as text without the cells at its end that nothing
   * was written to: at most the scrollback setting plus one screen. A line too long for one row
   * continues on the next.
   */
  readonly lines: readonly string[];
  /** The index in `lines` of the screen's top row when scrolled to the end; above it is scrollback. */
  readonly baseY: number;
  /** The index in `lines` of the top row shown (`baseY` unless the person scrolled up). */
  readonly viewportY: number;
  /** The cursor: its row (an index in `lines`) and its column. */
  readonly cursor: { readonly row: number; readonly column: number };
  /** The terminal's width in character cells. */
  readonly cols: number;
  /** The terminal's height in character cells. */
  readonly rows: number;
}

/** The screen of a console that shows nothing yet: an empty normal buffer of `cols` × `rows`. */
export function emptyScreen(cols: number, rows: number): ConsoleScreen {
  return {
    buffer: 'normal',
    lines: [],
    baseY: 0,
    viewportY: 0,
    cursor: { row: 0, column: 0 },
    cols,
    rows,
  };
}

/** Reads `buffer` (a terminal's `buffer.active`) of a terminal of `cols` × `rows`. */
export function readScreen(buffer: IBuffer, cols: number, rows: number): ConsoleScreen {
  const lines: string[] = [];
  for (let row = 0; row < buffer.length; row++) {
    lines.push(buffer.getLine(row)?.translateToString(true) ?? '');
  }
  return {
    buffer: buffer.type,
    lines,
    baseY: buffer.baseY,
    viewportY: buffer.viewportY,
    cursor: { row: buffer.baseY + buffer.cursorY, column: buffer.cursorX },
    cols,
    rows,
  };
}
