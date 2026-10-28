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

/** A block to look for in the toolbox's flyout: its type and the values of some of its fields. */
export interface E2eFlyoutQuery {
  readonly type: string;
  readonly fields?: Readonly<Record<string, string>> | null;
}

/** The Content Security Policy violations the window has seen (src/lib/trustedTypes.ts). */
export interface E2eTrustedTypesReport {
  /** How many violation events there were (report-only and enforced together). */
  readonly count: number;
  /** The violated directives, each once, in alphabetical order. */
  readonly directives: string[];
}

/** What the console's terminal holds (src/panels/console/screen.ts). */
export interface E2eConsoleScreen {
  /** The buffer shown: the normal one, with the scrollback, or a program's alternate screen. */
  readonly buffer: 'normal' | 'alternate';
  /**
   * Every row of the buffer, scrollback first, as text without the cells at its end that nothing
   * was written to. A line too long for one row continues on the next.
   */
  readonly lines: readonly string[];
  /** The index in `lines` of the screen's top row when scrolled to the end; above it is scrollback. */
  readonly baseY: number;
  /** The index in `lines` of the top row shown (`baseY` unless scrolled up). */
  readonly viewportY: number;
  /** The cursor: its row (an index in `lines`) and its column. */
  readonly cursor: { readonly row: number; readonly column: number };
  /** The terminal's width in character cells. */
  readonly cols: number;
  /** The terminal's height in character cells. */
  readonly rows: number;
}

/**
 * The latest duration, in milliseconds, of each User Timing span the app measures
 * (src/lib/timing.ts), by span name: `b2c:preview` and its phases `b2c:preview:read`,
 * `b2c:preview:update`, `b2c:preview:commit` and `b2c:preview:publish`;
 * `b2c:toolbox-refresh` (a rebuild of the toolbox's flyout); `b2c:select`; `b2c:code-panel`;
 * `b2c:keyboard-targets` (listing a keyboard move's places). A
 * span that has not run since the window started is `null`.
 */
export type E2eTimings = Readonly<Record<string, number | null>>;

/** What the preview worker's probe found (packages/b2c-core-wasm/src/worker/protocol.ts). */
export interface E2eWorkerProbe {
  readonly eval: 'blocked' | 'allowed';
  readonly request: 'blocked' | 'allowed' | 'failed';
  readonly directives: string[];
}

/**
 * The result of {@link E2eHookContract.startWorkerCheck} (src/e2e/workerCheck.ts): a preview worker
 * started beside the editor's preview, the open project previewed in it and on the main thread.
 * Times are in milliseconds on the page's clock.
 */
export type E2eWorkerCheck =
  | {
      readonly ok: true;
      readonly error: null;
      /** The page's origin (`tauri://localhost`, `http://tauri.localhost`). */
      readonly origin: string;
      /** From creating the worker until it answered `started`. */
      readonly startupMs: number;
      /** The first update's round trip (the worker's instance runs it cold). */
      readonly firstUpdateMs: number;
      /** Whether the worker's first answer equals the main thread's `update()` of the document. */
      readonly sameAsMain: boolean;
      readonly contentHash: string | null;
      /** Each round's update through `postMessage`, from posting to the answer, parsed. */
      readonly roundTripMs: number[];
      /** Each round's `update()` alone, timed in the worker. */
      readonly workerUpdateMs: number[];
      /** Each round's `update()` called on the main thread, with its parse. */
      readonly mainUpdateMs: number[];
      /** The UTF-16 length of the update request's document and of the reply's answer. */
      readonly requestChars: number;
      readonly replyChars: number;
      /** One `structuredClone` of the update request and of its reply (the mean of 20). */
      readonly requestCloneMs: number;
      readonly replyCloneMs: number;
      readonly probe: E2eWorkerProbe;
      /** The probe's request made from the page, whose policy allows it: the control. */
      readonly pageRequest: E2eWorkerProbe['request'];
      /** Violations the page saw for a worker made from a plain string URL. */
      readonly unwrappedViolations: number;
      /** Violations the page saw for the worker made through the `b2c-core-worker` policy. */
      readonly policyViolations: number;
    }
  | { readonly ok: false; readonly error: string };

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
  /**
   * What the console's terminal shows: its active buffer (scrollback and screen) as text lines,
   * the viewport and the cursor, as far as the terminal has processed its output. Unlike
   * {@link consoleText} it shows the effect of cursor moves and of Clear, as the person sees it.
   */
  consoleScreen(): E2eConsoleScreen;
  /** The Content Security Policy violations seen so far. */
  trustedTypes(): E2eTrustedTypesReport;
  /** Selects a block and centres the canvas on it. */
  selectBlock(id: string): void;
  /**
   * Scrolls the canvas by `dx`, `dy` CSS pixels, as dragging its empty background that far does,
   * but without the drag's side effects: the focus and the selection stay as they are (a
   * background drag selects nothing). Blockly keeps the canvas within its content's bounds, so
   * the canvas may move less. The benchmarks use it to bring a block into view before they click
   * it, so that the click changes the selection from the block selected before.
   */
  scrollCanvasBy(dx: number, dy: number): void;
  /** The C++ the code panel shows (every file it lists, in order), from the live preview. */
  code(): string;
  /** The latest duration of each User Timing span (read-only; a copy). */
  timings(): E2eTimings;
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
   * {@link flyoutBlockId} for each of `queries` (at most 64), all from the flyout as one showing
   * of it holds them: the toolbox shows its flyout again from a timer, which can run between two
   * calls of the hook but not within one.
   */
  flyoutBlockIds(queries: readonly E2eFlyoutQuery[]): (string | null)[];
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
  /**
   * Starts the preview worker's check with `rounds` round trips (1 to 50) on the open project:
   * a worker of its own, beside the editor's preview. Poll {@link workerCheck} for its result.
   */
  startWorkerCheck(rounds: number): void;
  /** The result of the latest {@link startWorkerCheck}, or `null` while it runs or before one. */
  workerCheck(): E2eWorkerCheck | null;
  /**
   * Where the editor's next preview runs: `worker` or `main` (the main thread until the worker
   * has started, and after it has failed), `off` in a build made with `B2C_PREVIEW_WORKER=0`,
   * and `null` when no editor is open.
   */
  previewThread(): 'worker' | 'main' | 'off' | null;
  /**
   * Terminates the editor's preview worker as if it had raised an error, so that its client
   * starts a new one. Throws when the editor has no worker.
   */
  stopPreviewWorker(): void;
}
