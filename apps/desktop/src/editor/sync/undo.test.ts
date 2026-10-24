/**
 * Undo in the editor with the real blocks, services and compiler core (03 §3.6, 05 §5.2): opening
 * a project leaves nothing to undo, one Ctrl+Z undoes one edit completely, and undoing a delete
 * brings the block back exactly as the file had it.
 *
 * Undo and Redo pressed right after a change, before Blockly has delivered (and recorded) it, act
 * once it has (./undo.ts), and the change a listener made in its event group as it heard of it:
 * through Blockly's keys and canvas menu and through the session, one step each, in the order they
 * were asked for. Blockly's own items are back once the last session ends.
 *
 * Real timers: Blockly delivers (and records) its events after an animation frame and a task.
 */
import type { BdmBlock, BdmDocument } from '@blocks2cpp/b2c-core-wasm';
import { B2cSymbolDeclField, readExprShadow, type TokenJson } from '@blocks2cpp/blockly-ext';
import * as Blockly from 'blockly/core';
import { afterEach, describe, expect, it, vi } from 'vitest';

import { useAppStore } from '../../app/store';
import { documentFixture } from '../../app/testing/fixtures';
import { installStartValueReshaping } from '../toolbox/reshape';
import {
  canonicalOf,
  disposeWorkspaces,
  EXAMPLE_PROJECTS,
  loadText,
  present,
  renderedWorkspace,
  startSession,
  testCore,
  type TestSession,
  undosMade,
} from './testing';
import { installUndoItems, UNDO_MENU_ITEM_IDS } from './undo';

const core = await testCore();

let running: TestSession | null = null;

afterEach(() => {
  running?.dispose();
  running = null;
  disposeWorkspaces();
});

/** Waits until Blockly has delivered (and recorded) the events fired so far. */
function eventsDelivered(): Promise<void> {
  return new Promise((resolve) => {
    requestAnimationFrame(() => {
      setTimeout(() => {
        setTimeout(resolve, 0);
      }, 0);
    });
  });
}

/** Lets Blockly deliver its events and the session preview what they changed. */
async function settle(session: TestSession): Promise<void> {
  await eventsDelivered();
  await session.session.flush();
  await eventsDelivered();
}

/** The text of every read-only expression on the canvas, in canvas order. */
function labels(workspace: Blockly.Workspace): string[] {
  return workspace.getAllBlocks(true).flatMap((block) => {
    const field = block.getField('TEXT_BEFORE');
    return field === null ? [] : [String(field.getValue())];
  });
}

/** The declaration field of symbol `sym` on the canvas. */
function declField(workspace: Blockly.Workspace, sym: string): B2cSymbolDeclField {
  for (const block of workspace.getAllBlocks(false)) {
    for (const input of block.inputList) {
      for (const field of input.fieldRow) {
        if (field instanceof B2cSymbolDeclField && field.getSymbolId() === sym) {
          return field;
        }
      }
    }
  }
  throw new Error(`no declaration of ${sym}`);
}

function dirty(): boolean | undefined {
  return useAppStore.getState().project?.dirty;
}

/** The hash of the canonical text of the document the canvas was last read into. */
function liveTextHash(): string | undefined {
  return useAppStore.getState().project?.textHash;
}

