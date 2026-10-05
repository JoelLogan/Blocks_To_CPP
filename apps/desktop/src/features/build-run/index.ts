/**
 * Build and run (milestone M2): the Build, Run, Stop and Run again commands, the build and run
 * sessions over IPC, the console's acknowledgements and input, and what the console header and
 * the Build output tab show. See ./feature.ts for the commands; src/app/panels.tsx connects the
 * console and the Build output tab.
 *
 * This module and everything it imports stays free of Blockly and the WebAssembly core at run
 * time, so the dock panels can use it (see src/app/panels.no-blockly.test.tsx).
 */
export { ACK_INTERVAL_MS, AckPacer } from './acks';
export {
  type ActiveBuild,
  BuildController,
  type BuildFinish,
  type BuildKey,
  MAX_BUILD_DIAGNOSTICS,
  type PreparedBuild,
} from './buildController';
export {
  buildStartLine,
  diagnosticLines,
  finishedLine,
  formatDuration,
  GENERATOR_BUG_LABEL,
  GENERATOR_BUG_NOTE,
  labelGeneratorBugs,
  MAX_RAW_LINES_PER_DIAGNOSTIC,
  progressLine,
  rawBlockIds,
} from './buildOutput';
export { checkBuildEvent, checkDiagnostic, checkRunEvent } from './channel';
export { type Clock, systemClock } from './clock';
export {
  ConsoleBridge,
  consoleBridge,
  type ConsoleListener,
  DEFAULT_TERMINAL_SIZE,
} from './consoleBridge';
export { consoleHeaderFrom } from './consoleHeader';
export { type BuildDocument, documentToBuild } from './document';
export { buildRunFeature, type BuildRunFeatureOptions, createBuildRunFeature } from './feature';
export { focusFirstError, gateAllows } from './gate';
export {
  chunkBytes,
  encodeInput,
  InputSender,
  MAX_PENDING_INPUT_BYTES,
  MAX_RUN_INPUT_BASE64,
  MAX_RUN_INPUT_BYTES,
  toBase64,
} from './input';
export { failureCode, failureMessage } from './messages';
export { clampSize, RUN_SEPARATOR, RunController, STOP_WAIT_MS } from './runController';
export { MISSING_OUTPUT_WAIT_MS, RunSession, type RunSessionHooks } from './runSession';
