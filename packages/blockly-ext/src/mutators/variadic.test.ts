/**
 * The variadic mutators on a headless workspace: parts and names, ⊕/⊖, blocks bumped out of
 * removed parts, one undo step per click, shadows, limits and call-argument labels.
 */
import * as Blockly from 'blockly/core';
import { afterEach, beforeAll, beforeEach, describe, expect, it } from 'vitest';

import { MutatorButton } from './buttons';
import { buttonKey } from './controller';
import { MutatorConfigError, MutatorStateError } from './errors';
import { configureMutators } from './hooks';
import { isB2cMutatorBlock, refreshMutatorLabels, registerB2cMutators } from './register';
import {
  defineHelperBlocks,
  defineTestBlocks,
  flushEvents,
  inputNames,
  labelTexts,
  TEST_SHADOW,
  ValueField,
} from './test-fixtures';
import { B2C_MUTATOR_ITEMS, B2C_MUTATOR_NAMES, type B2cMutatorBlock } from './types';

let workspace: Blockly.Workspace;

beforeAll(() => {
  registerB2cMutators();
  defineHelperBlocks();
});

beforeEach(() => {
  defineTestBlocks();
  workspace = new Blockly.Workspace();
});

afterEach(() => {
  configureMutators({ symbols: null, inputShadow: null, newSymbolId: null });
  workspace.dispose();
});

function create(type: string): B2cMutatorBlock {
  const block = workspace.newBlock(type);
  if (!isB2cMutatorBlock(block)) {
    throw new Error(`${type} has no b2c mutator`);
  }
  return block;
}

function buttons(block: Blockly.Block): MutatorButton[] {
  return block.inputList.flatMap((input) =>
    input.fieldRow.filter((field): field is MutatorButton => field instanceof MutatorButton),
  );
}

function buttonKeys(block: Blockly.Block): (string | undefined)[] {
  return buttons(block).map(buttonKey);
}

/** Clicks the button with `key`, as the user would. */
function press(block: Blockly.Block, key: string): void {
  const button = buttons(block).find((candidate) => buttonKey(candidate) === key);
  if (button === undefined) {
    throw new Error(`no button ${key} on ${block.type}: ${buttonKeys(block).join(', ')}`);
  }
  button.showEditor();
}

function plug(parent: Blockly.Block, input: string, child: Blockly.Block): void {
  const connection = parent.getInput(input)?.connection;
  const childConnection = child.outputConnection ?? child.previousConnection;
  if (connection === null || connection === undefined || childConnection === null) {
    throw new Error(`cannot plug ${child.type} into ${input}`);
  }
  connection.connect(childConnection);
}

describe('registration', () => {
  it('registers the four mutator extensions', () => {
    for (const name of B2C_MUTATOR_NAMES) {
      expect(Blockly.Extensions.isRegistered(name), name).toBe(true);
    }
  });

  it('replaces earlier registrations under the same names and can run again', () => {
    Blockly.Extensions.unregister(B2C_MUTATOR_ITEMS);
    Blockly.Extensions.registerMutator(B2C_MUTATOR_ITEMS, {
      saveExtraState: () => null,
      loadExtraState: () => undefined,
    });
    registerB2cMutators();
    registerB2cMutators();
    expect(create('io.print').b2cGetExtra()).toEqual({ itemCount: 1 });
  });

  it('marks only blocks with a b2c mutator', () => {
    expect(isB2cMutatorBlock(workspace.newBlock('io.print'))).toBe(true);
    expect(isB2cMutatorBlock(workspace.newBlock('control.repeat'))).toBe(false);
  });

  it('refuses blocks the mutator does not fit', () => {
    Blockly.Blocks['test_not_in_catalog'] = {
      init(this: Blockly.Block): void {
        Blockly.Extensions.apply(B2C_MUTATOR_ITEMS, this, true);
      },
    };
    expect(() => workspace.newBlock('test_not_in_catalog')).toThrow(MutatorConfigError);
    Blockly.Blocks['control.if'] = {
      init(this: Blockly.Block): void {
        Blockly.Extensions.apply(B2C_MUTATOR_ITEMS, this, true);
      },
    };
    expect(() => workspace.newBlock('control.if')).toThrow(/exactly one repeated value input/);
    Blockly.Blocks['logic.not'] = {
      init(this: Blockly.Block): void {
        Blockly.Extensions.apply('b2c_mutator_call_args', this, true);
      },
    };
    expect(() => workspace.newBlock('logic.not')).toThrow(MutatorConfigError);
  });
});

