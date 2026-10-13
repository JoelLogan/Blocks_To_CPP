import type { SymbolInfo } from '@blocks2cpp/b2c-core-wasm';
import * as Blockly from 'blockly/core';
import { afterEach, beforeAll, describe, expect, it } from 'vitest';

import {
  findMain,
  insertionPoint,
  isContainer,
  projectBlock,
  resolveBlockScope,
  resolveConnectionScope,
  symbolsAtBlock,
  symbolsAtConnection,
  takenNamesAtDrop,
  takenNamesAtInsertion,
  type SymbolSource,
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

function present<T>(value: T | null | undefined): T {
  if (value === null || value === undefined) {
    throw new Error('missing');
  }
  return value;
}

/** Adds a block (Blockly state) to the canvas: after block `after` when given, else loose. */
function add(
  workspace: Blockly.Workspace,
  state: Blockly.serialization.blocks.State,
  after?: string,
): Blockly.Block {
  const block = Blockly.serialization.blocks.append(state, workspace);
  if (after !== undefined) {
    present(get(workspace, after).nextConnection).connect(present(block.previousConnection));
  }
  return block;
}

/** The state of a `print` block. */
function printState(id: string): Blockly.serialization.blocks.State {
  return { type: 'io.print', id, extraState: { itemCount: 1 } };
}

/** A source that also records the scope queries it answers (`blockId` or `blockId/input`). */
function counting(source: SymbolSource): SymbolSource & { readonly asked: string[] } {
  const asked: string[] = [];
  return {
    asked,
    symbolsAt(blockId, input) {
      asked.push(input === null ? blockId : `${blockId}/${input}`);
      return source.symbolsAt(blockId, input);
    },
    allSymbols: () => source.allSymbols(),
  };
}

/** The names of the symbols a block's own menus list. */
function namesAtBlock(workspace: Blockly.Workspace, block: Blockly.Block, source: SymbolSource) {
  return symbolsAtBlock(workspace, block, null, source).map((symbol) => symbol.name);
}

/** The names of the symbols a block sees through the connection it is attached by. */
function namesAtConnection(
  workspace: Blockly.Workspace,
  block: Blockly.Block,
  source: SymbolSource,
) {
  return symbolsAtConnection(workspace, block, source).map((symbol) => symbol.name);
}

const guess = symbolFixture('s_guess', 'guess', { declBlock: 'decl' });
const factorial = symbolFixture('s_fact', 'factorial', {
  kind: 'function',
  params: ['s_n'],
  returns: 'int',
  declBlock: 'fn',
});
const n = symbolFixture('s_n', 'n', { kind: 'parameter', mode: 'copy', declBlock: 'fn' });

/**
 * The scope query's answers for guessDocument(): factorial everywhere in the module, guess below
 * its declaration, n in factorial's body. Blocks it never analysed have no answer.
 */
const SCOPES: Readonly<Record<string, readonly SymbolInfo[]>> = {
  main: [factorial],
  'main/BODY': [factorial],
  decl: [factorial],
  'decl/VALUE': [factorial],
  print: [factorial, guess],
  fn: [factorial],
  'fn/BODY': [factorial, n],
};
const ALL = [factorial, guess, n];

describe('the scope of a block the analysis has seen', () => {
  it('is the core’s own answer, a parked block’s included: one question, no walk', () => {
    // The core answers a parked block with the end of main (06 §6.5).
    const workspace = canvas();
    add(workspace, printState('loose'));
    const nested = add(workspace, printState('nestedLoose'), 'loose');
    const source = counting(
      fakeSymbols({ ...SCOPES, loose: [factorial, guess], nestedLoose: [factorial, guess] }, ALL),
    );
    expect(resolveBlockScope(workspace, nested, null, source)).toEqual({
      kind: 'at',
      blockId: 'nestedLoose',
      input: null,
    });
    expect(source.asked).toEqual(['nestedLoose']);
    expect(namesAtBlock(workspace, nested, source)).toEqual(['factorial', 'guess']);
  });

  it('is the core’s own answer for a block in a disabled statement, even an empty one', () => {
    const workspace = canvas();
    add(
      workspace,
      {
        type: 'control.forever',
        id: 'loop',
        disabledReasons: [Blockly.constants.MANUALLY_DISABLED],
        inputs: { BODY: { block: printState('inside') } },
      },
      'print',
    );
    // The core answers a block in a disabled statement like that statement: as it is.
    const source = counting(fakeSymbols({ ...SCOPES, inside: [factorial, guess] }, ALL));
    expect(resolveBlockScope(workspace, get(workspace, 'inside'), null, source)).toEqual({
      kind: 'at',
      blockId: 'inside',
      input: null,
    });
    expect(source.asked).toEqual(['inside']);
    // An empty answer is an answer: nothing is visible there, and the walk does not look around.
    const empty = counting(fakeSymbols({ ...SCOPES, inside: [] }, ALL));
    expect(resolveBlockScope(workspace, get(workspace, 'inside'), null, empty)).toEqual({
      kind: 'at',
      blockId: 'inside',
      input: null,
    });
    expect(empty.asked).toEqual(['inside']);
  });

  it('keeps a point that has an answer, at the block or at the start of its body', () => {
    const workspace = canvas();
    const source = fakeSymbols(SCOPES, ALL);
    expect(resolveBlockScope(workspace, get(workspace, 'print'), null, source)).toEqual({
      kind: 'at',
      blockId: 'print',
      input: null,
    });
    expect(resolveBlockScope(workspace, get(workspace, 'fn'), 'BODY', source)).toEqual({
      kind: 'at',
      blockId: 'fn',
      input: 'BODY',
    });
    expect(
      symbolsAtBlock(workspace, get(workspace, 'fn'), 'BODY', source).map((s) => s.name),
    ).toEqual(['factorial', 'n']);
  });
});

describe('the scope of a block the analysis has not seen yet', () => {
  it('is the end of main for a loose block, and for a block nested in one', () => {
    const workspace = canvas();
    const source = fakeSymbols(SCOPES, ALL);
    const loose = add(workspace, printState('loose'));
    const endOfMain = { kind: 'after', blockId: 'print' };
    expect(resolveBlockScope(workspace, loose, null, source)).toEqual(endOfMain);
    expect(namesAtBlock(workspace, loose, source)).toEqual(['factorial', 'guess']);
    // Inside a loose C-block.
    const repeat = add(workspace, {
      type: 'control.repeat',
      id: 'looseLoop',
      inputs: { BODY: { block: printState('nested') } },
    });
    const nested = get(workspace, 'nested');
    expect(nested.getRootBlock()).toBe(repeat);
    expect(resolveBlockScope(workspace, nested, null, source)).toEqual(endOfMain);
    expect(namesAtBlock(workspace, nested, source)).toEqual(['factorial', 'guess']);
  });

  it('is the end of main’s body the analyser reaches: disabled statements at the end skipped', () => {
    const workspace = canvas();
    get(workspace, 'print').setDisabledReason(true, Blockly.constants.MANUALLY_DISABLED);
    const loose = add(workspace, printState('loose'));
    // Just after `create guess`: guess is visible there, as after the statement that declares it.
    const source = fakeSymbols({ decl: [factorial] }, ALL);
    expect(resolveConnectionScope(workspace, loose, source)).toEqual({
      kind: 'after',
      blockId: 'decl',
    });
    expect(namesAtConnection(workspace, loose, source)).toEqual(['factorial', 'guess']);
    // An empty main: the start of its body.
    const empty = headlessWorkspace();
    add(empty, { type: 'program.main', id: 'm' });
    const alone = add(empty, printState('alone'));
    expect(resolveConnectionScope(empty, alone, fakeSymbols({}, [n]))).toEqual({
      kind: 'at',
      blockId: 'm',
      input: 'BODY',
    });
  });

  it('is the module’s functions for a loose block when there is no main', () => {
    const workspace = canvas();
    get(workspace, 'main').dispose(false);
    const source = fakeSymbols(SCOPES, ALL);
    const loose = add(workspace, printState('loose'));
    const functions = { kind: 'at', blockId: 'fn', input: null };
    expect(resolveBlockScope(workspace, loose, null, source)).toEqual(functions);
    expect(resolveConnectionScope(workspace, loose, source)).toEqual(functions);
    get(workspace, 'fn').dispose(false);
    expect(resolveConnectionScope(workspace, loose, source)).toBeNull();
    expect(namesAtBlock(workspace, loose, source)).toEqual([]);
  });

  it('is the end of the main the analyser runs, not of a disabled main above it', () => {
    // A disabled main above the enabled one: a valid program (a disabled main does not count), whose
    // parked scope is the end of the enabled main's body, as the core answers (06 §6.5).
    const workspace = canvas();
    add(workspace, {
      type: 'program.main',
      id: 'aMain',
      x: 40,
      y: -400,
      disabledReasons: [Blockly.constants.MANUALLY_DISABLED],
      inputs: { BODY: { block: printState('offPrint') } },
    });
    expect(workspace.getTopBlocks(true)[0]?.id).toBe('aMain');
    const source = fakeSymbols(SCOPES, ALL);
    const endOfMain = { kind: 'after', blockId: 'print' };
    expect(findMain(workspace)?.id).toBe('main');
    // A loose block the analysis has not seen yet, its menus and its connection.
    const loose = add(workspace, printState('loose'));
    expect(resolveBlockScope(workspace, loose, null, source)).toEqual(endOfMain);
    expect(resolveConnectionScope(workspace, loose, source)).toEqual(endOfMain);
    expect(namesAtConnection(workspace, loose, source)).toEqual(['factorial', 'guess']);
    // Make a variable inserts into the main that runs.
    expect(insertionPoint(workspace, null)).toEqual({
      kind: 'top',
      container: get(workspace, 'main'),
    });
    // Of two enabled mains (an error, B2C-E0406), the analyser runs the one with the smaller ID;
    // without an enabled one, the topmost stands.
    add(workspace, { type: 'program.main', id: 'zzMain', x: 40, y: -800 });
    expect(findMain(workspace)?.id).toBe('main');
    get(workspace, 'main').setDisabledReason(true, Blockly.constants.MANUALLY_DISABLED);
    get(workspace, 'zzMain').setDisabledReason(true, Blockly.constants.MANUALLY_DISABLED);
    expect(findMain(workspace)?.id).toBe('zzMain');
  });

  it('is the module’s functions for a new loose block without main, before any analysis of them', () => {
    const workspace = canvas();
    get(workspace, 'main').dispose(false);
    // The analysis has not seen the function definition yet, but knows its symbol.
    const source = fakeSymbols({}, ALL);
    const loose = add(workspace, printState('loose'));
    expect(namesAtConnection(workspace, loose, source)).toEqual(['factorial']);
    // Functions of other canvases are not this module's.
    const elsewhere = symbolFixture('s_other', 'other', {
      kind: 'function',
      params: [],
      returns: 'void',
      declBlock: 'elsewhere',
    });
    expect(namesAtConnection(workspace, loose, fakeSymbols({}, [...ALL, elsewhere]))).toEqual([
      'factorial',
    ]);
  });

  it('is the disabled statement a block is in', () => {
    const workspace = canvas();
    add(
      workspace,
      {
        type: 'control.forever',
        id: 'loop',
        disabledReasons: [Blockly.constants.MANUALLY_DISABLED],
        inputs: {
          BODY: { block: { ...printState('first'), next: { block: printState('second') } } },
        },
      },
      'print',
    );
    // The core answers a disabled statement (and its lists) from its own position; the blocks in
    // it are new.
    const source = fakeSymbols(
      { ...SCOPES, loop: [factorial, guess], 'loop/BODY': [factorial, guess] },
      ALL,
    );
    const second = get(workspace, 'second');
    expect(resolveBlockScope(workspace, second, null, source)).toEqual({
      kind: 'at',
      blockId: 'loop',
      input: 'BODY',
    });
    expect(namesAtBlock(workspace, second, source)).toEqual(['factorial', 'guess']);
  });

  it('is just after the statement before, with its declaration', () => {
    const workspace = canvas();
    const source = fakeSymbols(SCOPES, ALL);
    const fresh = add(workspace, printState('fresh'), 'decl');
    expect(fresh.getNextBlock()?.id).toBe('print');
    expect(resolveBlockScope(workspace, fresh, null, source)).toEqual({
      kind: 'after',
      blockId: 'decl',
    });
    expect(namesAtBlock(workspace, fresh, source)).toEqual(['factorial', 'guess']);
    // A value block not analysed yet, in the starting value of `create guess`: what is visible
    // inside that value (not guess).
    const getter = Blockly.serialization.blocks.append({ type: 'var.get', id: 'v' }, workspace);
    present(get(workspace, 'decl').getInput('VALUE')?.connection).connect(
      present(getter.outputConnection),
    );
    expect(resolveBlockScope(workspace, getter, null, source)).toEqual({
      kind: 'at',
      blockId: 'decl',
      input: 'VALUE',
    });
    expect(namesAtBlock(workspace, getter, source)).toEqual(['factorial']);
  });

  it('sees a loop’s counter only inside the loop, and lets a declaration hide an outer name', () => {
    const workspace = canvas();
    const loop = add(
      workspace,
      {
        type: 'control.for_range',
        id: 'loop',
        fields: { VAR: { sym: 's_i', name: 'i' } },
      },
      'print',
    );
    const after = add(workspace, printState('after'), 'loop');
    const counter = symbolFixture('s_i', 'i', { kind: 'loopVariable', declBlock: 'loop' });
    const loops = fakeSymbols({ ...SCOPES, loop: [factorial, guess] }, [...ALL, counter]);
    expect(loop.getNextBlock()).toBe(after);
    expect(namesAtConnection(workspace, after, loops)).toEqual(['factorial', 'guess']);

    // `create guess` again in a loop's body after the outer guess: the inner one hides it.
    const inner = symbolFixture('s_inner', 'guess', { declBlock: 'second' });
    add(
      workspace,
      {
        type: 'control.forever',
        id: 'forever',
        inputs: {
          BODY: {
            block: {
              type: 'var.declare',
              id: 'second',
              fields: { TYPE: 'int', NAME: { sym: 's_inner', name: 'guess' }, CONST: false },
            },
          },
        },
      },
      'after',
    );
    const below = add(workspace, printState('below'), 'second');
    const hiding = fakeSymbols({ second: [factorial, guess] }, [...ALL, inner]);
    expect(symbolsAtConnection(workspace, below, hiding)).toEqual([factorial, inner]);
  });

  it('never leaves a function for main, even when the function sees nothing', () => {
    const workspace = canvas();
    const inFunction = Blockly.serialization.blocks.append(printState('inFn'), workspace);
    present(get(workspace, 'fn').getInput('BODY')?.connection).connect(
      present(inFunction.previousConnection),
    );
    const analysed = fakeSymbols(SCOPES, ALL);
    expect(namesAtBlock(workspace, inFunction, analysed)).toEqual(['factorial', 'n']);
    // A function body the analysis sees nothing in: nothing, never main's variables.
    const unknown = fakeSymbols({ ...SCOPES, 'fn/BODY': [] }, ALL);
    expect(namesAtBlock(workspace, inFunction, unknown)).toEqual([]);
    // A function the analysis has not seen at all: nothing either.
    const withoutFunction = Object.fromEntries(
      Object.entries(SCOPES).filter(([key]) => !key.startsWith('fn')),
    );
    const newFunction = fakeSymbols(withoutFunction, ALL);
    expect(namesAtBlock(workspace, inFunction, newFunction)).toEqual([]);
    expect(namesAtBlock(workspace, get(workspace, 'fn'), newFunction)).toEqual([]);
  });

  it('asks each earlier position at most once, and nothing more without symbols', () => {
    const workspace = canvas();
    // 50 statements after print, none of them analysed.
    let last = 'print';
    for (let index = 0; index < 50; index += 1) {
      last = add(workspace, printState(`p${String(index)}`), last).id;
    }
    const point = { kind: 'at', blockId: last, input: null };
    // The analysis knows no symbol of this canvas: one question.
    const empty = counting(fakeSymbols({}, []));
    expect(resolveBlockScope(workspace, get(workspace, last), null, empty)).toEqual(point);
    expect(empty.asked).toEqual([last]);

    // A symbol of this canvas (factorial's parameter), but nothing visible anywhere in main: each
    // position up to the start of main's body once, and the block's own point stands.
    const nothing = counting(fakeSymbols({}, [n]));
    expect(resolveBlockScope(workspace, get(workspace, last), null, nothing)).toEqual(point);
    expect(nothing.asked).toHaveLength(53);
    expect(new Set(nothing.asked).size).toBe(53);
    expect(nothing.asked.at(-1)).toBe('main/BODY');
  });
});

describe('the scope at a block’s connection', () => {
  it('is just after the statement before, never the block’s own answer', () => {
    // A block moved below `create guess`: the analysis still knows it at its old place (the top
    // of factorial's body), but its new place sees guess (a dropped preset is re-targeted so).
    const workspace = canvas();
    const moved = add(workspace, printState('moved'), 'decl');
    const source = counting(fakeSymbols({ ...SCOPES, moved: [factorial, n] }, ALL));
    const point = resolveConnectionScope(workspace, moved, source);
    expect(point).toEqual({ kind: 'after', blockId: 'decl' });
    expect(source.asked).toEqual(['decl']);
    expect(namesAtConnection(workspace, moved, source)).toEqual(['factorial', 'guess']);
  });

  it('is the start of the list or the input that holds the block', () => {
    const workspace = canvas();
    const source = fakeSymbols(SCOPES, ALL);
    const first = Blockly.serialization.blocks.append(printState('first'), workspace);
    present(get(workspace, 'fn').getInput('BODY')?.connection).connect(
      present(first.previousConnection),
    );
    expect(resolveConnectionScope(workspace, first, source)).toEqual({
      kind: 'at',
      blockId: 'fn',
      input: 'BODY',
    });
    const getter = Blockly.serialization.blocks.append({ type: 'var.get', id: 'v' }, workspace);
    present(get(workspace, 'decl').getInput('VALUE')?.connection).connect(
      present(getter.outputConnection),
    );
    expect(resolveConnectionScope(workspace, getter, source)).toEqual({
      kind: 'at',
      blockId: 'decl',
      input: 'VALUE',
    });
    expect(namesAtConnection(workspace, getter, source)).toEqual(['factorial']);
  });

  it('walks back past statements the analysis has not seen either', () => {
    const workspace = canvas();
    const source = fakeSymbols(SCOPES, ALL);
    add(workspace, printState('earlier'), 'print');
    const dropped = add(workspace, printState('dropped'), 'earlier');
    expect(resolveConnectionScope(workspace, dropped, source)).toEqual({
      kind: 'after',
      blockId: 'print',
    });
  });

  it('is the parked scope on the canvas, and nothing for main or a function', () => {
    const workspace = canvas();
    const source = fakeSymbols(SCOPES, ALL);
    const loose = add(workspace, printState('loose'));
    expect(resolveConnectionScope(workspace, loose, source)).toEqual({
      kind: 'after',
      blockId: 'print',
    });
    expect(namesAtConnection(workspace, loose, source)).toEqual(['factorial', 'guess']);
    expect(resolveConnectionScope(workspace, get(workspace, 'main'), source)).toBeNull();
    expect(resolveConnectionScope(workspace, get(workspace, 'fn'), source)).toBeNull();
    expect(symbolsAtConnection(workspace, get(workspace, 'fn'), source)).toEqual([]);
  });
});

describe('the insertion point of Make a variable', () => {
  it('is before the statement, at the top of a container, or at the top of main', () => {
    const workspace = canvas();
    const print = get(workspace, 'print');
    expect(insertionPoint(workspace, print)).toEqual({ kind: 'before', block: print });
    const shadow = print.getInputTargetBlock('ITEM0');
    expect(projectBlock(shadow)?.id).toBe('print');
    expect(insertionPoint(workspace, shadow)).toEqual({ kind: 'before', block: print });
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

describe('names in use where a block was dropped', () => {
  it('for a variable: what is visible there, and what the rest of its list declares', () => {
    const workspace = canvas();
    const source = fakeSymbols(SCOPES, ALL);
    // A new `create value` just before print, in main's body after `create guess`.
    const dropped = add(
      workspace,
      {
        type: 'var.declare',
        id: 'dropped',
        fields: { TYPE: 'int', NAME: { sym: 's_value', name: 'value' }, CONST: false },
      },
      'decl',
    );
    expect([...takenNamesAtDrop(workspace, dropped, 'variable', source)].sort()).toEqual([
      'factorial',
      'guess',
    ]);
    // At the top of factorial's body: what the body sees (its own name is not taken).
    const inFunction = Blockly.serialization.blocks.append(
      {
        type: 'var.declare',
        id: 'inFn',
        fields: { TYPE: 'int', NAME: { sym: 's_v2', name: 'value' }, CONST: false },
      },
      workspace,
    );
    present(get(workspace, 'fn').getInput('BODY')?.connection).connect(
      present(inFunction.previousConnection),
    );
    expect([...takenNamesAtDrop(workspace, inFunction, 'variable', source)].sort()).toEqual([
      'factorial',
      'n',
    ]);
    // A later `create` of the same list counts too.
    const later = add(
      workspace,
      {
        type: 'var.declare',
        id: 'later',
        fields: { TYPE: 'int', NAME: { sym: 's_later', name: 'later' }, CONST: false },
      },
      'inFn',
    );
    expect(later.getPreviousBlock()?.id).toBe('inFn');
    expect([...takenNamesAtDrop(workspace, inFunction, 'variable', source)].sort()).toEqual([
      'factorial',
      'later',
      'n',
    ]);
  });

  it('for a loop counter: what is visible there', () => {
    const workspace = canvas();
    const loop = add(
      workspace,
      { type: 'control.for_range', id: 'loop', fields: { VAR: { sym: 's_i', name: 'i' } } },
      'print',
    );
    const source = fakeSymbols(SCOPES, ALL);
    expect([...takenNamesAtDrop(workspace, loop, 'loop', source)].sort()).toEqual([
      'factorial',
      'guess',
    ]);
  });

  it('for a function: every other function, analysed or on the canvas', () => {
    const workspace = canvas();
    const helper = Blockly.serialization.blocks.append(
      {
        type: 'func.define',
        fields: { NAME: { sym: 's_new', name: 'helper' } },
        extraState: { params: [] },
      },
      workspace,
    );
    const dropped = Blockly.serialization.blocks.append(
      {
        type: 'func.define',
        id: 'dropped',
        fields: { NAME: { sym: 's_mine', name: 'myFunction' } },
        extraState: { params: [] },
      },
      workspace,
    );
    const source = fakeSymbols({}, [
      factorial,
      guess,
      symbolFixture('s_x', 'other', { kind: 'function', params: [], returns: 'void' }),
      symbolFixture('s_self', 'myFunction', {
        kind: 'function',
        params: [],
        returns: 'void',
        declBlock: 'dropped',
      }),
    ]);
    expect(helper.type).toBe('func.define');
    expect([...takenNamesAtDrop(workspace, dropped, 'function', source)].sort()).toEqual([
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
