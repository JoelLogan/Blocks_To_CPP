/** The block search (docs/plans/m3-contracts/w4-editor-tools.md §2.3). */
import { catalogBlock } from '@blocks2cpp/blockly-ext';
import * as Blockly from 'blockly/core';
import { afterEach, beforeAll, describe, expect, it } from 'vitest';

import type { EditorMode } from '../gating';
import type { ContentsContext } from '../toolbox/contents';
import {
  disposeTestWorkspaces,
  fakeSymbols,
  headlessWorkspace,
  setUpToolboxBlocks,
  symbolFixture,
} from '../toolbox/testing';
import { SYNONYMS, entryScore, searchBlocks, searchEntries, templateTitle, wordScore } from '.';

beforeAll(() => {
  setUpToolboxBlocks();
});

afterEach(() => {
  disposeTestWorkspaces();
});

const BASIC: EditorMode = {
  labels: 'friendly',
  showAdvanced: false,
  standard: 'c++20',
  toolchain: null,
};

function context(mode: EditorMode = BASIC): ContentsContext {
  const workspace = headlessWorkspace();
  Blockly.serialization.blocks.append({ type: 'program.main', id: 'main' }, workspace);
  return {
    workspace,
    symbols: fakeSymbols({}, [
      symbolFixture('s_score', 'score', { declBlock: 'main' }),
      symbolFixture('s_area', 'area', {
        kind: 'function',
        params: [],
        returns: 'int',
        declBlock: 'fn',
      }),
    ]),
    modules: () => [{ id: 'mod_main', name: 'main' }],
    activeModuleId: () => 'mod_main',
    programModuleId: () => 'mod_main',
    mode: () => mode,
  };
}

const first = (query: string, mode?: EditorMode) =>
  searchBlocks(searchEntries(context(mode)), query)[0]?.item.type;

describe('the scorer', () => {
  it('ranks a whole word over a word start over a part over letters in order', () => {
    const whole = wordScore('print', 'print … …');
    const start = wordScore('pri', 'print');
    const part = wordScore('rin', 'print');
    const letters = wordScore('prt', 'print');
    expect(whole).toBeGreaterThan(start);
    expect(start).toBeGreaterThan(part);
    expect(part).toBeGreaterThan(letters);
    expect(letters).toBeGreaterThan(0);
    expect(wordScore('xyz', 'print')).toBe(0);
  });

  it('needs every word to match', () => {
    const texts = [{ text: 'print hello', weight: 1 }];
    expect(entryScore(['print'], texts)).toBeGreaterThan(0);
    expect(entryScore(['print', 'zebra'], texts)).toBe(0);
  });
});

describe('the entries', () => {
  it('list the blocks the mode offers, the variables and the functions', () => {
    const entries = searchEntries(context());
    const types = entries.map((entry) => entry.item.type);
    expect(types).toContain('io.print');
    expect(types).not.toContain('program.check');
    expect(entries.find((entry) => entry.title === 'score')?.item.type).toBe('var.get');
    expect(entries.find((entry) => entry.title === 'area (call)')).toBeDefined();
    const advanced = searchEntries(context({ ...BASIC, showAdvanced: true }));
    expect(advanced.map((entry) => entry.item.type)).toContain('program.check');
  });

  it('carry the label mode, so a block inserted from them is built in it', () => {
    const entries = searchEntries(context({ ...BASIC, labels: 'cpp' }));
    expect(entries.every((entry) => (entry.item as { labels?: string }).labels === 'cpp')).toBe(
      true,
    );
  });

  it('read templates with … for their parts', () => {
    expect(templateTitle('add %VALUE to %LIST')).toBe('add … to …');
  });

  it('name only catalog blocks in the synonyms', () => {
    for (const id of Object.keys(SYNONYMS)) {
      expect(catalogBlock(id), id).toBeDefined();
    }
  });
});

describe('searching', () => {
  it('finds blocks by label, C++ and synonym (03 §3.14)', () => {
    expect(first('print')).toBe('io.print');
    expect(first('cout')).toBe('io.print');
    expect(first('endl')).toBe('io.print');
    expect(first('push_back')).toBe('coll.add');
    expect(first('if')).toBe('control.if');
    expect(first('score')).toBe('var.get');
    expect(first('area')).toBe('func.call');
    const vector = searchBlocks(searchEntries(context()), 'vector').map((entry) => entry.item.type);
    expect(vector.slice(0, 5).every((type) => type.startsWith('coll.'))).toBe(true);
    expect(first('for')).toBe('control.for_range');
  });

  it('finds nothing for an empty query or a word nothing has', () => {
    expect(searchBlocks(searchEntries(context()), '   ')).toEqual([]);
    expect(searchBlocks(searchEntries(context()), 'qqqqzz')).toEqual([]);
  });

  it('keeps to the limit', () => {
    expect(searchBlocks(searchEntries(context()), 'e', 3)).toHaveLength(3);
  });
});