describe.skipIf(core === null)('undo', () => {
  it('has nothing to undo after a project opens; one Ctrl+Z undoes a rename and its labels', async () => {
    const wasm = present(core, 'core');
    const doc = loadText(wasm, present(EXAMPLE_PROJECTS['guessing_game.b2c'], 'guessing game'));
    running = startSession(wasm, doc, { workspace: renderedWorkspace() });
    const { workspace } = running;
    await settle(running);
    const before = ['guess == secret', 'guess < secret', 'guess > secret'];
    expect(labels(workspace)).toEqual(before);
    // Relabelling the read-only expressions when the project opened is not an edit.
    expect(workspace.getUndoStack()).toHaveLength(0);

    declField(workspace, 's_guess').setDecl({ sym: 's_guess', name: 'attempt' });
    await settle(running);
    expect(labels(workspace)).toEqual([
      'attempt == secret',
      'attempt < secret',
      'attempt > secret',
    ]);
    expect(workspace.getUndoStack()).toHaveLength(1);
    expect(dirty()).toBe(true);

    workspace.undo(false);
    await settle(running);
    expect(declField(workspace, 's_guess').getDecl()?.name).toBe('guess');
    expect(labels(workspace)).toEqual(before);
    expect(workspace.getUndoStack()).toHaveLength(0);
    expect(dirty()).toBe(false);
  });

  it.each([
    ['a block that left out its default fields and extra', 'p'],
    ['a block that wrote an empty statement list', 'f'],
  ])('brings back %s exactly when a delete is undone', async (_what, id) => {
    const wasm = present(core, 'core');
    const print: BdmBlock = {
      id: 'p',
      type: 'io.print',
      v: 1,
      inputs: { ITEM0: { expr: [{ str: 'hi' }] } },
    };
    const loop: BdmBlock = { id: 'f', type: 'control.forever', v: 1, statements: { BODY: [] } };
    const sparse: BdmDocument = documentFixture('Sparse');
    sparse.modules = [
      {
        id: 'mod_main',
        name: 'main',
        workspace: {
          blocks: [
            {
              id: 'main',
              type: 'program.main',
              v: 1,
              x: 0,
              y: 0,
              statements: { BODY: [print, loop] },
            },
          ],
        },
      },
    ];
    const doc = loadText(wasm, JSON.stringify(sparse));
    const saved = canonicalOf(wasm, doc).textHash;
    running = startSession(wasm, doc, { workspace: renderedWorkspace() });
    const { workspace } = running;
    await settle(running);
    expect(dirty()).toBe(false);

    // As the Delete key does: one undoable group, the blocks below move up.
    Blockly.Events.setGroup(true);
    present(workspace.getBlockById(id), id).dispose(true);
    Blockly.Events.setGroup(false);
    await settle(running);
    expect(dirty()).toBe(true);

    workspace.undo(false);
    await settle(running);
    expect(dirty()).toBe(false);
    expect(liveTextHash()).toBe(saved);

    // Redo and undo again: the block keeps its file's shape every time it comes back.
    workspace.undo(true);
    await settle(running);
    workspace.undo(false);
    await settle(running);
    expect(dirty()).toBe(false);
    expect(liveTextHash()).toBe(saved);
  });
});

/** The prints of the guessing game whose line break the tests below turn off, one per change. */
const PRINTS = ['b004', 'b006', 'b007'] as const;

/** The guessing game on a rendered canvas made with `options`, settled: nothing to undo yet. */
async function openGame(options: Blockly.BlocklyOptions = {}): Promise<Blockly.WorkspaceSvg> {
  const wasm = present(core, 'core');
  const doc = loadText(wasm, present(EXAMPLE_PROJECTS['guessing_game.b2c'], 'guessing game'));
  const workspace = renderedWorkspace(options);
  running = startSession(wasm, doc, { workspace });
  await settle(running);
  expect(workspace.getUndoStack()).toHaveLength(0);
  return workspace;
}

/** One change: print `id` no longer ends with a line break. */
function dropNewline(workspace: Blockly.Workspace, id: string): void {
  present(workspace.getBlockById(id), id).setFieldValue('FALSE', 'NEWLINE');
}

/** Whether each of {@link PRINTS} still ends with a line break. */
function newlines(workspace: Blockly.Workspace): boolean[] {
  return PRINTS.map(
    (id) => present(workspace.getBlockById(id), id).getFieldValue('NEWLINE') === 'TRUE',
  );
}

/** The tokens of `block`'s start value, or `null` when its value input holds no expression shadow. */
function startValue(block: Blockly.Block): readonly TokenJson[] | null {
  const shadow = block.getInputTargetBlock('VALUE');
  return shadow === null ? null : (readExprShadow(shadow)?.tokens ?? null);
}

