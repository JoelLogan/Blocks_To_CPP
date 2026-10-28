/**
 * `@blocks2cpp/b2c-core-wasm`: the Rust compiler core (crates/b2c-core-wasm) compiled to
 * WebAssembly for the editor (06 §6.13, ADR-0003). The editor runs the same load, canonical save
 * and code generation as the backend, so the preview cannot drift from what is built.
 *
 * ```ts
 * const core = await initCore();
 * const loaded = core.load(bytes);
 * if (loaded.ok) {
 *   const json = JSON.stringify(loaded.document);
 *   const update = core.update(json, { indentWidth: 4 }); // the preview and update.textHash
 *   const visible = core.symbolsInScope('b005', null); // from that update's analysis
 *   const saved = core.canonical(json); // the text itself, only to save or build
 * }
 * ```
 *
 * Every document that enters the editor goes through `load()` first; never build editor state
 * from a plain `JSON.parse` of untrusted text. Clipboard data goes through `pastePrepare()`, which
 * validates it the same way.
 */

export type {
  CanonicalResult,
  ClipboardMakeResult,
  ConversionRow,
  CoreVersion,
  CoreWasm,
  LoadResult,
  PastePrepareResult,
  PasteTarget,
  PreviewOptions,
  PreviewResult,
  PreviewStage,
  StaticConversion,
  UnresolvedRef,
  UpdateResult,
} from './api';
export type {
  BdmBlock,
  BdmBuildConfiguration,
  BdmBuildSettings,
  BdmComment,
  BdmDefine,
  BdmDocument,
  BdmFrame,
  BdmGenerator,
  BdmLanguage,
  BdmModule,
  BdmNote,
  BdmPackRef,
  BdmProject,
  BdmProjectOptions,
  BdmRunSettings,
  BdmViewport,
  BdmWorkspace,
  BlockId,
  CppStandard,
  ExprInput,
  FieldValue,
  FrameColor,
  InputValue,
  JsonValue,
  ModuleId,
  ProjectId,
  SymbolDecl,
  SymbolId,
  SymbolRef,
  TokenJson,
  TypeJson,
} from './bdm';
export { FORMAT_TAG, MAX_FORMAT_VERSION } from './bdm';
export { MAX_DOCUMENT_BYTES } from './core';
export { CoreError, type CoreErrorKind, CoreTrap } from './errors';
export { coreModule, initCore, initCoreFromBytes, resetCore } from './loader';
export { randomSeedHex, type RandomSource, SEED_BYTES } from './seed';
/**
 * The pipeline's shared shapes, generated from the Rust types into `@blocks2cpp/ipc-types` (the
 * same JSON as `b2c check --format json`). Re-exported so that users of the core need only this
 * package.
 */
export type {
  DiagSource,
  Diagnostic,
  FileKind,
  FileMap,
  GeneratedFile,
  Location,
  MappedRange,
  Part,
  PassMode,
  Position,
  Related,
  Severity,
  SourceMap,
  SymbolInfo,
  SymbolInfoKind,
} from '@blocks2cpp/ipc-types';
