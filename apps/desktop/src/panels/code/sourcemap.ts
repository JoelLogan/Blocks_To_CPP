/**
 * The source map of the live preview, indexed for the code panel (docs/spec/06-compiler-pipeline.md
 * §6.9, docs/spec/04-user-interface.md §4.3): which text a block produced (to highlight it) and
 * which block produced the text at a position (to select it on a click).
 *
 * Source-map positions are 1-based, with columns counted in UTF-8 bytes (GCC's unit) and an
 * exclusive end. CodeMirror and JavaScript strings count UTF-16 code units, so every position is
 * converted line by line against the generated file's text. The index is built once per preview;
 * lookups binary-search.
 */
import type { GeneratedFile, MappedRange, Part, Position, SourceMap } from '../types';

/**
 * The most ranges one index holds, over all files. A preview of the largest project the loader
 * accepts stays far below it; anything beyond is ignored rather than slowing the editor down.
 */
export const MAX_INDEXED_RANGES = 1_000_000;

/** A range of a generated file in UTF-16 offsets: `from` inclusive, `to` exclusive. */
export interface CodeRange {
  /** The generated file, as in `GeneratedFile.path`. */
  readonly path: string;
  readonly from: number;
  readonly to: number;
}

/** What the code panel asks of the source map. */
export interface SourceMapIndex {
  /**
   * Every range the block produced as a whole (`part.kind === 'whole'`), in every file: what is
   * highlighted when the block is hovered or selected. In file order, then by position.
   */
  rangesForBlock(blockId: string): readonly CodeRange[];
  /**
   * The ranges for one part of a block (a field, an input or a token range), compared by value.
   * Empty when the source map has no range for that part; callers then fall back to
   * {@link rangesForBlock}.
   */
  rangesForPart(blockId: string, part: Part): readonly CodeRange[];
  /**
   * The block of the innermost range containing `offset` in the file `path`, or `null`. Innermost
   * means the latest start and, on equal starts, the earliest end, exactly as
   * `b2c_ir::SourceMap::lookup` (and among identical ranges the one listed last). Ranges of every
   * part count, so a click inside an input selects the block that owns it.
   */
  blockAt(path: string, offset: number): string | null;
}

/**
 * Converts 1-based (line, UTF-8 byte column) positions in one text to UTF-16 offsets. Lines are
 * separated by `\n` only, as in generated files (and in the code panel's CodeMirror state).
 */
export class LineTable {
  readonly #text: string;
  /** The UTF-16 offset where each line starts. */
  readonly #starts: number[];
  /** Per line: 0 not yet known, 1 only ASCII, 2 has other characters. */
  readonly #ascii: Uint8Array;

  constructor(text: string) {
    this.#text = text;
    const starts = [0];
    for (let index = text.indexOf('\n'); index !== -1; index = text.indexOf('\n', index + 1)) {
      starts.push(index + 1);
    }
    this.#starts = starts;
    this.#ascii = new Uint8Array(starts.length);
  }

  /** How many lines the text has (a text ending with `\n` has an empty last line). */
  get lineCount(): number {
    return this.#starts.length;
  }

  /**
   * The UTF-16 offset of a 1-based line and 1-based UTF-8 byte column. Out-of-range values are
   * clamped: before the first line is 0, after the last line is the text's length, and a column
   * past the end of its line is the end of the line (before its `\n`). A column that falls inside a
   * multi-byte character gives the offset of that character.
   */
  offsetAt(line: number, column: number): number {
    if (!(line >= 1)) {
      return 0;
    }
    if (line > this.#starts.length) {
      return this.#text.length;
    }
    const index = Math.floor(line) - 1;
    const start = this.#starts[index] ?? 0;
    const end = this.#lineEnd(index);
    const bytes = Math.floor(column) - 1;
    if (!(bytes > 0)) {
      return start;
    }
    if (this.#isAscii(index, start, end)) {
      return Math.min(start + bytes, end);
    }
    let offset = start;
    let counted = 0;
    while (offset < end) {
      const codePoint = this.#text.codePointAt(offset) ?? 0;
      const size = utf8Length(codePoint);
      if (counted + size > bytes) {
        break;
      }
      counted += size;
      offset += codePoint > 0xffff ? 2 : 1;
    }
    return offset;
  }

