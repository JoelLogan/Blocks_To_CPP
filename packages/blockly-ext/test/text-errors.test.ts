/**
 * W2's text, math and error blocks in the editor, from the generated catalog (package B,
 * docs/plans/m3-contracts/w2.md §1.3, §1.4, §1.7, §5.2): the static types of their outputs, as
 * the analyser gives them, and *try … catch* drawn by the *branches* mutator with its repeated
 * fields (add, remove, undo, a catch part's variable keeping its symbol); *throw the error again*.
 */
import * as Blockly from 'blockly/core';
import { afterEach, beforeAll, beforeEach, describe, expect, it } from 'vitest';

import { blockDef } from '../src/checker/catalog';
import { staticOutputType } from '../src/checker/output-type';
import type { OutputTypeOracle } from '../src/checker/types';
import { registerB2cFields } from '../src/fields/register';
import { B2cSymbolDeclField } from '../src/fields/symbol-decl';
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
import { typeKey } from '../src/types/static-type';

const NO_ORACLE: OutputTypeOracle = { outputTypeOf: () => null };

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

/** The block's static output type, by its key (`int`, `std::string`), or `null`. */
function typeOf(block: Blockly.Block): string | null {
  const def = blockDef(block.type);
  if (def === undefined) {
    throw new Error(`not a catalog block: ${block.type}`);
  }
  const type = staticOutputType(block, def, NO_ORACLE);
  return type === null ? null : typeKey(type);
}

function plug(parent: Blockly.Block, input: string, child: Blockly.Block): void {
  const connection = parent.getInput(input)?.connection;
  if (connection === null || connection === undefined || child.outputConnection === null) {
    throw new Error(`cannot plug into ${input}`);
  }
  connection.connect(child.outputConnection);
}

function number(text: string): Blockly.Block {
  const block = workspace.newBlock('math.number');
  block.setFieldValue(text, 'VALUE');
  return block;
}

function character(): Blockly.Block {
  return workspace.newBlock('text.char');
}

describe('output types', () => {
  it('are fixed for the text blocks and the errors’ message', () => {
    // *split* gives a list, which typed slots type (`typeBlock`; the slot fixture compares it with
    // the analyser), not this checker.
    expect(typeOf(workspace.newBlock('text.split'))).toBeNull();
    for (const [type, expected] of [
      ['text.length', 'int'],
      ['text.letter', 'char'],
      ['text.case', 'std::string'],
      ['text.as_text', 'std::string'],
      ['text.find', 'bool'],
      ['text.part', 'std::string'],
      ['text.trim', 'std::string'],
      ['text.replace', 'std::string'],
      ['error.message', 'std::string'],
      ['math.random_real', 'double'],
    ] as const) {
      expect(typeOf(workspace.newBlock(type)), type).toBe(expected);
    }
  });

  it('read the chosen type of text as number', () => {
    const block = workspace.newBlock('text.to_number');
    expect(typeOf(block)).toBe('int');
    block.setFieldValue('double', 'TO');
    expect(typeOf(block)).toBe('double');
  });

  it('make a function of a whole number a decimal number, as the analyser does', () => {
    const root = workspace.newBlock('math.fn');
    root.setFieldValue('sqrt', 'FN');
    plug(root, 'VALUE', number('9'));
    expect(typeOf(root)).toBe('double');
    const power = workspace.newBlock('math.fn_two');
    expect(typeOf(power)).toBe('double');
  });

  it('give larger, smaller, absolute value and kept between the arithmetic type of their inputs', () => {
    const abs = workspace.newBlock('math.abs');
    plug(abs, 'VALUE', character());
    expect(typeOf(abs)).toBe('int');

    const smaller = workspace.newBlock('math.minmax');
    smaller.setFieldValue('min', 'FN');
    plug(smaller, 'A', character());
    expect(typeOf(smaller)).toBe('int');
    plug(smaller, 'B', number('2.5'));
    expect(typeOf(smaller)).toBe('double');

    const clamp = workspace.newBlock('math.clamp');
    // The catalog defaults: 15, 0 and 10.
    expect(typeOf(clamp)).toBe('int');
    plug(clamp, 'HIGH', number('99.5'));
    expect(typeOf(clamp)).toBe('double');
  });
});

