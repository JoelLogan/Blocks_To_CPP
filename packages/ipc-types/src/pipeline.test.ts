import { describe, expect, expectTypeOf, it } from 'vitest';

import type {
  FileKind,
  GeneratedFile,
  PassMode,
  SourceMap,
  StaticType,
  SymbolInfo,
  SymbolInfoKind,
} from './index';

// The pipeline's shared shapes (06 §6.5, §6.6, §6.9), checked against the JSON the Rust
// pipeline writes. crates/b2c-ipc/tests/pipeline.rs checks that the Rust copies behind these
// types serialise byte for byte like the shared b2c-ir types.
describe('pipeline types', () => {
  it('describe the spec example of a symbol', () => {
    const info = JSON.parse(
      '{"id":"s_guess","name":"guess","kind":"variable","isConst":false,"type":"int","module":"mod_main","declBlock":"b003"}',
    ) as SymbolInfo;
    const expected = {
      id: 's_guess',
      name: 'guess',
      kind: 'variable',
      isConst: false,
      type: 'int',
      module: 'mod_main',
      declBlock: 'b003',
    } satisfies SymbolInfo;
    expect(info).toEqual(expected);
  });

  it('narrow a symbol by its kind', () => {
    const symbols: SymbolInfo[] = [
      {
        id: 'p_text',
        name: 'text',
        kind: 'parameter',
        mode: 'read_only',
        type: 'string',
        module: 'mod_main',
        declBlock: 'fn_f',
      },
      { id: 's_i', name: 'i', kind: 'loopVariable', type: 'int', module: 'm', declBlock: 'b_for' },
      {
        id: 'f_area',
        name: 'area',
        kind: 'function',
        params: ['p_w', 'p_h'],
        returns: 'double',
        type: 'double',
        module: 'm',
        declBlock: 'fn_area',
      },
    ];
    const details = symbols.map((symbol) => {
      switch (symbol.kind) {
        case 'parameter':
          return symbol.mode;
        case 'function':
          return `${symbol.params.join(',')}->${symbol.returns}`;
        case 'variable':
          return String(symbol.isConst);
        case 'loopVariable':
          return 'counter';
      }
    });
    expect(details).toEqual(['read_only', 'counter', 'p_w,p_h->double']);
    expectTypeOf<SymbolInfoKind['kind']>().toEqualTypeOf<
      'variable' | 'parameter' | 'loopVariable' | 'function'
    >();
  });

  it('list every static type, pass mode and file kind', () => {
    expectTypeOf<StaticType>().toEqualTypeOf<
      'void' | 'bool' | 'char' | 'int' | 'double' | 'string' | 'error'
    >();
    expectTypeOf<PassMode>().toEqualTypeOf<'copy' | 'editable' | 'read_only'>();
    expectTypeOf<FileKind>().toEqualTypeOf<'source' | 'header'>();
  });

  it('describe a generated file and its source map', () => {
    const file = {
      path: 'main.cpp',
      kind: 'source',
      contents: 'int main() {\n}\n',
    } satisfies GeneratedFile;
    const map = {
      version: 1,
      files: [
        {
          path: file.path,
          ranges: [
            {
              start: { line: 1, column: 1 },
              end: { line: 2, column: 2 },
              module: 'mod_main',
              block: 'b001',
              part: { kind: 'tokens', input: 'EXPR', start: 0, end: 3 },
            },
          ],
        },
      ],
    } satisfies SourceMap;
    expect(map.files[0]?.ranges[0]?.part.kind).toBe('tokens');
  });
});
