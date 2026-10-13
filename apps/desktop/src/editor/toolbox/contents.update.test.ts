/**
 * The Variables category lists the update block only while the catalog still has it (decision D3
 * of the M2 polish plan): once `var.update` is merged into *change* (W4-5) and leaves the catalog,
 * the category shows *change* alone. The catalog here is the generated one without `var.update`;
 * ./contents.test.ts covers the category with today's catalog.
 */
import * as blocklyExt from '@blocks2cpp/blockly-ext';
import * as Blockly from 'blockly/core';
import { afterEach, beforeAll, describe, expect, it, vi } from 'vitest';

import { B2C_BLOCK_KIND, type B2cBlockInfo } from './presets';
import { variablesContents } from './contents';
import {
  disposeTestWorkspaces,
  fakeSymbols,
  headlessWorkspace,
  setUpToolboxBlocks,
  symbolFixture,
} from './testing';

vi.mock('@blocks2cpp/blockly-ext', async (importOriginal) => {
  const original = await importOriginal<typeof blocklyExt>();
  return {
    ...original,
    catalogBlock: (type: string) =>
      type === 'var.update' ? undefined : original.catalogBlock(type),
  };
});

beforeAll(() => {
  setUpToolboxBlocks();
});

afterEach(() => {
  disposeTestWorkspaces();
});

describe('the Variables category without var.update in the catalog', () => {
  it('lists change for a number variable, and no update block', () => {
    expect(blocklyExt.catalogBlock('var.update')).toBeUndefined();
    expect(blocklyExt.catalogBlock('var.change')).toBeDefined();
    const workspace = headlessWorkspace();
    Blockly.serialization.blocks.append(
      {
        type: 'program.main',
        id: 'main',
        inputs: { BODY: { block: { type: 'var.declare', id: 'd_score' } } },
      },
      workspace,
    );
    const score = symbolFixture('s_score', 'score', { declBlock: 'd_score' });
    const items = variablesContents({
      workspace,
      symbols: fakeSymbols({}, [score]),
      modules: () => [{ id: 'mod_main', name: 'main' }],
      activeModuleId: () => 'mod_main',
      programModuleId: () => 'mod_main',
    });
    const types = items
      .filter((item): item is B2cBlockInfo => item.kind === B2C_BLOCK_KIND)
      .map((item) => item.type);
    expect(types).toContain('var.change');
    expect(types).not.toContain('var.update');
    expect(types).toContain('var.get');
  });
});
