// A small catalog.json in the export's shape, with every kind of field, input, statement and
// extra, for tests that should not change whenever the real catalog does.

import path from 'node:path';
import { fileURLToPath } from 'node:url';

/** The repository root. */
export const ROOT = fileURLToPath(new URL('../../../', import.meta.url));

/** A path under the repository root. */
export function repoPath(relative: string): string {
  return path.join(ROOT, relative);
}

/** A block definition in the Rust (serde) shape, with empty parts unless given. */
export function rustBlock(id: string, overrides: Record<string, unknown> = {}): object {
  return {
    id,
    version: 1,
    category: 'control',
    shape: 'statement',
    output: null,
    label: { friendly: id, cpp: `${id};` },
    lowering: 'builtin',
    headers: [],
    help: `Help for ${id}.`,
    field: [],
    input: [],
    statement: [],
    extra: [],
    ...overrides,
  };
}

/** The fixture catalog, as the export writes it. */
export function fixture(): {
  catalogVersion: string;
  blocks: object[];
  toolbox: { category: object[] };
} {
  return {
    catalogVersion: '9.8.7',
    blocks: [
      rustBlock('control.loop', {
        category: 'loops',
        label: { friendly: 'repeat %MODE %COND', cpp: 'while (%COND) { … }' },
        help: 'Repeats <blocks> | *while* a [condition] holds.',
        field: [
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
        input: [
          { name: 'COND', check: 'bool', optional: false, repeat: null, default: [{ kw: 'true' }] },
        ],
        statement: [{ name: 'BODY', repeat: null, when: null }],
      }),
      rustBlock('control.pick', {
        label: { friendly: 'if %COND then %DO else if … else %ELSE', cpp: 'if (%COND) { … }' },
        input: [
          {
            name: 'COND',
            check: 'bool',
            optional: false,
            repeat: { count: 'elseIfCount', plus: 1 },
            default: [{ kw: 'true' }],
          },
        ],
        statement: [
          { name: 'DO', repeat: { count: 'elseIfCount', plus: 1 }, when: null },
          { name: 'ELSE', repeat: null, when: 'hasElse' },
        ],
        extra: [
          { name: 'elseIfCount', kind: 'count', min: 0, max: 32, default: '0', types: [] },
          { name: 'hasElse', kind: 'flag', min: 0, max: 1, default: false, types: [] },
        ],
      }),
      rustBlock('functions.make', {
        category: 'functions',
        shape: 'definition',
        label: { friendly: 'define %NAME with … returns %RETURNS', cpp: '%RETURNS %NAME(…)' },
        field: [
          { name: 'NAME', kind: 'symbol_decl', options: [], types: [], default: null },
          {
            name: 'RETURNS',
            kind: 'type',
            options: [],
            types: ['void', 'int', 'std::string'],
            default: 'void',
          },
        ],
        statement: [{ name: 'BODY', repeat: null, when: null }],
        extra: [{ name: 'params', kind: 'params', min: 0, max: 16, default: null, types: ['int'] }],
      }),
      rustBlock('functions.use', {
        category: 'functions',
        shape: 'reporter',
        output: 'symbol',
        label: { friendly: '%FUNC %ARG …', cpp: '%FUNC(%ARG, …)' },
        field: [{ name: 'FUNC', kind: 'symbol_ref', options: [], types: [], default: null }],
        input: [
          {
            name: 'ARG',
            check: 'any',
            optional: false,
            repeat: { count: 'argCount', plus: 0 },
            default: [],
          },
        ],
        extra: [{ name: 'argCount', kind: 'count', min: 0, max: 16, default: '0', types: [] }],
      }),
      rustBlock('text.show', {
        category: 'text',
        shape: 'reporter',
        output: 'field:TO',
        label: { friendly: '"%VALUE" as %TO with `ticks`', cpp: 'a || b' },
        field: [
          { name: 'VALUE', kind: 'text', options: [], types: [], default: '' },
          { name: 'ON', kind: 'checkbox', options: [], types: [], default: true },
          { name: 'COUNT', kind: 'number', options: [], types: [], default: '3' },
          { name: 'TO', kind: 'type', options: [], types: ['int', 'double'], default: null },
        ],
        input: [
          {
            name: 'PROMPT',
            check: 'text',
            optional: true,
            repeat: null,
            default: [],
          },
          {
            name: 'MIX',
            check: 'number',
            optional: false,
            repeat: null,
            default: [{ num: '1' }, { op: '+' }, { chr: "'" }, { str: 'a "b"' }, { text: 'x y' }],
          },
        ],
      }),
    ],
    toolbox: {
      category: [
        {
          id: 'text',
          name: 'Text',
          icon: 'T',
          colour: 'text',
          dynamic: null,
          entry: [
            {
              block: 'text.show',
              label: 'show *it*',
              preset: { fields: { ON: false }, inputs: { PROMPT: [{ str: 'Hi: ' }] } },
            },
          ],
        },
        {
          id: 'control',
          name: 'Control',
          icon: 'C',
          colour: 'control',
          dynamic: null,
          entry: [
            { block: 'control.pick', label: null, preset: null },
            {
              block: 'control.pick',
              label: 'if … else',
              preset: { extra: { hasElse: true, elseIfCount: 2 } },
            },
          ],
        },
        {
          id: 'loops',
          name: 'Loops',
          icon: 'L',
          colour: 'loops',
          dynamic: null,
          entry: [{ block: 'control.loop', label: null, preset: { fields: { MODE: 'until' } } }],
        },
        {
          id: 'functions',
          name: 'Functions',
          icon: 'ƒ',
          colour: 'functions',
          dynamic: 'functions',
          entry: [{ block: 'functions.make', label: null, preset: null }],
        },
      ],
    },
  };
}

/** The fixture as catalog.json text. */
export function fixtureText(): string {
  return `${JSON.stringify(fixture(), null, 2)}\n`;
}
