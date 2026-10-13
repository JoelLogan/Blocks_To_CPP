import type { SymbolInfo } from '@blocks2cpp/b2c-core-wasm';
import * as Blockly from 'blockly/core';
import { afterEach, beforeAll, describe, expect, it } from 'vitest';

import {
  findMain,
  insertionPoint,
  isContainer,
  listingPoint,
  projectBlock,
  resolveBlockScope,
  resolveConnectionScope,
  resolveScopeAnchor,
  symbolsAtConnection,
  symbolsAtPoint,
  takenFunctionNames,
  takenLoopNames,
  takenNamesAtInsertion,
  takenVariableNames,
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

/** The names of the symbols visible at a point. */
function namesAt(point: ReturnType<typeof resolveScopeAnchor>, source: SymbolSource): string[] {
  return symbolsAtPoint(point, source).map((symbol) => symbol.name);
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

  it('is just after a selected statement, also when a shadow of it is selected', () => {
    const workspace = canvas();
    // A selected `create guess` lists guess.
    expect(listingPoint(workspace, get(workspace, 'decl'))).toEqual({
      kind: 'after',
      blockId: 'decl',
    });
    const shadow = get(workspace, 'print').getInputTargetBlock('ITEM0');
    expect(projectBlock(shadow)?.id).toBe('print');
    expect(listingPoint(workspace, shadow)).toEqual({ kind: 'after', blockId: 'print' });
  });

  it('is at a selected value block', () => {
    const workspace = canvas();
    const getter = Blockly.serialization.blocks.append(
      { type: 'var.get', id: 'getter', fields: { VAR: { ref: 's_guess' } } },
      workspace,
    );
    present(get(workspace, 'print').getInput('ITEM0')?.connection).connect(
      present(getter.outputConnection),
    );
    expect(listingPoint(workspace, getter)).toEqual({ kind: 'at', blockId: 'getter', input: null });
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

  it('lets the declared variable hide an outer one with its name', () => {
    const outer = symbolFixture('s_outer', 'guess', { declBlock: 'first' });
    const inner = symbolFixture('s_inner', 'guess', { declBlock: 'second' });
    const hiding = fakeSymbols({ second: [factorial, outer] }, [factorial, outer, inner]);
    expect(symbolsAtPoint({ kind: 'after', blockId: 'second' }, hiding)).toEqual([
      factorial,
      inner,
    ]);
  });

  it('has nothing where the analysis has no answer', () => {
    expect(symbolsAtPoint({ kind: 'at', blockId: 'unseen', input: null }, source)).toEqual([]);
    expect(symbolsAtPoint({ kind: 'after', blockId: 'unseen' }, source)).toEqual([]);
  });
});

describe('the scope anchor', () => {
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

  it('is the core’s own answer for a parked block it has analysed', () => {
    // The core answers a parked block with the end of main (06 §6.5): one question, no walk.
    const workspace = canvas();
    const loose = add(workspace, printState('loose'));
    const nested = add(workspace, printState('nestedLoose'), 'loose');
    const source = counting(
      fakeSymbols({ ...SCOPES, loose: [factorial, guess], nestedLoose: [factorial, guess] }, ALL),
    );
    expect(resolveScopeAnchor(workspace, loose, source)).toEqual({
      kind: 'after',
      blockId: 'loose',
    });
    expect(resolveBlockScope(workspace, nested, null, source)).toEqual({
      kind: 'at',
      blockId: 'nestedLoose',
      input: null,
    });
    expect(source.asked).toEqual(['loose', 'nestedLoose']);
    expect(namesAt(resolveScopeAnchor(workspace, loose, source), source)).toEqual([
      'factorial',
      'guess',
    ]);
  });

  it('is the end of main for a loose block not analysed yet, and for a block nested in one', () => {
    const workspace = canvas();
    const source = fakeSymbols(SCOPES, ALL);
    const loose = add(workspace, printState('loose'));
    expect(symbolsAtPoint(listingPoint(workspace, loose), source)).toEqual([]);
    expect(resolveScopeAnchor(workspace, loose, source)).toEqual({
      kind: 'after',
      blockId: 'print',
    });
    expect(namesAt(resolveScopeAnchor(workspace, loose, source), source)).toEqual([
      'factorial',
      'guess',
    ]);
    // Inside a loose C-block.
    const repeat = add(workspace, {
      type: 'control.repeat',
      id: 'looseLoop',
      inputs: { BODY: { block: printState('nested') } },
    });
    const nested = get(workspace, 'nested');
    expect(nested.getRootBlock()).toBe(repeat);
    expect(namesAt(resolveScopeAnchor(workspace, nested, source), source)).toEqual([
      'factorial',
      'guess',
    ]);
    // The block's own menus see the same.
    expect(resolveBlockScope(workspace, nested, null, source)).toEqual({
      kind: 'after',
      blockId: 'print',
    });
  });

  it('is the module’s functions for a loose block when there is no main', () => {
    const workspace = canvas();
    get(workspace, 'main').dispose(false);
    const source = fakeSymbols(SCOPES, ALL);
    const loose = add(workspace, printState('loose'));
    const functions = { kind: 'at', blockId: 'fn', input: null };
    expect(resolveScopeAnchor(workspace, loose, source)).toEqual(functions);
    expect(resolveScopeAnchor(workspace, null, source)).toEqual(functions);
    expect(resolveBlockScope(workspace, loose, null, source)).toEqual(functions);
    get(workspace, 'fn').dispose(false);
    expect(resolveScopeAnchor(workspace, null, source)).toBeNull();
    expect(namesAt(resolveScopeAnchor(workspace, loose, source), source)).toEqual([]);
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
    expect(listingPoint(workspace, null)).toEqual(endOfMain);
    expect(resolveScopeAnchor(workspace, null, source)).toEqual(endOfMain);
    // A loose block the analysis has not seen yet, its menus and its connection.
    const loose = add(workspace, printState('loose'));
    expect(resolveScopeAnchor(workspace, loose, source)).toEqual(endOfMain);
    expect(resolveConnectionScope(workspace, loose, source)).toEqual(endOfMain);
    expect(symbolsAtConnection(workspace, loose, source).map((symbol) => symbol.name)).toEqual([
      'factorial',
      'guess',
    ]);
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
    expect(symbolsAtConnection(workspace, loose, source).map((symbol) => symbol.name)).toEqual([
      'factorial',
    ]);
    // Functions of other canvases are not this module's.
    const elsewhere = symbolFixture('s_other', 'other', {
      kind: 'function',
      params: [],
      returns: 'void',
      declBlock: 'elsewhere',
    });
    expect(
      symbolsAtConnection(workspace, loose, fakeSymbols({}, [...ALL, elsewhere])).map(
        (symbol) => symbol.name,
      ),
    ).toEqual(['factorial']);
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

  it('is the disabled statement a block is in, for a block not analysed yet', () => {
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
    expect(symbolsAtPoint(listingPoint(workspace, second), source)).toEqual([]);
    expect(resolveScopeAnchor(workspace, second, source)).toEqual({
      kind: 'at',
      blockId: 'loop',
      input: 'BODY',
    });
    expect(namesAt(resolveScopeAnchor(workspace, second, source), source)).toEqual([
      'factorial',
      'guess',
    ]);
  });

  it('is just after the statement before, with its declaration, for a block not analysed yet', () => {
    const workspace = canvas();
    const source = fakeSymbols(SCOPES, ALL);
    const fresh = add(workspace, printState('fresh'), 'decl');
    expect(fresh.getNextBlock()?.id).toBe('print');
    expect(resolveScopeAnchor(workspace, fresh, source)).toEqual({
      kind: 'after',
      blockId: 'decl',
    });
    expect(namesAt(resolveScopeAnchor(workspace, fresh, source), source)).toEqual([
      'factorial',
      'guess',
    ]);
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
  });

  it('never leaves a function for main, even when the function sees nothing', () => {
    const workspace = canvas();
    const inFunction = Blockly.serialization.blocks.append(printState('inFn'), workspace);
    present(get(workspace, 'fn').getInput('BODY')?.connection).connect(
      present(inFunction.previousConnection),
    );
    const analysed = fakeSymbols(SCOPES, ALL);
    expect(namesAt(resolveScopeAnchor(workspace, inFunction, analysed), analysed)).toEqual([
      'factorial',
      'n',
    ]);
    // A function body the analysis sees nothing in: nothing, never main's variables.
    const unknown = fakeSymbols({ ...SCOPES, 'fn/BODY': [] }, ALL);
    expect(namesAt(resolveScopeAnchor(workspace, inFunction, unknown), unknown)).toEqual([]);
    expect(namesAt(resolveScopeAnchor(workspace, get(workspace, 'fn'), unknown), unknown)).toEqual(
      [],
    );
    // A function the analysis has not seen at all: nothing either.
    const withoutFunction = Object.fromEntries(
      Object.entries(SCOPES).filter(([key]) => !key.startsWith('fn')),
    );
    const newFunction = fakeSymbols(withoutFunction, ALL);
    expect(namesAt(resolveScopeAnchor(workspace, inFunction, newFunction), newFunction)).toEqual(
      [],
    );
  });

  it('keeps a point that has an answer', () => {
    const workspace = canvas();
    const source = fakeSymbols(SCOPES, ALL);
    expect(resolveScopeAnchor(workspace, get(workspace, 'decl'), source)).toEqual({
      kind: 'after',
      blockId: 'decl',
    });
    expect(resolveScopeAnchor(workspace, null, source)).toEqual({
      kind: 'after',
      blockId: 'print',
    });
    expect(resolveScopeAnchor(workspace, get(workspace, 'fn'), source)).toEqual({
      kind: 'at',
      blockId: 'fn',
      input: 'BODY',
    });
    expect(resolveBlockScope(workspace, get(workspace, 'print'), null, source)).toEqual({
      kind: 'at',
      blockId: 'print',
      input: null,
    });
  });

  it('asks each earlier position at most once, and nothing more without symbols', () => {
    const workspace = canvas();
    // 50 statements after print, none of them analysed.
    let last = 'print';
    for (let index = 0; index < 50; index += 1) {
      last = add(workspace, printState(`p${String(index)}`), last).id;
    }
    const point = { kind: 'after', blockId: last };
    // The analysis knows no symbol of this canvas: one question.
    const empty = counting(fakeSymbols({}, []));
    expect(resolveScopeAnchor(workspace, get(workspace, last), empty)).toEqual(point);
    expect(empty.asked).toEqual([last]);

    // A symbol of this canvas (factorial's parameter), but nothing visible anywhere in main: each
    // position up to the start of main's body once, and the selection's own point stands.
    const nothing = counting(fakeSymbols({}, [n]));
    expect(resolveScopeAnchor(workspace, get(workspace, last), nothing)).toEqual(point);
    expect(nothing.asked).toHaveLength(53);
    expect(new Set(nothing.asked).size).toBe(53);
    expect(nothing.asked.at(-1)).toBe('main/BODY');
  });
});

describe('the scope at a block’s connection', () => {
  /** The answers for guessDocument(), as in the scope anchor's tests. */
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

  it('is just after the statement before, never the block’s own answer', () => {
    // A block moved below `create guess`: the analysis still knows it at its old place (the top
    // of factorial's body), but its new place sees guess (W4-2 re-targets a dropped preset so).
    const workspace = canvas();
    const moved = add(workspace, printState('moved'), 'decl');
    const source = counting(fakeSymbols({ ...SCOPES, moved: [factorial, n] }, ALL));
    const point = resolveConnectionScope(workspace, moved, source);
    expect(point).toEqual({ kind: 'after', blockId: 'decl' });
    expect(source.asked).toEqual(['decl']);
    expect(namesAt(point, source)).toEqual(['factorial', 'guess']);
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
    const point = resolveConnectionScope(workspace, getter, source);
    expect(point).toEqual({ kind: 'at', blockId: 'decl', input: 'VALUE' });
    expect(namesAt(point, source)).toEqual(['factorial']);
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
    const point = resolveConnectionScope(workspace, loose, source);
    expect(point).toEqual({ kind: 'after', blockId: 'print' });
    expect(namesAt(point, source)).toEqual(['factorial', 'guess']);
    expect(resolveConnectionScope(workspace, get(workspace, 'main'), source)).toBeNull();
    expect(resolveConnectionScope(workspace, get(workspace, 'fn'), source)).toBeNull();
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
