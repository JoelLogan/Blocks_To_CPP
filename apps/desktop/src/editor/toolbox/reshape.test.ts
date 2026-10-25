import type { TypeJson } from '@blocks2cpp/b2c-core-wasm';
import { readExprShadow, type TokenJson } from '@blocks2cpp/blockly-ext';
import * as Blockly from 'blockly/core';
import { afterEach, beforeAll, describe, expect, it, vi } from 'vitest';

import { newVariableBlock } from './contents';
import { blockState, presetBlock } from './presets';
import {
  START_VALUE_EVENT,
  StartValueChange,
  installStartValueReshaping,
  reshapeSetValue,
  reshapeStartValue,
} from './reshape';
import { disposeTestWorkspaces, headlessWorkspace, setUpToolboxBlocks } from './testing';
import { startTokensForDeclaredType, startTokensForStaticType } from './values';

beforeAll(() => {
  setUpToolboxBlocks();
});

afterEach(() => {
  disposeTestWorkspaces();
});

/** A type lookup that knows no symbol. */
const NO_TYPES = (): null => null;

/** One round of Blockly's event firing: after a frame. */
function frame(): Promise<void> {
  return new Promise((resolve) => {
    requestAnimationFrame(() => {
      setTimeout(resolve, 0);
    });
  });
}

/**
 * Lets Blockly fire its queued events, and the events its listeners fire in turn (the start value
 * change is fired from the type change's listener, so it goes out a round later).
 */
async function events(): Promise<void> {
  await frame();
  await frame();
}

/** A `create int value = 0` block, as the toolbox gives it, on a new headless canvas. */
function newVariable(): { workspace: Blockly.Workspace; block: Blockly.Block } {
  const workspace = headlessWorkspace();
  const block = Blockly.serialization.blocks.append(
    blockState(newVariableBlock('value')),
    workspace,
    {
      recordUndo: true,
    },
  );
  return { workspace, block };
}

/** The tokens of the block's start value, or `null` when it holds no expression shadow. */
function startValue(block: Blockly.Block): readonly TokenJson[] | null {
  const shadow = block.getInputTargetBlock('VALUE');
  return shadow === null ? null : (readExprShadow(shadow)?.tokens ?? null);
}

describe('start values (M2 smart defaults)', () => {
  it('give each type its own start value', () => {
    expect(startTokensForStaticType('int')).toEqual([{ num: '0' }]);
    expect(startTokensForStaticType('double')).toEqual([{ num: '0.0' }]);
    expect(startTokensForStaticType('bool')).toEqual([{ kw: 'true' }]);
    expect(startTokensForStaticType('char')).toEqual([{ chr: 'a' }]);
    expect(startTokensForStaticType('std::string')).toEqual([{ str: '' }]);
    expect(startTokensForStaticType('void')).toBeNull();
    expect(startTokensForStaticType('error')).toBeNull();
    // M2's name for text is no type name on the wire any more (types.md §3.1).
    expect(startTokensForStaticType('string')).toBeNull();
    // No start values for library and user types in M3 (slots.md §6).
    expect(startTokensForStaticType({ 'std::vector': 'int' })).toBeNull();
    expect(startTokensForStaticType({ ref: 's_point' })).toBeNull();
    expect(startTokensForDeclaredType('{"std::vector":"int"}')).toBeNull();
    expect(startTokensForDeclaredType('std::string')).toEqual([{ str: '' }]);
    expect(startTokensForDeclaredType('auto')).toBeNull();
    expect(startTokensForDeclaredType('nonsense')).toBeNull();
  });
});

