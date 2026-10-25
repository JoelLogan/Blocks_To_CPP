/**
 * Typed slots on the canvas (ADR-0018, docs/plans/m3-contracts/slots.md §4.3, §9): the parts read
 * from real blocks, `candidateFit`, the per-gesture cache, and the connection checker's type rule
 * on M2's typed inputs (`create`, `set`, call arguments, `return`).
 */
import * as Blockly from 'blockly/core';
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';

import { registerB2cBlocks } from '../blocks/register';
import { B2cSymbolRefField } from '../fields/symbol-ref';
import { installIdGenerator } from '../ids';
import { hasB2cMutator } from '../blocks/mutators';
import { registerB2cMutators } from '../mutators/register';
import type { SymbolInfo, TypeOracle } from '../services';
import { NO_FACTS, parseType, prim, type TypeJson } from '../types/static-type';
import { exprShadowState } from '../shadows/state';
import { B2C_CHECKER_NAME } from './checker';
import { setCheckerTypeOracle } from './oracle';
import {
  blockTyping,
  candidateFit,
  candidateRefused,
  currentSlotCache,
  setCurrentSlotCache,
  SlotCache,
  valueType,
} from './slots';

let workspace: Blockly.WorkspaceSvg;
let symbols: Map<string, SymbolInfo>;
let hidden: Set<string>;
let oracle: TypeOracle;

function sym(id: string, type: TypeJson, extra: Partial<SymbolInfo> = {}): SymbolInfo {
  return {
    id,
    name: id,
    kind: 'variable',
    type,
    module: 'mod_main',
    declBlock: 'blk_decl',
    ...extra,
  };
}

beforeAll(() => {
  installIdGenerator();
  registerB2cMutators();
  registerB2cBlocks();
});

beforeEach(() => {
  const host = document.createElement('div');
  document.body.append(host);
  workspace = Blockly.inject(host, {
    renderer: 'zelos',
    sounds: false,
    plugins: { connectionChecker: B2C_CHECKER_NAME },
  });
  symbols = new Map(
    [
      sym('s_score', 'int'),
      sym('s_name', 'std::string'),
      sym('s_price', 'double'),
      sym('s_p', 'int', { kind: 'parameter', mode: 'copy' }),
      sym('s_f', 'double', { kind: 'function', params: ['s_p'], returns: 'double' }),
    ].map((symbol) => [symbol.id, symbol]),
  );
  hidden = new Set();
  oracle = {
    outputTypeOf: () => null,
    symbol: (id) => symbols.get(id) ?? null,
    visibleAt: (id) => (symbols.has(id) ? !hidden.has(id) : null),
    facts: () => NO_FACTS,
  };
  setCheckerTypeOracle(oracle);
});

afterEach(() => {
  setCheckerTypeOracle(null);
  setCurrentSlotCache(null);
  workspace.dispose();
  document.body.replaceChildren();
});

function block(type: string): Blockly.BlockSvg {
  const created = workspace.newBlock(type);
  created.initSvg();
  created.render();
  return created;
}

function number(text: string): Blockly.BlockSvg {
  const created = block('math.number');
  created.setFieldValue(text, 'VALUE');
  return created;
}

function refer(target: Blockly.Block, field: string, id: string): void {
  const ref = target.getField(field);
  if (!(ref instanceof B2cSymbolRefField)) {
    throw new Error(`${field} is not a reference field`);
  }
  ref.setRef({ ref: id });
}

/** Whether `child` may be dragged into `parent`'s input `name` (the checker's drag rule). */
function canDrop(child: Blockly.Block, parent: Blockly.Block, name: string): boolean {
  const to = parent.getInput(name)?.connection;
  const from = child.outputConnection;
  if (to === null || to === undefined || from === null) {
    throw new Error(`no connection for ${name}`);
  }
  return workspace.connectionChecker.canConnect(from, to, true, 1e9);
}

/** Puts `child` into `parent`'s input `name`. */
function plug(parent: Blockly.Block, name: string, child: Blockly.Block): void {
  const to = parent.getInput(name)?.connection;
  if (to === null || to === undefined || child.outputConnection === null) {
    throw new Error(`no connection for ${name}`);
  }
  to.connect(child.outputConnection);
}

/** Puts `statement` at the top of `parent`'s statement list `name`. */
function nest(parent: Blockly.Block, name: string, statement: Blockly.Block): void {
  const to = parent.getInput(name)?.connection;
  if (to === null || to === undefined || statement.previousConnection === null) {
    throw new Error(`no statement list ${name}`);
  }
  to.connect(statement.previousConnection);
}

