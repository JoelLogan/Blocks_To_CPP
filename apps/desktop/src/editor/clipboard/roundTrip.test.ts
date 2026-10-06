/**
 * Copy, cut, paste and duplicate with the real compiler core (built by
 * `pnpm --filter @blocks2cpp/b2c-core-wasm build`) on a rendered workspace with the editing
 * session: round trips within a document and across documents (references bound again by name
 * when visible, `B2C-E0201` otherwise), the C++ for `text/plain`, stacks, value inputs, undo, and
 * fresh IDs over many duplicates.
 *
 * Without a build these tests are skipped, unless B2C_REQUIRE_WASM is set (as in CI).
 */
import type { BdmDocument, CoreWasm } from '@blocks2cpp/b2c-core-wasm';
import { afterEach, beforeAll, describe, expect, it } from 'vitest';

import { setEditorHandle } from '../../app/editor-types';
import { useAppStore } from '../../app/store';
import { forEachNode } from '../sync/bdmTree';
import {
  disposeWorkspaces,
  EXAMPLE_PROJECTS,
  headlessWorkspace,
  loadText,
  renderedWorkspace,
  startSession,
} from '../sync/testing';
import { anchorFor, anchorForBlock, ON_CANVAS } from './anchor';
import {
  canvasBlock,
  clipboardEditor,
  type ClipboardTestEditor,
  FRESH_BLOCK_ID,
  FRESH_SYMBOL_ID,
  listOf,
  loadGuessingGame,
  nodeOf,
  nodesById,
  requireCore,
  sessionController,
  settle,
  WITH_CORE,
} from './testing';

let core: CoreWasm;
const editors: ClipboardTestEditor[] = [];

beforeAll(async () => {
  if (WITH_CORE) {
    core = await requireCore();
  }
});

afterEach(() => {
  for (const editor of editors.splice(0)) {
    editor.dispose();
  }
  setEditorHandle(null);
  disposeWorkspaces();
});

/** The guessing game as the core loads it, optionally with its text changed first. */
function guessingGame(edit?: (text: string) => string): BdmDocument {
  return loadGuessingGame(core, edit);
}

/** An editor showing `doc` on a rendered workspace. */
function editorFor(doc: BdmDocument): ClipboardTestEditor {
  const editor = clipboardEditor(core, doc, renderedWorkspace());
  editors.push(editor);
  return editor;
}

/** The canonical text of a document, or a failed test naming the loader's problems. */
function canonical(doc: BdmDocument): string {
  const result = core.canonical(JSON.stringify(doc));
  if (!result.ok) {
    throw new Error(`the document does not load: ${result.diagnostics.map((d) => d.code).join()}`);
  }
  return result.text;
}

/** The codes of the live analysis after the session has previewed the canvas. */
async function liveCodes(editor: ClipboardTestEditor): Promise<string[]> {
  await settle();
  await editor.session.session.flush();
  return (useAppStore.getState().analysis.preview?.diagnostics ?? []).map((d) => d.code);
}

/** The guessing game's `repeat until` loop body. */
function loopBody(doc: BdmDocument): string[] {
  return listOf(nodeOf(doc, 'b010'), 'BODY').map((node) => node.id);
}