// --- try … catch -------------------------------------------------------------------------

function tryBlock(): B2cMutatorBlock {
  const block = workspace.newBlock('error.try');
  if (!isB2cMutatorBlock(block)) {
    throw new Error('error.try has no b2c mutator');
  }
  return block;
}

function press(block: Blockly.Block, key: string): void {
  const button = block.inputList
    .flatMap((input) => input.fieldRow)
    .find(
      (field): field is MutatorButton => field instanceof MutatorButton && buttonKey(field) === key,
    );
  if (button === undefined) {
    throw new Error(`no button ${key}`);
  }
  button.showEditor();
}

function declOf(block: Blockly.Block, name: string): { sym: string; name: string } | null {
  const field = block.getField(name);
  return field instanceof B2cSymbolDeclField ? field.getDecl() : null;
}

describe('try … catch', () => {
  it('starts with one catch part for any error', () => {
    const block = tryBlock();
    expect(block.b2cGetExtra()).toEqual({ catchCount: 1 });
    expect(inputNames(block)).toEqual(['TRY', 'CATCH0', 'B2C_BUTTONS', 'b2c_repeat']);
    expect(labelTexts(block, 'CATCH0')).toEqual(['catch', 'as']);
    expect(block.getFieldValue('ERROR0')).toBe('std::exception');
  });

  it('adds a catch part with the joining word, a fresh symbol and the name e', () => {
    let next = 0;
    configureMutators({
      newSymbolId: () => `sym_catch${String((next += 1))}`,
      declName: (_block, field) => (field === 'NAME' ? 'e' : null),
    });
    const block = tryBlock();
    press(block, 'count:add');
    press(block, 'count:add');
    expect(block.b2cGetExtra()).toEqual({ catchCount: 3 });
    expect(labelTexts(block, 'CATCH2')).toEqual(['catch', 'as']);
    expect(['ERROR1', 'ERROR2'].map((name): unknown => block.getFieldValue(name))).toEqual([
      'std::exception',
      'std::exception',
    ]);
    const symbols = ['NAME0', 'NAME1', 'NAME2'].map((name) => declOf(block, name)?.sym);
    expect(new Set(symbols).size).toBe(3);
    expect(declOf(block, 'NAME2')).toEqual({ sym: 'sym_catch3', name: 'e' });
  });

  it('removes the last part and brings it back with its symbol and error on undo', async () => {
    const block = tryBlock();
    block.b2cSetExtra({ catchCount: 2 });
    block.setFieldValue('std::out_of_range', 'ERROR1');
    const decl = declOf(block, 'NAME1');
    expect(decl).not.toBeNull();
    await flushEvents();
    press(block, 'count:remove');
    expect(block.b2cGetExtra()).toEqual({ catchCount: 1 });
    expect(block.getField('ERROR1')).toBeNull();
    await flushEvents();
    workspace.undo(false);
    expect(block.b2cGetExtra()).toEqual({ catchCount: 2 });
    expect(declOf(block, 'NAME1')).toEqual(decl);
    expect(block.getFieldValue('ERROR1')).toBe('std::out_of_range');
  });

  it('offers the four errors in each part, and throw the three with a message', () => {
    const block = tryBlock();
    const options = (field: string, on: Blockly.Block = block): string[] => {
      const dropdown = on.getField(field);
      if (!(dropdown instanceof Blockly.FieldDropdown)) {
        throw new Error(`${field} is not a menu`);
      }
      return dropdown.getOptions(false).map((option) => option[1]);
    };
    expect(options('ERROR0')).toEqual([
      'std::exception',
      'std::invalid_argument',
      'std::out_of_range',
      'std::runtime_error',
    ]);
    expect(options('ERROR', workspace.newBlock('error.throw'))).toEqual([
      'std::runtime_error',
      'std::invalid_argument',
      'std::out_of_range',
    ]);
  });

  it('throws the caught error again with a statement of no parts', () => {
    const rethrow = workspace.newBlock('error.rethrow');
    expect(labelTexts(rethrow)).toEqual(['throw the error again']);
    expect(rethrow.inputList.filter((input) => input.connection !== null)).toEqual([]);
    expect(rethrow.previousConnection).not.toBeNull();
    expect(rethrow.outputConnection).toBeNull();
  });
});
