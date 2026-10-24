/**
 * Where the shell keeps the compiler core (`@blocks2cpp/b2c-core-wasm`) once it is running. The
 * editor's preview pipeline (milestone M2, wave 3) starts it with `initCore()`, publishes it here,
 * and replaces it after a trap; features read it through `FeatureContext.core()` and editor
 * plugins through `EditorContext.core()`, both backed by {@link getCore}. Nothing may keep the
 * instance: after a trap it throws `CoreTrap` on every call.
 *
 * The shell itself never loads the WebAssembly module, so its tests run without it. Only a
 * development build's start (`main.tsx`) starts the core early, to compare its catalog version
 * with the backend's (`AppRuntimeOptions.coreCatalogVersion`); the editor then gets the same
 * instance.
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
