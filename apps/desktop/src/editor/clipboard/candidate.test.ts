/**
 * The document as it would be after a paste (checked against the loader's limits before anything
 * changes): the leading blocks at each kind of anchor, the rest on the canvas, and the given
 * document left untouched.
 */
import type { BdmBlock, BdmDocument } from '@blocks2cpp/b2c-core-wasm';
import type * as Blockly from 'blockly/core';
import { afterEach, describe, expect, it } from 'vitest';

import { guessingGame } from '../diagnostics/testing';
import { loadModule } from '../sync/bdmToWorkspace';
import { MAX_COORDINATE } from '../sync/limits';
import { disposeWorkspaces, headlessWorkspace } from '../sync/testing';
import type { PasteAnchor } from './anchor';
import { documentWithPasted } from './candidate';
import { listOf, nodeOf } from './testing';

const ORIGIN = { x: 300, y: 500 };

afterEach(() => {
  disposeWorkspaces();
});

/** Freezes a value and everything in it, so that any change to it throws. */
function deepFreeze<T>(value: T): T {
  const pending: unknown[] = [value];
  for (let item = pending.pop(); item !== undefined; item = pending.pop()) {
    if (typeof item === 'object' && item !== null && !Object.isFrozen(item)) {
      Object.freeze(item);
      pending.push(...Object.values(item as Record<string, unknown>));
    }
  }
  return value;
}

/** A pasted print statement with a fresh ID. */
function print(id: string): BdmBlock {
  return {
    id,
    type: 'io.print',
    v: 1,
    extra: { itemCount: 1 },
    fields: { NEWLINE: true, SEP: 'none', STREAM: 'out' },
    inputs: { ITEM0: { expr: [{ str: id }] } },
  };
}

/** A pasted reporter with a fresh ID. */
function randomInt(id: string): BdmBlock {
  return {
    id,
    type: 'math.random_int',
    v: 1,
    inputs: { LOW: { expr: [{ num: '1' }] }, HIGH: { expr: [{ num: '6' }] } },
  };
}

/** The guessing game (frozen) with a loose stack `L1, L2` on the canvas, and its canvas. */
function setUp(): { doc: BdmDocument; workspace: Blockly.Workspace } {
  const doc = guessingGame();
  doc.modules[0]?.workspace.blocks.push({ ...print('L1'), x: 600, y: 40, stack: [print('L2')] });
  const workspace = headlessWorkspace();
  loadModule(workspace, doc, 'mod_main');
  return { doc: deepFreeze(doc), workspace };
}

function block(workspace: Blockly.Workspace, id: string): Blockly.Block {
  const found = workspace.getBlockById(id);
  if (found === null) {
    throw new Error(`no block ${id}`);
  }
  return found;
}

function topIds(doc: BdmDocument): string[] {
  return (doc.modules[0]?.workspace.blocks ?? []).map((node) => node.id);
}

function bodyIds(doc: BdmDocument, id: string, list = 'BODY'): string[] {
  return listOf(nodeOf(doc, id), list).map((node) => node.id);
}