describe('the type rule on M2’s typed inputs', () => {
  it('refuses text in create [int]’s value, and follows the type field', () => {
    const create = block('var.declare');
    create.setFieldValue('int', 'TYPE');
    expect(canDrop(block('text.literal'), create, 'VALUE')).toBe(false);
    // int into double and double into int (a narrowing the analyser warns about) are allowed.
    expect(canDrop(number('1.5'), create, 'VALUE')).toBe(true);
    create.setFieldValue('double', 'TYPE');
    expect(canDrop(number('1'), create, 'VALUE')).toBe(true);
    expect(canDrop(block('text.literal'), create, 'VALUE')).toBe(false);
    create.setFieldValue('std::string', 'TYPE');
    expect(canDrop(block('text.literal'), create, 'VALUE')).toBe(true);
    expect(canDrop(number('1'), create, 'VALUE')).toBe(false);
    // `auto` takes its type from the value; a type the editor does not know checks nothing.
    create.setFieldValue('auto', 'TYPE');
    expect(canDrop(block('text.literal'), create, 'VALUE')).toBe(true);
    create.setFieldValue('long', 'TYPE');
    expect(canDrop(block('text.literal'), create, 'VALUE')).toBe(true);
  });

  it('refuses text in set [score] of an int variable, and allows a char in set of text', () => {
    const set = block('var.set');
    refer(set, 'VAR', 's_score');
    expect(canDrop(block('text.literal'), set, 'VALUE')).toBe(false);
    expect(canDrop(number('2.5'), set, 'VALUE')).toBe(true);
    refer(set, 'VAR', 's_name');
    expect(canDrop(block('text.char'), set, 'VALUE')).toBe(true);
    expect(canDrop(number('1'), set, 'VALUE')).toBe(false);
  });

  it('accepts anything in set of a variable out of scope, unknown, or not chosen', () => {
    const set = block('var.set');
    refer(set, 'VAR', 's_score');
    hidden.add('s_score');
    expect(canDrop(block('text.literal'), set, 'VALUE')).toBe(true);
    refer(set, 'VAR', 's_gone');
    expect(canDrop(block('text.literal'), set, 'VALUE')).toBe(true);
    const empty = block('var.set');
    expect(canDrop(block('text.literal'), empty, 'VALUE')).toBe(true);
  });

  it('refuses text in an int parameter’s argument', () => {
    const call = block('func.call');
    refer(call, 'FUNC', 's_f');
    if (!hasB2cMutator(call)) {
      throw new Error('func.call has no mutator');
    }
    call.b2cSetExtra({ argCount: 1 });
    expect(canDrop(block('text.literal'), call, 'ARG0')).toBe(false);
    expect(canDrop(number('3'), call, 'ARG0')).toBe(true);
  });

  it('refuses text in return of a function that returns double, and in main', () => {
    const define = block('func.define');
    define.setFieldValue('double', 'RETURNS');
    const inFunction = block('func.return');
    nest(define, 'BODY', inFunction);
    expect(canDrop(block('text.literal'), inFunction, 'VALUE')).toBe(false);
    expect(canDrop(number('1'), inFunction, 'VALUE')).toBe(true);
    const main = block('program.main');
    const inMain = block('func.return');
    nest(main, 'BODY', inMain);
    expect(canDrop(block('text.literal'), inMain, 'VALUE')).toBe(false);
    // Loose on the canvas, the result is not known.
    expect(canDrop(block('text.literal'), block('func.return'), 'VALUE')).toBe(true);
  });

  it('connects unknown and erroneous values, and asks the oracle for references', () => {
    const create = block('var.declare');
    create.setFieldValue('int', 'TYPE');
    const getter = block('var.get');
    expect(canDrop(getter, create, 'VALUE')).toBe(true);
    const types = new Map<Blockly.Block, TypeJson>([[getter, 'std::string']]);
    setCheckerTypeOracle({
      ...oracle,
      outputTypeOf: (asked) => parseType(types.get(asked) ?? null),
    });
    expect(canDrop(getter, create, 'VALUE')).toBe(false);
    types.set(getter, 'error');
    expect(canDrop(getter, create, 'VALUE')).toBe(true);
    types.set(getter, { 'std::vector': 'int' });
    expect(canDrop(getter, create, 'VALUE')).toBe(false);
  });

  it('keeps M2’s class rule for inputs without a pattern', () => {
    const repeat = block('control.repeat');
    expect(canDrop(block('text.literal'), repeat, 'TIMES')).toBe(false);
    expect(canDrop(number('2.5'), repeat, 'TIMES')).toBe(true);
    const ask = block('io.ask');
    expect(canDrop(number('1'), ask, 'PROMPT')).toBe(false);
  });

  it('never throws into Blockly when the oracle fails', () => {
    const set = block('var.set');
    refer(set, 'VAR', 's_score');
    setCheckerTypeOracle({
      outputTypeOf: () => {
        throw new Error('stale');
      },
      symbol: () => {
        throw new Error('stale');
      },
      visibleAt: () => {
        throw new Error('stale');
      },
      facts: () => {
        throw new Error('stale');
      },
    });
    expect(canDrop(block('text.literal'), set, 'VALUE')).toBe(true);
  });
});

