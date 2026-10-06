/** Inserting blocks for the end-to-end tests: where they go, and what is refused. */
import type { BdmBlock } from '@blocks2cpp/b2c-core-wasm';
import * as Blockly from 'blockly/core';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';

import { settle } from '../editor/clipboard/testing';
import { guessingGame } from '../editor/diagnostics/testing';
import { loadModule } from '../editor/sync/bdmToWorkspace';
import { disposeWorkspaces, renderedWorkspace } from '../editor/sync/testing';
import { readTopBlocks } from '../editor/sync/workspaceToBdm';
import {
  checkNodes,
  insertBlocks,
  InsertBlocksError,
  MAX_INSERTED_BLOCKS,
  MAX_INSERTED_ROOTS,
} from './blocks';

let workspace: Blockly.WorkspaceSvg;

beforeEach(() => {
  workspace = renderedWorkspace();
  loadModule(workspace, guessingGame(), 'mod_main');
});

afterEach(() => {
  disposeWorkspaces();
});

function print(id: string, text: string): BdmBlock {
  return {
    id,
    type: 'io.print',
    v: 1,
    extra: { itemCount: 1 },
    fields: { NEWLINE: true, SEP: 'none', STREAM: 'out' },
    inputs: { ITEM0: { expr: [{ str: text }] } },
  };
}

/** The IDs of a statement list, in order. */
function listIds(blockId: string, input: string): string[] {
  const ids: string[] = [];
  for (
    let block = workspace.getBlockById(blockId)?.getInputTargetBlock(input) ?? null;
    block !== null;
    block = block.getNextBlock()
  ) {
    ids.push(block.id);
  }
  return ids;
}

describe('insertBlocks', () => {
  it('appends statements to the end of a statement list, in order, as one undo step', async () => {
    insertBlocks(workspace, 'b010', 'BODY', [print('p1', 'one'), print('p2', 'two')]);
    expect(listIds('b010', 'BODY')).toEqual(['b005', 'b009', 'p1', 'p2']);
    const main = readTopBlocks(workspace).find((block) => block.id === 'b011');
    expect(JSON.stringify(main)).toContain('"p2"');
    await settle();
    workspace.undo(false);
    expect(listIds('b010', 'BODY')).toEqual(['b005', 'b009']);
  });

  it('fills an empty statement list', () => {
    insertBlocks(workspace, 'b011', 'BODY', [
      { id: 'loop', type: 'control.while', v: 1, fields: { MODE: 'until' } },
    ]);
    insertBlocks(workspace, 'loop', 'BODY', [print('inside', 'hi')]);
    expect(listIds('loop', 'BODY')).toEqual(['inside']);
    expect(listIds('b011', 'BODY').at(-1)).toBe('loop');
  });

  it('puts a reporter into a value input in place of its expression slot', () => {
    insertBlocks(workspace, 'b004', 'ITEM0', [
      { id: 'get', type: 'var.get', v: 1, fields: { VAR: { ref: 's_guess' } } },
    ]);
    const item = workspace.getBlockById('b004')?.getInputTargetBlock('ITEM0');
    expect(item?.id).toBe('get');
    expect(item?.isShadow()).toBe(false);
  });

  it('refuses blocks that do not fit there, and removes them again', () => {
    // A statement in a value input.
    expect(() => {
      insertBlocks(workspace, 'b004', 'ITEM0', [print('misfit', 'x')]);
    }).toThrow(/do not fit into ITEM0/);
    expect(workspace.getBlockById('misfit')).toBeNull();
    // A value input that already holds a block.
    expect(() => {
      insertBlocks(workspace, 'b002', 'VALUE', [{ id: 'second', type: 'math.random_int', v: 1 }]);
    }).toThrow(InsertBlocksError);
    expect(workspace.getBlockById('second')).toBeNull();
  });

  it('refuses a block the editor can only show as a placeholder', () => {
    expect(() => {
      insertBlocks(workspace, 'b010', 'BODY', [{ id: 'odd', type: 'pack.unknown', v: 1 }]);
    }).toThrow(/block odd is not one the editor can show exactly/);
    expect(workspace.getBlockById('odd')).toBeNull();
    expect(() => {
      insertBlocks(workspace, 'b010', 'BODY', [{ ...print('newer', 'x'), v: 99 }]);
    }).toThrow(/not one the editor can show/);
  });

  it('checks the target and the IDs', () => {
    const blocks = [print('fresh', 'x')];
    expect(() => {
      insertBlocks(workspace, 'no such block', 'BODY', blocks);
    }).toThrow('parentBlockId must be a block ID');
    expect(() => {
      insertBlocks(workspace, 42, 'BODY', blocks);
    }).toThrow('parentBlockId must be a block ID');
    expect(() => {
      insertBlocks(workspace, 'missing', 'BODY', blocks);
    }).toThrow('there is no block missing on the canvas');
    expect(() => {
      insertBlocks(workspace, 'b010', 'body', blocks);
    }).toThrow('input must be an input name');
    expect(() => {
      insertBlocks(workspace, 'b010', 'NOPE', blocks);
    }).toThrow('block b010 has no input NOPE');
    expect(() => {
      insertBlocks(workspace, 'b010', 'BODY', [print('b005', 'x')]);
    }).toThrow('block ID b005 is already used');
    expect(() => {
      insertBlocks(workspace, 'b010', 'BODY', [print('twice', 'x'), print('twice', 'y')]);
    }).toThrow('block ID twice is already used');
    // A shadow (an expression slot) is not a project block.
    const shadow = workspace.getBlockById('b004')?.getInputTargetBlock('ITEM0');
    expect(shadow?.isShadow()).toBe(true);
    expect(() => {
      insertBlocks(workspace, shadow?.id ?? '', 'VALUE', blocks);
    }).toThrow(InsertBlocksError);
  });
});

