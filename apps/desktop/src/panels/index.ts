/**
 * The presentational panels of the main window (docs/spec/04-user-interface.md §4.3–§4.5): the
 * C++ code panel, Problems, the console and Build output. They are driven by props only and never
 * read the app's store; the app shell and the editor features connect them.
 */
export {
  BuildOutputPanel,
  MAX_BUILD_LINE_CHARS,
  MAX_BUILD_OUTPUT_LINES,
  type BuildOutputLine,
  type BuildOutputPanelProps,
} from './build-output/BuildOutputPanel';
export { CodePanel, IDE_INIT_UNIT_PATH, isHiddenFile, type CodePanelProps } from './code/CodePanel';
export {
  buildSourceMapIndex,
  MAX_INDEXED_RANGES,
  type CodeRange,
  type SourceMapIndex,
} from './code/sourcemap';
export {
  clampScrollback,
  ConsolePanel,
  DEFAULT_SCROLLBACK_LINES,
  MAX_SCROLLBACK_LINES,
  MIN_SCROLLBACK_LINES,
  type ConsoleHandle,
  type ConsoleHeader,
  type ConsoleNotice,
  type ConsolePanelProps,
  type ConsoleState,
  type TerminalSize,
} from './console/ConsolePanel';
export { MIN_WRITE_INTERVAL_MS } from './console/outputScheduler';
export {
  MAX_RAW_DISPLAY_CHARS,
  ProblemsPanel,
  type ProblemItem,
  type ProblemsPanelProps,
} from './problems/ProblemsPanel';
export { MAX_PROBLEM_ROWS } from './problems/problems';
export { copyPlainText } from './shared/clipboard';
export { visibleInvisibles } from './shared/invisibles';
export type { RunExit } from './types';
