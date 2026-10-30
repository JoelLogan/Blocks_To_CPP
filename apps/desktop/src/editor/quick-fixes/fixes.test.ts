/**
 * Quick fixes (docs/plans/m3-contracts/w4-editor-tools.md §2.5): which diagnostics get one, what
 * each does, that it is one undo step, and that it is not offered once it no longer applies.
 */
import type { SymbolInfo } from '@blocks2cpp/b2c-core-wasm';
import {
  DEFAULT_EDITOR_SERVICES,
  hasB2cMutator,
  resetEditorServices,
  setEditorServices,
} from '@blocks2cpp/blockly-ext';
import type { Diagnostic } from '@blocks2cpp/ipc-types';
import * as Blockly from 'blockly/core';
import { afterEach, beforeAll, describe, expect, it } from 'vitest';

import { diagnosticFixture } from '../../app/testing/fixtures';
import {
  disposeTestWorkspaces,
  fakeSymbols,
  injectedWorkspace,
  setUpToolboxBlocks,
  symbolFixture,
} from '../toolbox/testing';
import { FIXABLE_CODES, fixesFor, type FixContext } from './fixes';

beforeAll(() => {
  setUpToolboxBlocks();
});

afterEach(() => {
  resetEditorServices();
  disposeTestWorkspaces();
});

/** A canvas with `states` on it, and a context over `symbols`. */
function canvas(
  states: readonly Blockly.serialization.blocks.State[],
  symbols: readonly SymbolInfo[] = [],
): FixContext {
  const workspace = injectedWorkspace('category');
  Blockly.Events.disable();
  try {
    for (const state of states) {
      Blockly.serialization.blocks.append(state, workspace);
    }
  } finally {
    Blockly.Events.enable();
  }
  return { workspace, symbols: fakeSymbols({}, symbols), moduleId: 'mod_main' };
}

function diagnostic(
  code: string,
  block?: string,
  part: Diagnostic['primary']['part'] = { kind: 'whole' },
): Diagnostic {
  return diagnosticFixture({
    code,
    primary: block === undefined ? { part } : { module: 'mod_main', block, part },
  });
}

/** Waits until Blockly has delivered its events (so they are on the undo stack). */
function delivered(): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, 10));
}

/**
 * Undoes the fix with one Undo: the canvas was loaded with events off, so the fix's events are all
 * there is on the stack, and one step must take all of them.
 */
async function undoOnce(workspace: Blockly.WorkspaceSvg): Promise<void> {
  await delivered();
  expect(workspace.getUndoStack().length).toBeGreaterThan(0);
  workspace.undo(false);
  expect(workspace.getUndoStack()).toHaveLength(0);
}

describe('which diagnostics', () => {
  it('get a fix: the five of §2.5 only', () => {
    expect([...FIXABLE_CODES].sort()).toEqual([
      'B2C-E0201',
      'B2C-E0403',
      'B2C-E0405',
      'B2C-E0410',
      'B2C-W0508',
    ]);
    const context = canvas([{ type: 'program.main', id: 'main' }]);
    expect(fixesFor(diagnostic('B2C-E0301', 'main'), context)).toEqual([]);
  });
});

