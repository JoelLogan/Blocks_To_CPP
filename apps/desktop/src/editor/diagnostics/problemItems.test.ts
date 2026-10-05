/**
 * Problems' rows: merging the live and build diagnostics, dropping and dimming build ones, module
 * names, block paths (the M2 decision "Block paths in Problems"), sorting and keys. Pure: no
 * Blockly, only the block catalog of blockly-ext.
 */
import type { BdmBlock, BdmDocument } from '@blocks2cpp/b2c-core-wasm';
import { catalogBlock, displayTypeName } from '@blocks2cpp/blockly-ext';
import type { Diagnostic } from '@blocks2cpp/ipc-types';
import { afterEach, describe, expect, it } from 'vitest';

import { documentFixture } from '../../app/testing/fixtures';
import {
  blockLabel,
  blockPath,
  indexDocument,
  MAX_LABEL_CHARS,
  MAX_PATH_LEVELS,
  PATH_SEPARATOR,
  shorten,
} from './blockPath';
import { type PathCatalog, pathCatalog, providePathCatalog, subscribePathCatalog } from './catalog';
import { buildProblemItems, MAX_PROBLEM_ITEMS } from './problemItems';
import { guessingGame } from './testing';

const CATALOG: PathCatalog = { block: catalogBlock, typeName: displayTypeName };

afterEach(() => {
  providePathCatalog(null);
});

function diagnostic(
  block: string | undefined,
  overrides: Partial<Diagnostic> = {},
  module: string | null = 'mod_main',
): Diagnostic {
  return {
    code: 'B2C-E0201',
    severity: 'error',
    message: 'This uses a variable that does not exist here.',
    primary: {
      ...(module === null ? {} : { module }),
      ...(block === undefined ? {} : { block }),
      part: { kind: 'whole' },
    },
    source: 'analyser',
    ...overrides,
  };
}

function compilerMessage(block: string | undefined): Diagnostic {
  return diagnostic(
    block,
    {
      code: 'C:error',
      source: 'compiler',
      message: 'no match for operator<<',
      raw: 'main.cpp:3: error',
    },
    null,
  );
}

/** A document whose main module holds `blocks` (and a second, empty module). */
function documentWith(blocks: BdmBlock[]): BdmDocument {
  const doc = documentFixture();
  doc.modules = [
    { id: 'mod_main', name: 'main', workspace: { blocks } },
    { id: 'mod_util', name: 'util', workspace: { blocks: [] } },
  ];
  return doc;
}

function pathIn(doc: BdmDocument, blockId: string, catalog: PathCatalog | null = CATALOG): string {
  return blockPath(indexDocument(doc), blockId, catalog);
}

