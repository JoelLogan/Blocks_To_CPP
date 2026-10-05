/**
 * Scripted editing sessions on a headless workspace with the real blocks, the real mutators and the
 * real compiler core (09 §9.2): random creates, deletes, moves, mutations, duplicates, disables,
 * comments and collapses. Every document the editor derives must load with no B2C-E01xx, every ID
 * must have the project format's form, and the result must survive a reload unchanged.
 */
import type { BdmDocument, CoreWasm } from '@blocks2cpp/b2c-core-wasm';
import {
  B2cSymbolDeclField,
  B2cSymbolRefField,
  BLOCK_DEFS,
  GENERATED_ID_PATTERN,
  hasB2cMutator,
  isManuallyDisabled,
  newId,
  PROJECT_ID_PATTERN,
  setManuallyDisabled,
} from '@blocks2cpp/blockly-ext';
import * as Blockly from 'blockly/core';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { useAppStore } from '../../app/store';
import { loadModule } from './bdmToWorkspace';
import { forEachNode, treeDecls } from './bdmTree';
import {
  canonicalText,
  disposeWorkspaces,
  EXAMPLE_PROJECTS,
  headlessWorkspace,
  loadText,
  seededRandom,
  startSession,
  testCore,
  type TestSession,
} from './testing';
import { readModule } from './workspaceToBdm';

const core = await testCore();

let running: TestSession | null = null;

beforeEach(() => {
  vi.useFakeTimers({
    toFake: ['setTimeout', 'clearTimeout', 'requestAnimationFrame', 'cancelAnimationFrame'],
  });
});

afterEach(() => {
  running?.dispose();
  running = null;
  disposeWorkspaces();
  vi.advanceTimersByTime(50);
  vi.useRealTimers();
});

const STATEMENT = Blockly.ConnectionType.NEXT_STATEMENT;
const VALUE = Blockly.ConnectionType.INPUT_VALUE;

/** A random editor, driving one workspace. */
class Monkey {
  private names = 0;
  private readonly workspace: Blockly.Workspace;
  private readonly random: () => number;

  constructor(workspace: Blockly.Workspace, random: () => number) {
    this.workspace = workspace;
    this.random = random;
  }

  pick<T>(items: readonly T[]): T | undefined {
    return items[Math.floor(this.random() * items.length)];
  }

  /** Project blocks (no shadows). */
  blocks(): Blockly.Block[] {
    return this.workspace.getAllBlocks(false).filter((block) => !block.isShadow());
  }

  /** Every symbol ID declared on the canvas. */
  private declaredSyms(): string[] {
    const syms: string[] = [];
    for (const block of this.blocks()) {
      for (const input of block.inputList) {
        for (const field of input.fieldRow) {
          if (field instanceof B2cSymbolDeclField) {
            const sym = field.getSymbolId();
            if (sym !== null) {
              syms.push(sym);
            }
          }
        }
      }
    }
    return syms;
  }

  /** Free connections `block` could go into (not inside itself). */
  private targetsFor(block: Blockly.Block): Blockly.Connection[] {
    const own = new Set(block.getDescendants(false));
    const wanted: Blockly.ConnectionType | null =
      block.outputConnection !== null
        ? VALUE
        : block.previousConnection !== null
          ? STATEMENT
          : null;
    if (wanted === null) {
      return [];
    }
    const targets: Blockly.Connection[] = [];
    for (const candidate of this.blocks()) {
      if (own.has(candidate)) {
        continue;
      }
      const connections = [
        ...candidate.inputList.map((input) => input.connection),
        candidate.nextConnection,
      ];
      for (const connection of connections) {
        if (connection?.type !== wanted) {
          continue;
        }
        const occupant = connection.targetBlock();
        if (wanted === VALUE && occupant !== null && !occupant.isShadow()) {
          continue;
        }
        targets.push(connection);
      }
    }
    return targets;
  }

  /** Puts `block` somewhere: into a free connection, or on the canvas. */
  private place(block: Blockly.Block): void {
    const target = this.random() < 0.8 ? this.pick(this.targetsFor(block)) : undefined;
    const child = block.outputConnection ?? block.previousConnection;
    if (target !== undefined && child !== null && target.connect(child)) {
      return;
    }
    if (block.getParent() === null) {
      block.moveBy(Math.floor(this.random() * 2_000) - 1_000, Math.floor(this.random() * 2_000));
    }
  }

  create(): void {
    const def = this.pick(BLOCK_DEFS);
    if (def === undefined) {
      return;
    }
    const block = this.workspace.newBlock(def.id);
    block.initModel();
    const syms = this.declaredSyms();
    for (const field of def.fields) {
      const created = block.getField(field.name);
      if (created instanceof B2cSymbolDeclField) {
        this.names += 1;
        created.setDecl({ sym: newId('sym'), name: `name${String(this.names)}` });
      } else if (created instanceof B2cSymbolRefField) {
        const sym = this.pick(syms);
        if (sym !== undefined) {
          created.setRef({ ref: sym });
        }
      }
    }
    this.place(block);
  }

  delete(): void {
    const block = this.pick(this.blocks());
    if (block !== undefined && this.blocks().length > 3) {
      block.dispose(true);
    }
  }

  move(): void {
    const block = this.pick(this.blocks());
    if (block === undefined) {
      return;
    }
    block.unplug(this.random() < 0.5);
    this.place(block);
  }