describe('B2C-E0201: create the variable again', () => {
  const states: Blockly.serialization.blocks.State[] = [
    {
      type: 'program.main',
      id: 'main',
      inputs: {
        BODY: {
          block: {
            type: 'io.print',
            id: 'print',
            extraState: { itemCount: 1 },
            inputs: {
              ITEM0: { block: { type: 'var.get', id: 'get', fields: { VAR: { ref: 's_gone' } } } },
            },
          },
        },
      },
    },
  ];

  it('makes a create with the symbol and its last name at the start of the body, as one undo step', async () => {
    setEditorServices({
      ...DEFAULT_EDITOR_SERVICES,
      symbols: { symbolsAt: () => [], nameOf: (id) => (id === 's_gone' ? 'score' : null) },
    });
    const context = canvas(states);
    const [fix] = fixesFor(diagnostic('B2C-E0201', 'get', { kind: 'field', name: 'VAR' }), context);
    expect(fix?.title).toBe('Create variable “score”');
    expect(fix?.apply()).toBe(true);
    const first = context.workspace.getBlockById('main')?.getInputTargetBlock('BODY');
    expect(first?.type).toBe('var.declare');
    expect(first?.getFieldValue('NAME')).toBe('score');
    expect(first?.getNextBlock()?.id).toBe('print');
    expect((Blockly.common.getSelected() as Blockly.Block | null)?.id).toBe(first?.id);
    // Declared now: no longer offered.
    expect(fixesFor(diagnostic('B2C-E0201', 'get'), context)).toEqual([]);
    await undoOnce(context.workspace);
    expect(context.workspace.getBlockById('main')?.getInputTargetBlock('BODY')?.id).toBe('print');
    expect(context.workspace.getBlocksByType('var.declare', false)).toEqual([]);
  });

  /** A print of the lost `s_gone` in a body, its getter `getter`. */
  const printOfGone = (getter: string): Blockly.serialization.blocks.State => ({
    type: 'io.print',
    extraState: { itemCount: 1 },
    inputs: {
      ITEM0: { block: { type: 'var.get', id: getter, fields: { VAR: { ref: 's_gone' } } } },
    },
  });

  it('is not offered when the lost variable is used in two bodies', () => {
    setEditorServices({
      ...DEFAULT_EDITOR_SERVICES,
      symbols: { symbolsAt: () => [], nameOf: (id) => (id === 's_gone' ? 'score' : null) },
    });
    const context = canvas([
      { type: 'program.main', id: 'main', inputs: { BODY: { block: printOfGone('get') } } },
      {
        type: 'func.define',
        id: 'fn',
        x: 400,
        fields: { NAME: { sym: 's_fn', name: 'addPoint' }, RETURNS: 'void' },
        extraState: { params: [] },
        inputs: { BODY: { block: printOfGone('get2') } },
      },
    ]);
    const field = { kind: 'field', name: 'VAR' } as const;
    expect(fixesFor(diagnostic('B2C-E0201', 'get', field), context)).toEqual([]);
    expect(fixesFor(diagnostic('B2C-E0201', 'get2', field), context)).toEqual([]);
  });

  it('is not offered for a reference outside any body', () => {
    setEditorServices({
      ...DEFAULT_EDITOR_SERVICES,
      symbols: { symbolsAt: () => [], nameOf: (id) => (id === 's_gone' ? 'score' : null) },
    });
    const context = canvas([
      {
        type: 'var.global',
        id: 'global',
        fields: { TYPE: 'int', NAME: { sym: 's_total', name: 'total' } },
        inputs: {
          VALUE: { block: { type: 'var.get', id: 'get', fields: { VAR: { ref: 's_gone' } } } },
        },
      },
    ]);
    expect(
      fixesFor(diagnostic('B2C-E0201', 'get', { kind: 'field', name: 'VAR' }), context),
    ).toEqual([]);
    expect(context.workspace.getBlocksByType('var.declare', false)).toEqual([]);
  });

  it('is not offered without a last name, or for a symbol the analysis knows', () => {
    const context = canvas(states);
    expect(fixesFor(diagnostic('B2C-E0201', 'get'), context)).toEqual([]);
    setEditorServices({
      ...DEFAULT_EDITOR_SERVICES,
      symbols: { symbolsAt: () => [], nameOf: () => 'score' },
    });
    const known = canvas(states, [symbolFixture('s_gone', 'score')]);
    expect(fixesFor(diagnostic('B2C-E0201', 'get'), known)).toEqual([]);
  });
});