describe('default parts', () => {
  it('match the catalog defaults, with the extra written in full', () => {
    const cases: readonly [string, Record<string, unknown>, string[]][] = [
      ['io.print', { itemCount: 1 }, ['ITEM0', 'B2C_BUTTONS', 'b2c_repeat', 'b2c_row0']],
      ['text.join', { itemCount: 2 }, ['ITEM0', 'ITEM1', 'B2C_BUTTONS', 'b2c_repeat']],
      [
        'logic.operation',
        { itemCount: 2 },
        ['ITEM0', 'b2c_row0', 'ITEM1', 'B2C_BUTTONS', 'b2c_repeat'],
      ],
      [
        'control.if',
        { elseIfCount: 0, hasElse: false },
        ['COND0', 'DO0', 'B2C_BUTTONS', 'b2c_repeat'],
      ],
      ['func.call', { argCount: 0 }, ['b2c_row0', 'B2C_BUTTONS', 'b2c_repeat']],
      ['func.call_stmt', { argCount: 0 }, ['b2c_row0', 'B2C_BUTTONS', 'b2c_repeat']],
    ];
    for (const [type, extra, inputs] of cases) {
      const block = create(type);
      expect(block.b2cGetExtra(), type).toEqual(extra);
      expect(inputNames(block), type).toEqual(inputs);
      expect(block.saveExtraState?.(), type).toEqual(extra);
    }
  });

  it('show the label words of the group', () => {
    const print = create('io.print');
    expect(labelTexts(print, 'ITEM0')).toEqual(['print']);
    const join = create('text.join');
    expect(labelTexts(join, 'ITEM0')).toEqual(['join']);
    const branch = create('control.if');
    expect(labelTexts(branch, 'COND0')).toEqual(['if']);
    expect(labelTexts(branch, 'DO0')).toEqual(['then']);
    expect(labelTexts(branch, 'B2C_BUTTONS')).toEqual(['else if', 'else']);
  });
});