describe('block paths', () => {
  it('read main › repeat until › if for the guessing game’s if block', () => {
    const doc = guessingGame();
    expect(pathIn(doc, 'b009')).toBe('main › repeat until › if');
    expect(pathIn(doc, 'b005')).toBe('main › repeat until › ask');
    expect(pathIn(doc, 'b008')).toBe('main › repeat until › if › print');
    expect(pathIn(doc, 'b001')).toBe('main › create int variable › random integer from');
    expect(pathIn(doc, 'b011')).toBe('main');
    expect(pathIn(doc, 'b999')).toBe('');
  });

  it('fill in the dropdown text the block has now', () => {
    const doc = guessingGame();
    const loop = doc.modules[0]?.workspace.blocks[0]?.statements?.['BODY']?.[3];
    expect(loop?.id).toBe('b010');
    if (loop?.fields !== undefined) {
      loop.fields['MODE'] = 'while';
    }
    expect(pathIn(doc, 'b009')).toBe('main › repeat while › if');
  });

  it('name blocks whose label starts with a placeholder by the whole label', () => {
    const doc = documentWith([
      {
        id: 'p1',
        type: 'program.main',
        v: 1,
        statements: {
          BODY: [
            {
              id: 'd1',
              type: 'var.declare',
              v: 1,
              fields: {
                NAME: { sym: 's_score', name: 'score' },
                TYPE: 'std::string',
                CONST: false,
              },
              inputs: {
                VALUE: {
                  block: {
                    id: 'c1',
                    type: 'math.compare',
                    v: 1,
                    fields: { OP: 'lt' },
                    inputs: {
                      A: {
                        block: {
                          id: 'g1',
                          type: 'var.get',
                          v: 1,
                          fields: { VAR: { ref: 's_score' } },
                        },
                      },
                      B: {
                        block: {
                          id: 'g2',
                          type: 'var.get',
                          v: 1,
                          fields: { VAR: { ref: 's_gone' } },
                        },
                      },
                    },
                  },
                },
              },
            },
            {
              id: 't0',
              type: 'io.print',
              v: 1,
              extra: { itemCount: 1 },
              inputs: {
                ITEM0: {
                  block: { id: 't1', type: 'text.literal', v: 1, fields: { VALUE: 'Hi\nthere' } },
                },
              },
            },
          ],
        },
      },
    ]);
    expect(pathIn(doc, 'd1')).toBe('main › create string variable');
    expect(pathIn(doc, 'c1')).toBe('main › create string variable › … < …');
    expect(pathIn(doc, 'g1')).toBe('main › create string variable › … < … › score');
    expect(pathIn(doc, 'g2')).toBe('main › create string variable › … < … › missing');
    expect(pathIn(doc, 't1')).toBe('main › print › "Hi⟨U+000A⟩there"');
  });

  it('name a function definition by its name, and its parameters in references', () => {
    const doc = documentWith([
      {
        id: 'f1',
        type: 'func.define',
        v: 1,
        fields: { NAME: { sym: 's_square', name: 'square' }, RETURNS: 'int' },
        extra: { params: [{ sym: 's_n', name: 'n', type: 'int', mode: 'value' }] },
        statements: {
          BODY: [
            {
              id: 'r1',
              type: 'func.return',
              v: 1,
              inputs: {
                VALUE: {
                  block: { id: 'n1', type: 'var.get', v: 1, fields: { VAR: { ref: 's_n' } } },
                },
              },
            },
          ],
        },
      },
    ]);
    expect(pathIn(doc, 'n1')).toBe('square › return › n');
  });

  it('name blocks by their type without a catalog, or when the catalog lacks the type', () => {
    const doc = guessingGame();
    expect(pathIn(doc, 'b009', null)).toBe('main › control.while › control.if');
    const index = indexDocument(doc);
    expect(blockLabel({ id: 'x', type: 'pack.thing', v: 1 }, index.names, CATALOG)).toBe(
      'pack.thing',
    );
  });

  it('use the catalog the editor provided by default, and tell subscribers when it changes', () => {
    const doc = guessingGame();
    const calls: (PathCatalog | null)[] = [];
    const unsubscribe = subscribePathCatalog(() => calls.push(pathCatalog()));
    expect(blockPath(indexDocument(doc), 'b009')).toBe('main › control.while › control.if');
    providePathCatalog(CATALOG);
    providePathCatalog(CATALOG);
    expect(blockPath(indexDocument(doc), 'b009')).toBe('main › repeat until › if');
    unsubscribe();
    providePathCatalog(null);
    expect(calls).toEqual([CATALOG]);
  });

  it('show hidden characters in names and shorten long ones', () => {
    const name = `a\u200bb${'x'.repeat(100)}`;
    const doc = documentWith([
      {
        id: 'f1',
        type: 'func.define',
        v: 1,
        fields: { NAME: { sym: 's_f', name }, RETURNS: 'void' },
      },
    ]);
    const label = pathIn(doc, 'f1');
    expect(label.startsWith('a⟨U+200B⟩b')).toBe(true);
    expect(Array.from(label)).toHaveLength(MAX_LABEL_CHARS);
    expect(label.endsWith('…')).toBe(true);
  });

  it('keep the first and last levels of a very deep path', () => {
    let inner: BdmBlock = { id: 'leaf', type: 'control.break', v: 1 };
    for (let depth = 11; depth >= 1; depth--) {
      inner = {
        id: `w${String(depth)}`,
        type: 'control.forever',
        v: 1,
        statements: { BODY: [inner] },
      };
    }
    const doc = documentWith([
      { id: 'p1', type: 'program.main', v: 1, statements: { BODY: [inner] } },
    ]);
    const levels = pathIn(doc, 'leaf').split(PATH_SEPARATOR);
    expect(levels).toHaveLength(MAX_PATH_LEVELS);
    expect(levels).toEqual([
      'main',
      'forever',
      '…',
      'forever',
      'forever',
      'forever',
      'forever',
      'leave loop',
    ]);
  });

  it('treat a loose stack’s blocks as top-level, and the first of two equal IDs as the block', () => {
    const doc = documentWith([
      {
        id: 'h1',
        type: 'io.print',
        v: 1,
        x: 0,
        y: 0,
        stack: [{ id: 'h2', type: 'var.set', v: 1 }],
      },
    ]);
    expect(pathIn(doc, 'h2')).toBe('set');
    expect(pathIn(doc, 'h1')).toBe('print');
  });

  it('survive documents that are not shaped like the format', () => {
    const odd = { ...documentFixture(), modules: [{ id: 'm', name: 'm', workspace: {} }, 7] };
    expect(indexDocument(odd as unknown as BdmDocument).blocks.size).toBe(0);
  });
});