describe('B2C-E0410 and B2C-E0403: returns', () => {
  const define = (
    body?: Blockly.serialization.blocks.State,
  ): Blockly.serialization.blocks.State => ({
    type: 'func.define',
    id: 'fn',
    fields: { NAME: { sym: 's_fn', name: 'half' }, RETURNS: 'double' },
    extraState: { params: [] },
    ...(body === undefined ? {} : { inputs: { BODY: { block: body } } }),
  });

  it('adds a return with the result type’s start value at the end of the function', async () => {
    const context = canvas([define({ type: 'control.break', id: 'stop' })]);
    const [fix] = fixesFor(diagnostic('B2C-E0410', 'fn'), context);
    expect(fix?.title).toBe('Add a “return” block at the end');
    expect(fix?.apply()).toBe(true);
    const added = context.workspace.getBlockById('stop')?.getNextBlock();
    expect(added?.type).toBe('func.return');
    expect(fixesFor(diagnostic('B2C-E0410', 'fn'), context)).toEqual([]);
    expect(fix?.apply()).toBe(false);
    await undoOnce(context.workspace);
    expect(context.workspace.getBlockById('stop')?.getNextBlock()).toBeNull();
  });

  it('is not offered when the result type has no start value (a list)', () => {
    const context = canvas([
      {
        ...define({ type: 'control.break', id: 'stop' }),
        fields: { NAME: { sym: 's_fn', name: 'scores' }, RETURNS: '{"std::vector":"int"}' },
      },
    ]);
    expect(context.workspace.getBlockById('fn')?.getFieldValue('RETURNS')).toBe(
      '{"std::vector":"int"}',
    );
    expect(fixesFor(diagnostic('B2C-E0410', 'fn'), context)).toEqual([]);
  });

  it('gives an empty return the start value of the result type', async () => {
    const context = canvas([define({ type: 'func.return', id: 'ret' })]);
    const [fix] = fixesFor(diagnostic('B2C-E0403', 'ret'), context);
    expect(fix?.title).toBe('Give back a value');
    expect(fix?.apply()).toBe(true);
    const value = context.workspace.getBlockById('ret')?.getInputTargetBlock('VALUE');
    expect(value?.isShadow()).toBe(true);
    expect(fix?.apply()).toBe(false);
    await undoOnce(context.workspace);
    expect(context.workspace.getBlockById('ret')?.getInputTargetBlock('VALUE')).toBeNull();
  });
});

describe('B2C-E0405: the program block', () => {
  it('adds when program starts while the canvas has none', async () => {
    const context = canvas([{ type: 'control.break', id: 'stop' }]);
    const [fix] = fixesFor(diagnostic('B2C-E0405'), context);
    expect(fix?.title).toBe('Add “when program starts”');
    expect(fix?.apply()).toBe(true);
    expect(context.workspace.getBlocksByType('program.main', false)).toHaveLength(1);
    expect(fixesFor(diagnostic('B2C-E0405'), context)).toEqual([]);
    await undoOnce(context.workspace);
    expect(context.workspace.getBlocksByType('program.main', false)).toHaveLength(0);
  });
});