/** Presses Ctrl (and Shift) and the key `keyCode` on the canvas; Blockly matches it by `keyCode`. */
function pressCtrl(workspace: Blockly.WorkspaceSvg, keyCode: number, shiftKey = false): void {
  workspace.getInjectionDiv().dispatchEvent(
    new KeyboardEvent('keydown', {
      keyCode,
      ctrlKey: true,
      shiftKey,
      bubbles: true,
      cancelable: true,
    }),
  );
}

/** Chooses the canvas menu's item `id`, as a click on it does. */
function choose(workspace: Blockly.WorkspaceSvg, id: string): void {
  const item = Blockly.ContextMenuRegistry.registry.getItem(id);
  if (item === null || item.separator === true) {
    throw new Error(`no menu item ${id}`);
  }
  item.callback(
    { workspace, focusedNode: workspace },
    new Event('contextmenu'),
    new Event('click'),
    new Blockly.utils.Coordinate(0, 0),
  );
}

describe.skipIf(core === null)('Undo and Redo right after a change', () => {
  const { KeyCodes } = Blockly.utils;

  it('undo the change and the ones before it, in the order the keys were pressed', async () => {
    const workspace = await openGame();
    dropNewline(workspace, 'b004');
    await settle(present(running, 'session'));
    dropNewline(workspace, 'b006');
    await settle(present(running, 'session'));
    // Changed, and three Ctrl+Z (a held key) before Blockly has delivered the change.
    dropNewline(workspace, 'b007');
    pressCtrl(workspace, KeyCodes.Z);
    pressCtrl(workspace, KeyCodes.Z);
    pressCtrl(workspace, KeyCodes.Z);
    await undosMade();
    expect(newlines(workspace)).toEqual([true, true, true]);

    // Undone last to first, so redo puts the first change back first: Ctrl+Y redoes it,
    // Ctrl+Shift+Z the second, and Ctrl+Z, pressed in the same go, takes the second back again.
    pressCtrl(workspace, KeyCodes.Y);
    pressCtrl(workspace, KeyCodes.Z, true);
    pressCtrl(workspace, KeyCodes.Z);
    await undosMade();
    expect(newlines(workspace)).toEqual([false, true, true]);
  });

  it('record a change made between two presses before the second acts', async () => {
    const workspace = await openGame();
    dropNewline(workspace, 'b004');
    await settle(present(running, 'session'));
    dropNewline(workspace, 'b006');
    await settle(present(running, 'session'));

    pressCtrl(workspace, KeyCodes.Z);
    dropNewline(workspace, 'b007');
    pressCtrl(workspace, KeyCodes.Z);
    await undosMade();
    // The first undid the second change, the second the change made in between.
    expect(newlines(workspace)).toEqual([false, true, true]);
    expect(workspace.getRedoStack()).toHaveLength(1);
  });

  it('undo a change together with the change a listener made as it heard of it', async () => {
    // A canvas that scrolls, as the editor's: on one that does not, Blockly moves blocks back
    // inside its edges after a change, a step of its own that Ctrl+Z would undo first.
    const workspace = await openGame({ move: { scrollbars: true, drag: true, wheel: true } });
    const stop = installStartValueReshaping(workspace, () => null);
    const guess = present(workspace.getBlockById('b003'), 'b003');
    // `create int guess = 0` becomes a `bool`, and its start value `true` once the listener hears
    // of it: a frame after the type, in the type's event group.
    guess.setFieldValue('bool', 'TYPE');
    pressCtrl(workspace, KeyCodes.Z);
    await undosMade();
    expect(guess.getFieldValue('TYPE')).toBe('int');
    expect(startValue(guess)).toEqual([{ num: '0' }]);

    pressCtrl(workspace, KeyCodes.Y);
    await undosMade();
    expect(guess.getFieldValue('TYPE')).toBe('bool');
    expect(startValue(guess)).toEqual([{ kw: 'true' }]);
    stop();
  });

  it('wait for the change through the session (the window’s keys, ≡ menu and banner)', async () => {
    const workspace = await openGame();
    const { session } = present(running, 'session');
    dropNewline(workspace, 'b004');
    session.undo(false);
    await undosMade();
    expect(newlines(workspace)).toEqual([true, true, true]);

    session.undo(true);
    await undosMade();
    expect(newlines(workspace)).toEqual([false, true, true]);
  });

  it('wait for the change through the canvas menu', async () => {
    const workspace = await openGame();
    dropNewline(workspace, 'b004');
    choose(workspace, UNDO_MENU_ITEM_IDS.undo);
    await undosMade();
    expect(newlines(workspace)).toEqual([true, true, true]);

    choose(workspace, UNDO_MENU_ITEM_IDS.redo);
    await undosMade();
    expect(newlines(workspace)).toEqual([false, true, true]);
  });

  it('still act after one that failed', async () => {
    const workspace = await openGame();
    dropNewline(workspace, 'b004');
    await settle(present(running, 'session'));
    dropNewline(workspace, 'b006');
    await settle(present(running, 'session'));
    const failure = new Error('an undo that fails');
    vi.spyOn(workspace, 'undo').mockImplementationOnce(() => {
      throw failure;
    });
    const logged = vi.spyOn(console, 'error').mockImplementation(() => undefined);

    pressCtrl(workspace, KeyCodes.Z);
    pressCtrl(workspace, KeyCodes.Z);
    await undosMade();
    expect(logged).toHaveBeenCalledWith('An undo failed', failure);
    expect(newlines(workspace)).toEqual([false, true, true]);
  });

  it('do nothing on a canvas disposed before they act', async () => {
    const workspace = await openGame();
    const undo = vi.spyOn(workspace, 'undo');
    dropNewline(workspace, 'b004');
    pressCtrl(workspace, KeyCodes.Z);
    running?.dispose();
    running = null;
    disposeWorkspaces();
    await eventsDelivered();
    expect(undo).not.toHaveBeenCalled();
  });
});

