import { describe, expect, it } from 'vitest';

import { readCatalogJson } from '../src/catalog-json.ts';
import { catalogTs } from '../src/emit-ts.ts';
import { normalize } from '../src/normalize.ts';
import { fixture, rustBlock } from './fixture.ts';

function model(change?: (catalog: ReturnType<typeof fixture>) => void) {
  const catalog = fixture();
  change?.(catalog);
  return normalize(readCatalogJson(JSON.stringify(catalog)));
}

function block(id: string) {
  const found = model().blocks.find((b) => b.id === id);
  if (found === undefined) {
    throw new Error(`no block ${id}`);
  }
  return found;
}

describe('normalize', () => {
  it('renames the Rust keys and parses the friendly label', () => {
    const loop = block('control.loop');
    expect(loop).toEqual({
      id: 'control.loop',
      version: 1,
      category: 'loops',
      shape: 'statement',
      label: { friendly: 'repeat %MODE %COND', cpp: 'while (%COND) { … }' },
      labelParts: [{ text: 'repeat' }, { arg: 'MODE' }, { arg: 'COND' }],
      statementsNotInLabel: ['BODY'],
      help: 'Repeats <blocks> | *while* a [condition] holds.',
      output: null,
      fields: [
        {
          name: 'MODE',
          kind: 'dropdown',
          options: [
            ['while', 'while'],
            ['until', 'until'],
          ],
          types: [],
          default: 'while',
          type: null,
          repeat: null,
        },
      ],
      inputs: [
        {
          name: 'COND',
          check: 'bool',
          optional: false,
          repeat: null,
          default: [{ kw: 'true' }],
          type: null,
          assign: false,
        },
      ],
      statements: [{ name: 'BODY', repeat: null, when: null }],
      extra: [],
      typeVars: {},
      order: [],
      placeVar: null,
      members: {},
      headers: [],
      optionHeaders: {},
    });
  });

  it('lists only the statements the label does not mention', () => {
    expect(block('control.pick').statementsNotInLabel).toEqual([]);
    expect(block('functions.make').statementsNotInLabel).toEqual(['BODY']);
  });

  it('turns count defaults into numbers and drops keys that do not apply', () => {
    expect(block('control.pick').extra).toEqual([
      {
        name: 'elseIfCount',
        kind: 'count',
        min: 0,
        max: 32,
        default: 0,
        types: [],
        typeClass: null,
      },
      {
        name: 'hasElse',
        kind: 'flag',
        min: null,
        max: null,
        default: false,
        types: [],
        typeClass: null,
      },
    ]);
    expect(block('functions.make').extra).toEqual([
      {
        name: 'params',
        kind: 'params',
        min: null,
        max: 16,
        default: null,
        types: ['int'],
        typeClass: null,
      },
    ]);
    const noDefault = model((c) =>
      c.blocks.push(
        rustBlock('x.y', {
          extra: [{ name: 'n', kind: 'count', min: 0, max: 3, default: null, types: [] }],
        }),
      ),
    );
    expect(noDefault.blocks.at(-1)?.extra[0]?.default).toBeNull();
  });

  it('marks a field the editor writes only when it differs from its default (W3M-D11)', () => {
    const field = (omitDefault?: boolean) => ({
      name: 'SHARED',
      kind: 'checkbox' as const,
      options: [],
      types: [],
      default: false,
      ...(omitDefault === undefined ? {} : { omitDefault }),
    });
    const fieldsOf = (omitDefault?: boolean) =>
      model((c) =>
        c.blocks.push(
          rustBlock('x.shared', {
            label: { friendly: 'thing share %SHARED', cpp: 'thing' },
            field: [field(omitDefault)],
          }),
        ),
      ).blocks.at(-1)?.fields;
    expect(fieldsOf(true)?.[0]).toMatchObject({
      name: 'SHARED',
      default: false,
      omitDefault: true,
    });
    // Absent or false, the key is left out of the generated definition.
    expect(fieldsOf(false)?.[0]).not.toHaveProperty('omitDefault');
    expect(fieldsOf()?.[0]).not.toHaveProperty('omitDefault');
  });

  it('reads the row extras, repeated fields and header fields of milestone M3 (w2.md §2.1)', () => {
    const rows = model((c) =>
      c.blocks.push(
        rustBlock('type.struct', {
          category: 'types',
          shape: 'definition',
          label: { friendly: 'struct %NAME has …', cpp: 'struct %NAME { … };' },
          field: [{ name: 'NAME', kind: 'symbol_decl', options: [], types: [], default: null }],
          input: [
            {
              name: 'DEFAULT',
              check: 'any',
              optional: true,
              repeat: { count: 'fields', plus: 0 },
              default: [],
            },
          ],
          extra: [
            {
              name: 'fields',
              kind: 'fields',
              min: 0,
              max: 64,
              default: null,
              types: ['int', { 'std::vector': 'int' }],
              typeClass: 'value',
            },
          ],
        }),
        rustBlock('type.enum', {
          category: 'types',
          shape: 'definition',
          extra: [{ name: 'values', kind: 'values', min: 0, max: 64, default: null, types: [] }],
        }),
        rustBlock('error.try', {
          category: 'errors',
          label: { friendly: 'try %TRY catch %ERROR as %NAME %CATCH catch …', cpp: 'try' },
          field: [
            {
              name: 'ERROR',
              kind: 'type',
              options: [],
              types: ['std::exception', 'std::runtime_error'],
              default: 'std::exception',
              repeat: { count: 'catchCount', plus: 0 },
            },
            {
              name: 'NAME',
              kind: 'symbol_decl',
              options: [],
              types: [],
              default: null,
              type: { kind: 'field', field: 'ERROR' },
              repeat: { count: 'catchCount', plus: 0 },
            },
            {
              name: 'SHARED',
              kind: 'checkbox',
              options: [],
              types: [],
              default: false,
              repeat: null,
            },
          ],
          statement: [
            { name: 'TRY', repeat: null, when: null },
            { name: 'CATCH', repeat: { count: 'catchCount', plus: 0 }, when: null },
          ],
          extra: [{ name: 'catchCount', kind: 'count', min: 1, max: 8, default: '1', types: [] }],
        }),
        rustBlock('program.use_header', {
          category: 'program',
          shape: 'definition',
          field: [{ name: 'HEADER', kind: 'header', options: [], types: [], default: '<cmath>' }],
        }),
      ),
    );
    const find = (id: string) => rows.blocks.find((b) => b.id === id);
    expect(find('type.struct')?.category).toBe('types');
    expect(find('type.struct')?.extra).toEqual([
      {
        name: 'fields',
        kind: 'fields',
        min: null,
        max: 64,
        default: null,
        types: ['int', '{"std::vector":"int"}'],
        typeClass: 'value',
      },
    ]);
    expect(find('type.struct')?.inputs[0]?.repeat).toEqual({ count: 'fields', plus: 0 });
    expect(find('type.enum')?.extra).toEqual([
      {
        name: 'values',
        kind: 'values',
        min: null,
        max: 64,
        default: null,
        types: [],
        typeClass: null,
      },
    ]);
    const tryBlock = find('error.try');
    expect(tryBlock?.category).toBe('errors');
    expect(tryBlock?.fields.map((field) => [field.name, field.repeat])).toEqual([
      ['ERROR', { count: 'catchCount', plus: 0 }],
      ['NAME', { count: 'catchCount', plus: 0 }],
      ['SHARED', null],
    ]);
    expect(tryBlock?.fields[1]?.type).toEqual({ kind: 'field', field: 'ERROR' });
    expect(tryBlock?.labelParts).toEqual([
      { text: 'try' },
      { arg: 'TRY' },
      { text: 'catch' },
      { arg: 'ERROR' },
      { text: 'as' },
      { arg: 'NAME' },
      { arg: 'CATCH' },
      { text: 'catch' },
      { repeat: true },
    ]);
    expect(find('program.use_header')?.fields[0]).toMatchObject({
      kind: 'header',
      default: '<cmath>',
      repeat: null,
    });
  });

  it('reads the categories W2 adds, also without entries', () => {
    const categories = model((c) => {
      for (const id of ['collections', 'types', 'files', 'errors']) {
        c.toolbox.category.push({
          id,
          name: id,
          icon: '?',
          colour: id,
          dynamic: null,
          entry: [],
        });
      }
    });
    expect(categories.toolbox.slice(-4).map((category) => [category.id, category.entries])).toEqual(
      [
        ['collections', []],
        ['types', []],
        ['files', []],
        ['errors', []],
      ],
    );
  });

  it('refuses defaults it cannot convert', () => {
    for (const [kind, value] of [
      ['count', '01'],
      ['count', 'x'],
      ['count', true],
      ['flag', 'yes'],
    ] as const) {
      expect(() =>
        model((c) =>
          c.blocks.push(
            rustBlock('x.y', {
              extra: [{ name: 'n', kind, min: 0, max: 3, default: value, types: [] }],
            }),
          ),
        ),
      ).toThrow(/blocks\[5\]\.extra\[0\]\.default/u);
    }
  });

  it('keeps every output pattern as the catalog writes it and refuses an empty one', () => {
    expect(block('functions.use').output).toBe('symbol');
    expect(block('text.show').output).toBe('field:TO');
    // The catalog checked the patterns; `std::string` is spelled as everywhere else (types.md §3.1).
    for (const output of [
      'any',
      'bool',
      'int',
      'double',
      'number',
      'char',
      'std::string',
      'elem(C)',
    ]) {
      const result = model((c) => c.blocks.push(rustBlock('x.y', { shape: 'reporter', output })));
      expect(result.blocks.at(-1)?.output).toBe(output);
    }
    expect(() =>
      model((c) => c.blocks.push(rustBlock('x.y', { shape: 'reporter', output: ' ' }))),
    ).toThrow('blocks[5].output is an empty output type');
  });

  it('carries the signature keys, with empty values where the export leaves them out', () => {
    const typed = model((c) =>
      c.blocks.push(
        rustBlock('coll.item', {
          shape: 'reporter',
          output: 'elem(C)',
          headers: ['<vector>'],
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
            {
              name: 'TYPE',
              kind: 'type',
              options: [],
              types: ['int', { 'std::vector': 'int' }, { 'std::map': ['std::string', 'int'] }],
              default: 'int',
            },
          ],
          input: [
            {
              name: 'INDEX',
              check: 'any',
              optional: false,
              repeat: null,
              default: [],
              type: { kind: 'fn', fn: 'index', of: { kind: 'var', name: 'C' } },
              assign: true,
            },
          ],
          extra: [
            {
              name: 'params',
              kind: 'params',
              min: 0,
              max: 16,
              default: null,
              types: ['int'],
              typeClass: 'value',
            },
          ],
          order: [{ field: 'LIST' }, { input: 'INDEX' }],
          placeVar: null,
          members: { MEMBER: 'S' },
          optionHeaders: { FN: { sqrt: ['<cmath>'] } },
        }),
      ),
    ).blocks.at(-1);
    expect(typed?.typeVars).toEqual({ C: 'indexable' });
    expect(typed?.order).toEqual([{ field: 'LIST' }, { input: 'INDEX' }]);
    expect(typed?.members).toEqual({ MEMBER: 'S' });
    expect(typed?.headers).toEqual(['<vector>']);
    expect(typed?.optionHeaders).toEqual({ FN: { sqrt: ['<cmath>'] } });
    expect(typed?.fields[0]?.type).toEqual({ kind: 'var', name: 'C' });
    // A type list holds type field values: names as they are, templates by their keys.
    expect(typed?.fields[1]?.types).toEqual([
      'int',
      '{"std::vector":"int"}',
      '{"std::map":["std::string","int"]}',
    ]);
    expect(typed?.fields[1]?.type).toBeNull();
    expect(typed?.inputs[0]?.type).toEqual({
      kind: 'fn',
      fn: 'index',
      of: { kind: 'var', name: 'C' },
    });
    expect(typed?.inputs[0]?.assign).toBe(true);
    expect(typed?.extra[0]?.typeClass).toBe('value');
  });

  it('generates the type table', () => {
    const types = model((c) => {
      c.types = [
        { name: 'int', arity: 0, label: 'int', header: null },
        { name: 'std::vector', arity: 1, label: 'list of %1', header: '<vector>' },
      ];
    }).types;
    expect(types).toEqual([
      { name: 'int', arity: 0, label: 'int', header: null },
      { name: 'std::vector', arity: 1, label: 'list of %1', header: '<vector>' },
    ]);
    expect(model().types).toEqual([]);
  });

  it('converts every token kind', () => {
    expect(block('text.show').inputs[1]?.default).toEqual([
      { num: '1' },
      { op: '+' },
      { chr: "'" },
      { str: 'a "b"' },
      { text: 'x y' },
    ]);
    const refs = model((c) =>
      c.blocks.push(
        rustBlock('x.y', {
          input: [
            {
              name: 'A',
              check: 'any',
              optional: false,
              repeat: null,
              default: [{ ref: 'sym_a' }, { kw: 'false' }],
            },
          ],
        }),
      ),
    );
    expect(refs.blocks.at(-1)?.inputs[0]?.default).toEqual([{ ref: 'sym_a' }, { kw: 'false' }]);
  });

  it('renames toolbox keys and keeps only the preset keys that are set', () => {
    const { toolbox } = model();
    expect(toolbox.map((c) => c.id)).toEqual(['text', 'control', 'loops', 'functions']);
    expect(toolbox[0]?.entries).toEqual([
      {
        block: 'text.show',
        label: 'show *it*',
        preset: { fields: { ON: false }, inputs: { PROMPT: [{ str: 'Hi: ' }] } },
      },
    ]);
    expect(toolbox[1]?.entries[1]?.preset).toEqual({ extra: { hasElse: true, elseIfCount: 2 } });
    expect(toolbox[1]?.entries[0]?.preset).toBeNull();
    expect(toolbox[3]?.dynamic).toBe('functions');
  });
});

describe('catalogTs', () => {
  it('copies the types without their file comment and writes the data as JSON', () => {
    const types = '// A file comment.\n// More.\n\nexport type A = 1;\n';
    const text = catalogTs(model(), types);
    expect(text.startsWith('// @generated by @blocks2cpp/catalog-gen')).toBe(true);
    expect(text).not.toContain('A file comment');
    expect(text).toContain('export type A = 1;');
    expect(text).toContain('export const CATALOG_VERSION = "9.8.7";');
    expect(text).toContain(
      'export const BLOCK_DEFS: readonly BlockDefJson[] = [{"id":"control.loop"',
    );
    expect(text).toContain('export const TOOLBOX: readonly ToolboxCategoryJson[] = [{"id":"text"');
    expect(text.endsWith(';\n')).toBe(true);
    expect(catalogTs(model(), '// only a comment\n')).toContain('export const CATALOG_VERSION');
  });
});
