/**
 * Where the shell keeps the compiler core (`@blocks2cpp/b2c-core-wasm`) once it is running. The
 * editor's preview pipeline (milestone M2, wave 3) starts it with `initCore()`, publishes it here,
 * and replaces it after a trap; features read it through `FeatureContext.core()`.
 *
 * The shell itself never loads the WebAssembly module, so the app starts (and its tests run)
 * without it.
 */
import type { CoreWasm } from '@blocks2cpp/b2c-core-wasm';

let current: CoreWasm | null = null;

/** Publishes the running core (or, with `null`, withdraws it). */
export function setCore(core: CoreWasm | null): void {
  current = core;
}

/** The running core, or `null` before it has started. */
export function getCore(): CoreWasm | null {
  return current;
}