describe('shorten', () => {
  it('keeps short text, cuts long text with …, and never splits a surrogate pair', () => {
    expect(shorten('abc', 3)).toBe('abc');
    expect(shorten('abcd', 3)).toBe('ab…');
    expect(shorten('a😀', 2)).toBe('a😀');
    expect(shorten('😀😀😀', 2)).toBe('😀…');
    expect(shorten('abc', 0)).toBe('abc');
  });
});

describe('buildProblemItems', () => {
  it('merges live and build diagnostics, most serious first, with module and block path', () => {
    const doc = guessingGame();
    const items = buildProblemItems(
      [
        diagnostic('b006', { severity: 'warning', code: 'B2C-W0501', message: 'Never used.' }),
        diagnostic(undefined, { severity: 'info', code: 'B2C-I0001', message: 'A note.' }),
        diagnostic('b009'),
      ],
      [compilerMessage('b005')],
      false,
      doc,
      CATALOG,
    );
    expect(
      items.map((item) => [
        item.diagnostic.severity,
        item.origin,
        item.modulePath,
        item.blockPath,
        item.stale,
      ]),
    ).toEqual([
      // The panel's order: severity, then module, block path, message and code.
      ['error', 'build', 'main', 'main › repeat until › ask', false],
      ['error', 'live', 'main', 'main › repeat until › if', false],
      ['warning', 'live', 'main', 'main › repeat until › if › print', false],
      ['info', 'live', 'main', '', false],
    ]);
  });

  it('dims the build’s diagnostics when they are stale, never the live ones', () => {
    const items = buildProblemItems(
      [diagnostic('b009')],
      [compilerMessage('b005'), compilerMessage(undefined)],
      true,
      guessingGame(),
      CATALOG,
    );
    expect(items.map((item) => [item.origin, item.diagnostic.primary.block, item.stale])).toEqual([
      // A toolchain-like message without a block has no module, so it sorts first.
      ['build', undefined, true],
      ['build', 'b005', true],
      ['live', 'b009', false],
    ]);
  });

  it('drops build diagnostics whose block was deleted, and keeps live ones', () => {
    const items = buildProblemItems(
      [diagnostic('b_gone')],
      [compilerMessage('b_gone'), compilerMessage('b007')],
      true,
      guessingGame(),
      CATALOG,
    );
    expect(items.map((item) => [item.origin, item.diagnostic.primary.block])).toEqual([
      ['live', 'b_gone'],
      ['build', 'b007'],
    ]);
    expect(items[0]?.blockPath).toBe('');
  });

  it('names the module from the diagnostic, else from where its block is', () => {
    const doc = guessingGame();
    doc.modules.push({ id: 'mod_util', name: 'util', workspace: { blocks: [] } });
    const items = buildProblemItems(
      [
        diagnostic(undefined, { code: 'B2C-E0102' }, 'mod_util'),
        diagnostic('b009', { code: 'B2C-E0103' }, 'mod_unknown'),
        diagnostic(undefined, { code: 'B2C-E0104' }, null),
      ],
      [],
      false,
      doc,
      CATALOG,
    );
    const byCode = new Map(items.map((item) => [item.diagnostic.code, item.modulePath]));
    expect(byCode.get('B2C-E0102')).toBe('util');
    expect(byCode.get('B2C-E0103')).toBe('main');
    expect(byCode.get('B2C-E0104')).toBe('');
  });

  it('gives every row a unique key that stays the same for the same diagnostics', () => {
    const live = [diagnostic('b009'), diagnostic('b009'), diagnostic('b005')];
    const build = [compilerMessage('b009')];
    const first = buildProblemItems(live, build, false, guessingGame(), CATALOG);
    const again = buildProblemItems(live, build, false, guessingGame(), CATALOG);
    const keys = first.map((item) => item.key);
    expect(new Set(keys).size).toBe(4);
    expect(again.map((item) => item.key)).toEqual(keys);
  });

  it('builds at most MAX_PROBLEM_ITEMS rows', () => {
    const many = Array.from({ length: MAX_PROBLEM_ITEMS + 10 }, () => diagnostic('b009'));
    expect(buildProblemItems(many, [], false, guessingGame(), CATALOG)).toHaveLength(
      MAX_PROBLEM_ITEMS,
    );
  });
});