describe('typings and candidates', () => {
  it('type a block from its parts on the canvas', () => {
    const create = block('var.declare');
    create.setFieldValue('double', 'TYPE');
    plug(create, 'VALUE', number('1'));
    const typing = blockTyping(create, oracle);
    expect(typing?.inputs.get('VALUE')).toEqual({ expects: prim('double'), fit: 'widening' });
    expect(typing?.declares.get('NAME')).toEqual(prim('double'));
    expect(valueType(number('1.5'), oracle)).toEqual(prim('double'));
    expect(valueType(block('text.join'), oracle)).toEqual(prim('std::string'));
    expect(blockTyping(block('control.forever'), oracle)?.inputs.size).toBe(0);
  });

  it('type a candidate in place, whatever the input holds now', () => {
    const create = block('var.declare');
    create.setFieldValue('int', 'TYPE');
    plug(create, 'VALUE', block('text.literal'));
    expect(candidateFit(create, 'VALUE', number('1'), oracle)).toBe('same');
    expect(candidateFit(create, 'VALUE', block('text.literal'), oracle)).toBe('invalid');
    // An `auto` create binds its type from the candidate itself.
    create.setFieldValue('auto', 'TYPE');
    expect(candidateFit(create, 'VALUE', block('text.literal'), oracle)).toBe('same');
  });

  it('count a disabled candidate as empty, as the analyser does', () => {
    const create = block('var.declare');
    create.setFieldValue('int', 'TYPE');
    const literal = block('text.literal');
    expect(candidateFit(create, 'VALUE', literal, oracle)).toBe('invalid');
    literal.setDisabledReason(true, 'test');
    expect(candidateFit(create, 'VALUE', literal, oracle)).toBe('unknown');
    expect(canDrop(literal, create, 'VALUE')).toBe(true);
  });

  it('read an expression shadow: a literal’s type, a visible reference’s type', () => {
    const create = block('var.declare');
    create.setFieldValue('int', 'TYPE');
    const fitOf = (tokens: Parameters<typeof exprShadowState>[0]): unknown => {
      create
        .getInput('VALUE')
        ?.connection?.setShadowState(exprShadowState(tokens, false, 'any', false));
      return blockTyping(create, oracle)?.inputs.get('VALUE')?.fit;
    };
    expect(fitOf([{ str: 'x' }])).toBe('invalid');
    expect(fitOf([{ num: '2' }])).toBe('same');
    expect(fitOf([{ ref: 's_name' }])).toBe('invalid');
    // A reference the analysis does not see there is not known.
    hidden.add('s_name');
    expect(fitOf([{ ref: 's_name' }])).toBe('unknown');
  });

  it('count the moved block as absent for a keyboard listing', () => {
    const set = block('var.set');
    refer(set, 'VAR', 's_score');
    const moved = block('text.literal');
    plug(set, 'VALUE', moved);
    const cache = new SlotCache(oracle, { absent: moved });
    expect(candidateFit(set, 'VALUE', moved, oracle, cache)).toBe('invalid');
    expect(candidateFit(set, 'VALUE', number('7'), oracle, cache)).toBe('same');
  });
});