describe('documentWithPasted', () => {
  it('puts statements at the start of a statement list', () => {
    const { doc, workspace } = setUp();
    const anchor: PasteAnchor = { kind: 'list', block: block(workspace, 'b011'), input: 'BODY' };
    const after = documentWithPasted(doc, 'mod_main', [print('p1'), print('p2')], anchor, ORIGIN);
    expect(bodyIds(after, 'b011')).toEqual(['p1', 'p2', 'b002', 'b003', 'b004', 'b010']);
    expect(bodyIds(doc, 'b011')).toEqual(['b002', 'b003', 'b004', 'b010']);
    // Only the path to the anchor is copied.
    expect(nodeOf(after, 'b010')).toBe(nodeOf(doc, 'b010'));
    expect(after.modules[0]?.workspace.blocks[1]).toBe(doc.modules[0]?.workspace.blocks[1]);
  });

  it('puts statements directly after a block in a nested list', () => {
    const { doc, workspace } = setUp();
    const anchor: PasteAnchor = { kind: 'after', block: block(workspace, 'b006') };
    const after = documentWithPasted(doc, 'mod_main', [print('p1')], anchor, ORIGIN);
    expect(bodyIds(after, 'b009', 'DO0')).toEqual(['b006', 'p1']);
    expect(bodyIds(after, 'b009', 'DO1')).toEqual(['b007']);
    expect(topIds(after)).toEqual(topIds(doc));
  });

  it('puts statements into a loose stack after its head or one of its blocks', () => {
    const { doc, workspace } = setUp();
    const afterHead = documentWithPasted(
      doc,
      'mod_main',
      [print('p1')],
      { kind: 'after', block: block(workspace, 'L1') },
      ORIGIN,
    );
    expect(nodeOf(afterHead, 'L1').stack?.map((node) => node.id)).toEqual(['p1', 'L2']);
    const afterSecond = documentWithPasted(
      doc,
      'mod_main',
      [print('p1')],
      { kind: 'after', block: block(workspace, 'L2') },
      ORIGIN,
    );
    expect(nodeOf(afterSecond, 'L1').stack?.map((node) => node.id)).toEqual(['L2', 'p1']);
  });

  it('puts a reporter into an expression slot, but not over a block', () => {
    const { doc, workspace } = setUp();
    const slot = documentWithPasted(
      doc,
      'mod_main',
      [randomInt('r1')],
      { kind: 'value', block: block(workspace, 'b003'), input: 'VALUE' },
      ORIGIN,
    );
    expect(nodeOf(slot, 'b003').inputs?.['VALUE']).toEqual({ block: randomInt('r1') });
    expect(topIds(slot)).toEqual(topIds(doc));

    const taken = documentWithPasted(
      doc,
      'mod_main',
      [randomInt('r1')],
      { kind: 'value', block: block(workspace, 'b002'), input: 'VALUE' },
      ORIGIN,
    );
    expect(nodeOf(taken, 'b002').inputs?.['VALUE']).toEqual(nodeOf(doc, 'b002').inputs?.['VALUE']);
    expect(topIds(taken)).toEqual([...topIds(doc), 'r1']);
    expect(nodeOf(taken, 'r1')).toMatchObject(ORIGIN);
  });

  it('leaves blocks that do not fit at the anchor on the canvas', () => {
    const { doc, workspace } = setUp();
    // A reporter after a statement, and a statement in a value input.
    const reporter = documentWithPasted(
      doc,
      'mod_main',
      [randomInt('r1')],
      { kind: 'after', block: block(workspace, 'b004') },
      ORIGIN,
    );
    expect(topIds(reporter)).toEqual([...topIds(doc), 'r1']);
    expect(bodyIds(reporter, 'b011')).toEqual(bodyIds(doc, 'b011'));
    const statement = documentWithPasted(
      doc,
      'mod_main',
      [print('p1')],
      { kind: 'value', block: block(workspace, 'b003'), input: 'VALUE' },
      ORIGIN,
    );
    expect(topIds(statement)).toEqual([...topIds(doc), 'p1']);
    // Statements after the first block that does not fit stay loose too.
    const mixed = documentWithPasted(
      doc,
      'mod_main',
      [print('p1'), randomInt('r1'), print('p2')],
      { kind: 'after', block: block(workspace, 'b004') },
      ORIGIN,
    );
    expect(bodyIds(mixed, 'b011')).toEqual(['b002', 'b003', 'b004', 'p1', 'b010']);
    expect(topIds(mixed)).toEqual([...topIds(doc), 'r1', 'p2']);
  });

  it('puts every block on the canvas for a canvas paste, at a position in range', () => {
    const { doc } = setUp();
    const stacked: BdmBlock = { ...print('p1'), stack: [print('p2')] };
    const after = documentWithPasted(
      doc,
      'mod_main',
      [stacked, randomInt('r1')],
      { kind: 'canvas', near: null, at: null },
      { x: MAX_COORDINATE * 4, y: -MAX_COORDINATE * 4 },
    );
    expect(topIds(after)).toEqual([...topIds(doc), 'p1', 'r1']);
    expect(nodeOf(after, 'p1')).toMatchObject({ x: MAX_COORDINATE, y: -MAX_COORDINATE });
    expect(nodeOf(after, 'p1').stack).toHaveLength(1);
  });

  it('puts blocks whose anchor is not in the module on the canvas', () => {
    const { doc } = setUp();
    const stray = headlessWorkspace().newBlock('io.print', 'elsewhere');
    const after = documentWithPasted(
      doc,
      'mod_main',
      [print('p1')],
      { kind: 'after', block: stray },
      ORIGIN,
    );
    expect(topIds(after)).toEqual([...topIds(doc), 'p1']);
  });

  it('returns a document without the module unchanged', () => {
    const { doc, workspace } = setUp();
    const anchor: PasteAnchor = { kind: 'after', block: block(workspace, 'b004') };
    expect(documentWithPasted(doc, 'mod_other', [print('p1')], anchor, ORIGIN)).toBe(doc);
  });

  it('keeps an input named like a prototype key as plain data', () => {
    const { doc, workspace } = setUp();
    const anchor: PasteAnchor = {
      kind: 'list',
      block: block(workspace, 'b011'),
      input: '__proto__',
    };
    const after = documentWithPasted(doc, 'mod_main', [print('p1')], anchor, ORIGIN);
    const statements = nodeOf(after, 'b011').statements ?? {};
    expect(Object.hasOwn(statements, '__proto__')).toBe(true);
    expect(Object.getPrototypeOf(statements)).toBe(Object.prototype);
  });
});
