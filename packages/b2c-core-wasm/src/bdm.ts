/**
 * The Block Document Model: the JSON shape of a `.b2c` project (05 §5.3–5.5), as `load()` returns
 * it. These types mirror `b2c_model::document`; the Rust loader is the authority on what is valid
 * (limits, text rules, IDs, 05 §5.6), so a document typed as `BdmDocument` must still come from
 * `CoreWasm.load()` and never from a plain `JSON.parse` of untrusted text.
 *
 * Optional keys are the ones the canonical writer leaves out when they are empty or `false`; every
 * other key is always present in a loaded document.
 *
 * Numbers: JSON numbers become JavaScript numbers, and every number of a loaded document is one
 * that JavaScript holds exactly (05 §5.6). The loader refuses a define's `int` value and a number
 * in `extra` or `x-ext` beyond ±(2^53 − 1) (`Number.MAX_SAFE_INTEGER`), stores a whole free-form
 * number as an integer (`1.0` is `1`), and the canonical writer writes free-form floats as
 * `JSON.stringify` does. So the document survives `JSON.parse` and `JSON.stringify` unchanged:
 * `canonical(JSON.stringify(load(t).document))` gives the same text and hash as `canonical(t)`.
 * The other numbers (versions, coordinates, zoom) are typed and range-checked.
 */

/** The value of the top-level `format` key. */
export const FORMAT_TAG = 'blocks2cpp/project';

/** The project format version this core reads and writes (`CoreVersion.formatVersion`). */
export const CURRENT_FORMAT_VERSION = 1;

/** A block ID: `[A-Za-z0-9_]{1,32}`, unique in the project. */
export type BlockId = string;
/** A symbol ID (variables, parameters, functions), referenced by `{ref}`. */
export type SymbolId = string;
/** A module ID. */
export type ModuleId = string;
/** A project ID. */
export type ProjectId = string;

/** Any JSON value (free-form `extra` and `x-ext` data). */
export type JsonValue =
  null | boolean | number | string | JsonValue[] | { [key: string]: JsonValue };

/** A whole project file. */
export interface BdmDocument {
  format: typeof FORMAT_TAG;
  formatVersion: number;
  generator: BdmGenerator;
  project: BdmProject;
  modules: BdmModule[];
  /** Tooling metadata: preserved verbatim, never interpreted. */
  'x-ext'?: JsonValue;
}

/** The app and catalog versions that last saved the file. */
export interface BdmGenerator {
  app: string;
  catalog: string;
}

/** Project settings: closed enums and validated scalars only (ADR-0005). */
export interface BdmProject {
  id: ProjectId;
  name: string;
  description?: string;
  language: BdmLanguage;
  options: BdmProjectOptions;
  build: BdmBuildSettings;
  run: BdmRunSettings;
}

/** The C++ standards a project can use. */
export type CppStandard = 'c++17' | 'c++20' | 'c++23' | 'c++26';

export interface BdmLanguage {
  standard: CppStandard;
  /** `gnu++NN` instead of `c++NN`; written only when true. */
  gnuExtensions?: boolean;
}

export interface BdmProjectOptions {
  showAdvanced: boolean;
  manualMemory: boolean;
  preferPlainStd: boolean;
  formattingStyle: 'stream' | 'format';
  checkedIndexing: boolean;
}

export interface BdmBuildSettings {
  configurations: { debug: BdmBuildConfiguration; release: BdmBuildConfiguration };
  defines?: BdmDefine[];
  libraries?: string[];
  packs?: BdmPackRef[];
}

export interface BdmBuildConfiguration {
  optimization: 'none' | 'debug' | 'speed' | 'size';
  debugInfo: boolean;
  sanitizers: ('address' | 'undefined')[];
  warnings: 'minimal' | 'helpful' | 'strict';
  /** Written only when true. */
  warningsAsErrors?: boolean;
  hardening: boolean;
}

/**
 * A preprocessor define with a typed value; the backend builds the `-D` argument itself. An `int`
 * is a whole number within ±(2^53 − 1) (`Number.isSafeInteger`); the loader refuses others.
 */
export interface BdmDefine {
  name: string;
  value: { int: number } | { bool: boolean } | { string: string };
}

/** A library pack reference. */
export interface BdmPackRef {
  id: string;
  /** A SemVer requirement. */
  version: string;
}

export interface BdmRunSettings {
  /** Program arguments, an argv list (never a shell string). */
  args?: string[];
  workingDirectory: 'project' | 'sandbox';
}

/** One module; it becomes `<name>.cpp`. */
export interface BdmModule {
  id: ModuleId;
  name: string;
  workspace: BdmWorkspace;
}

/** A module's canvas. */
export interface BdmWorkspace {
  /** Top-level blocks (canonical order: sorted by ID). */
  blocks: BdmBlock[];
  frames?: BdmFrame[];
  notes?: BdmNote[];
  viewport?: BdmViewport;
}

export type FrameColor = 'grey' | 'blue' | 'green' | 'yellow' | 'orange' | 'purple';

/** A titled frame grouping top-level blocks (layout only). */
export interface BdmFrame {
  id: BlockId;
  title: string;
  x: number;
  y: number;
  w: number;
  h: number;
  color: FrameColor;
  emitBanner?: boolean;
}

/** A sticky note (layout only, never emitted). */
export interface BdmNote {
  id: BlockId;
  text: string;
  x: number;
  y: number;
}

export interface BdmViewport {
  x: number;
  y: number;
  /** Zoom, 0.1–4.0. */
  scale: number;
}

/** One block (05 §5.4). Statement sequences are arrays, never linked `next` chains. */
export interface BdmBlock {
  id: BlockId;
  /** The catalog block type, for example `control.if`. */
  type: string;
  /** The catalog block version. */
  v: number;
  /** Canvas position: top-level blocks only. */
  x?: number;
  y?: number;
  collapsed?: boolean;
  disabled?: boolean;
  comment?: BdmComment;
  /** Mutator state (variadic counts, parameter rows), validated against the catalog. */
  extra?: Record<string, JsonValue>;
  fields?: Record<string, FieldValue>;
  /** Value inputs; an absent input means the catalog default. */
  inputs?: Record<string, InputValue>;
  statements?: Record<string, BdmBlock[]>;
  /**
   * On a top-level statement block only: the statement blocks attached below it on the canvas,
   * in order (a loose stack, M2 amendment A1). Never written empty.
   */
  stack?: BdmBlock[];
}

export interface BdmComment {
  text: string;
  pinned?: boolean;
}

/** Declares a symbol in a field: `{"sym": "sym_x", "name": "score"}`. */
export interface SymbolDecl {
  sym: SymbolId;
  name: string;
}

/** References a symbol in a field: `{"ref": "sym_x"}`. */
export interface SymbolRef {
  ref: SymbolId;
}

/** A field value: checkbox, text (numbers are stored as text), declaration or reference. */
export type FieldValue = boolean | string | SymbolDecl | SymbolRef;

/** A value input: a nested reporter block or an expression slot (03 §3.4). */
export type InputValue = { block: BdmBlock } | ExprInput;

/** An expression slot: a flat token list. */
export interface ExprInput {
  expr: TokenJson[];
  /** The tokens are an unfinished draft that does not parse yet; written only when true. */
  draft?: boolean;
}

/** One expression-slot token, a single-key object. */
export type TokenJson =
  | { num: string }
  | { str: string }
  | { chr: string }
  | { ref: SymbolId }
  | { op: string }
  | { kw: string }
  | { text: string };
