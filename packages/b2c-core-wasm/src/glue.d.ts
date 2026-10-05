/**
 * Declarations for `#glue` (pkg/glue.js), the JavaScript glue that `wasm-bindgen --target web`
 * generates for crates/b2c-core-wasm, so that type checking works without a build.
 *
 * scripts/build.mjs wraps the generated module in `createGlue()` so that every instance gets its
 * own glue state (the generated module keeps one instance in module-level variables). The build
 * fails when the crate's exports differ from the functions below (`EXPECTED_EXPORTS` in the build
 * script): change both together.
 */

/** The glue of one WebAssembly instance of the compiler core. */
export interface Glue {
  /**
   * Instantiates the compiled module for this glue. Call it once, before any other function;
   * later calls do nothing.
   */
  init(options: { module_or_path: WebAssembly.Module }): Promise<unknown>;
  /** `b2c_core_wasm::version`. */
  version(): string;
  /** `b2c_core_wasm::load`. */
  load(bytes: Uint8Array): string;
  /** `b2c_core_wasm::canonical`. */
  canonical(document_json: string): string;
  /** `b2c_core_wasm::preview`. */
  preview(document_json: string, options_json: string): string;
  /** `b2c_core_wasm::symbols_in_scope`. */
  symbols_in_scope(block_id: string, input?: string | null): string;
  /** `b2c_core_wasm::conversion_table`. */
  conversion_table(): string;
  /** `b2c_core_wasm::clipboard_make`. */
  clipboard_make(document_json: string, block_ids_json: string): string;
  /** `b2c_core_wasm::paste_prepare`. */
  paste_prepare(
    clipboard_text: string,
    document_json: string,
    target_json: string,
    seed_hex: string,
  ): string;
}

/** Creates fresh glue state, not yet bound to an instance. */
export function createGlue(): Glue;
