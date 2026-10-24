/**
 * The sync with blockly-ext's real blocks, fields, expression shadows, placeholders and mutators
 * (docs/spec/05-project-format.md §5.4, 03 §3.4): what the editor writes for each kind of change.
 * These tests need no compiler core.
 */
import type { BdmBlock, BdmDocument } from '@blocks2cpp/b2c-core-wasm';
import {
  B2cSymbolRefField,
  EXPR_SHADOW_TYPE,
  hasB2cMutator,
  isPlaceholder,
  readExprShadow,
  setManuallyDisabled,
  staticConversion,
} from '@blocks2cpp/blockly-ext';
import * as Blockly from 'blockly/core';
import { afterEach, describe, expect, it } from 'vitest';

import { documentFixture } from '../../app/testing/fixtures';
import { buildBlockTree, loadModule } from './bdmToWorkspace';
import { SyncError } from './errors';
import { MAX_COORDINATE } from './limits';
import { disposeWorkspaces, headlessWorkspace, present, testCore } from './testing';
import { readModule, readTopBlocks, withViewport } from './workspaceToBdm';

afterEach(() => {
  disposeWorkspaces();
});

/** A document whose one module holds `blocks` (and other modules, frames, notes kept aside). */
function docWith(blocks: BdmBlock[]): BdmDocument {
  const doc = documentFixture('Sync');
  doc.modules = [{ id: 'mod_main', name: 'main', workspace: { blocks } }];
  return doc;
}

/** `main` holding `body`. */
function mainWith(body: BdmBlock[]): BdmBlock {
  return { id: 'main', type: 'program.main', v: 1, x: 40, y: 40, statements: { BODY: body } };
}

/** Loads `blocks` into a new headless workspace. */
function load(blocks: BdmBlock[]): { workspace: Blockly.Workspace; doc: BdmDocument } {
  const doc = docWith(blocks);
  const workspace = headlessWorkspace();
  loadModule(workspace, doc, 'mod_main');
  return { workspace, doc };
}

/** The top-level blocks read back, in ID order (the canonical order). */
function readBack(workspace: Blockly.Workspace): BdmBlock[] {
  return readTopBlocks(workspace).sort((a, b) => (a.id < b.id ? -1 : a.id > b.id ? 1 : 0));
}

function blockOf(workspace: Blockly.Workspace, id: string): Blockly.Block {
  const block = workspace.getBlockById(id);
  if (block === null) {
    throw new Error(`no block ${id}`);
  }
  return block;
}

const REPEAT: BdmBlock = { id: 'rep', type: 'control.repeat', v: 1 };