  /** The offset just before the line's `\n` (or the text's end for the last line). */
  #lineEnd(index: number): number {
    const next = this.#starts[index + 1];
    return next === undefined ? this.#text.length : next - 1;
  }

  #isAscii(index: number, start: number, end: number): boolean {
    let known = this.#ascii[index] ?? 0;
    if (known === 0) {
      known = 1;
      for (let offset = start; offset < end; offset++) {
        if (this.#text.charCodeAt(offset) >= 0x80) {
          known = 2;
          break;
        }
      }
      this.#ascii[index] = known;
    }
    return known === 1;
  }
}

/** How many bytes UTF-8 uses for a code point (a lone surrogate counts as U+FFFD, 3 bytes). */
function utf8Length(codePoint: number): number {
  if (codePoint < 0x80) {
    return 1;
  }
  if (codePoint < 0x800) {
    return 2;
  }
  return codePoint < 0x10000 ? 3 : 4;
}

/** One converted range of a file. */
interface IndexedRange {
  readonly from: number;
  readonly to: number;
  readonly block: string;
  /** The position in the file map's list, which breaks ties as Rust's `max_by` does. */
  readonly order: number;
}

/** The ranges of one file, sorted by start and then by end, longest first. */
interface FileIndex {
  readonly ranges: readonly IndexedRange[];
  /** `ranges[i].from`, for the binary search. */
  readonly starts: Float64Array;
  /** The index of the innermost range that contains range `i`, or -1. */
  readonly parents: Int32Array;
  /** Whether the ranges nest properly, as 06 §6.9 promises; if not, lookups scan instead. */
  readonly nested: boolean;
}

/** A range of a block part, kept for {@link SourceMapIndex.rangesForPart}. */
interface PartRange extends CodeRange {
  readonly part: Part;
}

const NO_RANGES: readonly CodeRange[] = Object.freeze([]);

/** Options of {@link buildSourceMapIndex}. */
export interface SourceMapIndexOptions {
  /** The most ranges to index; {@link MAX_INDEXED_RANGES} by default, and never more. */
  readonly maxRanges?: number;
}

/**
 * Indexes a preview's source map against its files. Ranges in files that are not in `files`, and
 * ranges that are malformed (positions that are not positive integers, or an end before the start),
 * are ignored; so is everything after {@link MAX_INDEXED_RANGES} ranges. A `null` map gives an
 * empty index.
 */
export function buildSourceMapIndex(
  map: SourceMap | null,
  files: readonly GeneratedFile[],
  options: SourceMapIndexOptions = {},
): SourceMapIndex {
  const contents = new Map<string, string>();
  for (const file of files) {
    if (!contents.has(file.path)) {
      contents.set(file.path, file.contents);
    }
  }

  const byPath = new Map<string, FileIndex>();
  const wholeByBlock = new Map<string, CodeRange[]>();
  const partsByBlock = new Map<string, PartRange[]>();
  let budget = Math.max(0, Math.min(options.maxRanges ?? MAX_INDEXED_RANGES, MAX_INDEXED_RANGES));

  for (const fileMap of map?.files ?? []) {
    const text = contents.get(fileMap.path);
    // Like `SourceMap::lookup`, the first map for a path wins.
    if (text === undefined || byPath.has(fileMap.path)) {
      continue;
    }
    const table = new LineTable(text);
    const ranges: IndexedRange[] = [];
    for (const range of fileMap.ranges) {
      if (budget === 0) {
        break;
      }
      const converted = convertRange(table, range, ranges.length);
      if (converted !== null) {
        ranges.push(converted);
        budget--;
      }
    }
    ranges.sort((a, b) => a.from - b.from || b.to - a.to || a.order - b.order);
    byPath.set(fileMap.path, indexFile(ranges));

    for (const range of ranges) {
      const original = fileMap.ranges[range.order];
      if (original === undefined) {
        continue;
      }
      const codeRange = { path: fileMap.path, from: range.from, to: range.to };
      append(partsByBlock, range.block, { ...codeRange, part: original.part });
      if (original.part.kind === 'whole') {
        append(wholeByBlock, range.block, codeRange);
      }
    }
  }

  return {
    rangesForBlock: (blockId) => wholeByBlock.get(blockId) ?? NO_RANGES,
    rangesForPart: (blockId, part) =>
      (partsByBlock.get(blockId) ?? [])
        .filter((range) => samePart(range.part, part))
        .map(({ path, from, to }) => ({ path, from, to })),
    blockAt: (path, offset) => {
      const file = byPath.get(path);
      return file === undefined ? null : innermostAt(file, offset);
    },
  };
}

