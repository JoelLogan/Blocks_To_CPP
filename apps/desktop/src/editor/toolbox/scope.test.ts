import * as Blockly from 'blockly/core';
import { afterEach, beforeAll, describe, expect, it } from 'vitest';

import {
  findMain,
  insertionPoint,
  isContainer,
  listingPoint,
  projectBlock,
  symbolsAtPoint,
  takenFunctionNames,
  takenLoopNames,
  takenNamesAtInsertion,
  takenVariableNames,
} from './scope';
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
  disposeTestWorkspaces();
});

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

const guess = symbolFixture('s_guess', 'guess', { declBlock: 'decl' });
const factorial = symbolFixture('s_fact', 'factorial', {
  kind: 'function',
  params: ['s_n'],
  returns: 'int',
  declBlock: 'fn',
});
const n = symbolFixture('s_n', 'n', { kind: 'parameter', mode: 'copy', declBlock: 'fn' });

describe('the listing point', () => {
  it('is the end of main’s body with nothing selected, after its last statement', () => {
    const workspace = canvas();
    expect(listingPoint(workspace, null)).toEqual({ kind: 'after', blockId: 'print' });
    expect(listingPoint(workspace, get(workspace, 'main'))).toEqual({
      kind: 'after',
      blockId: 'print',
    });
  });

  it('is the start of an empty body, and nothing without main', () => {
    const workspace = canvas();
    expect(listingPoint(workspace, get(workspace, 'fn'))).toEqual({
      kind: 'at',
      blockId: 'fn',
      input: 'BODY',
    });
    expect(listingPoint(headlessWorkspace(), null)).toBeNull();
  });

  it('skips disabled statements at the end of the body', () => {
    const workspace = canvas();
    get(workspace, 'print').setDisabledReason(true, Blockly.constants.MANUALLY_DISABLED);
    expect(listingPoint(workspace, null)).toEqual({ kind: 'after', blockId: 'decl' });
  });

  it('is the selected block, or the block that holds a selected shadow', () => {
    const workspace = canvas();
    expect(listingPoint(workspace, get(workspace, 'decl'))).toEqual({
      kind: 'at',
      blockId: 'decl',
      input: null,
    });
    const shadow = get(workspace, 'print').getInputTargetBlock('ITEM0');
    expect(projectBlock(shadow)?.id).toBe('print');
    expect(listingPoint(workspace, shadow)).toEqual({ kind: 'at', blockId: 'print', input: null });
  });
});

describe('the symbols at a listing point', () => {
  const source = fakeSymbols(
    { decl: [factorial], print: [factorial, guess], 'fn/BODY': [factorial, n] },
    [factorial, guess, n],
  );

  it('asks the scope query at a block or at the start of a body', () => {
    expect(symbolsAtPoint({ kind: 'at', blockId: 'decl', input: null }, source)).toEqual([
      factorial,
    ]);
    expect(symbolsAtPoint({ kind: 'at', blockId: 'fn', input: 'BODY' }, source)).toEqual([
      factorial,
      n,
    ]);
    expect(symbolsAtPoint(null, source)).toEqual([]);
  });

  it('adds the variable a statement declares when listing after it', () => {
    expect(symbolsAtPoint({ kind: 'after', blockId: 'decl' }, source)).toEqual([factorial, guess]);
    // A loop's counter is visible only inside the loop.
    const counter = symbolFixture('s_i', 'i', { kind: 'loopVariable', declBlock: 'loop' });
    const loops = fakeSymbols({ loop: [factorial] }, [counter, factorial]);
    expect(symbolsAtPoint({ kind: 'after', blockId: 'loop' }, loops)).toEqual([factorial]);
  });
});

describe('the insertion point of Make a variable', () => {
  it('is before the statement, at the top of a container, or at the top of main', () => {
    const workspace = canvas();
    const print = get(workspace, 'print');
    expect(insertionPoint(workspace, print)).toEqual({ kind: 'before', block: print });
    expect(insertionPoint(workspace, print.getInputTargetBlock('ITEM0'))).toEqual({
      kind: 'before',
      block: print,
    });
    const fn = get(workspace, 'fn');
    expect(insertionPoint(workspace, fn)).toEqual({ kind: 'top', container: fn });
    const main = get(workspace, 'main');
    expect(insertionPoint(workspace, null)).toEqual({ kind: 'top', container: main });
    expect(insertionPoint(headlessWorkspace(), null)).toEqual({ kind: 'newMain' });
  });

  it('is the top of main for a block outside the program', () => {
    const workspace = canvas();
    const loose = Blockly.serialization.blocks.append(
      { type: 'io.print', extraState: { itemCount: 1 } },
      workspace,
    );
    expect(insertionPoint(workspace, loose)).toEqual({
      kind: 'top',
      container: get(workspace, 'main'),
    });
  });

  it('knows the names a new variable would clash with there', () => {
    const workspace = canvas();
    const source = fakeSymbols({ print: [factorial, guess], 'main/BODY': [factorial] }, [
      factorial,
    ]);
    expect(
      [...takenNamesAtInsertion({ kind: 'before', block: get(workspace, 'print') }, source)].sort(),
    ).toEqual(['factorial', 'guess']);
    // At the top of main, guess is not visible yet but is declared later in the same list.
    expect(
      [...takenNamesAtInsertion({ kind: 'top', container: get(workspace, 'main') }, source)].sort(),
    ).toEqual(['factorial', 'guess']);
    expect([...takenNamesAtInsertion({ kind: 'newMain' }, source)]).toEqual(['factorial']);
  });
});

describe('names in use', () => {
  it('for a new variable: what is visible, and what the same list declares', () => {
    const workspace = canvas();
    const source = fakeSymbols({ decl: [factorial], 'fn/BODY': [factorial, n] }, [factorial]);
    expect(
      [
        ...takenVariableNames(workspace, { kind: 'at', blockId: 'decl', input: null }, source),
      ].sort(),
    ).toEqual(['factorial', 'guess']);
    expect(
      [
        ...takenVariableNames(workspace, { kind: 'at', blockId: 'fn', input: 'BODY' }, source),
      ].sort(),
    ).toEqual(['factorial', 'n']);
    expect([...takenVariableNames(workspace, null, source)]).toEqual([]);
    expect([
      ...takenVariableNames(workspace, { kind: 'at', blockId: 'gone', input: null }, source),
    ]).toEqual([]);
  });

  it('for a loop counter: what is visible', () => {
    const source = fakeSymbols({ print: [guess] });
    expect([...takenLoopNames({ kind: 'at', blockId: 'print', input: null }, source)]).toEqual([
      'guess',
    ]);
  });

  it('for a new function: every function, analysed or on the canvas', () => {
    const workspace = canvas();
    Blockly.serialization.blocks.append(
      {
        type: 'func.define',
        fields: { NAME: { sym: 's_new', name: 'helper' } },
        extraState: { params: [] },
      },
      workspace,
    );
    const source = fakeSymbols({}, [
      factorial,
      guess,
      symbolFixture('s_x', 'other', { kind: 'function', params: [], returns: 'void' }),
    ]);
    expect([...takenFunctionNames(workspace, source)].sort()).toEqual([
      'factorial',
      'helper',
      'other',
    ]);
  });
});

describe('containers', () => {
  it('are main and function definitions', () => {
    const workspace = canvas();
    expect(findMain(workspace)?.id).toBe('main');
    expect(isContainer(get(workspace, 'main'))).toBe(true);
    expect(isContainer(get(workspace, 'fn'))).toBe(true);
    expect(isContainer(get(workspace, 'decl'))).toBe(false);
  });
});
