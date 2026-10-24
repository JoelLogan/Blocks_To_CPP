import { describe, expect, it } from 'vitest';

import { CatalogJsonError, MAX_CATALOG_JSON_BYTES, readCatalogJson } from '../src/catalog-json.ts';
import { fixture, fixtureText, rustBlock } from './fixture.ts';

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
  ])('rejects %s', (_, change, message) => {
    expect(errorFor(change)).toContain(message);
  });

  it('names the place of the problem', () => {
    const error = new CatalogJsonError('blocks[1].id', 'should be a string');
    expect(error.path).toBe('blocks[1].id');
    expect(error.name).toBe('CatalogJsonError');
    expect(error.message).toBe('catalog.json: blocks[1].id should be a string');
  });
});