/** The converted range, or `null` when it is malformed or empty. */
function convertRange(table: LineTable, range: MappedRange, order: number): IndexedRange | null {
  if (!isPosition(range.start) || !isPosition(range.end)) {
    return null;
  }
  const from = table.offsetAt(range.start.line, range.start.column);
  const to = table.offsetAt(range.end.line, range.end.column);
  return to > from ? { from, to, block: range.block, order } : null;
}

/** Whether a position has a positive integer line and column (the map is data, so check it). */
function isPosition(position: Position): boolean {
  return (
    Number.isSafeInteger(position.line) &&
    position.line >= 1 &&
    Number.isSafeInteger(position.column) &&
    position.column >= 1
  );
}

/** Computes each range's parent and whether the ranges nest properly. */
function indexFile(ranges: readonly IndexedRange[]): FileIndex {
  const starts = new Float64Array(ranges.length);
  const parents = new Int32Array(ranges.length).fill(-1);
  // The ranges that are still open at the current start: their indices and their ends.
  const openIndices: number[] = [];
  const openEnds: number[] = [];
  let nested = true;
  ranges.forEach((range, index) => {
    starts[index] = range.from;
    while (openEnds.length > 0 && (openEnds[openEnds.length - 1] ?? 0) <= range.from) {
      openEnds.pop();
      openIndices.pop();
    }
    const enclosing = openIndices[openIndices.length - 1];
    if (enclosing !== undefined) {
      if ((openEnds[openEnds.length - 1] ?? 0) < range.to) {
        nested = false;
      }
      parents[index] = enclosing;
    }
    openIndices.push(index);
    openEnds.push(range.to);
  });
  return { ranges, starts, parents, nested };
}

/** The block of the innermost range containing `offset`. */
function innermostAt(file: FileIndex, offset: number): string | null {
  // The last range that starts at or before the offset.
  let low = 0;
  let high = file.starts.length;
  while (low < high) {
    const middle = (low + high) >>> 1;
    if ((file.starts[middle] ?? 0) <= offset) {
      low = middle + 1;
    } else {
      high = middle;
    }
  }
  let index = low - 1;
  if (file.nested) {
    // Every range containing the offset encloses the one found, so the innermost one is the
    // first on its chain of enclosing ranges that contains the offset.
    while (index >= 0) {
      const range = file.ranges[index];
      if (range !== undefined && offset < range.to) {
        return range.block;
      }
      index = file.parents[index] ?? -1;
    }
    return null;
  }
  // Crossing ranges: scan back. Going backwards visits later starts first and, on equal starts,
  // earlier ends first, which is the innermost rule.
  for (; index >= 0; index--) {
    const range = file.ranges[index];
    if (range !== undefined && offset < range.to) {
      return range.block;
    }
  }
  return null;
}

function append<T>(map: Map<string, T[]>, key: string, value: T): void {
  const list = map.get(key);
  if (list === undefined) {
    map.set(key, [value]);
  } else {
    list.push(value);
  }
}

/** Whether two parts are the same part of a block. */
export function samePart(a: Part, b: Part): boolean {
  switch (a.kind) {
    case 'whole':
      return b.kind === 'whole';
    case 'field':
    case 'input':
      return b.kind === a.kind && b.name === a.name;
    case 'tokens':
      return b.kind === 'tokens' && b.input === a.input && b.start === a.start && b.end === a.end;
  }
}