  mutate(): void {
    const block = this.pick(this.blocks().filter(hasB2cMutator));
    if (block === undefined || !hasB2cMutator(block)) {
      return;
    }
    const before = block.b2cGetExtra();
    const extra: Record<string, unknown> = {};
    for (const [key, value] of Object.entries(before)) {
      if (typeof value === 'number') {
        extra[key] = Math.floor(this.random() * 4);
      } else if (typeof value === 'boolean') {
        extra[key] = this.random() < 0.5;
      } else if (Array.isArray(value)) {
        const rows = Math.floor(this.random() * 3);
        extra[key] = Array.from({ length: rows }, (_unused, index) => ({
          sym: newId('sym'),
          name: `param${String(index)}`,
          type: 'int',
          mode: 'copy',
        }));
      }
    }
    try {
      block.b2cSetExtra(extra);
    } catch {
      return;
    }
    // As the ⊕/⊖ buttons do: one mutation event with the state before and after.
    const BlockChange = Blockly.Events.get(Blockly.Events.BLOCK_CHANGE);
    Blockly.Events.fire(
      new BlockChange(block, 'mutation', null, JSON.stringify(before), JSON.stringify(extra)),
    );
  }

  duplicate(): void {
    const source = this.pick(this.blocks());
    if (source === undefined) {
      return;
    }
    const state = Blockly.serialization.blocks.save(source, {
      addCoordinates: true,
      addNextBlocks: false,
      saveIds: false,
    });
    if (state === null) {
      return;
    }
    Blockly.Events.disable();
    let copy: Blockly.Block;
    try {
      copy = Blockly.serialization.blocks.append(state, this.workspace);
    } finally {
      Blockly.Events.enable();
    }
    const BlockCreate = Blockly.Events.get(Blockly.Events.BLOCK_CREATE);
    Blockly.Events.fire(new BlockCreate(copy));
    copy.moveBy(40, 40);
  }

  disable(): void {
    const block = this.pick(this.blocks());
    if (block !== undefined) {
      setManuallyDisabled(block, !isManuallyDisabled(block));
    }
  }

  comment(): void {
    const block = this.pick(this.blocks());
    if (block !== undefined) {
      block.setCommentText(block.getCommentText() === null ? `note ${String(this.names)}` : null);
    }
  }

  collapse(): void {
    const block = this.pick(this.blocks());
    if (block !== undefined) {
      block.setCollapsed(!block.isCollapsed());
    }
  }

  /** One random operation. */
  step(): string {
    const operations = [
      'create',
      'create',
      'delete',
      'move',
      'move',
      'mutate',
      'duplicate',
      'disable',
      'comment',
      'collapse',
    ] as const;
    const operation = this.pick(operations) ?? 'create';
    this[operation]();
    return operation;
  }
}

/** The IDs of `doc` that `original` does not have. */
function newIds(doc: BdmDocument, original: BdmDocument): { blocks: string[]; syms: string[] } {
  const known = new Set<string>();
  const knownSyms = new Set<string>();
  for (const module of original.modules) {
    forEachNode(module.workspace.blocks, (node) => known.add(node.id));
    for (const decl of treeDecls(module.workspace.blocks)) {
      knownSyms.add(decl.sym);
    }
  }
  const blocks: string[] = [];
  const syms: string[] = [];
  for (const module of doc.modules) {
    forEachNode(module.workspace.blocks, (node) => {
      expect(node.id).toMatch(PROJECT_ID_PATTERN);
      if (!known.has(node.id)) {
        blocks.push(node.id);
      }
    });
    for (const decl of treeDecls(module.workspace.blocks)) {
      if (!knownSyms.has(decl.sym)) {
        syms.push(decl.sym);
      }
    }
  }
  return { blocks, syms };
}

function expectLoads(wasm: CoreWasm, doc: BdmDocument, context: string): void {
  const loaded = wasm.load(new TextEncoder().encode(JSON.stringify(doc)));
  expect(
    loaded.diagnostics.map((diagnostic) => `${diagnostic.code} ${diagnostic.message}`),
    context,
  ).toEqual([]);
  expect(loaded.ok, context).toBe(true);
}

describe.skipIf(core === null)('scripted editing sessions', () => {
  it.each([1, 2, 3])(
    '1,000 random operations (seed %i) always give a loadable document',
    async (seed) => {
      if (core === null) {
        return;
      }
      const original = loadText(core, EXAMPLE_PROJECTS['guessing_game.b2c'] ?? '');
      running = startSession(core, original);
      const monkey = new Monkey(running.workspace, seededRandom(seed));
      const done: string[] = [];
      for (let step = 1; step <= 1_000; step += 1) {
        done.push(monkey.step());
        vi.advanceTimersByTime(20);
        if (step % 50 === 0) {
          const doc = running.session.currentDocument();
          expectLoads(core, doc, `after ${done.slice(-5).join(', ')} (step ${String(step)})`);
        }
      }
      vi.advanceTimersByTime(20);
      await running.session.flush();
      const doc = running.session.currentDocument();
      expectLoads(core, doc, 'at the end');
      const fresh = newIds(doc, original);
      expect(fresh.blocks.length).toBeGreaterThan(0);
      for (const id of fresh.blocks) {
        expect(id).toMatch(GENERATED_ID_PATTERN.blk);
      }
      for (const sym of fresh.syms) {
        expect(sym).toMatch(GENERATED_ID_PATTERN.sym);
      }

      // The derived document is what the store holds, and it survives a reload byte for byte.
      expect(useAppStore.getState().project?.canonicalText).toBe(canonicalText(core, doc));
      const reloaded = headlessWorkspace();
      loadModule(reloaded, doc, 'mod_main');
      expect(canonicalText(core, readModule(reloaded, doc, 'mod_main'))).toBe(
        canonicalText(core, doc),
      );
    },
    120_000,
  );
});
