/**
 * The contract between the end-to-end test hook (./index.ts, in the app) and the WebDriver tests
 * (apps/desktop/e2e/support/hook.ts, in Node.js), which both import this file. It has no imports
 * of its own, so the tests' TypeScript project can include it without the app.
 */

/** The name of the hook on `window`. */
export const E2E_HOOK_NAME = '__B2C_E2E__';

/** A point in client (viewport) coordinates, in CSS pixels. */
export interface E2ePoint {
  readonly x: number;
  readonly y: number;
}

/** The Content Security Policy violations the window has seen (src/lib/trustedTypes.ts). */
export interface E2eTrustedTypesReport {
  /** How many violation events there were (report-only and enforced together). */
  readonly count: number;
  /** The violated directives, each once, in alphabetical order. */
  readonly directives: string[];
}

/**
 * `window.__B2C_E2E__`: what the end-to-end tests can call through WebDriver's `executeScript`.
 * Every call throws (a JavaScript error in WebDriver) when its arguments are not valid.
 */
export interface E2eHookContract {
  /** Whether the window has started (the backend answered and the features are installed). */
  ready(): boolean;
  /**
   * Inserts BDM `blocks` (05 §5.4 block nodes, without `x`/`y`) into `input` of block
   * `parentBlockId` on the shown canvas: appended to the end of a statement list, or, for a value
   * input, the one reporter or predicate in place of the input's expression slot. One undoable
   * step. Throws when the target or the blocks are not valid, a block is not one the editor can
   * show exactly, or the blocks do not fit there; nothing is changed then, and the undo stack
   * gains no step.
   */
  insertBlocks(parentBlockId: string, input: string, blocks: readonly unknown[]): void;
  /** The open project as the canvas has it now: the canonical BDM text a save would write. */
  document(): string;
  /**
   * Everything the console was given since it was last cleared (program output with the
   * terminal's echo, the console's own separators and its "… N lines skipped" markers), as plain
   * text with `\n` line ends, in the order the console was given it. Output still queued for the
   * terminal when the console is cleared is painted after the clear but is not in the transcript.
   */
  consoleText(): string;
  /** The Content Security Policy violations seen so far. */
  trustedTypes(): E2eTrustedTypesReport;
  /** Selects a block and centres the canvas on it. */
  selectBlock(id: string): void;
  /** The C++ the code panel shows (every file it lists, in order), from the live preview. */
  code(): string;
  /** The SVG group of a block on the canvas or in the toolbox's flyout, or `null`. */
  blockElement(id: string): Element | null;
  /**
   * The SVG group of field `field` of a block, or, with `input`, of the block in that input (an
   * expression slot's `VALUE`); `null` when there is none or it is hidden.
   */
  fieldElement(blockId: string, field: string, input?: string | null): Element | null;
  /**
   * The ID of the first top-level block of `type` in the toolbox's flyout whose fields have the
   * given values (`{MODE: 'until'}` picks the *repeat until* entry); `null` when there is none.
   */
  flyoutBlockId(type: string, fields?: Readonly<Record<string, string>> | null): string | null;
  /**
   * Where a block's `previous`, `next` or `output` connection, or the connection of one of its
   * inputs, is on screen; `null` when there is no such block or connection.
   */
  connectionPoint(blockId: string, connection: string): E2ePoint | null;
  /**
   * A point on screen where pressing grabs the block itself (its own outline, not a field or a
   * block inside it, and not covered); `null` when the block is not visible.
   */
  grabPoint(blockId: string): E2ePoint | null;
  /**
   * Hands a constant string to an HTML sink (`DOMParser`) so that a Trusted Types policy in force,
   * even report-only, reports one `require-trusted-types-for` violation: the tests use it to see
   * whether the trial's header reached the page. Parsing into a detached document runs nothing.
   */
  probeTrustedTypes(): void;
}
