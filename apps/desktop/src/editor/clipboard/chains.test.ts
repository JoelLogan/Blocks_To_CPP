/**
 * Long statement chains with the real compiler core (./chains.ts). Blockly serialises a chain
 * recursively for the events that make undo work, so a long enough one overflows the JavaScript
 * stack. A paste, duplicate or cut of such a chain must be refused whole, with a notice: nothing
 * may change on the canvas, in the document or on the undo stack, and a refused cut keeps no copy.
 * Shorter chains still paste as one undo step: there is no fixed length limit.
 *
 * Headless workspaces keep these tests fast; the rendered path is the same code (controller.test.ts
 * simulates the failure on blocks of both kinds).
 *
 * Without a build of the core these tests are skipped, unless B2C_REQUIRE_WASM is set (as in CI).
 */
import type { BdmBlock, BdmDocument, CoreWasm } from '@blocks2cpp/b2c-core-wasm';
import * as Blockly from 'blockly/core';
import { afterEach, beforeAll, describe, expect, it, vi } from 'vitest';

import { setEditorHandle } from '../../app/editor-types';
import {
  disposeWorkspaces,
  present,
  renderedWorkspace,
  startSession,
  type TestSession,
} from '../sync/testing';
import { anchorForBlock, ON_CANVAS } from './anchor';
import { ClipboardMemory } from './memory';
import type { ClipboardNotice } from './notices';
import {
  clipboardEditor,
  type ClipboardTestEditor,
  loadGuessingGame,
  requireCore,
  sessionController,
  settle,
  WITH_CORE,
} from './testing';

/**
 * Statements in the long chains: far more than V8's default stack lets Blockly serialise (it runs
 * out at about 5,000 here), well within the limits of a project file (05 §5.6).
 */
const LONG = 12_000;

/** A chain Blockly serialises without trouble. */
const SHORT = 300;

/** Long enough for the setup and the walks of {@link LONG} blocks. */
const TIMEOUT_MS = 120_000;

let core: CoreWasm;
let session: TestSession | null = null;
let editor: ClipboardTestEditor | null = null;

beforeAll(async () => {
  if (WITH_CORE) {
    core = await requireCore();
  }
});

afterEach(() => {
  session?.dispose();
  session = null;
  editor?.dispose();
  editor = null;
  setEditorHandle(null);
  disposeWorkspaces();
});

/** A print statement with its own ID. */
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

/** `count` print statements, IDs from `prefix`. */
function prints(prefix: string, count: number): BdmBlock[] {
  return Array.from({ length: count }, (_unused, index) => print(`${prefix}${String(index)}`));
}

/** A loose stack of `length` statements: a print with `length - 1` stacked below it. */
function looseStack(id: string, length: number): BdmBlock {
  return { ...print(id), stack: prints(`${id}_`, length - 1) };
}

/** A clipboard payload holding `blocks`. */
function payload(blocks: readonly BdmBlock[]): string {
  return JSON.stringify({
    format: 'blocks2cpp/clipboard',
    formatVersion: 1,
    catalog: '1.0.0',
    blocks,
    refs: {},
  });
}

/** The guessing game, changed by `edit`. */
function guessingGameWith(edit: (doc: BdmDocument) => void): BdmDocument {
  const doc = loadGuessingGame(core);
  edit(doc);
  return doc;
}

/**
 * A session on `doc` with a controller whose notices and in-app copy the test sees, once the
 * events of loading (labels the session refreshes) have reached the undo stack.
 */
async function open(doc: BdmDocument) {
  const opened = startSession(core, doc);
  session = opened;
  const notices: ClipboardNotice[] = [];
  const memory = new ClipboardMemory();
  const controller = sessionController(opened, { notices, memory });
  await settle();
  return { session: opened, notices, memory, controller };
}

/** What must not change: the document, every block on the canvas, and the undo stack. */
function state(opened: TestSession) {
  return {
    document: JSON.stringify(opened.session.currentDocument()),
    blocks: opened.workspace.getAllBlocks(false).length,
    top: opened.workspace
      .getTopBlocks(false)
      .map((top) => top.id)
      .sort(),
    undo: opened.workspace.getUndoStack().length,
  };
}

/** The one notice: a limit of the editor, naming a chain of `length` statements. */
function chainNotice(action: ClipboardNotice['action'], length: number): unknown {
  return [
    {
      kind: 'limits',
      action,
      diagnostics: [
        expect.objectContaining({
          code: 'B2C-E0104',
          message: expect.stringContaining(`A chain of ${String(length)} statements`) as unknown,
        }),
      ],
    },
  ];
}

