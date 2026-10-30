/**
 * Quick fixes (docs/plans/m3-contracts/w4-editor-tools.md §2.5): which diagnostics get one, what
 * each does, that it is one undo step, and that it is not offered once it no longer applies.
 */
import type { SymbolInfo } from '@blocks2cpp/b2c-core-wasm';
import {
  DEFAULT_EDITOR_SERVICES,
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

/** Undoes everything on the undo stack. */
function undoAll(workspace: Blockly.WorkspaceSvg): void {
  while (workspace.getUndoStack().length > 0) {
    workspace.undo(false);
  }
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
    await delivered();
    undoAll(context.workspace);
    expect(context.workspace.getBlockById('main')?.getInputTargetBlock('BODY')?.id).toBe('print');
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
    await delivered();
    undoAll(context.workspace);
    expect(context.workspace.getBlockById('stop')?.getNextBlock()).toBeNull();
  });

  it('gives an empty return the start value of the result type', () => {
    const context = canvas([define({ type: 'func.return', id: 'ret' })]);
    const [fix] = fixesFor(diagnostic('B2C-E0403', 'ret'), context);
    expect(fix?.title).toBe('Give back a value');
    expect(fix?.apply()).toBe(true);
    const value = context.workspace.getBlockById('ret')?.getInputTargetBlock('VALUE');
    expect(value?.isShadow()).toBe(true);
    expect(fix?.apply()).toBe(false);
  });
});

describe('B2C-E0405: the program block', () => {
  it('adds when program starts while the canvas has none', () => {
    const context = canvas([{ type: 'control.break', id: 'stop' }]);
    const [fix] = fixesFor(diagnostic('B2C-E0405'), context);
    expect(fix?.title).toBe('Add “when program starts”');
    expect(fix?.apply()).toBe(true);
    expect(context.workspace.getBlocksByType('program.main', false)).toHaveLength(1);
    expect(fixesFor(diagnostic('B2C-E0405'), context)).toEqual([]);
  });
});

describe('B2C-W0508: the missing cases', () => {
  it('adds a case for each value of the enum the switch does not name', () => {
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
  });
});
