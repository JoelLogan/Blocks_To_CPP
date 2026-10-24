/**
 * Starting the compiler core (06 §6.13; spec gap decision "WASM delivery under the CSP").
 *
 * The optimised module is embedded as base64 in a separate, lazily imported chunk
 * (`#pkg-bytes`, served from 'self' like the rest of the app), decoded here and compiled with
 * `WebAssembly.compile`, which the unchanged CSP allows through 'wasm-unsafe-eval'. It is never
 * fetched and never a data: URL. The module is compiled once; every instance gets its own copy of
 * the wasm-bindgen glue state (`createGlue()`), so a trapped instance can be replaced without
 * reloading the page.
 */

import type { CoreWasm } from './api';
import { decodeBase64 } from './base64';
import { type CoreHandle, createCore } from './core';
import { CoreError } from './errors';

/** The compiled embedded module, kept across instances; forgotten if compiling failed. */
let compiled: Promise<WebAssembly.Module> | undefined;
/** The current instance (starting or started); forgotten if starting failed. */
let current: Promise<CoreHandle> | undefined;

function startFailed(error: unknown): CoreError {
  if (error instanceof CoreError) {
    return error;
  }
  const reason = error instanceof Error ? error.message : String(error);
  return new CoreError('init', `the compiler core could not be started: ${reason}`, {
    cause: error,
  });
}

async function compileEmbedded(): Promise<WebAssembly.Module> {
  const { WASM_BASE64 } = await import('#pkg-bytes');
  return WebAssembly.compile(decodeBase64(WASM_BASE64));
}

function embeddedModule(): Promise<WebAssembly.Module> {
  if (compiled === undefined) {
    const compiling = compileEmbedded();
    compiled = compiling;
    compiling.catch(() => {
      if (compiled === compiling) {
        compiled = undefined;
      }
    });
  }
  return compiled;
}

/** Instantiates a compiled module with fresh glue state. */
async function instantiate(module: WebAssembly.Module): Promise<CoreHandle> {
  try {
    const { createGlue } = await import('#glue');
    const glue = createGlue();
    await glue.init({ module_or_path: module });
    return createCore(glue);
  } catch (error) {
    throw startFailed(error);
  }
}

async function start(): Promise<CoreHandle> {
  let module: WebAssembly.Module;
  try {
    module = await embeddedModule();
  } catch (error) {
    throw startFailed(error);
  }
  return instantiate(module);
}

/**
 * Returns the compiler core, starting it on first use: the embedded module is decoded, compiled
 * and instantiated once, and later calls return the same instance. If that instance has trapped
 * (see `CoreTrap`), a fresh one is started from the already compiled module. A failed start is not
 * remembered, so calling again retries.
 *
 * @throws CoreError with kind `init` when the module cannot be loaded or compiled.
 */
export async function initCore(): Promise<CoreWasm> {
  if (current === undefined) {
    const starting = start();
    current = starting;
    starting.catch(() => {
      if (current === starting) {
        current = undefined;
      }
    });
  }
  const pending = current;
  const handle = await pending;
  if (!handle.trapped) {
    return handle.core;
  }
  if (current === pending) {
    current = undefined;
  }
  return initCore();
}

/**
 * Forgets the current instance, so that the next `initCore()` starts a fresh one (from the
 * already compiled module). Call it after a `CoreTrap`; `initCore()` also does this by itself when
 * the instance it would return has trapped. Instances already handed out keep working (or keep
 * throwing `CoreTrap` if they trapped).
 */
export function resetCore(): void {
  current = undefined;
}

/**
 * Starts an independent instance from the given module bytes (for example
 * `pkg/b2c_core_wasm_bg.wasm` read in a Node test). It is not cached and does not affect
 * `initCore()`.
 *
 * @throws CoreError with kind `init` when the bytes are not a valid module for this glue.
 */
export async function initCoreFromBytes(bytes: Uint8Array): Promise<CoreWasm> {
  let module: WebAssembly.Module;
  try {
    // A copy with its own ArrayBuffer, as WebAssembly.compile requires.
    module = await WebAssembly.compile(new Uint8Array(bytes));
  } catch (error) {
    throw startFailed(error);
  }
  return (await instantiate(module)).core;
}
