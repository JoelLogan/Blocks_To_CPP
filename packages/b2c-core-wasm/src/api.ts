/**
 * The compiler core's API: the functions of `crates/b2c-core-wasm` (06 §6.13, ADR-0003) and the
 * shapes of their results. The pipeline's shared shapes (diagnostics, generated files, source
 * maps, static types, symbols) come from `@blocks2cpp/ipc-types`, generated from the same Rust
 * types, so the preview and the backend can never disagree about them.
 */

import type {
  Diagnostic,
  GeneratedFile,
  SourceMap,
  StaticType,
  SymbolInfo,
} from '@blocks2cpp/ipc-types';

import type { BdmBlock, BdmDocument, TypeJson } from './bdm';

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
 * The result of `CoreWasm.load()`: the loaded document (migrated to the current format, with its
 * blocks upgraded (05 §5.7), not resolved against the catalog, keys in canonical order) and a
 * `B2C-E0603` for each block that could not be upgraded, or the loader's `B2C-E01xx` problems.
 * Every number in the document is one JavaScript holds exactly (05 §5.6), so
 * `canonical(JSON.stringify(document))` gives the same text and hash as `canonical()` of the file
 * when the load upgraded no block; an upgraded document is the file changed, and `canonical()` of
 * the file gives the text before the upgrade.
 */
export type LoadResult =
  | { ok: true; document: BdmDocument; diagnostics: Diagnostic[] }
  | { ok: false; diagnostics: Diagnostic[] };

/**
 * The result of `CoreWasm.canonical()`: the canonical file text (what a save writes, 05 §5.2), the
 * content hash (64 lower-case hex digits, 05 §5.11) and `textHash`, the SHA-256 of the text's
 * UTF-8 bytes (64 lower-case hex digits), or the loader's problems.
 *
 * Unlike the content hash, `textHash` covers the layout too (block positions, notes, viewports):
 * two documents have the same `textHash` exactly when a save writes the same file for both. It is
 * the `textHash` that `update()` reports for the same document.
 */
export type CanonicalResult =
  | { ok: true; text: string; hash: string; textHash: string; diagnostics: Diagnostic[] }
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
  /**
   * The static type of each value block of the program, by block ID: what the connection checker
   * and the output types of `var.get` and `func.call` use. Blocks outside the program (disabled
   * or loose) are not listed; empty when loading failed.
   */
  blockTypes: Record<string, StaticType>;
  /** Every symbol of the program, sorted by name, then ID; empty when loading failed. */
  symbols: SymbolInfo[];
  /**
   * The static type of each expression slot the analysis lowered, by block ID, then input name
   * (ADR-0018, docs/plans/m3-contracts/slots.md §4.1). Absent until the core records slot types.
   */
  slotTypes?: Record<string, Record<string, TypeJson>>;
}

/**
 * The result of `CoreWasm.update()`: the preview, with `textHash`, the hash of the document's
 * canonical text (the `textHash` that `canonical()` reports for it; `null`, like `contentHash`,
 * only when loading failed). A failed load is `stage: 'load'` with the loader's problems as
 * `diagnostics`.
 */
export interface UpdateResult extends PreviewResult {
  textHash: string | null;
}

/**
 * How the analyser treats a value of one static type in a place of another
 * (`b2c_lang::Conversion`). Only `invalid` is an analyser error; `narrowing` and `boolNumber` are
 * warned about.
 */
export type StaticConversion = 'same' | 'widening' | 'narrowing' | 'boolNumber' | 'invalid';

/** One entry of `CoreWasm.conversionTable()`. */
export interface ConversionRow {
  /** The type of the value. */
  from: StaticType;
  /** The type of the place it goes to. */
  to: StaticType;
  conversion: StaticConversion;
}

/**
 * The result of `CoreWasm.clipboardMake()`: the clipboard payload for
 * `application/x-blocks2cpp+json` (canonical clipboard JSON, 05 §5.12) and the copied blocks' C++
 * for `text/plain` (absent when none of them produce code, for example loose or disabled blocks),
 * or the loader's problems when the document does not load.
 */
export type ClipboardMakeResult =
  | { ok: true; payload: string; text?: string; diagnostics: Diagnostic[] }
  | { ok: false; diagnostics: Diagnostic[] };

/**
 * Where pasted blocks go, and so which symbols their references can bind to (05 §5.12):
 * - `block: null`: the module's canvas, where the pasted blocks are parked: the parked scope,
 *   what is visible at the end of the module's `main` body (its functions alone without one);
 * - `block` with an `input`: the start of that statement list of the block, or inside that value
 *   input;
 * - `block` with `input: null`: directly after the block in its statement list (a variable the
 *   block itself creates is visible).
 *
 * A disabled statement in a list the analyser reaches has the scope of its position, like an
 * enabled block there; a block inside a disabled statement has that statement's scope; and a
 * paste into or after a parked block has the parked scope.
 */
