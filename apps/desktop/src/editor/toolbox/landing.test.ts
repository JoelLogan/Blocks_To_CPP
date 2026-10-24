/**
 * A block taken from the toolbox, settled where it lands (./landing.ts): free default names, and
 * the variable of a generic *set* or *change* block re-targeted when the one the toolbox chose is
 * not visible there (04 §4.2). The blocks here are created as the flyout creates them (its
 * `createBlock` tells landing.ts which flyout block the new one came from), in an open Blockly
 * event group as Blockly's gesture opens one; the real pointer drags are in ../sync/drag.test.ts.
 */
import type { SymbolInfo } from '@blocks2cpp/b2c-core-wasm';
import { readExprShadow, type TokenJson } from '@blocks2cpp/blockly-ext';
import * as Blockly from 'blockly/core';
import { afterEach, beforeAll, describe, expect, it } from 'vitest';

import { newVariableBlock, staticCategoryContents } from './contents';
import {
  noteCreatedFromToolbox,
  rememberToolboxItem,
  settleDroppedBlock,
  symbolType,
} from './landing';
import { blockState, presetBlock, type B2cBlockInfo } from './presets';
import {
  disposeTestWorkspaces,
  fakeSymbols,
  guessDocument,
  headlessWorkspace,
  loadModule,
  setUpToolboxBlocks,
  symbolFixture,
} from './testing';

beforeAll(() => {
  setUpToolboxBlocks();
});

afterEach(() => {
  Blockly.Events.setGroup(false);
  disposeTestWorkspaces();
});

const guess = symbolFixture('s_guess', 'guess', { declBlock: 'decl' });
const label = symbolFixture('s_label', 'label', { declBlock: 'decl', type: 'string' });
const n = symbolFixture('s_n', 'n', { kind: 'parameter', mode: 'copy', declBlock: 'fn' });
const factorial = symbolFixture('s_fact', 'factorial', {
  kind: 'function',
  params: ['s_n'],
  returns: 'int',
  declBlock: 'fn',
});

/** guessDocument()'s answers: guess below its declaration in main, n in factorial's body. */
const SCOPES: Record<string, readonly SymbolInfo[]> = {
  main: [factorial],
  'main/BODY': [factorial],
  decl: [factorial],
  print: [factorial, guess],
  fn: [factorial],
  'fn/BODY': [factorial, n],
};
const source = fakeSymbols(SCOPES, [factorial, guess, n]);

/** Lets Blockly fire its queued events (and so record them for undo): after a frame. */
function events(): Promise<void> {
  return new Promise((resolve) => {
    requestAnimationFrame(() => {
      setTimeout(resolve, 0);
    });
  });
}

/** A headless canvas with the guess document. */
function canvas(): Blockly.Workspace {
  const workspace = headlessWorkspace();
  loadModule(workspace, guessDocument());
  return workspace;
}

function get(workspace: Blockly.Workspace, id: string): Blockly.Block {
  const block = workspace.getBlockById(id);
  if (block === null) {
    throw new Error(`no block ${id}`);
  }
  return block;
}

/**
 * A block of toolbox item `info` taken out onto `workspace`, in a new event group (left open, as
 * Blockly's gesture leaves it while the block is dragged): a "flyout" block built from the item,
 * changed by `inFlyout` before it is taken out, and the canvas block made from it.
 */
function takeOut(
  workspace: Blockly.Workspace,
  info: B2cBlockInfo,
  inFlyout?: (original: Blockly.Block) => void,
): Blockly.Block {
  const flyout = headlessWorkspace();
  const original = Blockly.serialization.blocks.append(blockState(info), flyout);
  rememberToolboxItem(original, info);
  inFlyout?.(original);
  Blockly.Events.setGroup(true);
  const created = Blockly.serialization.blocks.append(
    Blockly.serialization.blocks.save(original, { addNextBlocks: false }) ?? { type: info.type },
    workspace,
    { recordUndo: true },
  );
  noteCreatedFromToolbox(original, created);
  return created;
}

/** Connects a statement block below block `after`, or as the first of `container`'s body. */
function place(workspace: Blockly.Workspace, block: Blockly.Block, where: string): void {
  const target =
    where === 'fn' || where === 'main'
      ? get(workspace, where).getInput('BODY')?.connection
      : get(workspace, where).nextConnection;
  const own = block.previousConnection;
  if (target === null || target === undefined || own === null) {
    throw new Error('nowhere to place the block');
  }
  own.connect(target);
}