describe('b2cSetExtra', () => {
  it('names every part exactly, in order', () => {
    const branch = create('control.if');
    branch.b2cSetExtra({ elseIfCount: 2, hasElse: true });
    expect(inputNames(branch)).toEqual([
      'COND0',
      'DO0',
      'COND1',
      'DO1',
      'COND2',
      'DO2',
      'ELSE',
      'B2C_BUTTONS',
      'b2c_repeat',
    ]);
    expect(labelTexts(branch, 'COND1')).toEqual(['else if']);
    expect(labelTexts(branch, 'DO2')).toEqual(['then']);
    expect(labelTexts(branch, 'ELSE')).toEqual(['else']);
    expect(branch.getInput('COND1')?.type).toBe(Blockly.inputs.inputTypes.VALUE);
    expect(branch.getInput('DO1')?.type).toBe(Blockly.inputs.inputTypes.STATEMENT);
    expect(branch.b2cGetExtra()).toEqual({ elseIfCount: 2, hasElse: true });

    const print = create('io.print');
    print.b2cSetExtra({ itemCount: 4 });
    expect(inputNames(print)).toEqual([
      'ITEM0',
      'ITEM1',
      'ITEM2',
      'ITEM3',
      'B2C_BUTTONS',
      'b2c_repeat',
      'b2c_row0',
    ]);
  });

  it('takes catalog defaults for absent keys', () => {
    const branch = create('control.if');
    branch.b2cSetExtra({ elseIfCount: 1, hasElse: true });
    branch.b2cSetExtra({});
    expect(branch.b2cGetExtra()).toEqual({ elseIfCount: 0, hasElse: false });
    expect(inputNames(branch)).toEqual(['COND0', 'DO0', 'B2C_BUTTONS', 'b2c_repeat']);
  });

  it('leaves the block unchanged when it throws', () => {
    const print = create('io.print');
    print.b2cSetExtra({ itemCount: 3 });
    expect(() => {
      print.b2cSetExtra({ itemCount: 3, bogus: true });
    }).toThrow(MutatorStateError);
    expect(() => {
      print.b2cSetExtra({ itemCount: 99 });
    }).toThrow(MutatorStateError);
    expect(print.b2cGetExtra()).toEqual({ itemCount: 3 });
    expect(inputNames(print)).toContain('ITEM2');
  });

  it('shows counts outside the catalog range, and offers only the way back', () => {
    const print = create('io.print');
    print.b2cSetExtra({ itemCount: 0 });
    expect(print.b2cGetExtra()).toEqual({ itemCount: 0 });
    expect(inputNames(print)).toEqual(['B2C_BUTTONS', 'b2c_repeat', 'b2c_row0']);
    // Without a first item, the group's words stay visible with the buttons.
    expect(labelTexts(print, 'B2C_BUTTONS')).toEqual(['print']);
    expect(buttonKeys(print)).toEqual(['count:add']);

    print.b2cSetExtra({ itemCount: 40 });
    expect(buttonKeys(print)).toEqual(['count:remove']);
  });

  it('is ignored when Blockly loads state the block cannot show', () => {
    const block = Blockly.serialization.blocks.append(
      { type: 'io.print', extraState: { itemCount: 'many' } },
      workspace,
    );
    expect(isB2cMutatorBlock(block) && block.b2cGetExtra()).toEqual({ itemCount: 1 });
    expect(() => {
      block.loadExtraState?.({ itemCount: 2, extra: true });
    }).not.toThrow();
    expect(isB2cMutatorBlock(block) && block.b2cGetExtra()).toEqual({ itemCount: 1 });
  });

  it('runs through Blockly’s own serialization', () => {
    const branch = create('control.if');
    branch.b2cSetExtra({ elseIfCount: 1, hasElse: true });
    plug(branch, 'DO1', workspace.newBlock('io.print'));
    const state = Blockly.serialization.blocks.save(branch);
    const other = new Blockly.Workspace();
    try {
      const copy = Blockly.serialization.blocks.append(state ?? { type: 'x' }, other);
      expect(isB2cMutatorBlock(copy) && copy.b2cGetExtra()).toEqual({
        elseIfCount: 1,
        hasElse: true,
      });
      expect(copy.getInputTargetBlock('DO1')?.type).toBe('io.print');
    } finally {
      other.dispose();
    }
  });
});