describe('value inputs and expression shadows', () => {
  it('keeps an input the file left out absent until its value changes', () => {
    const { workspace } = load([mainWith([REPEAT])]);
    const shadow = blockOf(workspace, 'rep').getInput('TIMES')?.connection?.targetBlock();
    expect(shadow?.type).toBe(EXPR_SHADOW_TYPE.num);
    expect(readBack(workspace)).toEqual([mainWith([REPEAT])]);

    shadow?.setFieldValue('5', 'VALUE');
    expect(readBack(workspace)[0]?.statements?.['BODY']?.[0]?.inputs).toEqual({
      TIMES: { expr: [{ num: '5' }] },
    });
    // Back to the default: absent again.
    shadow?.setFieldValue('10', 'VALUE');
    expect(readBack(workspace)).toEqual([mainWith([REPEAT])]);
  });

  it('keeps an explicit value equal to the default explicit', () => {
    const explicit: BdmBlock = { ...REPEAT, inputs: { TIMES: { expr: [{ num: '10' }] } } };
    const { workspace } = load([mainWith([explicit])]);
    expect(readBack(workspace)).toEqual([mainWith([explicit])]);
  });

  it('saves a block dropped on a slot as {block}, and the default again once it is removed', () => {
    const { workspace } = load([mainWith([REPEAT])]);
    const repeat = blockOf(workspace, 'rep');
    const number = workspace.newBlock('math.number', 'num1');
    number.initModel();
    number.setFieldValue('3', 'VALUE');
    repeat.getInput('TIMES')?.connection?.connect(present(number.outputConnection, 'the output'));
    const withBlock = readBack(workspace).find((node) => node.id === 'main');
    expect(withBlock?.statements?.['BODY']?.[0]?.inputs).toEqual({
      TIMES: { block: { id: 'num1', type: 'math.number', v: 1, fields: { VALUE: '3' } } },
    });

    number.unplug();
    number.dispose(false);
    expect(readBack(workspace)).toEqual([mainWith([REPEAT])]);
  });

  it('shows references, operators and drafts read-only and keeps their tokens', () => {
    const condition = {
      expr: [{ ref: 's_guess' }, { op: '<' }, { num: '7' }],
    };
    const draft = { expr: [{ text: 'guess +' }], draft: true };
    const blocks: BdmBlock[] = [
      mainWith([
        {
          id: 'w',
          type: 'control.while',
          v: 1,
          fields: { MODE: 'while' },
          inputs: { COND: condition },
        },
        { id: 'e', type: 'program.exit', v: 1, inputs: { CODE: draft } },
        {
          id: 'r',
          type: 'var.change',
          v: 1,
          fields: { VAR: { ref: 's_guess' } },
          inputs: { BY: { expr: [{ ref: 's_step' }] } },
        },
      ]),
    ];
    const { workspace } = load(blocks);
    const shadowOf = (id: string, input: string): Blockly.Block | null | undefined =>
      blockOf(workspace, id).getInput(input)?.connection?.targetBlock();
    expect(shadowOf('w', 'COND')?.type).toBe(EXPR_SHADOW_TYPE.tokens);
    expect(shadowOf('e', 'CODE')?.type).toBe(EXPR_SHADOW_TYPE.tokens);
    const ref = shadowOf('r', 'BY');
    expect(ref?.type).toBe(EXPR_SHADOW_TYPE.ref);
    expect((ref?.getField('VALUE') as B2cSymbolRefField).getRef()).toEqual({ ref: 's_step' });
    expect(readBack(workspace)).toEqual(blocks);
  });

  it('keeps tokens an editable shadow would change, read-only', () => {
    // A character slot written with two characters: the one-character field would refuse it.
    const blocks: BdmBlock[] = [
      mainWith([
        {
          id: 'p',
          type: 'io.print',
          v: 1,
          extra: { itemCount: 1 },
          fields: { NEWLINE: true, SEP: 'none', STREAM: 'out' },
          inputs: { ITEM0: { expr: [{ chr: 'ab' }] } },
        },
      ]),
    ];
    const { workspace } = load(blocks);
    const shadow = blockOf(workspace, 'p').getInput('ITEM0')?.connection?.targetBlock();
    expect(shadow === null || shadow === undefined ? null : readExprShadow(shadow)?.tokens).toEqual(
      [{ chr: 'ab' }],
    );
    expect(readBack(workspace)).toEqual(blocks);
  });
});

describe('statements, loose blocks and stacks', () => {
  const print = (id: string, text: string): BdmBlock => ({
    id,
    type: 'io.print',
    v: 1,
    extra: { itemCount: 1 },
    fields: { NEWLINE: true, SEP: 'none', STREAM: 'out' },
    inputs: { ITEM0: { expr: [{ str: text }] } },
  });

  it('writes statement lists as arrays and a loose stack under its head (amendment A1)', () => {
    const head: BdmBlock = {
      ...print('a', 'one'),
      x: 500,
      y: 60,
      stack: [print('b', 'two'), print('c', 'three')],
    };
    const blocks = [mainWith([print('m1', 'in main'), print('m2', 'also in main')]), head];
    const { workspace } = load(blocks);
    expect(blockOf(workspace, 'a').getNextBlock()?.id).toBe('b');
    expect(readBack(workspace)).toEqual([
      head,
      mainWith([print('m1', 'in main'), print('m2', 'also in main')]),
    ]);

    // Splitting the stack makes two loose stacks; the lower part gets its own position.
    const lower = blockOf(workspace, 'b');
    lower.unplug(false);
    lower.moveBy(10, 400);
    const read = readBack(workspace);
    expect(read.find((node) => node.id === 'a')).toEqual({ ...print('a', 'one'), x: 500, y: 60 });
    const split = read.find((node) => node.id === 'b');
    expect(split?.stack).toEqual([print('c', 'three')]);
    expect(typeof split?.x).toBe('number');
    expect(typeof split?.y).toBe('number');
  });

  it('keeps an empty statement list the file wrote, and writes none for new blocks', () => {
    const withEmpty: BdmBlock = {
      id: 'f',
      type: 'control.forever',
      v: 1,
      statements: { BODY: [] },
    };
    const { workspace } = load([mainWith([withEmpty])]);
    expect(readBack(workspace)).toEqual([mainWith([withEmpty])]);

    const created = workspace.newBlock('control.forever', 'f2');
    created.initModel();
    blockOf(workspace, 'f').nextConnection?.connect(
      present(created.previousConnection, 'the previous connection'),
    );
    expect(readBack(workspace)[0]?.statements?.['BODY']?.[1]).toEqual({
      id: 'f2',
      type: 'control.forever',
      v: 1,
    });
  });

  it('clamps positions to whole numbers within ±10^7', () => {
    const { workspace } = load([{ ...REPEAT, x: 10, y: 20 }]);
    blockOf(workspace, 'rep').moveBy(0.6, 3 * MAX_COORDINATE);
    expect(readBack(workspace)[0]).toMatchObject({ x: 11, y: MAX_COORDINATE });
  });
});

