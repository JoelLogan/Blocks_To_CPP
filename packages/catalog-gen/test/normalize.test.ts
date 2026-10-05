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
        },
      ],
      inputs: [
        { name: 'COND', check: 'bool', optional: false, repeat: null, default: [{ kw: 'true' }] },
      ],
      statements: [{ name: 'BODY', repeat: null, when: null }],
      extra: [],
    });
  });

  it('lists only the statements the label does not mention', () => {
    expect(block('control.pick').statementsNotInLabel).toEqual([]);
    expect(block('functions.make').statementsNotInLabel).toEqual(['BODY']);
  });

  it('turns count defaults into numbers and drops keys that do not apply', () => {
    expect(block('control.pick').extra).toEqual([
      { name: 'elseIfCount', kind: 'count', min: 0, max: 32, default: 0, types: [] },
      { name: 'hasElse', kind: 'flag', min: null, max: null, default: false, types: [] },
    ]);
    expect(block('functions.make').extra).toEqual([
      { name: 'params', kind: 'params', min: null, max: 16, default: null, types: ['int'] },
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

  it('keeps every output type and refuses unknown ones', () => {
    expect(block('functions.use').output).toBe('symbol');
    expect(block('text.show').output).toBe('field:TO');
    for (const output of ['any', 'bool', 'int', 'double', 'number', 'char', 'string']) {
      const result = model((c) => c.blocks.push(rustBlock('x.y', { shape: 'reporter', output })));
      expect(result.blocks.at(-1)?.output).toBe(output);
    }
    expect(() =>
      model((c) => c.blocks.push(rustBlock('x.y', { shape: 'reporter', output: 'float' }))),
    ).toThrow('blocks[5].output is not an output type ("float")');
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