describe('⊕ and ⊖', () => {
  it('add and remove items within the catalog range', () => {
    const print = create('io.print');
    expect(buttonKeys(print)).toEqual(['count:add']);
    press(print, 'count:add');
    press(print, 'count:add');
    expect(print.b2cGetExtra()).toEqual({ itemCount: 3 });
    expect(inputNames(print)).toEqual([
      'ITEM0',
      'ITEM1',
      'ITEM2',
      'B2C_BUTTONS',
      'b2c_repeat',
      'b2c_row0',
    ]);
    expect(buttonKeys(print)).toEqual(['count:remove', 'count:add']);
    press(print, 'count:remove');
    expect(print.b2cGetExtra()).toEqual({ itemCount: 2 });
    press(print, 'count:remove');
    expect(print.b2cGetExtra()).toEqual({ itemCount: 1 });
    expect(buttonKeys(print)).toEqual(['count:add']);
  });

  it('stop at the catalog maximum', () => {
    const args = create('func.call');
    args.b2cSetExtra({ argCount: 15 });
    press(args, 'count:add');
    expect(args.b2cGetExtra()).toEqual({ argCount: 16 });
    expect(buttonKeys(args)).toEqual(['count:remove']);
  });

  it('add and remove else-if and else parts', () => {
    const branch = create('control.if');
    expect(buttonKeys(branch)).toEqual(['count:add', 'flag:hasElse:add']);
    press(branch, 'count:add');
    press(branch, 'flag:hasElse:add');
    expect(branch.b2cGetExtra()).toEqual({ elseIfCount: 1, hasElse: true });
    expect(inputNames(branch)).toEqual([
      'COND0',
      'DO0',
      'COND1',
      'DO1',
      'ELSE',
      'B2C_BUTTONS',
      'b2c_repeat',
    ]);
    expect(buttonKeys(branch)).toEqual(['count:remove', 'count:add', 'flag:hasElse:remove']);
    press(branch, 'flag:hasElse:remove');
    press(branch, 'count:remove');
    expect(branch.b2cGetExtra()).toEqual({ elseIfCount: 0, hasElse: false });
    expect(inputNames(branch)).toEqual(['COND0', 'DO0', 'B2C_BUTTONS', 'b2c_repeat']);
  });

  it('add and remove call arguments', () => {
    for (const type of ['func.call', 'func.call_stmt']) {
      const call = create(type);
      press(call, 'count:add');
      press(call, 'count:add');
      expect(call.b2cGetExtra(), type).toEqual({ argCount: 2 });
      expect(inputNames(call), type).toEqual([
        'b2c_row0',
        'ARG0',
        'ARG1',
        'B2C_BUTTONS',
        'b2c_repeat',
      ]);
      press(call, 'count:remove');
      expect(call.b2cGetExtra(), type).toEqual({ argCount: 1 });
    }
  });

  it('describe themselves in words', () => {
    const branch = create('control.if');
    branch.b2cSetExtra({ elseIfCount: 1, hasElse: true });
    expect(buttons(branch).map((button) => button.label)).toEqual([
      'Remove the last "else if"',
      'Add "else if"',
      'Remove "else"',
    ]);
    const print = create('io.print');
    print.b2cSetExtra({ itemCount: 2 });
    expect(buttons(print).map((button) => button.label)).toEqual([
      'Remove the last input',
      'Add an input',
    ]);
    const call = create('func.call');
    call.b2cSetExtra({ argCount: 1 });
    expect(buttons(call).map((button) => button.label)).toEqual([
      'Remove the last argument',
      'Add an argument',
    ]);
    expect(buttons(call).every((button) => !button.SERIALIZABLE)).toBe(true);
  });

  it('do nothing on a read-only workspace', () => {
    const readOnly = new Blockly.Workspace(new Blockly.Options({ readOnly: true }));
    try {
      const block = readOnly.newBlock('io.print');
      if (!isB2cMutatorBlock(block)) {
        throw new Error('no mutator');
      }
      press(block, 'count:add');
      expect(block.b2cGetExtra()).toEqual({ itemCount: 1 });
    } finally {
      readOnly.dispose();
    }
  });
});