describe('checkNodes', () => {
  it('accepts well-shaped nodes with nested blocks, lists and stacks', () => {
    const nodes = [
      {
        id: 'a',
        type: 't',
        v: 1,
        inputs: { X: { block: { id: 'b', type: 't', v: 1 } }, Y: { expr: [] } },
        statements: { S: [{ id: 'c', type: 't', v: 1 }] },
        stack: [{ id: 'd', type: 't', v: 1 }],
      },
    ];
    expect(checkNodes(nodes)).toBe(nodes);
  });

  it('refuses anything else', () => {
    const bad: unknown[] = [
      undefined,
      {},
      [],
      Array.from({ length: MAX_INSERTED_ROOTS + 1 }, (_, index) => print(`p${String(index)}`, 'x')),
      [null],
      [{ id: 'bad id', type: 't', v: 1 }],
      [{ id: 'a', type: 1, v: 1 }],
      [{ id: 'a', type: 't', v: '1' }],
      [{ id: 'a', type: 't', v: 1, inputs: [] }],
      [{ id: 'a', type: 't', v: 1, inputs: { X: 'text' } }],
      [{ id: 'a', type: 't', v: 1, inputs: { X: { block: 'nope' } } }],
      [{ id: 'a', type: 't', v: 1, statements: { S: {} } }],
      [{ id: 'a', type: 't', v: 1, statements: [] }],
      [{ id: 'a', type: 't', v: 1, stack: {} }],
    ];
    for (const [index, blocks] of bad.entries()) {
      expect(() => checkNodes(blocks), `case ${String(index)}`).toThrow(InsertBlocksError);
    }
  });

  it('limits the number of blocks, nested ones included', () => {
    let node: BdmBlock = { id: 'n0', type: 't', v: 1 };
    for (let index = 1; index <= MAX_INSERTED_BLOCKS; index += 1) {
      node = { id: `n${String(index)}`, type: 't', v: 1, statements: { S: [node] } };
    }
    expect(() => checkNodes([node])).toThrow(/at most 2000 blocks/);
  });
});