describe('reshaping a new variable’s start value', () => {
  it('follows the type while the value is still the old type’s start value', async () => {
    const { workspace, block } = newVariable();
    const stop = installStartValueReshaping(workspace, NO_TYPES);
    await events();
    const steps: [string, readonly TokenJson[]][] = [
      ['double', [{ num: '0.0' }]],
      ['bool', [{ kw: 'true' }]],
      ['char', [{ chr: 'a' }]],
      ['std::string', [{ str: '' }]],
      ['int', [{ num: '0' }]],
    ];
    for (const [type, tokens] of steps) {
      block.setFieldValue(type, 'TYPE');
      await events();
      expect(startValue(block)).toEqual(tokens);
    }
    stop();
  });

  it('leaves a value the user changed, a block in the slot, and auto alone', async () => {
    const { workspace, block } = newVariable();
    const stop = installStartValueReshaping(workspace, NO_TYPES);
    block.getInputTargetBlock('VALUE')?.setFieldValue('42', 'VALUE');
    await events();
    block.setFieldValue('double', 'TYPE');
    await events();
    expect(startValue(block)).toEqual([{ num: '42' }]);

    const other = newVariable().block;
    expect(reshapeStartValue(other, 'int', 'auto')).toBeNull();
    expect(reshapeStartValue(other, 'auto', 'int')).toBeNull();
    expect(reshapeStartValue(other, 'int', 'int')).toBeNull();
    expect(reshapeStartValue(other, 'double', 'bool')).toBeNull();
    expect(startValue(other)).toEqual([{ num: '0' }]);

    const plugged = newVariable();
    const number = Blockly.serialization.blocks.append({ type: 'math.number' }, plugged.workspace);
    const output = number.outputConnection;
    if (output === null) {
      throw new Error('a number block has an output');
    }
    plugged.block.getInput('VALUE')?.connection?.connect(output);
    expect(reshapeStartValue(plugged.block, 'int', 'double')).toBeNull();
    stop();
  });

  it('is undone and redone together with the type change', async () => {
    const { workspace, block } = newVariable();
    const stop = installStartValueReshaping(workspace, NO_TYPES);
    await events();
    workspace.clearUndo();
    block.setFieldValue('bool', 'TYPE');
    await events();
    expect(startValue(block)).toEqual([{ kw: 'true' }]);
    expect(workspace.getUndoStack().map((event) => event.type)).toEqual([
      'change',
      START_VALUE_EVENT,
    ]);
    workspace.undo(false);
    await events();
    expect(block.getFieldValue('TYPE')).toBe('int');
    expect(startValue(block)).toEqual([{ num: '0' }]);
    workspace.undo(true);
    await events();
    expect(block.getFieldValue('TYPE')).toBe('bool');
    expect(startValue(block)).toEqual([{ kw: 'true' }]);
    stop();
  });

  it('describes its change as an event that can be replayed', () => {
    const { block } = newVariable();
    const change = reshapeStartValue(block, 'int', 'char');
    expect(change).toBeInstanceOf(StartValueChange);
    expect(change?.isNull()).toBe(false);
    expect(startValue(block)).toEqual([{ chr: 'a' }]);
    change?.run(false);
    expect(startValue(block)).toEqual([{ num: '0' }]);
    change?.run(true);
    expect(startValue(block)).toEqual([{ chr: 'a' }]);
    expect(new StartValueChange(block, [{ num: '0' }], [{ num: '0' }]).isNull()).toBe(true);

    const warn = vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    block.dispose(false);
    change?.run(false);
    expect(warn).toHaveBeenCalledWith('The start value to restore is gone');
  });

  it('stops when uninstalled, and ignores changes it did not make', async () => {
    const { workspace, block } = newVariable();
    const stop = installStartValueReshaping(workspace, NO_TYPES);
    stop();
    block.setFieldValue('double', 'TYPE');
    await events();
    expect(startValue(block)).toEqual([{ num: '0' }]);

    // Loading (events off) and undo replays are not the user changing the type.
    const again = installStartValueReshaping(workspace, NO_TYPES);
    Blockly.Events.disable();
    try {
      block.setFieldValue('bool', 'TYPE');
    } finally {
      Blockly.Events.enable();
    }
    await events();
    expect(startValue(block)).toEqual([{ num: '0' }]);
    again();
  });
});

describe('reshaping the value of set when its variable changes', () => {
  /** The types of the variables the tests choose: int, double, text, and one not known. */
  const TYPES: Readonly<Record<string, TypeJson>> = {
    s_count: 'int',
    s_price: 'double',
    s_name: 'std::string',
  };
  const typeOf = (sym: string): TypeJson | null => TYPES[sym] ?? null;

  /** `set count to 0`, as the toolbox gives it, on a new headless canvas. */
  function newSet(): { workspace: Blockly.Workspace; block: Blockly.Block } {
    const workspace = headlessWorkspace();
    const block = Blockly.serialization.blocks.append(
      blockState(
        presetBlock('var.set', {
          fields: { VAR: { ref: 's_count' } },
          inputs: { VALUE: [{ num: '0' }] },
        }),
      ),
      workspace,
      { recordUndo: true },
    );
    return { workspace, block };
  }

  it('follows the new variable’s type while the value is still the old type’s start value', async () => {
    const { workspace, block } = newSet();
    const stop = installStartValueReshaping(workspace, typeOf);
    await events();
    block.setFieldValue('s_price', 'VAR');
    await events();
    expect(startValue(block)).toEqual([{ num: '0.0' }]);
    block.setFieldValue('s_name', 'VAR');
    await events();
    expect(startValue(block)).toEqual([{ str: '' }]);
    // A variable whose type the analysis does not know (yet): the value stays.
    block.setFieldValue('s_new', 'VAR');
    await events();
    expect(startValue(block)).toEqual([{ str: '' }]);
    stop();
  });

  it('leaves a value the user typed alone', async () => {
    const { workspace, block } = newSet();
    const stop = installStartValueReshaping(workspace, typeOf);
    block.getInputTargetBlock('VALUE')?.setFieldValue('7', 'VALUE');
    await events();
    block.setFieldValue('s_price', 'VAR');
    await events();
    expect(startValue(block)).toEqual([{ num: '7' }]);
    expect(reshapeSetValue(block, 'int', 'double')).toBeNull();
    stop();
  });

  it('is one undo step with the choice of the variable', async () => {
    const { workspace, block } = newSet();
    const stop = installStartValueReshaping(workspace, typeOf);
    await events();
    workspace.clearUndo();
    block.setFieldValue('s_price', 'VAR');
    await events();
    expect(workspace.getUndoStack().map((event) => event.type)).toEqual([
      'change',
      START_VALUE_EVENT,
    ]);
    workspace.undo(false);
    await events();
    expect(block.getFieldValue('VAR')).toBe('s_count');
    expect(startValue(block)).toEqual([{ num: '0' }]);
    stop();
  });

  it('changes only set blocks, and only between known types', () => {
    const { block } = newSet();
    expect(reshapeSetValue(block, null, 'double')).toBeNull();
    expect(reshapeSetValue(block, 'int', null)).toBeNull();
    expect(reshapeSetValue(newVariable().block, 'int', 'double')).toBeNull();
    const change = reshapeSetValue(block, 'int', 'char');
    expect(change).toBeInstanceOf(StartValueChange);
    expect(startValue(block)).toEqual([{ chr: 'a' }]);
  });
});