describe('removing a part', () => {
  it('bumps a connected value block out instead of deleting it', () => {
    const print = create('io.print');
    print.b2cSetExtra({ itemCount: 2 });
    const literal = workspace.newBlock('text.literal');
    plug(print, 'ITEM1', literal);
    press(print, 'count:remove');
    expect(print.getInput('ITEM1')).toBeNull();
    expect(literal.isDisposed()).toBe(false);
    expect(literal.getParent()).toBeNull();
    expect(workspace.getTopBlocks(false)).toContain(literal);
  });

  it('bumps a statement list out with every block in it', () => {
    const branch = create('control.if');
    branch.b2cSetExtra({ elseIfCount: 1, hasElse: true });
    const first = workspace.newBlock('io.print');
    const second = workspace.newBlock('var.set');
    plug(branch, 'DO1', first);
    const link = second.previousConnection;
    if (link === null) {
      throw new Error('var.set has no previous connection');
    }
    first.nextConnection?.connect(link);
    const otherwise = workspace.newBlock('control.break');
    plug(branch, 'ELSE', otherwise);

    press(branch, 'count:remove');
    expect(first.getParent()).toBeNull();
    expect(first.getNextBlock()).toBe(second);

    press(branch, 'flag:hasElse:remove');
    expect(otherwise.isDisposed()).toBe(false);
    expect(otherwise.getParent()).toBeNull();
  });

  it('is one undo step that brings the part and its block back', async () => {
    const print = create('io.print');
    print.b2cSetExtra({ itemCount: 2 });
    const literal = workspace.newBlock('text.literal');
    plug(print, 'ITEM1', literal);
    await flushEvents();
    workspace.clearUndo();

    press(print, 'count:remove');
    await flushEvents();
    const recorded = workspace.getUndoStack();
    expect(recorded.length).toBeGreaterThanOrEqual(2);
    expect(new Set(recorded.map((event) => event.group)).size).toBe(1);
    const mutation = recorded.filter(
      (event) => event instanceof Blockly.Events.BlockChange && event.element === 'mutation',
    );
    expect(mutation).toHaveLength(1);
    expect(mutation[0]).toMatchObject({ oldValue: '{"itemCount":2}', newValue: '{"itemCount":1}' });

    workspace.undo(false);
    expect(print.b2cGetExtra()).toEqual({ itemCount: 2 });
    expect(print.getInputTargetBlock('ITEM1')).toBe(literal);
    await flushEvents();

    workspace.undo(true);
    expect(print.b2cGetExtra()).toEqual({ itemCount: 1 });
    expect(literal.getParent()).toBeNull();
  });

  it('undoes an added part in one step too', async () => {
    const branch = create('control.if');
    await flushEvents();
    workspace.clearUndo();
    press(branch, 'flag:hasElse:add');
    await flushEvents();
    workspace.undo(false);
    expect(branch.b2cGetExtra()).toEqual({ elseIfCount: 0, hasElse: false });
    expect(branch.getInput('ELSE')).toBeNull();
  });

  it('records nothing when nothing changes', async () => {
    const print = create('io.print');
    await flushEvents();
    workspace.clearUndo();
    print.b2cSetExtra({ itemCount: 3 });
    await flushEvents();
    expect(workspace.getUndoStack()).toEqual([]);
  });
});

