/**
 * W2's control and file blocks in the editor, from the generated catalog (package D,
 * docs/plans/m3-contracts/w2.md §1.6, §1.8, §2.1, §5.2): *switch on* drawn by the *branches*
 * mutator, its second *case* word joining the copies as *else if* does; *do … then repeat*; and
 * *for each line in file*, a block with a flag and no repeated group, whose *if it can't be
 * opened* part comes and goes with ⊕ and ⊖.
 */
import * as Blockly from 'blockly/core';
import { afterEach, beforeAll, beforeEach, describe, expect, it } from 'vitest';

import { registerB2cFields } from '../src/fields/register';
import { MutatorButton } from '../src/mutators/buttons';
import { buttonKey } from '../src/mutators/controller';
import { configureMutators } from '../src/mutators/hooks';
import { isB2cMutatorBlock, registerB2cMutators } from '../src/mutators/register';
import {
  defineHelperBlocks,
  defineTestBlocks,
  flushEvents,
  inputNames,
  labelTexts,
} from '../src/mutators/test-fixtures';
import type { B2cMutatorBlock } from '../src/mutators/types';

let workspace: Blockly.Workspace;

beforeAll(() => {
  registerB2cFields();
  registerB2cMutators();
  defineHelperBlocks();
});

beforeEach(() => {
  defineTestBlocks();
  workspace = new Blockly.Workspace();
});

afterEach(() => {
  configureMutators({ symbols: null, inputShadow: null, newSymbolId: null, declName: null });
  workspace.dispose();
});

function mutatorBlock(type: string): B2cMutatorBlock {
  const block = workspace.newBlock(type);
  if (!isB2cMutatorBlock(block)) {
    throw new Error(`${type} has no b2c mutator`);
  }
  return block;
}

function buttons(block: Blockly.Block): string[] {
  return block.inputList
    .flatMap((input) => input.fieldRow)
    .filter((field): field is MutatorButton => field instanceof MutatorButton)
    .map((field) => buttonKey(field) ?? '?');
}

function press(block: Blockly.Block, key: string): void {
  const button = block.inputList
    .flatMap((input) => input.fieldRow)
    .find(
      (field): field is MutatorButton => field instanceof MutatorButton && buttonKey(field) === key,
    );
  if (button === undefined) {
    throw new Error(`no button ${key} (${buttons(block).join(', ')})`);
  }
  button.showEditor();
}

/** A statement block from the catalog to put in a part. */
function statement(parent: Blockly.Block, input: string): Blockly.Block {
  const child = workspace.newBlock('control.break');
  const connection = parent.getInput(input)?.connection;
  if (connection === null || connection === undefined || child.previousConnection === null) {
    throw new Error(`cannot put a statement into ${input}`);
  }
  connection.connect(child.previousConnection);
  return child;
}

describe('switch on', () => {
  it('starts with two empty cases and a default part', () => {
    const block = mutatorBlock('control.switch');
    expect(block.b2cGetExtra()).toEqual({ caseCount: 2, hasDefault: true });
    expect(inputNames(block)).toEqual([
      'VALUE',
      'CASE0',
      'DO0',
      'CASE1',
      'DO1',
      'DEFAULT',
      'B2C_BUTTONS',
      'b2c_repeat',
    ]);
    // The second *case* is the joining word of the copies, as *else if* is.
    expect(labelTexts(block, 'VALUE')).toEqual(['switch on']);
    expect(labelTexts(block, 'CASE1')).toEqual(['case']);
    expect(labelTexts(block, 'DEFAULT')).toEqual(['default']);
    expect(buttons(block)).toEqual(['count:remove', 'count:add', 'flag:hasDefault:remove']);
    // Case values start empty (w2.md §2.9): no start value is known at landing.
    expect(block.getInput('CASE0')?.connection?.targetBlock()).toBeNull();
  });

  it('adds a case with the joining word and removes the default part', () => {
    const block = mutatorBlock('control.switch');
    press(block, 'count:add');
    expect(block.b2cGetExtra()).toEqual({ caseCount: 3, hasDefault: true });
    expect(labelTexts(block, 'CASE2')).toEqual(['case']);
    expect(inputNames(block).slice(5, 8)).toEqual(['CASE2', 'DO2', 'DEFAULT']);
    press(block, 'flag:hasDefault:remove');
    expect(block.b2cGetExtra()).toEqual({ caseCount: 3, hasDefault: false });
    expect(block.getInput('DEFAULT')).toBeNull();
    expect(buttons(block)).toContain('flag:hasDefault:add');
  });

  it('removes the last case and brings it back with its blocks on undo', async () => {
    const block = mutatorBlock('control.switch');
    const inner = statement(block, 'DO1');
    await flushEvents();
    press(block, 'count:remove');
    expect(block.b2cGetExtra()).toEqual({ caseCount: 1, hasDefault: true });
    expect(block.getInput('DO1')).toBeNull();
    await flushEvents();
    workspace.undo(false);
    expect(block.b2cGetExtra()).toEqual({ caseCount: 2, hasDefault: true });
    expect(block.getInput('DO1')?.connection?.targetBlock()?.id).toBe(inner.id);
  });

  it('keeps at least one case', () => {
    const block = mutatorBlock('control.switch');
    block.b2cSetExtra({ caseCount: 1, hasDefault: true });
    expect(buttons(block)).not.toContain('count:remove');
  });
});

describe('do … then repeat', () => {
  it('has a body, then the while/until menu and a condition', () => {
    const block = workspace.newBlock('control.do_while');
    expect(inputNames(block)).toEqual(['BODY', 'COND']);
    expect(labelTexts(block, 'BODY')).toEqual(['do']);
    expect(labelTexts(block, 'COND')).toEqual(['then repeat']);
    expect(block.getFieldValue('MODE')).toBe('while');
  });
});

describe('for each line in file', () => {
  it('starts without the part for a file that cannot be opened, offered by ⊕', () => {
    const block = mutatorBlock('file.for_each_line');
    expect(block.b2cGetExtra()).toEqual({ hasFailed: false });
    expect(inputNames(block)).toEqual(['PATH', 'BODY', 'B2C_BUTTONS', 'b2c_repeat']);
    expect(labelTexts(block, 'PATH')).toEqual(['for each line', 'in file']);
    expect(labelTexts(block, 'B2C_BUTTONS')).toEqual(["if it can't be opened"]);
    expect(buttons(block)).toEqual(['flag:hasFailed:add']);
    expect(block.getField('LINE')).not.toBeNull();
  });

  it('adds the part, removes it, and brings it back with its blocks on undo', async () => {
    const block = mutatorBlock('file.for_each_line');
    press(block, 'flag:hasFailed:add');
    expect(block.b2cGetExtra()).toEqual({ hasFailed: true });
    expect(inputNames(block)).toEqual(['PATH', 'BODY', 'FAILED', 'B2C_BUTTONS', 'b2c_repeat']);
    expect(labelTexts(block, 'FAILED')).toEqual(["if it can't be opened"]);
    const inner = statement(block, 'FAILED');
    await flushEvents();
    press(block, 'flag:hasFailed:remove');
    expect(block.getInput('FAILED')).toBeNull();
    await flushEvents();
    workspace.undo(false);
    expect(block.b2cGetExtra()).toEqual({ hasFailed: true });
    expect(block.getInput('FAILED')?.connection?.targetBlock()?.id).toBe(inner.id);
  });
});