describe('the per-gesture cache', () => {
  it('types each block once per gesture and starts again with a new preview', () => {
    const blocks = Array.from({ length: 50 }, () => {
      const set = block('var.set');
      refer(set, 'VAR', 's_score');
      plug(set, 'VALUE', number('1'));
      return set;
    });
    const ask = vi.fn((id: string) => symbols.get(id) ?? null);
    let facts = { userType: () => null };
    const counted: TypeOracle = { ...oracle, symbol: ask, facts: () => facts };
    const cache = new SlotCache(counted);
    setCurrentSlotCache(cache);
    expect(currentSlotCache()).toBe(cache);
    for (let round = 0; round < 3; round += 1) {
      for (const set of blocks) {
        expect(blockTyping(set, counted, currentSlotCache())?.inputs.get('VALUE')?.fit).toBe(
          'same',
        );
      }
    }
    expect(ask).toHaveBeenCalledTimes(blocks.length);
    // A new preview brings new facts: the cache starts again.
    facts = { userType: () => null };
    blockTyping(blocks[0] ?? block('var.set'), counted, cache);
    expect(ask).toHaveBeenCalledTimes(blocks.length + 1);
    setCurrentSlotCache(null);
    expect(currentSlotCache()).toBeNull();
  });
});

describe('what a candidate’s fit asks (slots.md §4.3, Speed)', () => {
  it('types no parent and asks about no symbol for an input without a pattern', () => {
    // print [score] [ ]: the reference in the first item is not read for the second.
    const print = block('io.print') as Blockly.BlockSvg & {
      b2cSetExtra(extra: unknown): void;
    };
    print.b2cSetExtra({ itemCount: 2 });
    print
      .getInput('ITEM0')
      ?.connection?.setShadowState(exprShadowState([{ ref: 's_score' }], false, 'any', false));
    const visibleAt = vi.fn(
      (id: string, at: Blockly.Block, input: string | null) =>
        oracle.visibleAt?.(id, at, input) ?? null,
    );
    const counted: TypeOracle = { ...oracle, visibleAt };
    const repeat = block('control.repeat');
    expect(candidateFit(print, 'ITEM1', number('1'), counted)).toBe('unknown');
    expect(candidateFit(repeat, 'TIMES', block('text.literal'), counted)).toBe('outsideClass');
    expect(candidateFit(repeat, 'TIMES', number('2'), counted)).toBe('unknown');
    expect(visibleAt).not.toHaveBeenCalled();
    // The same answers as typing the parent with the candidate in place.
    plug(repeat, 'TIMES', block('text.literal'));
    expect(blockTyping(repeat, oracle)?.inputs.get('TIMES')?.fit).toBe('outsideClass');
    plug(print, 'ITEM1', number('1'));
    expect(blockTyping(print, oracle)?.inputs.get('ITEM1')?.fit).toBe('unknown');
    // A disabled candidate counts as empty.
    const disabled = block('text.literal');
    disabled.setDisabledReason(true, 'test');
    expect(candidateFit(repeat, 'TIMES', disabled, counted)).toBe('unknown');
    // An input part the block does not have is typed as before.
    expect(candidateFit(print, 'ITEM9', number('1'), counted)).toBe('unknown');
  });

  it('asks each visibility once per gesture, and keeps each fit', () => {
    const sets = Array.from({ length: 5 }, () => {
      const set = block('var.set');
      refer(set, 'VAR', 's_name');
      return set;
    });
    const visibleAt = vi.fn(
      (id: string, at: Blockly.Block, input: string | null) =>
        oracle.visibleAt?.(id, at, input) ?? null,
    );
    const counted: TypeOracle = { ...oracle, visibleAt };
    const cache = new SlotCache(counted);
    const candidate = number('1');
    for (let round = 0; round < 3; round += 1) {
      for (const set of sets) {
        expect(candidateFit(set, 'VALUE', candidate, counted, cache)).toBe('invalid');
      }
    }
    expect(visibleAt).toHaveBeenCalledTimes(sets.length);
  });

  it('asks about scopes only to confirm a refusal, with the same answer as the fit', () => {
    const name = block('var.set');
    refer(name, 'VAR', 's_name');
    const score = block('var.set');
    refer(score, 'VAR', 's_score');
    const visibleAt = vi.fn(
      (id: string, at: Blockly.Block, input: string | null) =>
        oracle.visibleAt?.(id, at, input) ?? null,
    );
    const counted: TypeOracle = { ...oracle, visibleAt };
    const candidate = number('1');
    // A number fits the int variable: nothing is asked.
    expect(candidateRefused(score, 'VALUE', candidate, counted)).toBe(false);
    expect(visibleAt).not.toHaveBeenCalled();
    // Into the text variable it would be refused: the refusal is confirmed.
    expect(candidateRefused(name, 'VALUE', candidate, counted)).toBe(true);
    expect(visibleAt).toHaveBeenCalledTimes(1);
    // Out of scope, the variable's type is not known, and the number connects.
    hidden.add('s_name');
    expect(candidateRefused(name, 'VALUE', candidate, counted)).toBe(false);
    expect(candidateFit(name, 'VALUE', candidate, counted)).not.toBe('invalid');
    hidden.clear();
    for (const [parent, child] of [
      [name, candidate],
      [score, candidate],
      [name, block('text.literal')],
      [score, block('text.literal')],
    ] as const) {
      expect(candidateRefused(parent, 'VALUE', child, oracle)).toBe(
        candidateFit(parent, 'VALUE', child, oracle) === 'invalid' ||
          candidateFit(parent, 'VALUE', child, oracle) === 'outsideClass',
      );
    }
  });
});