describe('shadows', () => {
  beforeEach(() => {
    configureMutators({
      inputShadow: (_block, input, name) => ({
        type: TEST_SHADOW,
        fields: { TEXT: `${input.name}:${name}` },
      }),
    });
  });

  it('give new value parts their default shadow', () => {
    const print = create('io.print');
    expect(print.getInputTargetBlock('ITEM0')?.isShadow()).toBe(true);
    expect(print.getInputTargetBlock('ITEM0')?.getFieldValue('TEXT')).toBe('ITEM:ITEM0');
    press(print, 'count:add');
    expect(print.getInputTargetBlock('ITEM1')?.getFieldValue('TEXT')).toBe('ITEM:ITEM1');
  });

  it('come back with their edited value when a part comes back', async () => {
    const print = create('io.print');
    press(print, 'count:add');
    print.getInputTargetBlock('ITEM1')?.setFieldValue('edited', 'TEXT');
    await flushEvents();
    workspace.clearUndo();

    press(print, 'count:remove');
    await flushEvents();
    // No shadow deletions are recorded: only the mutation.
    expect(workspace.getUndoStack().map((event) => event.type)).toEqual([
      Blockly.Events.BLOCK_CHANGE,
    ]);
    workspace.undo(false);
    expect(print.getInputTargetBlock('ITEM1')?.getFieldValue('TEXT')).toBe('edited');
  });

  it('are restored under a block that was bumped out and put back', async () => {
    const print = create('io.print');
    press(print, 'count:add');
    const literal = workspace.newBlock('text.literal');
    plug(print, 'ITEM1', literal);
    await flushEvents();
    workspace.clearUndo();
    press(print, 'count:remove');
    await flushEvents();
    workspace.undo(false);
    expect(print.getInputTargetBlock('ITEM1')).toBe(literal);
    literal.unplug();
    expect(print.getInputTargetBlock('ITEM1')?.getFieldValue('TEXT')).toBe('ITEM:ITEM1');
  });

  it('leave the input empty when the factory fails or names an unknown block', () => {
    configureMutators({
      inputShadow: () => {
        throw new Error('no catalog yet');
      },
    });
    const print = create('io.print');
    expect(print.getInputTargetBlock('ITEM0')).toBeNull();

    configureMutators({ inputShadow: () => ({ type: 'not_a_block_type' }) });
    press(print, 'count:add');
    expect(print.getInputTargetBlock('ITEM1')).toBeNull();
    // The bad state is forgotten, so emptying the input again does not retry it.
    const literal = workspace.newBlock('text.literal');
    plug(print, 'ITEM1', literal);
    expect(() => {
      literal.unplug();
    }).not.toThrow();
    expect(print.getInputTargetBlock('ITEM1')).toBeNull();
  });

  it('are not created for statement parts or without a factory', () => {
    const branch = create('control.if');
    expect(branch.getInputTargetBlock('COND0')?.getFieldValue('TEXT')).toBe('COND:COND0');
    expect(branch.getInputTargetBlock('DO0')).toBeNull();
    configureMutators({ inputShadow: null });
    expect(create('io.print').getInputTargetBlock('ITEM0')).toBeNull();
  });
});

describe('logic.operation’s operator', () => {
  it('sits between the first two items and is repeated as text after that', () => {
    const operation = create('logic.operation');
    operation.b2cSetExtra({ itemCount: 4 });
    expect(inputNames(operation)).toEqual([
      'ITEM0',
      'b2c_row0',
      'ITEM1',
      'ITEM2',
      'ITEM3',
      'B2C_BUTTONS',
      'b2c_repeat',
    ]);
    expect(labelTexts(operation, 'ITEM2')).toEqual(['and']);
    operation.setFieldValue('or', 'OP');
    expect(labelTexts(operation, 'ITEM3')).toEqual(['or']);
    expect(operation.getFieldValue('OP')).toBe('or');
    expect(operation.b2cGetExtra()).toEqual({ itemCount: 4 });
  });

  it('is created by the mutator when registration did not draw it', () => {
    Blockly.Blocks['logic.operation'] = {
      init(this: Blockly.Block): void {
        this.setOutput(true);
        Blockly.Extensions.apply(B2C_MUTATOR_ITEMS, this, true);
      },
    };
    const operation = create('logic.operation');
    expect(inputNames(operation)).toEqual(['ITEM0', 'B2C_JOIN', 'ITEM1', 'B2C_BUTTONS']);
    expect(operation.getFieldValue('OP')).toBe('and');
    operation.setFieldValue('or', 'OP');
    press(operation, 'count:add');
    expect(labelTexts(operation, 'ITEM2')).toEqual(['or']);
    // The operator survives even with fewer items than the catalog minimum.
    operation.b2cSetExtra({ itemCount: 1 });
    operation.b2cSetExtra({ itemCount: 3 });
    expect(operation.getFieldValue('OP')).toBe('or');
  });

  it('stays where registration drew it among other fields', () => {
    Blockly.Blocks['logic.operation'] = {
      init(this: Blockly.Block): void {
        const dropdown = new Blockly.FieldDropdown([
          ['and', 'and'],
          ['or', 'or'],
        ]);
        this.appendDummyInput('ROW')
          .appendField(new Blockly.FieldLabel('either'))
          .appendField(dropdown, 'OP');
        this.setOutput(true);
        Blockly.Extensions.apply(B2C_MUTATOR_ITEMS, this, true);
      },
    };
    const operation = create('logic.operation');
    expect(inputNames(operation)).toEqual(['ROW', 'ITEM0', 'ITEM1', 'B2C_BUTTONS']);
    expect(labelTexts(operation, 'ITEM1')).toEqual(['and']);
  });
});