describe.skipIf(!WITH_CORE)('the clipboard with the real core', () => {
  it('copies a block with its C++ as text and pastes it after another with fresh IDs', async () => {
    const editor = editorFor(guessingGame());
    const ask = canvasBlock(editor.workspace, 'b005');
    const data = editor.clipboard.controller.copy(ask);
    expect(data?.text).toContain('guess = b2c::ask<int>("Your guess: ");');
    expect(data?.payload).toContain('"format": "blocks2cpp/clipboard"');
    expect(editor.memory.get()).toEqual(data);

    const outcome = editor.clipboard.controller.paste(
      null,
      anchorForBlock(canvasBlock(editor.workspace, 'b009')),
    );
    expect(outcome).toMatchObject({ kind: 'pasted', attached: true, unresolved: [] });

    const doc = editor.document();
    const body = loopBody(doc);
    expect(body.slice(0, 2)).toEqual(['b005', 'b009']);
    const pastedId = body[2] ?? '';
    expect(pastedId).toMatch(FRESH_BLOCK_ID);
    // The reference to `guess` stays bound: the variable is visible after the `if`.
    expect(nodeOf(doc, pastedId)).toMatchObject({
      type: 'io.ask',
      fields: { VAR: { ref: 's_guess' } },
    });
    expect(canonical(doc)).toContain(pastedId);
    expect(editor.notices).toEqual([]);
    expect(await liveCodes(editor)).toEqual([]);
  });

  it('binds references by name in another document, and leaves the rest to B2C-E0201', async () => {
    const source = editorFor(guessingGame());
    const data = source.clipboard.controller.copy(canvasBlock(source.workspace, 'b005'));
    expect(data).not.toBeNull();
    source.dispose();
    editors.splice(editors.indexOf(source), 1);
    disposeWorkspaces();

    // The same program, but its `guess` has another symbol ID: bound by name and kind.
    const renamed = editorFor(guessingGame((text) => text.replaceAll('s_guess', 's_mine')));
    renamed.clipboard.controller.memory.set(data ?? { payload: '', text: null });
    const bound = renamed.clipboard.controller.paste(
      null,
      anchorForBlock(canvasBlock(renamed.workspace, 'b009')),
    );
    expect(bound).toMatchObject({ kind: 'pasted', attached: true, unresolved: [] });
    const boundId = loopBody(renamed.document())[2] ?? '';
    expect(nodeOf(renamed.document(), boundId).fields?.['VAR']).toEqual({ ref: 's_mine' });
    expect(await liveCodes(renamed)).toEqual([]);
    renamed.dispose();
    editors.splice(editors.indexOf(renamed), 1);
    disposeWorkspaces();

    // A program without `guess`: the reference keeps its symbol and the analyser reports it.
    const hello = editorFor(loadText(core, EXAMPLE_PROJECTS['hello_world.b2c'] ?? ''));
    hello.clipboard.controller.memory.set(data ?? { payload: '', text: null });
    const main = hello.workspace.getTopBlocks(false).find((b) => b.type === 'program.main');
    if (main === undefined) {
      throw new Error('hello_world has no main');
    }
    const unbound = hello.clipboard.controller.paste(null, anchorForBlock(main));
    expect(unbound).toMatchObject({
      kind: 'pasted',
      attached: true,
      unresolved: [{ sym: 's_guess', name: 'guess' }],
    });
    const codes = await liveCodes(hello);
    expect(codes).toContain('B2C-E0201');
    expect(hello.notices).toEqual([]);
  });

  it('pastes a copied reporter into an expression slot', () => {
    const editor = editorFor(guessingGame());
    editor.clipboard.controller.copy(canvasBlock(editor.workspace, 'b001'));
    // The slot of `create int guess = 0` (b003's VALUE shadow).
    const slot = canvasBlock(editor.workspace, 'b003').getInput('VALUE')?.connection?.targetBlock();
    expect(slot?.isShadow()).toBe(true);
    const outcome = editor.clipboard.controller.paste(null, anchorFor(editor.workspace, slot));
    expect(outcome).toMatchObject({ kind: 'pasted', attached: true });
    const value = nodeOf(editor.document(), 'b003').inputs?.['VALUE'];
    expect(value !== undefined && 'block' in value ? value.block.type : null).toBe(
      'math.random_int',
    );
  });

  it('keeps blocks that do not fit at the target on the canvas next to it', () => {
    const editor = editorFor(guessingGame());
    const tops = editor.workspace.getTopBlocks(false).length;
    editor.clipboard.controller.copy(canvasBlock(editor.workspace, 'b001'));
    // A reporter after a statement does not fit: it stays loose.
    const outcome = editor.clipboard.controller.paste(
      null,
      anchorForBlock(canvasBlock(editor.workspace, 'b004')),
    );
    expect(outcome).toMatchObject({ kind: 'pasted', attached: false });
    expect(editor.workspace.getTopBlocks(false)).toHaveLength(tops + 1);
    expect(canonical(editor.document())).toBeTruthy();
  });

  it('cuts exactly what it copied, as one undo step', async () => {
    const editor = editorFor(guessingGame());
    const data = editor.clipboard.controller.cut(canvasBlock(editor.workspace, 'b004'));
    expect(data?.text).toContain('Guess a number from 1 to 100!');
    const after = editor.document();
    expect(nodesById(after).has('b004')).toBe(false);
    // The list closed up behind the cut block.
    expect(listOf(nodeOf(after, 'b011'), 'BODY').map((n) => n.id)).toEqual([
      'b002',
      'b003',
      'b010',
    ]);
    await settle();
    editor.workspace.undo(false);
    expect(listOf(nodeOf(editor.document(), 'b011'), 'BODY').map((n) => n.id)).toEqual([
      'b002',
      'b003',
      'b004',
      'b010',
    ]);
  });

  it('undoes a paste in one step and puts back what it moved', async () => {
    const editor = editorFor(guessingGame());
    editor.clipboard.controller.copy(canvasBlock(editor.workspace, 'b004'));
    // Insert after b003: b004 and b010 follow the pasted block.
    editor.clipboard.controller.paste(null, anchorForBlock(canvasBlock(editor.workspace, 'b003')));
    const body = listOf(nodeOf(editor.document(), 'b011'), 'BODY').map((n) => n.id);
    expect(body).toHaveLength(5);
    expect(body.slice(3)).toEqual(['b004', 'b010']);
    await settle();
    editor.workspace.undo(false);
    await settle();
    expect(listOf(nodeOf(editor.document(), 'b011'), 'BODY').map((n) => n.id)).toEqual([
      'b002',
      'b003',
      'b004',
      'b010',
    ]);
    expect(editor.workspace.getTopBlocks(false)).toHaveLength(1);
  });

  it('copies a loose stack whole, and pastes it as a stack or into a list', () => {
    const doc = guessingGame();
    const main = doc.modules[0]?.workspace.blocks[0];
    if (main === undefined) {
      throw new Error('no main');
    }
    // Two loose prints stacked on the canvas.
    const print = nodeOf(doc, 'b004');
    doc.modules[0]?.workspace.blocks.push({
      ...print,
      id: 'loose1',
      x: 600,
      y: 40,
      stack: [{ ...print, id: 'loose2' }],
    });
    const editor = editorFor(loadText(core, canonical(doc)));
    const data = editor.clipboard.controller.copy(canvasBlock(editor.workspace, 'loose1'));
    expect(data?.payload).toContain('"stack"');

    const onCanvas = editor.clipboard.controller.paste(null, ON_CANVAS);
    expect(onCanvas).toMatchObject({ kind: 'pasted', attached: false });
    const tops = editor.document().modules[0]?.workspace.blocks ?? [];
    const pastedStack = tops.find((node) => FRESH_BLOCK_ID.test(node.id));
    expect(pastedStack?.stack).toHaveLength(1);

    const intoList = editor.clipboard.controller.paste(
      null,
      anchorForBlock(canvasBlock(editor.workspace, 'b003')),
    );
    expect(intoList).toMatchObject({ kind: 'pasted', attached: true });
    const body = listOf(nodeOf(editor.document(), 'b011'), 'BODY');
    expect(body).toHaveLength(6);
    expect(body.slice(2, 4).every((node) => FRESH_BLOCK_ID.test(node.id))).toBe(true);
    expect(canonical(editor.document())).toBeTruthy();
  });

  it('keeps a copied stack together on the canvas when it does not fit at the target', () => {
    const doc = guessingGame();
    const print = nodeOf(doc, 'b004');
    doc.modules[0]?.workspace.blocks.push({
      ...print,
      id: 'loose1',
      x: 600,
      y: 40,
      stack: [
        { ...print, id: 'loose2' },
        { ...print, id: 'loose3' },
      ],
    });
    const editor = editorFor(loadText(core, canonical(doc)));
    editor.clipboard.controller.copy(canvasBlock(editor.workspace, 'loose1'));
    const tops = editor.workspace.getTopBlocks(false).length;
    // Statements do not fit into an expression slot (b003's VALUE shadow); the core gives them
    // as a flat list for that target.
    const slot = canvasBlock(editor.workspace, 'b003').getInput('VALUE')?.connection?.targetBlock();
    const outcome = editor.clipboard.controller.paste(null, anchorFor(editor.workspace, slot));
    expect(outcome).toMatchObject({ kind: 'pasted', attached: false });
    expect(outcome.kind === 'pasted' ? outcome.blocks : []).toHaveLength(1);
    expect(editor.workspace.getTopBlocks(false)).toHaveLength(tops + 1);
    const pasted = (editor.document().modules[0]?.workspace.blocks ?? []).filter((node) =>
      FRESH_BLOCK_ID.test(node.id),
    );
    expect(pasted).toHaveLength(1);
    expect(pasted[0]?.stack?.map((node) => FRESH_BLOCK_ID.test(node.id))).toEqual([true, true]);
    expect(canonical(editor.document())).toBeTruthy();
  });

  it('gives a duplicated declaration a fresh symbol and keeps the copy after the original', () => {
    const editor = editorFor(guessingGame());
    const outcome = editor.clipboard.controller.duplicate(canvasBlock(editor.workspace, 'b003'));
    expect(outcome).toMatchObject({ kind: 'pasted', attached: true });
    const body = listOf(nodeOf(editor.document(), 'b011'), 'BODY');
    const copy = body[2];
    expect(copy?.id).toMatch(FRESH_BLOCK_ID);
    const name = copy?.fields?.['NAME'];
    expect(name !== undefined && typeof name === 'object' && 'sym' in name ? name.sym : '').toMatch(
      FRESH_SYMBOL_ID,
    );
    // Duplicating leaves both clipboards alone.
    expect(editor.memory.get()).toBeNull();
  });
});

