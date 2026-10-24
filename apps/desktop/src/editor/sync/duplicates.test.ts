/**
 * Fresh IDs for duplicated blocks (docs/spec/05-project-format.md §5.4–5.5): Blockly's duplicate,
 * paste, toolbox drags and redo never produce B2C-E0114 or B2C-E0115, with the real compiler core.
 */
import type { BdmBlock, BdmDocument, CoreWasm } from '@blocks2cpp/b2c-core-wasm';
import {
  B2cSymbolDeclField,
  B2cSymbolRefField,
  GENERATED_ID_PATTERN,
  isPlaceholder,
  PROJECT_ID_PATTERN,
} from '@blocks2cpp/blockly-ext';
import * as Blockly from 'blockly/core';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { documentFixture } from '../../app/testing/fixtures';
import { forEachNode, treeDecls } from './bdmTree';
import { placeholderNode } from './placeholders';
import {
  disposeWorkspaces,
  EXAMPLE_PROJECTS,
  loadText,
  present,
  seededRandom,
  startSession,
  testCore,
  type TestSession,
} from './testing';

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
  // Blockly schedules its event queue once until it is emptied: empty it before the fake timers go.
  vi.advanceTimersByTime(50);
  vi.useRealTimers();
});

/** Lets Blockly report its events to the session. */
function deliverEvents(): void {
  vi.advanceTimersByTime(20);
}

/** Duplicates a block the way Blockly's *Duplicate* and paste do: one undoable creation. */
function duplicate(block: Blockly.Block): Blockly.Block {
  const state = Blockly.serialization.blocks.save(block, {
    addCoordinates: true,
    addNextBlocks: false,
    saveIds: false,
  });
  if (state === null) {
    throw new Error('the block could not be saved');
  }
  Blockly.Events.disable();
  let copy: Blockly.Block;
  try {
    copy = Blockly.serialization.blocks.append(state, block.workspace);
  } finally {
    Blockly.Events.enable();
  }
  const BlockCreate = Blockly.Events.get(Blockly.Events.BLOCK_CREATE);
  Blockly.Events.fire(new BlockCreate(copy));
  return copy;
}

function open(wasm: CoreWasm, doc: BdmDocument): TestSession {
  running = startSession(wasm, doc);
  return running;
}

/** The document the canvas holds now, and whether the loader accepts it. */
function loadsCleanly(wasm: CoreWasm, session: TestSession): { doc: BdmDocument; codes: string[] } {
  const doc = session.session.currentDocument();
  const loaded = wasm.load(new TextEncoder().encode(JSON.stringify(doc)));
  return { doc, codes: loaded.diagnostics.map((diagnostic) => diagnostic.code) };
}

function declField(block: Blockly.Block, name: string): B2cSymbolDeclField {
  const field = block.getField(name);
  if (!(field instanceof B2cSymbolDeclField)) {
    throw new Error(`no declaration field ${name}`);
  }
  return field;
}

