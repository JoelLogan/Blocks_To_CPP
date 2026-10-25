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
  variables: ['var.get', 'var.set', 'var.change'],
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
    expect(ids).toHaveLength(31);
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
    // Every field is named by the words around it (03 §3.11.1).
    expect(byId.get('io.print')?.labelParts).toEqual([
      { text: 'print' },
      { arg: 'ITEM' },
      { repeat: true },
      { arg: 'SEP' },
      { text: 'new line' },
      { arg: 'NEWLINE' },
      { text: 'to' },
      { arg: 'STREAM' },
    ]);
    expect(byId.get('io.print')?.fields.find((field) => field.name === 'STREAM')?.options).toEqual([
      ['the console', 'out'],
      ['the error stream', 'err'],
    ]);
    expect(byId.get('var.declare')?.labelParts).toEqual([
      { text: 'create' },
      { arg: 'TYPE' },
      { text: 'variable' },
      { arg: 'NAME' },
      { text: '=' },
      { arg: 'VALUE' },
      { text: 'constant' },
      { arg: 'CONST' },
    ]);
    expect(byId.get('text.char')?.labelParts).toEqual([
      { text: "character '" },
      { arg: 'VALUE' },
      { text: "'" },
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
      typeClass: null,
    });
    // The signatures of M2's typed blocks (slots.md §5) and the type table come through.
    const declare = byId.get('var.declare');
    expect(declare?.typeVars).toEqual({ T: 'value' });
    expect(declare?.order).toEqual([{ field: 'TYPE' }, { field: 'NAME' }, { input: 'VALUE' }]);
    expect(declare?.fields.find((f) => f.name === 'TYPE')?.type).toEqual({
      kind: 'var',
      name: 'T',
    });
    expect(byId.get('var.set')?.inputs[0]?.assign).toBe(true);
    expect(byId.get('func.call')?.inputs[0]?.type).toEqual({ kind: 'param', field: 'FUNC' });
    expect(byId.get('func.return')?.inputs[0]?.type).toEqual({ kind: 'result' });
    expect(byId.get('func.define')?.extra[0]?.typeClass).toBe('value');
    expect(byId.get('io.print')?.headers).toEqual(['<iostream>']);
    expect(generated.TYPE_TABLE.map((entry) => entry.name)).toContain('std::priority_queue');
    for (const block of generated.BLOCK_DEFS) {
      const value = block.shape === 'reporter' || block.shape === 'predicate';
      expect(block.output !== null, block.id).toBe(value);
    }
    const entries = generated.TOOLBOX.flatMap((category) => category.entries);
    // One idea, one block, one toolbox entry (03 §3.1): *until* and *else* are chosen on the
    // block, so `control.while` and `control.if` appear once, as the catalog defines them.
    expect(new Set(entries.map((e) => e.block)).size).toBe(entries.length);
    expect(entries.filter((e) => e.label !== null)).toEqual([]);
    for (const block of ['control.while', 'control.if']) {
      expect(entries.find((e) => e.block === block)).toEqual({ block, label: null, preset: null });
    }
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