describe.skipIf(!WITH_CORE)('many duplicates', () => {
  it('still loads after 1,000 duplicates, every ID fresh and well formed', async () => {
    // A loose declaration on the canvas (the analyser skips loose blocks, which keeps the test
    // fast): every copy needs a fresh block ID and a fresh symbol ID.
    const doc = guessingGame();
    const declaration = nodeOf(doc, 'b003');
    doc.modules[0]?.workspace.blocks.push({
      ...declaration,
      id: 'loose',
      x: 600,
      y: 40,
      fields: { ...declaration.fields, NAME: { sym: 's_loose', name: 'loose' } },
    });
    const session = startSession(core, loadText(core, canonical(doc)), {
      workspace: headlessWorkspace(),
    });
    try {
      const controller = sessionController(session);
      const before = new Set(nodesById(session.session.currentDocument()).keys());
      const original = session.workspace.getBlockById('loose');
      if (original === null) {
        throw new Error('no loose declaration');
      }
      for (let round = 0; round < 1000; round += 1) {
        expect(controller.duplicate(original).kind).toBe('pasted');
      }
      const after = session.session.currentDocument();
      const ids: string[] = [];
      const syms: string[] = [];
      forEachNode(after.modules[0]?.workspace.blocks ?? [], (node) => {
        ids.push(node.id);
        const name = node.fields?.['NAME'];
        if (typeof name === 'object' && 'sym' in name) {
          syms.push(name.sym);
        }
      });
      expect(new Set(ids).size).toBe(ids.length);
      expect(new Set(syms).size).toBe(syms.length);
      const fresh = ids.filter((id) => !before.has(id));
      expect(fresh).toHaveLength(1000);
      expect(fresh.every((id) => FRESH_BLOCK_ID.test(id))).toBe(true);
      expect(syms.filter((sym) => FRESH_SYMBOL_ID.test(sym))).toHaveLength(1000);
      expect(canonical(after)).toBeTruthy();
      await session.session.flush();
      // The only errors are the loose blocks' own ("not in a program", B2C-E0604).
      const errors = (useAppStore.getState().analysis.preview?.diagnostics ?? []).filter(
        (diagnostic) => diagnostic.severity === 'error',
      );
      expect(errors).toHaveLength(1001);
      expect(errors.every((diagnostic) => diagnostic.code === 'B2C-E0604')).toBe(true);
    } finally {
      session.dispose();
    }
  }, 180_000);
});