describe.skipIf(core === null)('duplicating blocks', () => {
  it('gives a duplicated declaration a fresh symbol ID, and references inside follow it', () => {
    if (core === null) {
      return;
    }
    const session = open(core, loadText(core, EXAMPLE_PROJECTS['factorial.b2c'] ?? ''));
    const { workspace } = session;
    const define = workspace.getTopBlocks(false).find((block) => block.type === 'func.define');
    if (define === undefined) {
      throw new Error('factorial.b2c has no function');
    }
    const original = declField(define, 'NAME').getDecl();
    const originalParams = (
      define as unknown as { b2cGetExtra(): { params: { sym: string }[] } }
    ).b2cGetExtra().params;

    const copy = duplicate(define);
    deliverEvents();

    const copied = declField(copy, 'NAME').getDecl();
    expect(copied?.name).toBe(original?.name);
    expect(copied?.sym).not.toBe(original?.sym);
    expect(copied?.sym).toMatch(GENERATED_ID_PATTERN.sym);
    const copiedParams = (
      copy as unknown as { b2cGetExtra(): { params: { sym: string }[] } }
    ).b2cGetExtra().params;
    expect(copiedParams).toHaveLength(originalParams.length);
    copiedParams.forEach((row, index) => {
      expect(row.sym).not.toBe(originalParams[index]?.sym);
    });
    // Every reference inside the copy names the copy's own symbols.
    const copySyms = new Set([copied?.sym, ...copiedParams.map((row) => row.sym)]);
    const originalSyms = new Set([original?.sym, ...originalParams.map((row) => row.sym)]);
    const { doc, codes } = loadsCleanly(core, session);
    expect(codes).toEqual([]);
    const copyNode = doc.modules[0]?.workspace.blocks.find((node) => node.id === copy.id);
    const refs: string[] = [];
    forEachNode(copyNode === undefined ? [] : [copyNode], (node) => {
      for (const value of Object.values(node.fields ?? {})) {
        if (typeof value === 'object' && 'ref' in value) {
          refs.push(value.ref);
        }
      }
      for (const input of Object.values(node.inputs ?? {})) {
        if ('expr' in input) {
          for (const token of input.expr) {
            if ('ref' in token) {
              refs.push(token.ref);
            }
          }
        }
      }
    });
    expect(refs.length).toBeGreaterThan(0);
    for (const ref of refs) {
      expect(originalSyms.has(ref), ref).toBe(false);
      expect(copySyms.has(ref) || !originalSyms.has(ref), ref).toBe(true);
    }
  });

  it('keeps references to symbols declared outside the copied blocks', () => {
    if (core === null) {
      return;
    }
    const session = open(core, loadText(core, EXAMPLE_PROJECTS['guessing_game.b2c'] ?? ''));
    const ask = session.workspace.getBlockById('b005');
    if (ask === null) {
      throw new Error('no ask block');
    }
    const copy = duplicate(ask);
    deliverEvents();
    expect((copy.getField('VAR') as B2cSymbolRefField).getRef()).toEqual({ ref: 's_guess' });
  });

  it('gives a redone duplicate the same fresh IDs again', () => {
    if (core === null) {
      return;
    }
    const session = open(core, loadText(core, EXAMPLE_PROJECTS['guessing_game.b2c'] ?? ''));
    const { workspace } = session;
    const declaration = workspace.getBlockById('b003');
    if (declaration === null) {
      throw new Error('no declaration');
    }
    const copy = duplicate(declaration);
    deliverEvents();
    const fresh = declField(copy, 'NAME').getDecl();
    expect(fresh?.sym).not.toBe('s_guess');

    workspace.undo(false);
    deliverEvents();
    expect(workspace.getBlockById(copy.id)).toBeNull();
    workspace.undo(true);
    deliverEvents();
    const redone = workspace.getBlockById(copy.id);
    expect(redone).not.toBeNull();
    expect(declField(present(redone, 'the redone block'), 'NAME').getDecl()).toEqual(fresh);
    expect(loadsCleanly(core, session).codes).toEqual([]);
  });

  it('gives a copied placeholder fresh IDs for everything it keeps', () => {
    if (core === null) {
      return;
    }
    const kept: BdmBlock = {
      id: 'b_pack',
      type: 'sfml.window.loop',
      v: 1,
      x: 40,
      y: 40,
      fields: { NAME: { sym: 's_window', name: 'window' } },
      statements: {
        BODY: [
          {
            id: 'b_inner',
            type: 'var.declare',
            v: 1,
            fields: { CONST: false, NAME: { sym: 's_inner', name: 'inner' }, TYPE: 'int' },
            inputs: { VALUE: { expr: [{ ref: 's_window' }] } },
          },
        ],
      },
    };
    const doc = documentFixture();
    doc.modules = [{ id: 'mod_main', name: 'main', workspace: { blocks: [kept] } }];
    const session = open(core, loadText(core, JSON.stringify(doc)));
    const placeholder = session.workspace.getBlockById('b_pack');
    expect(placeholder === null ? false : isPlaceholder(placeholder)).toBe(true);

    const copy = duplicate(present(placeholder, 'the placeholder'));
    deliverEvents();
    const node = placeholderNode(copy);
    expect(node?.id).toBe(copy.id);
    const inner = node?.statements?.['BODY']?.[0];
    expect(inner?.id).toMatch(GENERATED_ID_PATTERN.blk);
    const decls = treeDecls(node === null ? [] : [node]);
    expect(decls.map((decl) => decl.name)).toEqual(['window', 'inner']);
    for (const decl of decls) {
      expect(decl.sym).toMatch(GENERATED_ID_PATTERN.sym);
    }
    // The kept reference follows its renamed declaration.
    const value = inner?.inputs?.['VALUE'];
    expect(value !== undefined && 'expr' in value ? value.expr : []).toEqual([
      { ref: decls[0]?.sym },
    ]);
    expect(loadsCleanly(core, session).codes).toEqual([]);
  });

  it('keeps 10,000 random duplicates loadable, with IDs of the right form', () => {
    if (core === null) {
      return;
    }
    const session = open(core, loadText(core, EXAMPLE_PROJECTS['max_of_three.b2c'] ?? ''));
    const { workspace } = session;
    const random = seededRandom(20261005);
    const copies: string[] = [];
    for (let round = 1; round <= 10_000; round += 1) {
      const blocks = workspace.getAllBlocks(false).filter((block) => !block.isShadow());
      const source = blocks[Math.floor(random() * blocks.length)];
      if (source === undefined) {
        throw new Error('the workspace is empty');
      }
      copies.push(duplicate(source).id);
      deliverEvents();
      // Keep the canvas within a size the loader's limits and the test's time allow.
      while (workspace.getAllBlocks(false).length > 1_500 && copies.length > 0) {
        const index = Math.floor(random() * copies.length);
        const [id] = copies.splice(index, 1);
        workspace.getBlockById(id ?? '')?.dispose(false);
      }
      if (round % 1_000 === 0) {
        deliverEvents();
        const { doc, codes } = loadsCleanly(core, session);
        expect(codes, `after ${String(round)} duplicates`).toEqual([]);
        const syms = new Set<string>();
        for (const module of doc.modules) {
          forEachNode(module.workspace.blocks, (node) => {
            expect(node.id).toMatch(PROJECT_ID_PATTERN);
          });
          for (const decl of treeDecls(module.workspace.blocks)) {
            expect(syms.has(decl.sym), decl.sym).toBe(false);
            syms.add(decl.sym);
          }
        }
      }
    }
  }, 300_000);
});