export interface PasteTarget {
  /** The module ID. */
  module: string;
  /** A block of that module, or `null` for its canvas. */
  block: string | null;
  /** A statement or value input name of `block`, or `null`. */
  input: string | null;
}

/**
 * A pasted reference that found nothing to bind to at the target. It still refers to its original
 * symbol, and the analyser reports it (`B2C-E0201`, or `B2C-E0203` when that symbol exists in the
 * document but is not visible there).
 */
export interface UnresolvedRef {
  /** The symbol ID the pasted blocks still refer to. */
  sym: string;
  /** The qualified name the payload recorded (`score`, `::area`), or `null` when it named none. */
  name: string | null;
}

/**
 * The result of `CoreWasm.pastePrepare()`: the blocks to insert at the target, or the loader's
 * problems when the payload (or the document) does not load.
 */
export type PastePrepareResult =
  | {
      ok: true;
      /**
       * The payload's blocks, in order, with fresh block IDs and fresh IDs for the symbols they
       * declare (none used in the document), outside references re-bound at the target, and no
       * canvas position. Insert them as they are:
       * - for a canvas target (`block: null`), a copied loose stack stays one block with `stack`;
       * - for a target in or after a block, the stacked blocks follow their head in this list and
       *   no block has `stack` (a block inside another one cannot have one, `B2C-E0139`).
       */
      blocks: BdmBlock[];
      unresolved: UnresolvedRef[];
      /** One `B2C-E0201` per unresolved reference, at its first use, naming the original. */
      diagnostics: Diagnostic[];
    }
  | { ok: false; unresolved: UnresolvedRef[]; diagnostics: Diagnostic[] };

/**
 * One running instance of the compiler core. Every method is synchronous and never throws for a
 * problem in the project or a clipboard payload (those are diagnostics). It throws `CoreError`
 * when called with arguments the core refuses (for example invalid preview options or a paste
 * target the document does not have), and `CoreTrap` when the WebAssembly instance has stopped;
 * after a trap, get a fresh instance with `initCore()`.
 */
export interface CoreWasm {
  /** The versions this core implements. */
  version(): CoreVersion;
  /** Loads untrusted project bytes with every limit and rule of 05 §5.6. */
  load(bytes: Uint8Array): LoadResult;
  /**
   * Loads a document given as JSON text and serialises it canonically: the text a save, a build or
   * a recovery snapshot sends, with its hashes.
   */
  canonical(documentJson: string): CanonicalResult;
  /**
   * What the editor runs after each edit (06 §6.13): loads the document **once** and returns its
   * preview (the whole front half of the pipeline) with the hash of its canonical text, so the
   * editor can tell whether there are unsaved changes without asking for the text. Keeps the
   * analysis for `symbolsInScope` and the clipboard; a document that does not load keeps the
   * analysis of the last one that did.
   */
  update(documentJson: string, options: PreviewOptions): UpdateResult;
  /**
   * The symbols a block may refer to, from the analysis of the **last update** whose document
   * loaded (06 §6.5), sorted by name, then ID. `input` null (or a value input) gives what is
   * visible at the block; the name of one of its statement inputs gives what is visible at the
   * start of that list.
   *
   * Every block of the previewed document answers: a disabled block in a list or input the
   * analyser reaches answers for its position, like an enabled block there, and every block
   * nested in it like it; a parked block (loose, in a loose stack, or nested in one) answers with
   * the parked scope, what is visible at the end of its module's `main` body (the module's
   * functions alone without one).
   *
   * `null` when that analysis has **no answer** for the block: before an update has loaded a
   * document, for a block the previewed document does not have (one added since), and for one
   * nested too deeply. An empty list means that nothing is visible there. Call it only after your
   * own update has finished, so the answer is about the document you show.
   */
  symbolsInScope(blockId: string, input: string | null): SymbolInfo[] | null;
  /** The analyser's conversion rule for every pair of static types (49 rows). */
  conversionTable(): ConversionRow[];
  /**
   * Copies blocks of a document: `blockIds` in copy order (a listed block inside another listed
   * block is copied once, as part of it; a top-level block keeps its loose stack).
   */
  clipboardMake(documentJson: string, blockIds: readonly string[]): ClipboardMakeResult;
  /**
   * Validates a clipboard payload like a project file and prepares its blocks for insertion at
   * `target` in the document. `seedHex` is 64 hex digits of fresh randomness (see
   * `randomSeedHex()`); the same seed always gives the same IDs.
   *
   * Each outside reference binds to the symbol visible at the target with its recorded qualified
   * name and kind (itself when it is one of them). When no visible symbol has that name, a
   * reference whose original symbol is visible there with the same kind keeps it, even when it
   * was renamed since the copy; so projects that share symbol IDs (copies of one example) bind
   * such references silently, whatever the symbol is called in the target. Otherwise (no match,
   * or several) the reference is listed in `unresolved` with a `B2C-E0201` (06 §6.14.11).
   */
  pastePrepare(
    clipboardText: string,
    documentJson: string,
    target: PasteTarget,
    seedHex: string,
  ): PastePrepareResult;
}