describe('block state', () => {
  it('maps comments, pinned comments, collapsed and disabled one to one', () => {
    const blocks: BdmBlock[] = [
      {
        ...mainWith([
          { ...REPEAT, comment: { text: 'twice\nas <b>much</b>', pinned: true }, disabled: true },
          { id: 'f', type: 'control.forever', v: 1, collapsed: true },
        ]),
        comment: { text: 'start' },
      },
    ];
    const { workspace } = load(blocks);
    expect(blockOf(workspace, 'f').isCollapsed()).toBe(true);
    expect(readBack(workspace)).toEqual(blocks);

    const repeat = blockOf(workspace, 'rep');
    repeat.setCommentText(null);
    setManuallyDisabled(repeat, false);
    blockOf(workspace, 'f').setCollapsed(false);
    expect(readBack(workspace)).toEqual([
      {
        ...mainWith([REPEAT, { id: 'f', type: 'control.forever', v: 1 }]),
        comment: { text: 'start' },
      },
    ]);
  });

  it('keeps fields and extra keys a file left out while they have their defaults', () => {
    const sparse: BdmBlock = {
      id: 'p',
      type: 'io.print',
      v: 1,
      inputs: { ITEM0: { expr: [{ str: 'hi' }] } },
    };
    const { workspace } = load([mainWith([sparse])]);
    expect(readBack(workspace)).toEqual([mainWith([sparse])]);

    const block = blockOf(workspace, 'p');
    block.setFieldValue('space', 'SEP');
    if (!hasB2cMutator(block)) {
      throw new Error('io.print has no mutator');
    }
    block.b2cSetExtra({ itemCount: 2 });
    expect(readBack(workspace)[0]?.statements?.['BODY']?.[0]).toEqual({
      ...sparse,
      extra: { itemCount: 2 },
      fields: { SEP: 'space' },
    });
  });

  it('writes every key for a block the editor created', () => {
    const { workspace } = load([mainWith([])]);
    const created = workspace.newBlock('io.print', 'np');
    created.initModel();
    blockOf(workspace, 'main')
      .getInput('BODY')
      ?.connection?.connect(present(created.previousConnection, 'the previous connection'));
    expect(readBack(workspace)[0]?.statements?.['BODY']).toEqual([
      {
        id: 'np',
        type: 'io.print',
        v: 1,
        extra: { itemCount: 1 },
        fields: { SEP: 'none', NEWLINE: true, STREAM: 'out' },
      },
    ]);
  });

  it('reshapes variadic blocks through their mutators and reads the parts back', () => {
    const ifBlock: BdmBlock = {
      id: 'if',
      type: 'control.if',
      v: 1,
      extra: { elseIfCount: 1, hasElse: true },
      inputs: { COND0: { expr: [{ kw: 'false' }] }, COND1: { expr: [{ kw: 'true' }] } },
      statements: { DO0: [REPEAT], ELSE: [{ id: 'f', type: 'control.forever', v: 1 }] },
    };
    const { workspace } = load([mainWith([ifBlock])]);
    const block = blockOf(workspace, 'if');
    expect(block.getInput('COND1')).not.toBeNull();
    expect(block.getInput('ELSE')).not.toBeNull();
    expect(readBack(workspace)).toEqual([mainWith([ifBlock])]);

    if (!hasB2cMutator(block)) {
      throw new Error('control.if has no mutator');
    }
    block.b2cSetExtra({ elseIfCount: 2, hasElse: true });
    const read = readBack(workspace)[0]?.statements?.['BODY']?.[0];
    expect(read?.extra).toEqual({ elseIfCount: 2, hasElse: true });
    expect(read?.statements?.['DO0']).toEqual([REPEAT]);
  });
});

