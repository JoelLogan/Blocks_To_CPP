/**
 * `@blocks2cpp/b2c-core-wasm`: the Rust compiler core (crates/b2c-core-wasm) compiled to
 * WebAssembly for the editor (06 §6.13, ADR-0003). The editor runs the same load, canonical save
 * and code generation as the backend, so the preview cannot drift from what is built.
 *
 * ```ts
 * const core = await initCore();
 * const loaded = core.load(bytes);
 * if (loaded.ok) {
 *   const preview = core.preview(JSON.stringify(loaded.document), { indentWidth: 4 });
 * }
 * ```
 *
 * Every document that enters the editor goes through `load()` first; never build editor state from
 * a plain `JSON.parse` of untrusted text.
 */

export type {
  CanonicalResult,
  CoreVersion,
  CoreWasm,
  LoadResult,
  PreviewOptions,
  PreviewResult,
  PreviewStage,
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
} from './bdm';
export { CURRENT_FORMAT_VERSION, FORMAT_TAG } from './bdm';
export { MAX_DOCUMENT_BYTES } from './core';
export { CoreError, type CoreErrorKind, CoreTrap } from './errors';
export { initCore, initCoreFromBytes, resetCore } from './loader';
export type {
  DiagSource,
  Diagnostic,
  FileKind,
  FileMap,
  GeneratedFile,
  Location,
  MappedRange,
  Part,
  Position,
  Related,
  Severity,
  SourceMap,
  StaticType,
} from './pipeline-types';