describe('without the registration anchor', () => {
  beforeEach(() => {
    defineTestBlocks({ anchor: false });
  });

  it('places the parts before the first label part after the group', () => {
    expect(inputNames(create('io.print'))).toEqual(['ITEM0', 'B2C_BUTTONS', 'b2c_row0']);
    expect(inputNames(create('control.if'))).toEqual(['COND0', 'DO0', 'B2C_BUTTONS']);
    expect(inputNames(create('func.call'))).toEqual(['b2c_row0', 'B2C_BUTTONS']);
  });
});

describe('call argument labels', () => {
  function callTo(ref: unknown, count: number): B2cMutatorBlock {
    const call = create('func.call');
    const field = call.getField('FUNC');
    if (!(field instanceof ValueField)) {
      throw new Error('FUNC is not a value field');
    }
    field.setValue(ref);
    call.b2cSetExtra({ argCount: count });
    return call;
  }

  const names = new Map<string, string>([
    ['sym_name', 'name'],
    ['sym_times', 'times'],
  ]);

  beforeEach(() => {
    configureMutators({
      symbols: {
        symbolsAt: () => [
          { id: 'sym_greet', kind: 'function', params: ['sym_name', 'sym_times'] },
          { id: 'sym_x', kind: 'variable' },
        ],
        nameOf: (id) => names.get(id) ?? null,
      },
    });
  });

  it('name each argument after its parameter', () => {
    const call = callTo({ ref: 'sym_greet' }, 3);
    expect(labelTexts(call, 'ARG0')).toEqual(['name:']);
    expect(labelTexts(call, 'ARG1')).toEqual(['times:']);
    expect(labelTexts(call, 'ARG2')).toEqual([]);
  });

  it('follow renames after each analysis', () => {
    const call = callTo({ ref: 'sym_greet' }, 2);
    names.set('sym_name', 'who');
    refreshMutatorLabels(workspace);
    expect(labelTexts(call, 'ARG0')).toEqual(['who:']);
    names.set('sym_name', 'bad‮name');
    refreshMutatorLabels(workspace);
    expect(labelTexts(call, 'ARG0')).toEqual(['bad⟨U+202E⟩name:']);
    names.delete('sym_name');
    refreshMutatorLabels(workspace);
    expect(labelTexts(call, 'ARG0')).toEqual([]);
    names.set('sym_name', 'name');
  });

  it('are absent when the function is unknown, not a function or the analysis fails', () => {
    expect(labelTexts(callTo({ ref: 'sym_missing' }, 1), 'ARG0')).toEqual([]);
    expect(labelTexts(callTo({ ref: 'sym_x' }, 1), 'ARG0')).toEqual([]);
    expect(labelTexts(callTo(null, 1), 'ARG0')).toEqual([]);
    expect(labelTexts(callTo('sym_greet', 1), 'ARG0')).toEqual(['name:']);
    configureMutators({
      symbols: {
        symbolsAt: () => {
          throw new Error('stale');
        },
        nameOf: () => null,
      },
    });
    expect(labelTexts(callTo({ ref: 'sym_greet' }, 1), 'ARG0')).toEqual([]);
  });

  it('record no undo step when they change', async () => {
    const call = callTo({ ref: 'sym_greet' }, 1);
    await flushEvents();
    workspace.clearUndo();
    names.set('sym_name', 'other');
    refreshMutatorLabels(workspace);
    await flushEvents();
    expect(workspace.getUndoStack()).toEqual([]);
    expect(labelTexts(call, 'ARG0')).toEqual(['other:']);
    names.set('sym_name', 'name');
  });
});