describe.skipIf(!WITH_CORE)('a statement chain too long for Blockly', () => {
  it(
    'is not pasted, on the canvas or into a list, and nothing changes',
    async () => {
      vi.spyOn(console, 'warn').mockImplementation(() => undefined);
      const { session: opened, notices, controller } = await open(loadGuessingGame(core));
      const before = state(opened);
      const text = payload([looseStack('long', LONG)]);

      expect(controller.paste(text, ON_CANVAS).kind).toBe('refused');
      expect(notices).toEqual(chainNotice('paste', LONG));
      // After a block, the stacked blocks follow it as one run in the list.
      const after = anchorForBlock(present(opened.workspace.getBlockById('b009'), 'b009'));
      expect(controller.paste(text, after).kind).toBe('refused');
      expect(notices).toHaveLength(2);

      await settle();
      expect(state(opened)).toEqual(before);
    },
    TIMEOUT_MS,
  );

  it(
    'is not duplicated or cut, the old copy stays, and nothing changes',
    async () => {
      vi.spyOn(console, 'warn').mockImplementation(() => undefined);
      const doc = guessingGameWith((edited) => {
        const module = present(edited.modules[0], 'the module');
        module.workspace.blocks.push({ ...looseStack('long', LONG), x: 900, y: 40 });
        // The loop of main gets a long body too.
        const main = present(module.workspace.blocks[0], 'main');
        const body = present(main.statements?.['BODY'], 'the body of main');
        const loop = present(
          body.find((node) => node.id === 'b010'),
          'the loop b010',
        );
        loop.statements = { ...loop.statements, BODY: prints('inner', LONG) };
      });
      const { session: opened, notices, memory, controller } = await open(doc);
      memory.set({ payload: 'OLD', text: null });
      const before = state(opened);
      const head = present(opened.workspace.getBlockById('long'), 'the loose stack');
      const loop = present(opened.workspace.getBlockById('b010'), 'the loop');

      expect(controller.duplicate(head).kind).toBe('refused');
      expect(notices).toEqual(chainNotice('duplicate', LONG));
      notices.length = 0;

      // A top-level block: its delete event would serialise the whole stack.
      expect(controller.cut(head)).toBeNull();
      expect(notices).toEqual(chainNotice('cut', LONG));
      expect(head.isDeadOrDying()).toBe(false);
      notices.length = 0;

      // A block in a list: its delete event would serialise the loop's body.
      expect(controller.cut(loop)).toBeNull();
      expect(notices).toEqual(chainNotice('cut', LONG));
      expect(loop.isDeadOrDying()).toBe(false);
      expect(loop.getPreviousBlock()?.id).toBe('b004');

      expect(memory.get()).toEqual({ payload: 'OLD', text: null });
      await settle();
      expect(state(opened)).toEqual(before);
    },
    TIMEOUT_MS,
  );
});

describe.skipIf(!WITH_CORE)('on a rendered canvas', () => {
  it('a paste, duplicate or cut whose events run out of stack changes nothing', async () => {
    vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    const doc = guessingGameWith((edited) => {
      const module = present(edited.modules[0], 'the module');
      module.workspace.blocks.push({ ...looseStack('loose', 3), x: 900, y: 40 });
    });
    const opened = clipboardEditor(core, doc, renderedWorkspace());
    editor = opened;
    await settle();
    const before = state(opened.session);
    const made = core.clipboardMake(JSON.stringify(opened.document()), ['b010']);
    if (!made.ok) {
      throw new Error('the copy failed');
    }
    const overflow = function (): never {
      throw new RangeError('Maximum call stack size exceeded');
    };

    // Rendered blocks whose create events cannot be made are removed again.
    const create = vi.spyOn(Blockly.Events, 'BlockCreate').mockImplementation(overflow);
    const controller = opened.clipboard.controller;
    expect(controller.paste(made.payload, ON_CANVAS).kind).toBe('refused');
    const loose = present(opened.workspace.getBlockById('loose'), 'the loose stack');
    expect(controller.duplicate(loose).kind).toBe('refused');
    create.mockRestore();

    // A rendered top-level block whose delete event cannot be made stays.
    vi.spyOn(Blockly.Events, 'BlockDelete').mockImplementation(overflow);
    expect(controller.cut(loose)).toBeNull();
    expect(loose.isDeadOrDying()).toBe(false);
    // So does a block in a list whose blocks cannot be serialised.
    vi.spyOn(Blockly.serialization.blocks, 'save').mockImplementation(overflow);
    const loop = present(opened.workspace.getBlockById('b010'), 'the loop');
    expect(controller.cut(loop)).toBeNull();
    expect(loop.getPreviousBlock()?.id).toBe('b004');

    expect(opened.notices.map((notice) => `${notice.action} ${notice.kind}`)).toEqual([
      'paste limits',
      'duplicate limits',
      'cut limits',
      'cut limits',
    ]);
    expect(opened.memory.get()).toBeNull();
    await settle();
    expect(state(opened.session)).toEqual(before);
  });
});

describe.skipIf(!WITH_CORE)('a statement chain Blockly can serialise', () => {
  it('is pasted and cut as one undo step each', async () => {
    const { session: opened, notices, controller } = await open(loadGuessingGame(core));
    const before = state(opened);

    const pasted = controller.paste(payload([looseStack('short', SHORT)]), ON_CANVAS);
    expect(pasted.kind).toBe('pasted');
    expect(notices).toEqual([]);
    expect(opened.workspace.getTopBlocks(false)).toHaveLength(before.top.length + 1);
    await settle();
    expect(opened.workspace.getUndoStack()).toHaveLength(before.undo + 1);

    const head = present(pasted.kind === 'pasted' ? pasted.blocks[0] : null, 'the pasted stack');
    expect(controller.cut(head)).not.toBeNull();
    expect(opened.workspace.getTopBlocks(false)).toHaveLength(before.top.length);
    await settle();
    expect(opened.workspace.getUndoStack()).toHaveLength(before.undo + 2);

    opened.workspace.undo(false);
    opened.workspace.undo(false);
    await settle();
    expect(JSON.stringify(opened.session.currentDocument())).toBe(before.document);
  });
});