describe('a function’s parameter rows with the registered type field', () => {
  it('keep a library or user type by its key and give the term back', () => {
    const define = block('func.define') as Blockly.BlockSvg & {
      b2cSetExtra(extra: unknown): void;
      b2cGetExtra(): unknown;
    };
    const params = [
      { sym: 's_list', name: 'list', type: { 'std::vector': 'int' }, mode: 'read_only' },
      { sym: 's_pt', name: 'pt', type: { ref: 'sym_Pt' }, mode: 'copy' },
    ];
    define.b2cSetExtra({ params });
    expect(define.getFieldValue('B2C_PARAM0_TYPE')).toBe('{"std::vector":"int"}');
    expect(define.b2cGetExtra()).toEqual({ params });
    // Choosing a listed type from the menu still works.
    define.setFieldValue('double', 'B2C_PARAM0_TYPE');
    expect(define.b2cGetExtra()).toEqual({ params: [{ ...params[0], type: 'double' }, params[1]] });
  });
});

describe('a place-typed block (W2’s list of, slots.md §9)', () => {
  const LIST_OF_INT = '{"std::vector":"int"}';

  beforeEach(() => {
    symbols.set('s_scores', sym('s_scores', { 'std::vector': 'int' }));
    symbols.set('s_grid', sym('s_grid', { 'std::vector': { 'std::vector': 'int' } }));
  });

  it('connects where no place type exists, and is refused in an int place', () => {
    // print’s items have no pattern: the analyser reports a list without a place (E0315).
    const print = block('io.print');
    expect(canDrop(block('coll.list_of'), print, 'ITEM0')).toBe(true);
    const create = block('var.declare');
    create.setFieldValue('int', 'TYPE');
    const list = block('coll.list_of');
    expect(candidateFit(create, 'VALUE', list, oracle)).toBe('outsideClass');
    expect(canDrop(list, create, 'VALUE')).toBe(false);
    // An `auto` create gives no place either.
    create.setFieldValue('auto', 'TYPE');
    expect(canDrop(list, create, 'VALUE')).toBe(true);
    // A list type gives it its place.
    create.setFieldValue(LIST_OF_INT, 'TYPE');
    expect(candidateFit(create, 'VALUE', list, oracle)).toBe('same');
    expect(canDrop(list, create, 'VALUE')).toBe(true);
    expect(valueType(list, oracle)).toBe('fromPlace');
  });

  it('types its items from its place, up the chain', () => {
    const create = block('var.declare');
    create.setFieldValue(LIST_OF_INT, 'TYPE');
    const list = block('coll.list_of');
    plug(create, 'VALUE', list);
    expect(canDrop(block('text.literal'), list, 'ITEM0')).toBe(false);
    expect(canDrop(number('1'), list, 'ITEM0')).toBe(true);
    // A list of lists: the inner list takes its place from the outer one.
    create.setFieldValue('{"std::vector":{"std::vector":"int"}}', 'TYPE');
    const inner = block('coll.list_of');
    expect(canDrop(inner, list, 'ITEM1')).toBe(true);
    plug(list, 'ITEM1', inner);
    expect(canDrop(block('text.literal'), inner, 'ITEM0')).toBe(false);
    expect(canDrop(number('2'), inner, 'ITEM0')).toBe(true);
  });

  it('fits add’s value as the chosen collection’s item type says', () => {
    const add = block('coll.add');
    refer(add, 'LIST', 's_grid');
    expect(canDrop(block('coll.list_of'), add, 'VALUE')).toBe(true);
    refer(add, 'LIST', 's_scores');
    expect(canDrop(block('coll.list_of'), add, 'VALUE')).toBe(false);
    expect(canDrop(number('3'), add, 'VALUE')).toBe(true);
    expect(canDrop(block('text.literal'), add, 'VALUE')).toBe(false);
  });
});
