/**
 * The pipeline's shared JSON shapes: diagnostics (06 §6.12), generated files and source maps
 * (06 §6.9), and static types (06 §6.6). They mirror the serde shapes of `b2c-ir`, which are also
 * what `b2c check --format json` prints (docs/reference/cli.md).
 *
 * These hand-written copies are replaced by imports from `@blocks2cpp/ipc-types` (generated from
 * the Rust types) in milestone M2, wave 2.
 */

/** How serious a diagnostic is. Only `error` blocks building and running. */
export type Severity = 'info' | 'warning' | 'error';

/** Which stage produced a diagnostic. */
export type DiagSource =
  'loader' | 'catalog' | 'analyser' | 'generator' | 'toolchain' | 'compiler' | 'linker' | 'runtime';

/** The part of a block a diagnostic or a source-map range points at. */
export type Part =
  | { kind: 'whole' }
  | { kind: 'field'; name: string }
  | { kind: 'input'; name: string }
  /** A token range in an expression slot: `start` inclusive, `end` exclusive. */
  | { kind: 'tokens'; input: string; start: number; end: number };

/** Where a diagnostic points. A problem with the whole file has neither module nor block. */
export interface Location {
  module?: string;
  block?: string;
  part: Part;
}

/** A secondary location with its own message. */
export interface Related {
  location: Location;
  message: string;
}

/**
 * A problem found while loading, analysing, generating, compiling or running. `code` is stable
 * (for example `B2C-E0201`) and documented in docs/reference/diagnostics/. `message` is plain
 * text and may quote project content: always render it as text, never as HTML.
 */
export interface Diagnostic {
  code: string;
  severity: Severity;
  message: string;
  primary: Location;
  related?: Related[];
  source: DiagSource;
  /** The compiler's or linker's original text, for diagnostics that are not ours. */
  raw?: string;
}

/** A translation unit (`.cpp`) or a header (`.hpp`). */
export type FileKind = 'source' | 'header';

/** One generated file. */
export interface GeneratedFile {
  /** Relative, `/`-separated, built only from validated module names (for example `main.cpp`). */
  path: string;
  kind: FileKind;
  /** UTF-8 text with `\n` line endings, ending with one newline. */
  contents: string;
}

/** A position in a generated file: 1-based line, 1-based column counted in UTF-8 bytes. */
export interface Position {
  line: number;
  column: number;
}

/** A range of generated text and the block part that produced it. */
export interface MappedRange {
  /** Inclusive. */
  start: Position;
  /** Exclusive. */
  end: Position;
  module: string;
  block: string;
  part: Part;
}

/** The mapped ranges of one generated file, sorted by start position. */
export interface FileMap {
  path: string;
  ranges: MappedRange[];
}

/** Maps generated text back to the blocks that produced it (06 §6.9). */
export interface SourceMap {
  /** The format version; see `CoreVersion.sourceMapVersion`. */
  version: number;
  files: FileMap[];
}

/**
 * A static type (06 §6.6). `string` is `std::string`; `error` is the type of an expression that
 * already has an error and is compatible with everything.
 */
export type StaticType = 'void' | 'bool' | 'char' | 'int' | 'double' | 'string' | 'error';
