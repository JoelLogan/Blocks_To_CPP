import { readFileSync } from 'node:fs';

import { describe, expect, it } from 'vitest';

import { CatalogJsonError, MAX_CATALOG_JSON_BYTES, readCatalogJson } from '../src/catalog-json.ts';
import { fixture, fixtureText, repoPath, rustBlock } from './fixture.ts';

/** A value input in the Rust shape, to which tests add a signature's keys. */
const SIGNED_INPUT = { name: 'A', check: 'any', optional: false, repeat: null, default: [] };

/** The error message for a changed fixture. */
function errorFor(change: (catalog: ReturnType<typeof fixture>) => void): string {
  const catalog = fixture();
  change(catalog);
  try {
    readCatalogJson(JSON.stringify(catalog));
  } catch (error) {
    expect(error).toBeInstanceOf(CatalogJsonError);
    return (error as CatalogJsonError).message;
  }
  throw new Error('the changed fixture was accepted');
}

describe('readCatalogJson', () => {
  it('decodes the export shape', () => {
    const catalog = readCatalogJson(fixtureText());
    expect(catalog.catalogVersion).toBe('9.8.7');
    expect(catalog.blocks.map((b) => b.id)).toEqual([
      'control.loop',
      'control.pick',
      'functions.make',
      'functions.use',
      'text.show',
    ]);
    const show = catalog.blocks[4];
    expect(show?.input[1]?.default).toEqual([
      { kind: 'num', text: '1' },
      { kind: 'op', text: '+' },
      { kind: 'chr', text: "'" },
      { kind: 'str', text: 'a "b"' },
      { kind: 'text', text: 'x y' },
    ]);
    const preset = catalog.toolbox.category[0]?.entry[0]?.preset;
    expect(preset?.fields).toEqual(new Map([['ON', false]]));
    expect(preset?.extra).toEqual(new Map());
    expect(preset?.inputs.get('PROMPT')).toEqual([{ kind: 'str', text: 'Hi: ' }]);
  });

  it('rejects text that is not JSON or too large', () => {
    expect(() => readCatalogJson('{')).toThrow(/catalog\.json: the document is not JSON/u);
    expect(() => readCatalogJson(' '.repeat(MAX_CATALOG_JSON_BYTES + 1))).toThrow(
      /is larger than/u,
    );
    expect(() => readCatalogJson('[]')).toThrow('catalog.json: the document should be an object');
  });

  it.each<[string, (catalog: ReturnType<typeof fixture>) => void, string]>([
    [
      'an unknown top-level key',
      (c) => Object.assign(c, { extra: 1 }),
      'the document has the unknown key "extra"',
    ],
    [
      'a missing key',
      (c) => Object.assign(c, { catalogVersion: undefined }),
      'catalogVersion should be a string',
    ],
    [
      'an unknown block key',
      (c) => c.blocks.push(rustBlock('x.y', { requires: 'c++20' })),
      'blocks[5] has the unknown key "requires"',
    ],
    [
      'a bad shape',
      (c) => c.blocks.push(rustBlock('x.y', { shape: 'round' })),
      'blocks[5].shape should be one of hat',
    ],
    [
      'a bad category',
      (c) => c.blocks.push(rustBlock('x.y', { category: 'lists' })),
      'blocks[5].category should be one of program',
    ],
    [
      'a fractional version',
      (c) => c.blocks.push(rustBlock('x.y', { version: 1.5 })),
      'blocks[5].version should be a whole number',
    ],
    [
      'a bad field kind',
      (c) =>
        c.blocks.push(
          rustBlock('x.y', {
            field: [{ name: 'A', kind: 'slider', options: [], types: [], default: null }],
          }),
        ),
      'blocks[5].field[0].kind',
    ],
    [
      'a bad option',
      (c) =>
        c.blocks.push(
          rustBlock('x.y', {
            field: [{ name: 'A', kind: 'dropdown', options: [['a']], types: [], default: null }],
          }),
        ),
      'blocks[5].field[0].options[0] should be a [label, value] pair',
    ],
    [
      'a numeric default',
      (c) =>
        c.blocks.push(
          rustBlock('x.y', {
            field: [{ name: 'A', kind: 'number', options: [], types: [], default: 3 }],
          }),
        ),
      'blocks[5].field[0].default should be a string or true or false',
    ],
    [
      'a two-key token',
      (c) =>
        c.blocks.push(
          rustBlock('x.y', {
            input: [
              {
                name: 'A',
                check: 'any',
                optional: false,
                repeat: null,
                default: [{ num: '1', str: '1' }],
              },
            ],
          }),
        ),
      'exactly one key',
    ],
    [
      'an unknown token kind',
      (c) =>
        c.blocks.push(
          rustBlock('x.y', {
            input: [
              { name: 'A', check: 'any', optional: false, repeat: null, default: [{ sym: 'x' }] },
            ],
          }),
        ),
      'has the unknown key "sym"',
    ],
    [
      'a non-string token',
      (c) =>
        c.blocks.push(
          rustBlock('x.y', {
            input: [
              { name: 'A', check: 'any', optional: false, repeat: null, default: [{ num: 1 }] },
            ],
          }),
        ),
      'default[0].num should be a string',
    ],
    [
      'a bad check',
      (c) =>
        c.blocks.push(
          rustBlock('x.y', {
            input: [{ name: 'A', check: 'list', optional: false, repeat: null, default: [] }],
          }),
        ),
      'input[0].check',
    ],
    [
      'a bad repeat',
      (c) =>
        c.blocks.push(
          rustBlock('x.y', {
            statement: [{ name: 'A', repeat: { count: 'n', plus: -1 }, when: null }],
          }),
        ),
      'statement[0].repeat.plus should be a whole number',
    ],
    [
      'a bad field repeat',
      (c) =>
        c.blocks.push(
          rustBlock('x.y', {
            field: [
              {
                name: 'A',
                kind: 'checkbox',
                options: [],
                types: [],
                default: false,
                repeat: { count: 'n' },
              },
            ],
          }),
        ),
      'field[0].repeat.plus should be a whole number',
    ],
    [
      'a bad extra kind',
      (c) =>
        c.blocks.push(
          rustBlock('x.y', {
            extra: [{ name: 'n', kind: 'list', min: 0, max: 1, default: null, types: [] }],
          }),
        ),
      'extra[0].kind',
    ],
    [
      'a bad dynamic kind',
      (c) => Object.assign(c.toolbox.category[0] ?? {}, { dynamic: 'lists' }),
      'toolbox.category[0].dynamic should be one of variables, functions',
    ],
    [
      'a bad preset extra',
      (c) =>
        Object.assign(c.toolbox.category[1] ?? {}, {
          entry: [{ block: 'b', label: null, preset: { extra: { n: 'x' } } }],
        }),
      'toolbox.category[1].entry[0].preset.extra.n should be a whole number',
    ],
    [
      'a preset that is not an object',
      (c) =>
        Object.assign(c.toolbox.category[1] ?? {}, {
          entry: [{ block: 'b', label: null, preset: { fields: [] } }],
        }),
      'preset.fields should be an object',
    ],
    [
      'a toolbox that is not an object',
      (c) => Object.assign(c, { toolbox: [] }),
      'toolbox should be an object',
    ],
    [
      'blocks that are not an array',
      (c) => Object.assign(c, { blocks: {} }),
      'blocks should be an array',
    ],
    [
      'a non-boolean flag',
      (c) =>
        c.blocks.push(
          rustBlock('x.y', {
            input: [{ name: 'A', check: 'any', optional: 'no', repeat: null, default: [] }],
          }),
        ),
      'input[0].optional should be true or false',
    ],
    [
      'an unknown pattern kind',
      (c) =>
        c.blocks.push(
          rustBlock('x.y', {
            input: [{ ...SIGNED_INPUT, type: { kind: 'list' } }],
          }),
        ),
      'input[0].type.kind should be one of any',
    ],
    [
      'a pattern with a stray key',
      (c) =>
        c.blocks.push(
          rustBlock('x.y', {
            input: [{ ...SIGNED_INPUT, type: { kind: 'result', field: 'F' } }],
          }),
        ),
      'input[0].type has the unknown key "field"',
    ],
    [
      'a pattern nested too deep',
      (c) => {
        let deep: object = { kind: 'var', name: 'C' };
        for (let i = 0; i < 9; i += 1) {
          deep = { kind: 'fn', fn: 'elem', of: deep };
        }
        c.blocks.push(rustBlock('x.y', { input: [{ ...SIGNED_INPUT, type: deep }] }));
      },
      'nests more than 8 levels',
    ],
    [
      'a type that is a one-element list',
      (c) =>
        c.blocks.push(
          rustBlock('x.y', {
            input: [{ ...SIGNED_INPUT, type: { kind: 'type', type: { 'std::map': ['int'] } } }],
          }),
        ),
      'input[0].type.type.std::map should list two or more arguments',
    ],
    [
      'a type with a prototype key',
      (c) =>
        c.blocks.push(
          rustBlock('x.y', {
            input: [
              {
                ...SIGNED_INPUT,
                type: { kind: 'type', type: JSON.parse('{"__proto__": "int"}') as unknown },
              },
            ],
          }),
        ),
      'should be a type',
    ],
    [
      'a bad class of a type variable',
      (c) => c.blocks.push(rustBlock('x.y', { typeVars: { T: 'lists' } })),
      'blocks[5].typeVars.T should be one of any',
    ],
    [
      'a part that is neither a field nor an input',
      (c) => c.blocks.push(rustBlock('x.y', { order: [{ field: 'A', input: 'B' }] })),
      'blocks[5].order[0] should be {"field": name} or {"input": name}',
    ],
    [
      'a non-boolean assign',
      (c) => c.blocks.push(rustBlock('x.y', { input: [{ ...SIGNED_INPUT, assign: 'yes' }] })),
      'input[0].assign should be true or false',
    ],
    [
      'a bad row class',
      (c) =>
        c.blocks.push(
          rustBlock('x.y', {
            extra: [
              {
                name: 'params',
                kind: 'params',
                min: 0,
                max: 1,
                default: null,
                types: ['int'],
                typeClass: 'lists',
              },
            ],
          }),
        ),
      'extra[0].typeClass should be one of any',
    ],
    [
      'a bad types entry',
      (c) => Object.assign(c, { types: [{ name: 'int', arity: -1, label: 'int', header: null }] }),
      'types[0].arity should be a whole number',
    ],
  ])('rejects %s', (_, change, message) => {
    expect(errorFor(change)).toContain(message);
  });

  it('decodes the signatures of catalog 1.3.0', () => {
    const catalog = fixture();
    const signed = rustBlock('coll.item', {
      shape: 'reporter',
      output: 'elem(C)',
      typeVars: { C: 'indexable' },
      field: [
        {
          name: 'LIST',
          kind: 'symbol_ref',
          options: [],
          types: [],
          default: null,
          type: { kind: 'var', name: 'C' },
        },
      ],
      input: [
        {
          ...SIGNED_INPUT,
          type: { kind: 'fn', fn: 'index', of: { kind: 'var', name: 'C' } },
          assign: true,
        },
      ],
      order: [{ field: 'LIST' }, { input: 'A' }],
      placeVar: null,
      members: { MEMBER: 'S' },
      optionHeaders: { FN: { sqrt: ['<cmath>'] } },
    });
    const types = [
      { name: 'std::string', arity: 0, label: 'string', header: '<string>' },
      { name: 'std::map', arity: 2, label: 'map from %1 to %2', header: '<map>' },
    ];
    Object.assign(catalog, { types });
    catalog.blocks.push(signed);
    const decoded = readCatalogJson(JSON.stringify(catalog));
    expect(decoded.types).toEqual(types);
    expect(decoded.blocks.at(-1)).toEqual(signed);
    // A block without a signature has none of the keys.
    expect(Object.keys(decoded.blocks[0] ?? {})).not.toContain('typeVars');
    expect(Object.keys(decoded.blocks[0]?.input[0] ?? {})).not.toContain('type');
    // Every pattern form and a nested type term.
    const forms = [
      { kind: 'any' },
      { kind: 'number' },
      { kind: 'result' },
      { kind: 'type', type: { 'std::map': ['std::string', { 'std::vector': { ref: 'sym_Pt' } }] } },
      { kind: 'lib', name: 'std::vector', args: [{ kind: 'var', name: 'T' }] },
      { kind: 'field', field: 'TO' },
      { kind: 'symbol', field: null },
      { kind: 'symbol', field: 'VAR' },
      { kind: 'param', field: 'FUNC' },
      { kind: 'member', field: 'TYPE' },
    ];
    const many = fixture();
    many.blocks.push(rustBlock('x.y', { input: forms.map((type) => ({ ...SIGNED_INPUT, type })) }));
    expect(
      readCatalogJson(JSON.stringify(many))
        .blocks.at(-1)
        ?.input.map((i) => i.type),
    ).toEqual(forms);
  });

  it('reads the exported catalog', () => {
    const exported = readCatalogJson(
      readFileSync(repoPath('packages/catalog-gen/catalog.json'), 'utf8'),
    );
    const declare = exported.blocks.find((b) => b.id === 'var.declare');
    expect(declare?.typeVars).toEqual({ T: 'value' });
    expect(declare?.order).toEqual([{ field: 'TYPE' }, { field: 'NAME' }, { input: 'VALUE' }]);
    expect(exported.types?.find((t) => t.name === 'std::vector')?.label).toBe('list of %1');
    // Every SHARED field is written only when ticked (w3-modules.md §6 W3M-D11).
    const shared = exported.blocks.flatMap((b) => b.field.filter((f) => f.name === 'SHARED'));
    expect(shared.map((f) => f.omitDefault)).toEqual([true, true, true, true]);
    expect(exported.blocks.find((b) => b.id === 'func.define')?.field.map((f) => f.name)).toContain(
      'SHARED',
    );
  });

  it('names the place of the problem', () => {
    const error = new CatalogJsonError('blocks[1].id', 'should be a string');
    expect(error.path).toBe('blocks[1].id');
    expect(error.name).toBe('CatalogJsonError');
    expect(error.message).toBe('catalog.json: blocks[1].id should be a string');
  });
});