/** Plugs value block `block` into the first slot of block `into`. */
function plug(workspace: Blockly.Workspace, block: Blockly.Block, into: string): void {
  const slot = get(workspace, into).getInput('ITEM0')?.connection;
  const own = block.outputConnection;
  if (slot === null || slot === undefined || own === null) {
    throw new Error('nowhere to plug the block');
  }
  own.connect(slot);
}

/** Drops the block where it is: settles it in the open group, then closes the group. */
function drop(workspace: Blockly.Workspace, block: Blockly.Block, symbols = source): void {
  try {
    settleDroppedBlock(workspace, block, symbols);
  } finally {
    Blockly.Events.setGroup(false);
  }
}

/** The name a declaration field declares. */
function nameIn(block: Blockly.Block, field: string): string | null {
  return block.getField(field)?.getValue() as string | null;
}

/** The tokens of a block's VALUE shadow, or `null`. */
function valueOf(block: Blockly.Block): readonly TokenJson[] | null {
  const shadow = block.getInputTargetBlock('VALUE');
  return shadow === null ? null : (readExprShadow(shadow)?.tokens ?? null);
}

/** The toolbox's generic block of `type`, preset to `ref` (with set's start value `start`). */
function generic(type: string, ref: string | null, start?: readonly TokenJson[]): B2cBlockInfo {
  return presetBlock(type, {
    ...(ref === null ? {} : { fields: { VAR: { ref } } }),
    ...(start === undefined ? {} : { inputs: { VALUE: start } }),
    retarget: 'VAR',
  });
}

describe('a declaration dropped from the toolbox', () => {
  it('gets the first free name where it lands', () => {
    const workspace = canvas();
    // `create value` below `create guess` in main, where value is free: it stays value.
    const free = takeOut(workspace, newVariableBlock('value'));
    place(workspace, free, 'decl');
    drop(workspace, free);
    expect(nameIn(free, 'NAME')).toBe('value');
    // A second one in the same list: value2.
    const second = takeOut(workspace, newVariableBlock('value'));
    place(workspace, second, 'print');
    drop(workspace, second);
    expect(nameIn(second, 'NAME')).toBe('value2');
  });

  it('names a counted loop’s counter and a function after what is in use there', () => {
    const workspace = canvas();
    const loop = (staticCategoryContents('loops') as B2cBlockInfo[]).find(
      (item) => item.type === 'control.for_range',
    );
    if (loop === undefined) {
      throw new Error('no counted loop');
    }
    const counter = takeOut(workspace, loop);
    place(workspace, counter, 'print');
    const withI = fakeSymbols(
      { ...SCOPES, print: [factorial, guess, symbolFixture('s_i', 'i', { declBlock: 'x' })] },
      [factorial, guess, n],
    );
    drop(workspace, counter, withI);
    expect(nameIn(counter, 'VAR')).toBe('j');

    const define = (staticCategoryContents('functions') as B2cBlockInfo[]).find(
      (item) => item.type === 'func.define',
    );
    if (define === undefined) {
      throw new Error('no define');
    }
    const first = takeOut(workspace, define);
    drop(workspace, first);
    expect(nameIn(first, 'NAME')).toBe('myFunction');
    const second = takeOut(workspace, define);
    drop(workspace, second);
    expect(nameIn(second, 'NAME')).toBe('myFunction2');
  });

  it('keeps a name the user gave it in the toolbox', () => {
    const workspace = canvas();
    const named = takeOut(workspace, newVariableBlock('value'), (original) => {
      original.setFieldValue('guess', 'NAME');
    });
    place(workspace, named, 'print');
    drop(workspace, named);
    // A clash the analyser reports; the editor does not rename what the user named.
    expect(nameIn(named, 'NAME')).toBe('guess');
  });
});