describe('placeholders', () => {
  it('keeps a block of an unknown type verbatim, with its own position, flags and comment', () => {
    const unknown: BdmBlock = {
      id: 'pk',
      type: 'sfml.window.open',
      v: 3,
      x: 100,
      y: 200,
      fields: { TITLE: 'Game', SIZE: { ref: 's_size' } },
      inputs: {
        WIDTH: { expr: [{ num: '800' }] },
        INNER: { block: { id: 'in', type: 'math.number', v: 1, fields: { VALUE: '1' } } },
      },
      statements: { BODY: [REPEAT] },
      stack: [{ id: 'st', type: 'control.forever', v: 1 }],
    };
    const { workspace } = load([unknown]);
    const block = blockOf(workspace, 'pk');
    expect(isPlaceholder(block)).toBe(true);
    expect(workspace.getBlockById('rep')).toBeNull();
    expect(readBack(workspace)).toEqual([unknown]);

    block.moveBy(5, 5);
    block.setCommentText('kept');
    expect(readBack(workspace)).toEqual([
      { ...unknown, x: 105, y: 205, comment: { text: 'kept' } },
    ]);
  });

  it('keeps a catalog block it cannot show as a placeholder that says so', () => {
    const hidden: BdmBlock = {
      id: 'p',
      type: 'io.print',
      v: 1,
      extra: { itemCount: 1 },
      fields: { NEWLINE: true, SEP: 'none', STREAM: 'out' },
      inputs: { ITEM0: { expr: [{ str: 'shown' }] }, ITEM5: { expr: [{ str: 'hidden' }] } },
    };
    const newer: BdmBlock = { id: 'n', type: 'control.repeat', v: 9 };
    const statementInValue: BdmBlock = {
      id: 'x',
      type: 'program.exit',
      v: 1,
      inputs: { CODE: { block: { id: 'inner', type: 'control.forever', v: 1 } } },
    };
    const blocks = [mainWith([hidden, newer, statementInValue])];
    const { workspace } = load(blocks);
    for (const id of ['p', 'n', 'inner']) {
      expect(isPlaceholder(blockOf(workspace, id)), id).toBe(true);
    }
    expect(blockOf(workspace, 'p').getFieldValue('PACK')).toBe('Cannot show');
    expect(isPlaceholder(blockOf(workspace, 'x'))).toBe(false);
    expect(readBack(workspace)).toEqual(blocks);
  });
});

describe('documents', () => {
  it('keeps other modules, frames, notes, the viewport and x-ext verbatim', () => {
    const doc = docWith([mainWith([REPEAT])]);
    const other = {
      id: 'mod_util',
      name: 'util',
      workspace: { blocks: [REPEAT], frames: [], notes: [] },
    };
    const full: BdmDocument = {
      ...doc,
      modules: [
        {
          id: 'mod_main',
          name: 'main',
          workspace: {
            blocks: [mainWith([REPEAT])],
            frames: [{ id: 'fr', title: 'Area', x: 0, y: 0, w: 10, h: 10, color: 'blue' }],
            notes: [{ id: 'nt', text: 'note', x: 1, y: 2 }],
            viewport: { x: 5, y: 6, scale: 1.5 },
          },
        },
        other,
      ],
      'x-ext': JSON.parse('{"__proto__": {"polluted": true}, "tool": [1, 2]}') as never,
    };
    const workspace = headlessWorkspace();
    loadModule(workspace, full, 'mod_main');
    const read = readModule(workspace, full, 'mod_main');
    expect(read).toEqual(full);
    expect(read.modules[1]).toBe(other);
    expect(read['x-ext']).toBe(full['x-ext']);
    expect(({} as Record<string, unknown>)['polluted']).toBeUndefined();

    expect(
      withViewport(read, 'mod_main', undefined).modules[0]?.workspace.viewport,
    ).toBeUndefined();
    expect(
      withViewport(read, 'mod_util', { x: 1, y: 1, scale: 2 }).modules[1]?.workspace.viewport,
    ).toEqual({
      x: 1,
      y: 1,
      scale: 2,
    });
  });

  it('refuses a module the document does not have', () => {
    const doc = docWith([]);
    const workspace = headlessWorkspace();
    expect(() => {
      loadModule(workspace, doc, 'mod_none');
    }).toThrow(SyncError);
    expect(() => readModule(workspace, doc, 'mod_none')).toThrow(SyncError);
  });

  it('builds a pasted tree into a connection', () => {
    const { workspace } = load([mainWith([])]);
    const body = blockOf(workspace, 'main').getInput('BODY')?.connection;
    if (body === null || body === undefined) {
      throw new Error('main has no body');
    }
    const built = buildBlockTree(
      workspace,
      { ...REPEAT, id: 'pasted' },
      { kind: 'statement', connection: body },
    );
    expect(built.getParent()?.id).toBe('main');
    expect(readBack(workspace)).toEqual([mainWith([{ ...REPEAT, id: 'pasted' }])]);
    const top = buildBlockTree(
      workspace,
      { id: 'loose', type: 'control.forever', v: 1 },
      { kind: 'top', x: 7, y: 8 },
    );
    expect(top.getRelativeToSurfaceXY()).toMatchObject({ x: 7, y: 8 });
  });
});

const core = await testCore();

describe.skipIf(core === null)('the connection checker and the analyser', () => {
  it('agree on every conversion (blockly-ext staticConversion = the core conversion table)', () => {
    const table = core?.conversionTable() ?? [];
    expect(table).toHaveLength(49);
    for (const row of table) {
      expect(staticConversion(row.from, row.to), `${row.from} → ${row.to}`).toBe(row.conversion);
    }
  });
});
