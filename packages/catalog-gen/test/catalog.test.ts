// The real catalog: the committed outputs are current, every catalog block has exactly one
// generated definition, and the generated module holds exactly the generator's model.

import { readFile } from 'node:fs/promises';

import { describe, expect, it } from 'vitest';

import * as generated from '../../blockly-ext/src/generated/catalog.ts';
import { readCatalogJson } from '../src/catalog-json.ts';
import type { BlockDefJson, ToolboxCategoryJson } from '../src/catalog-types.ts';
import { checkOutputs, generateFrom } from '../src/files.ts';
import { CATALOG_JSON_PATH } from '../src/generate.ts';
import { normalize } from '../src/normalize.ts';
import { ROOT, repoPath } from './fixture.ts';

/** The blocks the editor's dynamic categories add (b2c_catalog::DynamicCategory::blocks). */
const DYNAMIC_BLOCKS = {
  variables: ['var.get', 'var.set', 'var.change', 'var.update'],
  functions: ['func.call', 'func.call_stmt'],
} as const;

async function catalogJson() {
  return readCatalogJson(await readFile(repoPath(CATALOG_JSON_PATH), 'utf8'));
}

describe('the generated outputs', () => {
  it('are current (run `pnpm --filter @blocks2cpp/catalog-gen run generate` if not)', async () => {
    expect(await checkOutputs(ROOT, await generateFrom(ROOT))).toEqual([]);
  });

  it('define every catalog block exactly once', async () => {
    const ids = (await catalogJson()).blocks.map((block) => block.id);
    const defined = generated.BLOCK_DEFS.map((block) => block.id);
    expect(new Set(defined).size).toBe(defined.length);
    expect(defined).toEqual(ids);
    expect(ids).toHaveLength(32);
  });

  it('hold exactly the generator model, with mutually compatible types', async () => {
    const model = normalize(await catalogJson());
    // Each direction must type-check: the copied types are the generator's types.
    const blocks: readonly BlockDefJson[] = generated.BLOCK_DEFS;
    const toolbox: readonly ToolboxCategoryJson[] = generated.TOOLBOX;
    const back: readonly generated.BlockDefJson[] = model.blocks;
    const backToolbox: readonly generated.ToolboxCategoryJson[] = model.toolbox;
    expect(blocks).toEqual(back);
    expect(toolbox).toEqual(backToolbox);
    expect(generated.CATALOG_VERSION).toBe(model.catalogVersion);
  });

  it('reach every block through the toolbox or a dynamic category', () => {
    const reachable = new Set<string>();
    for (const category of generated.TOOLBOX) {
      for (const entry of category.entries) {
        reachable.add(entry.block);
      }
      if (category.dynamic !== null) {
        for (const id of DYNAMIC_BLOCKS[category.dynamic]) {
          reachable.add(id);
        }
      }
    }
    expect([...reachable].sort()).toEqual(generated.BLOCK_DEFS.map((block) => block.id));
  });

  it('keep the labels, outputs and presets the editor relies on', () => {
    const byId = new Map(generated.BLOCK_DEFS.map((block) => [block.id, block]));
    expect(byId.get('io.print')?.labelParts).toEqual([
      { text: 'print' },
      { arg: 'ITEM' },
      { repeat: true },
      { arg: 'SEP' },
      { arg: 'NEWLINE' },
    ]);
    expect(byId.get('program.main')?.statementsNotInLabel).toEqual(['BODY']);
    expect(byId.get('control.if')?.statementsNotInLabel).toEqual([]);
    expect(byId.get('math.random_int')?.output).toBe('int');
    expect(byId.get('math.convert')?.output).toBe('field:TO');
    expect(byId.get('var.get')?.output).toBe('symbol');
    expect(byId.get('io.print')?.extra[0]).toEqual({
      name: 'itemCount',
      kind: 'count',
      min: 1,
      max: 32,
      default: 1,
      types: [],
    });
    for (const block of generated.BLOCK_DEFS) {
      const value = block.shape === 'reporter' || block.shape === 'predicate';
      expect(block.output !== null, block.id).toBe(value);
    }
    const entries = generated.TOOLBOX.flatMap((category) => category.entries);
    expect(entries.find((e) => e.label === 'repeat until')).toEqual({
      block: 'control.while',
      label: 'repeat until',
      preset: { fields: { MODE: 'until' } },
    });
    expect(entries.find((e) => e.label === 'if … else')?.preset).toEqual({
      extra: { hasElse: true },
    });
    expect(entries.find((e) => e.block === 'io.ask')?.preset).toEqual({
      inputs: { PROMPT: [{ str: 'Your answer: ' }] },
    });
    expect(generated.TOOLBOX.map((category) => category.id)).toEqual([
      'program',
      'variables',
      'math',
      'logic',
      'text',
      'control',
      'loops',
      'io',
      'functions',
    ]);
  });
});
