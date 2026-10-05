/**
 * The compiler core's API: the functions of `crates/b2c-core-wasm` (06 §6.13, ADR-0003) and the
 * shapes of their results.
 */

import type { BdmDocument } from './bdm';
import type { Diagnostic, GeneratedFile, SourceMap, StaticType } from './pipeline-types';

/** What `CoreWasm.version()` reports. */
export interface CoreVersion {
  /** The app (workspace) version, for example `0.1.0`. */
  app: string;
  /** The block catalog version, for example `1.0.0`. */
  catalog: string;
  /** The project format version this core reads and writes. */
  formatVersion: number;
  /** The source-map format version of `PreviewResult.sourceMap`. */
  sourceMapVersion: number;
}

/**
 * The result of `CoreWasm.load()`: the loaded document (migrated to the current format, not
 * resolved against the catalog, keys in canonical order), or the loader's `B2C-E01xx` problems.
 */
export type LoadResult =
  | { ok: true; document: BdmDocument; diagnostics: Diagnostic[] }
  | { ok: false; diagnostics: Diagnostic[] };

/**
 * The result of `CoreWasm.canonical()`: the canonical file text (what a save writes, 05 §5.2) and
 * the content hash (64 lower-case hex digits, 05 §5.11), or the loader's problems.
 */
export type CanonicalResult =
  | { ok: true; text: string; hash: string; diagnostics: Diagnostic[] }
  | { ok: false; diagnostics: Diagnostic[] };

/** The machine's code style for generated C++ (indent width 2 or 4 in milestone M2). */
export interface PreviewOptions {
  indentWidth: 2 | 4;
}

/**
 * The first pipeline stage that reported an error, or `generate` when none did: where a build
 * would stop. `load` means the text is not a valid project and there are no files.
 */
export type PreviewStage = 'load' | 'resolve' | 'analyze' | 'generate';

/** Everything the live preview shows. */
export interface PreviewResult {
  stage: PreviewStage;
  /** Every diagnostic of every stage, in pipeline order. */
  diagnostics: Diagnostic[];
  /**
   * Best-effort C++: after a successful load the preview carries on through catalog and analyser
   * errors (with `/* error *\/` placeholders), so this is empty only when loading failed.
   */
  files: GeneratedFile[];
  /** `null` only when loading failed. */
  sourceMap: SourceMap | null;
  /** Whether a build would accept this code: no error diagnostics and no placeholders. */
  buildable: boolean;
  /** How many error placeholders the files contain. */
  placeholders: number;
  /** The document's content hash (as `canonical()` reports it); `null` only when loading failed. */
  contentHash: string | null;
  /** Static types of value blocks by block ID. Always empty until milestone M2, wave 2. */
  blockTypes: Record<string, StaticType>;
  /** The program's symbols. Always empty until milestone M2, wave 2. */
  symbols: never[];
}

/**
 * One running instance of the compiler core. Every method is synchronous and never throws for a
 * problem in the project (those are diagnostics). It throws `CoreError` when called with arguments
 * the core refuses (for example invalid preview options), and `CoreTrap` when the WebAssembly
 * instance has stopped; after a trap, get a fresh instance with `initCore()`.
 */
export interface CoreWasm {
  /** The versions this core implements. */
  version(): CoreVersion;
  /** Loads untrusted project bytes with every limit and rule of 05 §5.6. */
  load(bytes: Uint8Array): LoadResult;
  /** Loads a document given as JSON text and serialises it canonically. */
  canonical(documentJson: string): CanonicalResult;
  /** Runs the whole front half of the pipeline for the live preview. */
  preview(documentJson: string, options: PreviewOptions): PreviewResult;
}
