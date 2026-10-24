/**
 * Fresh IDs (docs/spec/05-project-format.md §5.4, §5.12): a seeded random mix of copies, pastes at
 * every kind of target, duplicates and cuts on the guessing game, with real random seeds. After
 * every step the canvas still loads, block and symbol IDs stay unique, and every ID a paste made
 * has the seeded generator's form.
 *
 * Without a build of the core these tests are skipped, unless B2C_REQUIRE_WASM is set (as in CI).
 */
import type { BdmDocument, CoreWasm } from '@blocks2cpp/b2c-core-wasm';
import { randomSeedHex } from '@blocks2cpp/b2c-core-wasm';
import * as Blockly from 'blockly/core';
import { beforeAll, describe, expect, it } from 'vitest';

import { useAppStore } from '../../app/store';
import { forEachNode, nodeDecls } from '../sync/bdmTree';
import { disposeWorkspaces, headlessWorkspace, seededRandom, startSession } from '../sync/testing';
import { anchorFor, anchorForBlock, isProjectBlock, ON_CANVAS, type PasteAnchor } from './anchor';
import { ClipboardController } from './controller';
import { ClipboardMemory } from './memory';
import type { ClipboardNotice } from './notices';
import {
  FRESH_BLOCK_ID,
  FRESH_SYMBOL_ID,
  loadGuessingGame,
  requireCore,
  WITH_CORE,
} from './testing';

let core: CoreWasm;

beforeAll(async () => {
  if (WITH_CORE) {
    core = await requireCore();
  }
});

/** The block IDs and declared symbol IDs of a document, in document order. */
function idsOf(doc: BdmDocument): { blocks: string[]; symbols: string[] } {
  const blocks: string[] = [];
  const symbols: string[] = [];
  for (const module of doc.modules) {
    forEachNode(module.workspace.blocks, (node) => {
      blocks.push(node.id);
      symbols.push(...nodeDecls(node).map((decl) => decl.sym));
    });
  }
  return { blocks, symbols };
}

/** Picks one element of a non-empty list. */
function pick<T>(random: () => number, items: readonly T[]): T {
  const item = items[Math.floor(random() * items.length)];
  if (item === undefined) {
    throw new Error('nothing to pick from');
  }
  return item;
}

describe.skipIf(!WITH_CORE)('fresh IDs over random clipboard actions', () => {
  it('keep every ID unique and well formed, and the document loadable', () => {
    const random = seededRandom(0x5eed);
    const session = startSession(core, loadGuessingGame(core), { workspace: headlessWorkspace() });
    const notices: ClipboardNotice[] = [];
    const controller = new ClipboardController({
      workspace: session.workspace,
      store: useAppStore,
      core: () => core,
      activeModuleId: () => session.session.shownModuleId() ?? '',
      memory: new ClipboardMemory(),
      notify: (notice) => notices.push(notice),
      seed: () => randomSeedHex(),
      restartCore: null,
    });
    const original = idsOf(session.session.currentDocument());
    const knownBlocks = new Set(original.blocks);
    const knownSymbols = new Set(original.symbols);
    let pastes = 0;
    try {
      for (let step = 0; step < 150; step += 1) {
        const blocks = session.workspace
          .getAllBlocks(false)
          .filter((block) => isProjectBlock(session.workspace, block));
        const block = pick(random, blocks);
        const roll = random();
        if (roll < 0.3) {
          if (controller.duplicate(block).kind === 'pasted') {
            pastes += 1;
          }
        } else if (roll < 0.9 || blocks.length < 20) {
          controller.copy(block);
          const target = pick(random, blocks);
          const slots = target.inputList
            .map((input) => input.connection?.targetBlock() ?? null)
            .filter((child): child is Blockly.Block => child?.isShadow() === true);
          const anchors: PasteAnchor[] = [ON_CANVAS, anchorForBlock(target)];
          if (slots.length > 0) {
            anchors.push(anchorFor(session.workspace, pick(random, slots)));
          }
          if (controller.paste(null, pick(random, anchors)).kind === 'pasted') {
            pastes += 1;
          }
        } else if (block.isDeletable()) {
          controller.cut(block);
        }

        const doc = session.session.currentDocument();
        expect(core.canonical(JSON.stringify(doc)).ok).toBe(true);
        const ids = idsOf(doc);
        expect(new Set(ids.blocks).size).toBe(ids.blocks.length);
        expect(new Set(ids.symbols).size).toBe(ids.symbols.length);
        for (const id of ids.blocks.filter((id) => !knownBlocks.has(id))) {
          expect(id).toMatch(FRESH_BLOCK_ID);
          knownBlocks.add(id);
        }
        for (const sym of ids.symbols.filter((sym) => !knownSymbols.has(sym))) {
          expect(sym).toMatch(FRESH_SYMBOL_ID);
          knownSymbols.add(sym);
        }
      }
      expect(pastes).toBeGreaterThan(100);
      expect(notices).toEqual([]);
      expect(knownSymbols.size).toBeGreaterThan(original.symbols.length);
    } finally {
      session.dispose();
      disposeWorkspaces();
    }
  }, 120_000);
});
