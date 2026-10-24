import { describe, expect, it } from 'vitest';

import trickyTextMain from '../../../../../tests/golden/tricky_text/main.cpp?raw';
import type { GeneratedFile, MappedRange, Part, SourceMap } from '../types';
import { buildSourceMapIndex, LineTable, MAX_INDEXED_RANGES, samePart } from './sourcemap';

const WHOLE: Part = { kind: 'whole' };

function range(
  sl: number,
  sc: number,
  el: number,
  ec: number,
  block: string,
  part: Part = WHOLE,
): MappedRange {
  return {
    start: { line: sl, column: sc },
    end: { line: el, column: ec },
    module: 'mod_main',
    block,
    part,
  };
}

function file(path: string, contents: string): GeneratedFile {
  return { path, kind: path.endsWith('.hpp') ? 'header' : 'source', contents };
}

function sourceMap(path: string, ranges: MappedRange[]): SourceMap {
  return { version: 1, files: [{ path, ranges }] };
}

/** Eleven ASCII lines of 40 characters: offset = (line - 1) * 41 + (column - 1). */
const ASCII_TEXT = `${'x'.repeat(40)}\n`.repeat(11);
const asciiOffset = (line: number, column: number) => (line - 1) * 41 + (column - 1);

/** The ranges of `b2c_ir::source_map::tests::lookup_finds_innermost`. */
const INNERMOST_RANGES = [
  range(3, 1, 10, 2, 'outer'),
  range(4, 5, 4, 20, 'before'),
  range(5, 5, 5, 30, 'inner'),
  range(5, 9, 5, 14, 'tiny'),
  range(5, 9, 5, 12, 'tinier'),
  range(6, 5, 8, 6, 'multi'),
  range(6, 9, 8, 2, 'multi_inner'),
];

/** The expectations of the same Rust test: (line, column) → block. */
const INNERMOST_VECTORS: [number, number, string | null][] = [
  [5, 10, 'tinier'],
  [5, 13, 'tiny'],
  [5, 20, 'inner'],
  [6, 6, 'multi'],
  [7, 1, 'multi_inner'],
  [8, 3, 'multi'],
  [9, 1, 'outer'],
  [11, 1, null],
];

describe('LineTable', () => {
  it('converts ASCII columns directly and clamps out-of-range positions', () => {
    const table = new LineTable('abc\ndefg\n');
    expect(table.lineCount).toBe(3);
    expect(table.offsetAt(1, 1)).toBe(0);
    expect(table.offsetAt(1, 4)).toBe(3);
    expect(table.offsetAt(2, 1)).toBe(4);
    expect(table.offsetAt(2, 5)).toBe(8);
    // Past the end of a line: the end of that line, before its line feed.
    expect(table.offsetAt(1, 99)).toBe(3);
    // The empty line after the final line feed, and lines past the end.
    expect(table.offsetAt(3, 1)).toBe(9);
    expect(table.offsetAt(4, 1)).toBe(9);
    expect(table.offsetAt(0, 5)).toBe(0);
  });

  it('counts columns in UTF-8 bytes and offsets in UTF-16 code units', () => {
    // é is 2 bytes, ✓ 3 bytes, 😀 4 bytes (two UTF-16 code units).
    const table = new LineTable('aé✓😀b\n');
    expect(table.offsetAt(1, 2)).toBe(1); // é
    expect(table.offsetAt(1, 4)).toBe(2); // ✓
    expect(table.offsetAt(1, 7)).toBe(3); // 😀
    expect(table.offsetAt(1, 11)).toBe(5); // b
    expect(table.offsetAt(1, 12)).toBe(6); // the end of the line
    // A column inside a multi-byte character gives that character's offset.
    expect(table.offsetAt(1, 3)).toBe(1);
    expect(table.offsetAt(1, 9)).toBe(3);
  });
});

