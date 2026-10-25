/**
 * The collection blocks of W2 in the editor (docs/plans/m3-contracts/w2.md §1.2): their reference
 * menus list only the variables whose type fits the block's pattern (slots.md §4.3), and a block
 * that changes its collection lists only variables that may be changed.
 */
import { beforeEach, describe, expect, it } from 'vitest';

import { B2cSymbolRefField, type SymbolInfo } from '../src';
import { fakeSymbols, headlessWorkspace, setUpBlocks, symbol } from './helpers';

beforeEach(() => {
  setUpBlocks();
});

/** Variables of every collection kind, and some that are not collections. */
const SCOPE: readonly SymbolInfo[] = [
  symbol('s_scores', 'scores', 'variable', { type: { 'std::vector': 'int' } }),
  symbol('s_letters', 'letters', 'variable', { type: { 'std::deque': 'char' } }),
  symbol('s_words', 'words', 'variable', { type: { 'std::set': 'std::string' } }),
  symbol('s_ages', 'ages', 'variable', { type: { 'std::map': ['std::string', 'int'] } }),
  symbol('s_line', 'line', 'variable', { type: { 'std::queue': 'int' } }),
  symbol('s_pile', 'pile', 'variable', { type: { 'std::stack': 'int' } }),
  symbol('s_best', 'best', 'variable', { type: { 'std::priority_queue': 'double' } }),
  symbol('s_n', 'n'),
  symbol('s_name', 'name', 'variable', { type: 'std::string' }),
  symbol('s_fixed', 'fixed', 'variable', { type: { 'std::vector': 'int' }, isConst: true }),
  symbol('s_row', 'row', 'loopVariable', { type: { 'std::vector': 'int' } }),
  symbol('s_in', 'input', 'parameter', { type: { 'std::vector': 'int' }, mode: 'read_only' }),
  symbol('s_out', 'output', 'parameter', { type: { 'std::vector': 'int' }, mode: 'editable' }),
];

/** The names a block's reference field lists, with `SCOPE` in scope. */
function listed(type: string, field = 'LIST'): string[] {
  const block = headlessWorkspace().newBlock(type);
  fakeSymbols({ [block.id]: SCOPE }, {});
  const ref = block.getField(field);
  if (!(ref instanceof B2cSymbolRefField)) {
    throw new Error(`${type} has no reference field ${field}`);
  }
  return ref.menuOptions().map(([label]) => (typeof label === 'string' ? label : '?'));
}

describe('the collection blocks’ menus', () => {
  it('list only the collections a block that changes them may change', () => {
    // add: lists, deques, sets and the adaptors; not a constant, a read-only parameter or a
    // read-only loop item.
    expect(listed('coll.add')).toEqual([
      'scores',
      'letters',
      'words',
      'line',
      'pile',
      'best',
      'output',
    ]);
    expect(listed('coll.set_item')).toEqual(['scores', 'letters', 'ages', 'output']);
    expect(listed('coll.clear')).toEqual([
      'scores',
      'letters',
      'words',
      'ages',
      'line',
      'pile',
      'best',
      'output',
    ]);
    expect(listed('coll.remove_at')).toEqual(['scores', 'letters', 'output']);
    expect(listed('coll.remove_value')).toEqual(['words', 'ages']);
    expect(listed('coll.remove_next')).toEqual(['line', 'pile', 'best']);
    expect(listed('coll.sort')).toEqual(['scores', 'letters', 'output']);
  });

  it('list every collection of the class for a block that only reads it', () => {
    expect(listed('coll.item')).toEqual([
      'scores',
      'letters',
      'ages',
      'fixed',
      'row',
      'input',
      'output',
    ]);
    expect(listed('coll.length')).toEqual([
      'scores',
      'letters',
      'words',
      'ages',
      'line',
      'pile',
      'best',
      'fixed',
      'row',
      'input',
      'output',
    ]);
    expect(listed('coll.next')).toEqual(['line', 'pile', 'best']);
    expect(listed('coll.contains')).toEqual([
      'scores',
      'letters',
      'words',
      'ages',
      'fixed',
      'row',
      'input',
      'output',
    ]);
    expect(listed('coll.for_each_pair', 'MAP')).toEqual(['ages']);
  });
});