describe('B2C-W0508: the missing cases', () => {
  /** An enum `s_e` with `count` values `s_v0`… (named `V0`…), as the editor's type services see it. */
  function bigEnum(count: number): void {
    const ids = Array.from({ length: count }, (_, index) => `s_v${String(index)}`);
    const symbols = [
      symbolFixture('s_e', 'Big', { kind: 'enum', values: ids, type: { ref: 's_e' } }),
      ...ids.map((id, index) =>
        symbolFixture(id, `V${String(index)}`, { kind: 'enumerator', owner: 's_e' }),
      ),
    ];
    const byId = new Map(symbols.map((symbol) => [symbol.id, symbol]));
    setEditorServices({
      ...DEFAULT_EDITOR_SERVICES,
      types: {
        outputTypeOf: (block) => (block.id === 'value' ? { kind: 'named', id: 's_e' } : null),
        symbol: (id) => byId.get(id) ?? null,
      },
    });
  }

  /** A switch on a value of `s_e` with `caseCount` cases, `filled[i]` naming case `i` (or empty). */
  function bigSwitch(
    caseCount: number,
    filled: readonly (string | null)[],
  ): Blockly.serialization.blocks.State {
    const value = (id: string, sym: string): Blockly.serialization.blocks.State => ({
      type: 'type.enum_value',
      id,
      fields: { ENUM: { ref: 's_e' }, VALUE: { ref: sym } },
    });
    const inputs: Record<string, { block: Blockly.serialization.blocks.State }> = {
      VALUE: { block: value('value', 's_v0') },
    };
    filled.forEach((sym, index) => {
      if (sym !== null) {
        inputs[`CASE${String(index)}`] = { block: value(`case${String(index)}`, sym) };
      }
    });
    return {
      type: 'control.switch',
      id: 'switch',
      extraState: { caseCount, hasDefault: false },
      inputs,
    };
  }

  /** The enumerators the switch's cases name, in order (`null` for an empty case). */
  function caseValues(context: FixContext): (string | null)[] {
    const block = context.workspace.getBlockById('switch');
    return (block?.inputList ?? [])
      .filter((input) => /^CASE\d+$/u.test(input.name))
      .map((input) => {
        const value = input.connection?.targetBlock()?.getField('VALUE')?.getValue() as
          { ref?: string } | string | undefined;
        return (typeof value === 'object' ? value.ref : value) ?? null;
      });
  }

  it('fills the empty cases first and never goes past 64, leaving no loose block', async () => {
    bigEnum(64);
    const filled = Array.from({ length: 32 }, (_, index) => `s_v${String(index)}`);
    const context = canvas([bigSwitch(34, [...filled, null, null])]);
    const [fix] = fixesFor(diagnostic('B2C-W0508', 'switch'), context);
    expect(fix?.title).toBe('Add cases for V32, V33, V34 and 29 more');
    expect(fix?.apply()).toBe(true);
    const values = caseValues(context);
    expect(values).toHaveLength(64);
    expect(new Set(values).size).toBe(64);
    expect(values).not.toContain(null);
    expect(context.workspace.getTopBlocks(false).map((block) => block.id)).toEqual(['switch']);
    expect(fixesFor(diagnostic('B2C-W0508', 'switch'), context)).toEqual([]);
    await undoOnce(context.workspace);
    expect(caseValues(context)).toEqual([...filled, null, null]);
    expect(context.workspace.getTopBlocks(false).map((block) => block.id)).toEqual(['switch']);
  });

  it('is not offered when the switch has 64 cases and none is empty', () => {
    bigEnum(64);
    // Case 63 names V0 a second time: V63 is missing, with nowhere to put it.
    const filled = Array.from({ length: 64 }, (_, index) => `s_v${String(index % 63)}`);
    const context = canvas([bigSwitch(64, filled)]);
    expect(fixesFor(diagnostic('B2C-W0508', 'switch'), context)).toEqual([]);
  });

  it('adds a case for each value of the enum the switch does not name', async () => {
    const action = symbolFixture('s_action', 'Action', {
      kind: 'enum',
      values: ['s_add', 's_quit', 's_list'],
      type: { ref: 's_action' },
    });
    const values = ['add', 'quit', 'list'].map((name) =>
      symbolFixture(`s_${name}`, `${name.charAt(0).toUpperCase()}${name.slice(1)}`, {
        kind: 'enumerator',
        owner: 's_action',
      }),
    );
    const byId = new Map([action, ...values].map((symbol) => [symbol.id, symbol]));
    setEditorServices({
      ...DEFAULT_EDITOR_SERVICES,
      types: {
        outputTypeOf: (block) => (block.id === 'value' ? { kind: 'named', id: 's_action' } : null),
        symbol: (id) => byId.get(id) ?? null,
      },
    });
    const context = canvas([
      {
        type: 'control.switch',
        id: 'switch',
        extraState: { caseCount: 1, hasDefault: false },
        inputs: {
          VALUE: {
            block: {
              type: 'type.enum_value',
              id: 'value',
              fields: { ENUM: { ref: 's_action' }, VALUE: { ref: 's_add' } },
            },
          },
          CASE0: {
            block: {
              type: 'type.enum_value',
              id: 'case0',
              fields: { ENUM: { ref: 's_action' }, VALUE: { ref: 's_add' } },
            },
          },
        },
      },
    ]);
    const [fix] = fixesFor(diagnostic('B2C-W0508', 'switch'), context);
    expect(fix?.title).toBe('Add cases for Quit, List');
    expect(fix?.apply()).toBe(true);
    const block = context.workspace.getBlockById('switch');
    const named = ['CASE1', 'CASE2'].map(
      (input) =>
        block?.getInputTargetBlock(input)?.getField('VALUE')?.getValue() as
          { ref?: string } | string | undefined,
    );
    expect(named.map((value) => (typeof value === 'object' ? value.ref : value))).toEqual([
      's_quit',
      's_list',
    ]);
    expect(fixesFor(diagnostic('B2C-W0508', 'switch'), context)).toEqual([]);
    await undoOnce(context.workspace);
    if (block === null || !hasB2cMutator(block)) {
      throw new Error('no switch');
    }
    expect(block.b2cGetExtra()['caseCount']).toBe(1);
    expect(block.getInput('CASE1')).toBeNull();
  });
});
