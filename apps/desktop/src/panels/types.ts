/**
 * The data types the panels display. They come from the generated IPC contract
 * (`@blocks2cpp/ipc-types`, the same JSON as `b2c check --format json`) and from the WebAssembly
 * preview (`@blocks2cpp/b2c-core-wasm`). The panels import them only from here, so when a type
 * moves between the two packages only this file changes.
 */

import type { RunEvent } from '@blocks2cpp/ipc-types';

export type {
  Containment,
  Crash,
  DiagSource,
  Diagnostic,
  ExitStatus,
  Part,
  RunEvent,
  RunMode,
  Severity,
} from '@blocks2cpp/ipc-types';
export type {
  FileMap,
  GeneratedFile,
  MappedRange,
  Position,
  SourceMap,
} from '@blocks2cpp/b2c-core-wasm';

/** How a program ended: the `exit` message of a run's `onEvent` channel (07 §7.6.4). */
export type RunExit = Extract<RunEvent, { kind: 'exit' }>;
