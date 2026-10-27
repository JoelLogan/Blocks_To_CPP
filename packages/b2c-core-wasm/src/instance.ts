/**
 * Instantiating a compiled compiler core module (06 §6.13). Kept apart from `loader.ts`, which
 * imports the embedded bytes (`#pkg-bytes`), so that the preview worker (ADR-0016) can instantiate
 * the `WebAssembly.Module` the page posts it without anything it loads holding the bytes.
 */

import { type CoreHandle, createCore } from './core';
import { CoreError } from './errors';

/** A start-up failure as the `CoreError` (`init`) the loader throws. */
export function startFailed(error: unknown): CoreError {
  if (error instanceof CoreError) {
    return error;
  }
  const reason = error instanceof Error ? error.message : String(error);
  return new CoreError('init', `the compiler core could not be started: ${reason}`, {
    cause: error,
  });
}

/**
 * Instantiates a compiled module with fresh glue state.
 *
 * @throws CoreError with kind `init` when the module is not one for this glue.
 */
export async function instantiate(module: WebAssembly.Module): Promise<CoreHandle> {
  try {
    const { createGlue } = await import('#glue');
    const glue = createGlue();
    await glue.init({ module_or_path: module });
    return createCore(glue);
  } catch (error) {
    throw startFailed(error);
  }
}