describe('Blockly’s undo items', () => {
  const shortcuts = Blockly.ShortcutRegistry.registry;
  const menu = Blockly.ContextMenuRegistry.registry;
  const { UNDO, REDO } = Blockly.ShortcutItems.names;

  it('are Blockly’s own again once the last install is removed', () => {
    const own = {
      undo: shortcuts.getRegistry()[UNDO]?.callback,
      redo: shortcuts.getRegistry()[REDO]?.callback,
      keys: shortcuts.getKeyCodesByShortcutName(REDO),
      undoItem: menu.getItem(UNDO_MENU_ITEM_IDS.undo),
      redoItem: menu.getItem(UNDO_MENU_ITEM_IDS.redo),
    };
    const first = installUndoItems();
    const second = installUndoItems();
    expect(shortcuts.getRegistry()[UNDO]?.callback).not.toBe(own.undo);
    expect(shortcuts.getKeyCodesByShortcutName(REDO)).toEqual(own.keys);
    expect(menu.getItem(UNDO_MENU_ITEM_IDS.redo)).not.toBe(own.redoItem);

    first();
    first();
    expect(shortcuts.getRegistry()[REDO]?.callback).not.toBe(own.redo);
    second();
    expect(shortcuts.getRegistry()[UNDO]?.callback).toBe(own.undo);
    expect(shortcuts.getRegistry()[REDO]?.callback).toBe(own.redo);
    expect(shortcuts.getKeyCodesByShortcutName(REDO)).toEqual(own.keys);
    expect(menu.getItem(UNDO_MENU_ITEM_IDS.undo)).toBe(own.undoItem);
    expect(menu.getItem(UNDO_MENU_ITEM_IDS.redo)).toBe(own.redoItem);
  });
});