describe('a set or change block dropped from the toolbox', () => {
  it('takes the first fitting variable visible there, with set’s value of its type', () => {
    const workspace = canvas();
    // The toolbox chose label (a text, ""); in factorial's body only n (int) is visible.
    const set = takeOut(workspace, generic('var.set', 's_label', [{ str: '' }]));
    place(workspace, set, 'fn');
    drop(workspace, set, fakeSymbols(SCOPES, [factorial, guess, label, n]));
    expect(set.getFieldValue('VAR')).toBe('s_n');
    expect(valueOf(set)).toEqual([{ num: '0' }]);
    expect(symbolType(source, 's_n')).toBe('int');
    expect(symbolType(source, null)).toBeNull();
  });

  it('keeps the variable it came with when it is visible there', () => {
    const workspace = canvas();
    const change = takeOut(workspace, generic('var.change', 's_guess'));
    place(workspace, change, 'print');
    drop(workspace, change);
    expect(change.getFieldValue('VAR')).toBe('s_guess');
  });

  it('takes a variable when the toolbox had none to give it', () => {
    const workspace = canvas();
    const set = takeOut(workspace, generic('var.set', null));
    place(workspace, set, 'print');
    drop(workspace, set);
    expect(set.getFieldValue('VAR')).toBe('s_guess');
  });

  it('skips what it may not change: a change takes no text', () => {
    const workspace = canvas();
    const change = takeOut(workspace, generic('var.change', 's_n'));
    place(workspace, change, 'print');
    drop(
      workspace,
      change,
      fakeSymbols({ print: [factorial, label, guess] }, [factorial, label, guess, n]),
    );
    expect(change.getFieldValue('VAR')).toBe('s_guess');
  });

  it('keeps its variable when nothing fitting is visible there', () => {
    const workspace = canvas();
    const set = takeOut(workspace, generic('var.set', 's_guess', [{ num: '0' }]));
    place(workspace, set, 'main');
    drop(workspace, set);
    // At the top of main only factorial is visible: guess stays (the analyser says E0203).
    expect(set.getFieldValue('VAR')).toBe('s_guess');
  });

  it('is one undo step with the drop', async () => {
    const workspace = canvas();
    await events();
    workspace.clearUndo();
    const set = takeOut(workspace, generic('var.set', 's_label', [{ str: '' }]));
    place(workspace, set, 'fn');
    drop(workspace, set, fakeSymbols(SCOPES, [factorial, guess, label, n]));
    await events();
    await events();
    const groups = new Set(workspace.getUndoStack().map((event) => event.group));
    expect(groups.size).toBe(1);
    workspace.undo(false);
    await events();
    expect(workspace.getBlockById(set.id)).toBeNull();
    expect(get(workspace, 'fn').getInputTargetBlock('BODY')).toBeNull();
  });
});

describe('blocks that keep their variable', () => {
  it('a getter, wherever it is dropped', () => {
    const workspace = canvas();
    const getter = takeOut(workspace, presetBlock('var.get', { fields: { VAR: { ref: 's_n' } } }));
    plug(workspace, getter, 'print');
    drop(workspace, getter);
    expect(getter.getFieldValue('VAR')).toBe('s_n');
  });

  it('a set whose variable the user chose in the toolbox', () => {
    const workspace = canvas();
    const set = takeOut(workspace, generic('var.set', 's_guess', [{ num: '0' }]), (original) => {
      original.setFieldValue('s_label', 'VAR');
    });
    place(workspace, set, 'fn');
    drop(workspace, set, fakeSymbols(SCOPES, [factorial, guess, label, n]));
    expect(set.getFieldValue('VAR')).toBe('s_label');
  });

  it('a block dropped in another gesture than the one that took it out, and the next drop', () => {
    const workspace = canvas();
    const set = takeOut(workspace, generic('var.set', 's_label', [{ str: '' }]));
    Blockly.Events.setGroup(false);
    place(workspace, set, 'fn');
    // Another gesture (a keyboard add dragged later): left alone, and not settled again.
    Blockly.Events.setGroup(true);
    drop(workspace, set, fakeSymbols(SCOPES, [factorial, guess, label, n]));
    expect(set.getFieldValue('VAR')).toBe('s_label');

    const again = takeOut(workspace, generic('var.set', 's_label', [{ str: '' }]));
    place(workspace, again, 'fn');
    drop(workspace, again, fakeSymbols(SCOPES, [factorial, guess, label, n]));
    expect(again.getFieldValue('VAR')).toBe('s_n');
    // Moved again: it is the user's block now.
    again.getField('VAR')?.setValue('s_label');
    Blockly.Events.setGroup(true);
    drop(workspace, again, fakeSymbols(SCOPES, [factorial, guess, label, n]));
    expect(again.getFieldValue('VAR')).toBe('s_label');
  });

  it('a block that did not come from the toolbox', () => {
    const workspace = canvas();
    Blockly.Events.setGroup(true);
    const set = Blockly.serialization.blocks.append(
      blockState(generic('var.set', 's_label', [{ str: '' }])),
      workspace,
    );
    place(workspace, set, 'fn');
    drop(workspace, set, fakeSymbols(SCOPES, [factorial, guess, label, n]));
    expect(set.getFieldValue('VAR')).toBe('s_label');
  });
});