describe('buildSourceMapIndex', () => {
  it('finds the innermost range exactly as b2c_ir SourceMap::lookup', () => {
    const index = buildSourceMapIndex(sourceMap('main.cpp', INNERMOST_RANGES), [
      file('main.cpp', ASCII_TEXT),
    ]);
    for (const [line, column, block] of INNERMOST_VECTORS) {
      expect(
        index.blockAt('main.cpp', asciiOffset(line, column)),
        `${String(line)}:${String(column)}`,
      ).toBe(block);
    }
    expect(index.blockAt('other.cpp', asciiOffset(5, 10))).toBeNull();
  });

  it('does not depend on the order of the ranges in the map', () => {
    const shuffled = [...INNERMOST_RANGES].reverse();
    const index = buildSourceMapIndex(sourceMap('main.cpp', shuffled), [
      file('main.cpp', ASCII_TEXT),
    ]);
    for (const [line, column, block] of INNERMOST_VECTORS) {
      expect(index.blockAt('main.cpp', asciiOffset(line, column))).toBe(block);
    }
  });

  it('prefers the range listed last among identical ranges, like max_by', () => {
    const index = buildSourceMapIndex(
      sourceMap('main.cpp', [range(1, 1, 1, 10, 'first'), range(1, 1, 1, 10, 'second')]),
      [file('main.cpp', ASCII_TEXT)],
    );
    expect(index.blockAt('main.cpp', 3)).toBe('second');
  });

  it('gives every whole range of a block in file order, and nothing else', () => {
    const files = [file('main.cpp', ASCII_TEXT), file('main.hpp', ASCII_TEXT)];
    const map: SourceMap = {
      version: 1,
      files: [
        { path: 'main.cpp', ranges: [range(2, 1, 2, 5, 'b1'), range(1, 1, 1, 3, 'b1')] },
        { path: 'main.hpp', ranges: [range(1, 1, 1, 9, 'b1')] },
      ],
    };
    const index = buildSourceMapIndex(map, files);
    expect(index.rangesForBlock('b1')).toEqual([
      { path: 'main.cpp', from: 0, to: 2 },
      { path: 'main.cpp', from: 41, to: 45 },
      { path: 'main.hpp', from: 0, to: 8 },
    ]);
    expect(index.rangesForBlock('missing')).toEqual([]);
  });

  it('finds the ranges of a part by value and leaves the whole block to the caller', () => {
    const cond: Part = { kind: 'input', name: 'COND0' };
    const index = buildSourceMapIndex(
      sourceMap('main.cpp', [range(3, 1, 10, 2, 'b4'), range(4, 5, 4, 12, 'b4', cond)]),
      [file('main.cpp', ASCII_TEXT)],
    );
    expect(index.rangesForPart('b4', { kind: 'input', name: 'COND0' })).toEqual([
      { path: 'main.cpp', from: asciiOffset(4, 5), to: asciiOffset(4, 12) },
    ]);
    expect(index.rangesForPart('b4', { kind: 'field', name: 'COND0' })).toEqual([]);
    expect(index.rangesForPart('b4', { kind: 'input', name: 'BODY' })).toEqual([]);
    // A click inside the input belongs to the block that owns the input.
    expect(index.blockAt('main.cpp', asciiOffset(4, 6))).toBe('b4');
  });

  it('ignores malformed ranges, unknown files and a missing map', () => {
    const index = buildSourceMapIndex(
      {
        version: 1,
        files: [
          {
            path: 'main.cpp',
            ranges: [
              range(0, 1, 1, 5, 'zero-line'),
              range(1, 5, 1, 2, 'reversed'),
              range(1, 1.5, 1, 9, 'fractional'),
              range(2, 3, 2, 3, 'empty'),
              range(3, 1, 3, 9, 'good'),
            ],
          },
          { path: 'gone.cpp', ranges: [range(1, 1, 1, 9, 'elsewhere')] },
        ],
      },
      [file('main.cpp', ASCII_TEXT)],
    );
    expect(index.blockAt('main.cpp', asciiOffset(1, 2))).toBeNull();
    expect(index.blockAt('main.cpp', asciiOffset(2, 3))).toBeNull();
    expect(index.blockAt('main.cpp', asciiOffset(3, 2))).toBe('good');
    expect(index.rangesForBlock('elsewhere')).toEqual([]);

    const empty = buildSourceMapIndex(null, [file('main.cpp', ASCII_TEXT)]);
    expect(empty.blockAt('main.cpp', 0)).toBeNull();
    expect(empty.rangesForBlock('good')).toEqual([]);
  });

  it('maps non-ASCII lines of the tricky_text golden main.cpp to the right UTF-16 text', () => {
    const files = [file('main.cpp', trickyTextMain)];
    // Line 10: `    std::cout << "Unicode: héllo ✓ — 日本" << '\n';`. The literal starts at byte column
    // 18 and is 32 bytes long; the statement starts at column 5 and ends before column 59.
    const literal = range(10, 18, 10, 50, 'b_literal', { kind: 'input', name: 'VALUE' });
    const statement = range(10, 5, 10, 59, 'b_print');
    // Line 2 has an en dash (3 bytes) before "changes": 65 UTF-16 code units, 67 bytes.
    const banner = range(2, 1, 2, 68, 'b_banner');
    const index = buildSourceMapIndex(sourceMap('main.cpp', [banner, statement, literal]), files);

    const [whole] = index.rangesForBlock('b_print');
    expect(whole).toBeDefined();
    expect(trickyTextMain.slice(whole?.from, whole?.to)).toBe(
      `std::cout << "Unicode: héllo ✓ — 日本" << '\\n';`,
    );
    const [value] = index.rangesForPart('b_literal', { kind: 'input', name: 'VALUE' });
    expect(trickyTextMain.slice(value?.from, value?.to)).toBe('"Unicode: héllo ✓ — 日本"');

    const tick = trickyTextMain.indexOf('✓');
    expect(index.blockAt('main.cpp', tick)).toBe('b_literal');
    expect(index.blockAt('main.cpp', trickyTextMain.indexOf('std::cout << "Unicode'))).toBe(
      'b_print',
    );
    const [bannerRange] = index.rangesForBlock('b_banner');
    expect(trickyTextMain.slice(bannerRange?.from, bannerRange?.to)).toBe(
      '// Edit the blocks, not this file – changes here are overwritten.',
    );
  });

  it('agrees with a brute-force lookup on random maps, nested or not', () => {
    let seed = 0x2545f491;
    const random = (limit: number) => {
      // xorshift32: deterministic, so a failure can be reproduced.
      seed ^= seed << 13;
      seed ^= seed >>> 17;
      seed ^= seed << 5;
      return (seed >>> 0) % limit;
    };
    const text = 'aé✓😀 b\n'.repeat(6);
    const table = new LineTable(text);
    for (let round = 0; round < 200; round++) {
      const ranges: MappedRange[] = [];
      const count = 1 + random(12);
      for (let i = 0; i < count; i++) {
        const sl = 1 + random(6);
        const el = sl + random(3);
        ranges.push(range(sl, 1 + random(12), el, 1 + random(12), `b${String(i)}`));
      }
      const index = buildSourceMapIndex(sourceMap('main.cpp', ranges), [file('main.cpp', text)]);
      const converted = ranges.map((r, order) => ({
        from: table.offsetAt(r.start.line, r.start.column),
        to: table.offsetAt(r.end.line, r.end.column),
        block: r.block,
        order,
      }));
      for (let offset = 0; offset <= text.length; offset++) {
        let best: (typeof converted)[number] | null = null;
        for (const candidate of converted) {
          if (candidate.from <= offset && offset < candidate.to) {
            // max_by keeps the last of equal elements.
            if (
              best === null ||
              candidate.from > best.from ||
              (candidate.from === best.from && candidate.to <= best.to)
            ) {
              best = candidate;
            }
          }
        }
        expect(index.blockAt('main.cpp', offset)).toBe(best?.block ?? null);
      }
    }
  });

  it('stops indexing at the range limit', () => {
    const ranges = [range(1, 1, 1, 9, 'b1'), range(2, 1, 2, 9, 'b2'), range(3, 1, 3, 9, 'b3')];
    const index = buildSourceMapIndex(
      sourceMap('main.cpp', ranges),
      [file('main.cpp', ASCII_TEXT)],
      {
        maxRanges: 2,
      },
    );
    expect(index.blockAt('main.cpp', asciiOffset(2, 2))).toBe('b2');
    expect(index.blockAt('main.cpp', asciiOffset(3, 2))).toBeNull();
    // The limit can be lowered but never raised.
    expect(MAX_INDEXED_RANGES).toBe(1_000_000);
  });
});

describe('samePart', () => {
  it('compares parts by value', () => {
    expect(samePart(WHOLE, { kind: 'whole' })).toBe(true);
    expect(samePart({ kind: 'field', name: 'A' }, { kind: 'field', name: 'A' })).toBe(true);
    expect(samePart({ kind: 'field', name: 'A' }, { kind: 'input', name: 'A' })).toBe(false);
    const tokens: Part = { kind: 'tokens', input: 'E', start: 0, end: 2 };
    expect(samePart(tokens, { kind: 'tokens', input: 'E', start: 0, end: 2 })).toBe(true);
    expect(samePart(tokens, { kind: 'tokens', input: 'E', start: 0, end: 3 })).toBe(false);
    expect(samePart(tokens, WHOLE)).toBe(false);
  });
});
