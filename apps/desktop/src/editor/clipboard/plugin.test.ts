/**
 * The clipboard plugin as the user drives it, with the real compiler core on a rendered canvas:
 * the keys go through Blockly's own key handling and the webview's clipboard events (simulated as
 * an engine fires them after the key), the block and canvas menus, the `edit.*` commands from
 * outside the canvas, and detaching.
 *
 * Without a build of the core these tests are skipped, unless B2C_REQUIRE_WASM is set (as in CI).
 */
import type { BdmDocument, CoreWasm } from '@blocks2cpp/b2c-core-wasm';
import * as Blockly from 'blockly/core';
import { afterEach, beforeAll, describe, expect, it, vi } from 'vitest';

import { setEditorHandle } from '../../app/editor-types';
import { useAppStore } from '../../app/store';
import { disposeWorkspaces, renderedWorkspace } from '../sync/testing';
import { CLIPBOARD_MIME, PLAIN_TEXT_MIME } from './formats';
import { MENU_ITEM_IDS, SHORTCUT_NAMES } from './registry';
import {
  canvasBlock,
  clipboardEditor,
  type ClipboardTestEditor,
  FRESH_BLOCK_ID,
  listOf,
  loadGuessingGame,
  nodeOf,
  nodesById,
  requireCore,
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
  Reflect.deleteProperty(document, 'execCommand');
  document.body.replaceChildren();
});

function editorFor(doc: BdmDocument): ClipboardTestEditor {
  const editor = clipboardEditor(core, doc, renderedWorkspace());
  editors.push(editor);
  return editor;
}

/** Gives a block of the canvas the focus, as clicking it does. */
function focus(editor: ClipboardTestEditor, id: string): Blockly.BlockSvg {
  const block = canvasBlock(editor.workspace, id);
  Blockly.getFocusManager().focusNode(block);
  return block;
}

/** Presses Ctrl+`letter` where the focus is; returns the key event. */
function pressCtrl(letter: 'C' | 'X' | 'V' | 'D', repeat = false): KeyboardEvent {
  const event = new KeyboardEvent('keydown', {
    key: letter.toLowerCase(),
    keyCode: letter.charCodeAt(0),
    ctrlKey: true,
    repeat,
    bubbles: true,
    cancelable: true,
  });
  (document.activeElement ?? document.body).dispatchEvent(event);
  return event;
}

/** What an engine fires after a copy, cut or paste key it did not see cancelled. */
function clipboardEvent(type: 'copy' | 'cut' | 'paste', transfer = new DataTransfer()) {
  const event = new ClipboardEvent(type, {
    clipboardData: transfer,
    bubbles: true,
    cancelable: true,
  });
  (document.activeElement ?? document.body).dispatchEvent(event);
  return { event, transfer };
}

/** A paste event whose clipboard holds `payload` as the Blocks2Cpp type. */
function pasteEvent(payload: string) {
  const transfer = new DataTransfer();
  transfer.setData(CLIPBOARD_MIME, payload);
  return clipboardEvent('paste', transfer);
}

function body(editor: ClipboardTestEditor, id: string, list = 'BODY'): string[] {
  return listOf(nodeOf(editor.document(), id), list).map((node) => node.id);
}

async function nextTask(): Promise<void> {
  await new Promise((resolve) => setTimeout(resolve, 0));
}

/** A copy of `id` made by the core from `doc`, as another window would put it on the clipboard. */
function payloadOf(doc: BdmDocument, id: string): string {
  const made = core.clipboardMake(JSON.stringify(doc), [id]);
  if (!made.ok) {
    throw new Error('the copy failed');
  }
  return made.payload;
}

describe.skipIf(!WITH_CORE)('the keys', () => {
  it('Ctrl+C copies the focused block; the copy event gets its payload and C++', () => {
    const editor = editorFor(loadGuessingGame(core));
    focus(editor, 'b005');
    const key = pressCtrl('C');
    // The key is left to the webview, which fires the copy event.
    expect(key.defaultPrevented).toBe(false);
    const { event, transfer } = clipboardEvent('copy');
    expect(event.defaultPrevented).toBe(true);
    const payload = transfer.getData(CLIPBOARD_MIME);
    expect(JSON.parse(payload)).toMatchObject({
      format: 'blocks2cpp/clipboard',
      formatVersion: 1,
      blocks: [{ id: 'b005', type: 'io.ask' }],
      refs: { s_guess: { name: 'guess', kind: 'variable' } },
    });
    expect(transfer.getData(PLAIN_TEXT_MIME)).toContain('guess = b2c::ask<int>("Your guess: ");');
    expect(editor.memory.get()?.payload).toBe(payload);
    expect(editor.notices).toEqual([]);
  });

  it('Ctrl+X cuts the focused block into both clipboards', () => {
    const editor = editorFor(loadGuessingGame(core));
    focus(editor, 'b004');
    pressCtrl('X');
    const { transfer } = clipboardEvent('cut');
    expect(transfer.getData(PLAIN_TEXT_MIME)).toContain('Guess a number from 1 to 100!');
    expect(transfer.getData(CLIPBOARD_MIME)).toBe(editor.memory.get()?.payload);
    expect(body(editor, 'b011')).toEqual(['b002', 'b003', 'b010']);
  });

  it('Ctrl+V pastes the payload of the paste event, even from another project', () => {
    const editor = editorFor(loadGuessingGame(core));
    // Another project, where the variable has another ID: bound again by name.
    const other = loadGuessingGame(core, (text) => text.replaceAll('s_guess', 's_theirs'));
    const payload = payloadOf(other, 'b005');
    focus(editor, 'b009');
    pressCtrl('V');
    const { event } = pasteEvent(payload);
    expect(event.defaultPrevented).toBe(true);
    const loop = body(editor, 'b010');
    expect(loop).toHaveLength(3);
    const pasted = nodeOf(editor.document(), loop[2] ?? '');
    expect(pasted.id).toMatch(FRESH_BLOCK_ID);
    expect(pasted.fields?.['VAR']).toEqual({ ref: 's_guess' });
    expect(editor.notices).toEqual([]);
  });

  it('Ctrl+V pastes the in-app copy when the webview fires no paste event', async () => {
    const editor = editorFor(loadGuessingGame(core));
    editor.clipboard.controller.copy(canvasBlock(editor.workspace, 'b004'));
    focus(editor, 'b009');
    pressCtrl('V');
    expect(body(editor, 'b010')).toHaveLength(2);
    await nextTask();
    expect(body(editor, 'b010')).toHaveLength(3);
  });

  it('Ctrl+D duplicates the focused block once, even when the key is held', () => {
    const editor = editorFor(loadGuessingGame(core));
    focus(editor, 'b004');
    const key = pressCtrl('D');
    expect(key.defaultPrevented).toBe(true);
    const after = body(editor, 'b011');
    expect(after).toHaveLength(5);
    expect(after[3]).toMatch(FRESH_BLOCK_ID);
    // The copy has the focus now; a held key does not duplicate again.
    pressCtrl('D', true);
    pressCtrl('C', true);
    pressCtrl('V', true);
    expect(body(editor, 'b011')).toHaveLength(5);
    expect(editor.memory.get()).toBeNull();
  });

  it('do nothing without a focused block, and leave the key to the webview', () => {
    const editor = editorFor(loadGuessingGame(core));
    // The canvas itself has the focus.
    Blockly.getFocusManager().focusNode(editor.workspace);
    expect(Blockly.getFocusManager().getFocusedNode()).toBe(editor.workspace);
    const key = pressCtrl('C');
    expect(key.defaultPrevented).toBe(false);
    expect(clipboardEvent('copy').event.defaultPrevented).toBe(false);
    // Another part of the window has it.
    const button = document.createElement('button');
    document.body.append(button);
    button.focus();
    expect(pressCtrl('C').defaultPrevented).toBe(false);
    expect(clipboardEvent('copy').event.defaultPrevented).toBe(false);
    expect(editor.memory.get()).toBeNull();
  });
});

describe.skipIf(!WITH_CORE)('untrusted clipboard data', () => {
  it('is refused with the loader’s codes and changes nothing', () => {
    const editor = editorFor(loadGuessingGame(core));
    const before = JSON.stringify(editor.document());
    focus(editor, 'b009');
    pressCtrl('V');
    const valid = payloadOf(editor.document(), 'b004');
    pasteEvent(valid.replace('"catalog": "1.0.0",', '"catalog": "1.0.0", "catalog": "9.9.9",'));
    expect(JSON.stringify(editor.document())).toBe(before);
    expect(editor.notices).toHaveLength(1);
    const notice = editor.notices[0];
    expect(notice?.kind === 'refused' ? notice.diagnostics.map((d) => d.code) : []).toEqual([
      'B2C-E0105',
    ]);
  });

  it('pasted by the webview’s own menu at the canvas is checked the same way', () => {
    const editor = editorFor(loadGuessingGame(core));
    const before = JSON.stringify(editor.document());
    focus(editor, 'b009');
    pasteEvent('{"format": "blocks2cpp/project", "formatVersion": 1}');
    expect(JSON.stringify(editor.document())).toBe(before);
    const notice = editor.notices[0];
    expect(notice?.kind === 'refused' ? notice.diagnostics.map((d) => d.code) : []).toEqual([
      'B2C-E0138',
    ]);
  });
});

describe.skipIf(!WITH_CORE)('the menus', () => {
  /** Makes `document.execCommand('copy')` fire a copy event, as a webview does in a click. */
  function captureMenuCopy(): DataTransfer {
    const transfer = new DataTransfer();
    Object.defineProperty(document, 'execCommand', {
      configurable: true,
      value: (command: string) => {
        if (command === 'copy') {
          document.body.dispatchEvent(
            new ClipboardEvent('copy', {
              clipboardData: transfer,
              bubbles: true,
              cancelable: true,
            }),
          );
        }
        return command === 'copy';
      },
    });
    return transfer;
  }

  function run(id: string, scope: Blockly.ContextMenuRegistry.Scope): void {
    const item = Blockly.ContextMenuRegistry.registry.getItem(id);
    if (item === null || item.separator === true) {
      throw new Error(`no menu item ${id}`);
    }
    expect(item.preconditionFn(scope, new Event('contextmenu'))).toBe('enabled');
    item.callback(
      scope,
      new Event('contextmenu'),
      new Event('click'),
      new Blockly.utils.Coordinate(0, 0),
    );
  }

  it('Copy puts the block on both clipboards and Paste on a block inserts after it', () => {
    const editor = editorFor(loadGuessingGame(core));
    const transfer = captureMenuCopy();
    run(MENU_ITEM_IDS.copy, { block: canvasBlock(editor.workspace, 'b006') });
    expect(transfer.getData(CLIPBOARD_MIME)).toBe(editor.memory.get()?.payload);
    expect(transfer.getData(PLAIN_TEXT_MIME)).toContain('Too low!');
    run(MENU_ITEM_IDS.pasteOnBlock, { block: canvasBlock(editor.workspace, 'b007') });
    const higher = body(editor, 'b009', 'DO1');
    expect(higher).toHaveLength(2);
    expect(higher[1]).toMatch(FRESH_BLOCK_ID);
  });

  it('Cut removes the block and Paste on the canvas puts it back loose', () => {
    const editor = editorFor(loadGuessingGame(core));
    captureMenuCopy();
    const tops = editor.workspace.getTopBlocks(false).length;
    run(MENU_ITEM_IDS.cut, { block: canvasBlock(editor.workspace, 'b004') });
    expect(nodesById(editor.document()).has('b004')).toBe(false);
    run(MENU_ITEM_IDS.pasteOnCanvas, { workspace: editor.workspace });
    expect(editor.workspace.getTopBlocks(false)).toHaveLength(tops + 1);
  });

  it('Duplicate puts the copy after the block and leaves the clipboards alone', () => {
    const editor = editorFor(loadGuessingGame(core));
    const copy = vi.fn();
    document.addEventListener('copy', copy);
    run(MENU_ITEM_IDS.duplicate, { block: canvasBlock(editor.workspace, 'b008') });
    document.removeEventListener('copy', copy);
    expect(body(editor, 'b009', 'ELSE')).toHaveLength(2);
    expect(copy).not.toHaveBeenCalled();
    expect(editor.memory.get()).toBeNull();
  });
});

describe.skipIf(!WITH_CORE)('the edit commands', () => {
  it('copy, cut and paste the block selected last when the focus is elsewhere', async () => {
    const editor = editorFor(loadGuessingGame(core));
    const button = document.createElement('button');
    document.body.append(button);
    Blockly.common.setSelected(canvasBlock(editor.workspace, 'b006'));
    await settle();
    button.focus();

    await editor.commands.runCommand('edit.copy');
    expect(editor.memory.get()?.text).toContain('Too low!');
    await editor.commands.runCommand('edit.paste');
    expect(body(editor, 'b009', 'DO0')).toHaveLength(2);

    Blockly.common.setSelected(canvasBlock(editor.workspace, 'b007'));
    await settle();
    button.focus();
    await editor.commands.runCommand('edit.cut');
    expect(body(editor, 'b009', 'DO1')).toEqual([]);
    expect(editor.memory.get()?.text).toContain('Too high!');
  });

  it('forget the block selected last when another project is opened', async () => {
    const editor = editorFor(loadGuessingGame(core));
    const button = document.createElement('button');
    document.body.append(button);
    Blockly.common.setSelected(canvasBlock(editor.workspace, 'b006'));
    await settle();
    button.focus();
    const project = useAppStore.getState().project;
    if (project === null) {
      throw new Error('no project is open');
    }
    // Another project whose blocks happen to have the same IDs.
    useAppStore
      .getState()
      .actions.setProject({ ...project, handle: 'ph_ffffffffffffffffffffffffffffffff' });
    await editor.commands.runCommand('edit.copy');
    await editor.commands.runCommand('edit.cut');
    expect(editor.memory.get()).toBeNull();
    expect(body(editor, 'b009', 'DO0')).toEqual(['b006']);
  });

  it('paste on the canvas when the canvas itself has the focus', async () => {
    const editor = editorFor(loadGuessingGame(core));
    editor.clipboard.controller.copy(canvasBlock(editor.workspace, 'b004'));
    const tops = editor.workspace.getTopBlocks(false).length;
    Blockly.getFocusManager().focusNode(editor.workspace);
    await editor.commands.runCommand('edit.paste');
    expect(editor.workspace.getTopBlocks(false)).toHaveLength(tops + 1);
    expect(body(editor, 'b011')).toHaveLength(4);
  });
});

describe.skipIf(!WITH_CORE)('detaching', () => {
  it('removes the commands, the DOM listeners and the replaced keys', () => {
    const editor = editorFor(loadGuessingGame(core));
    expect(editor.commands.hasCommand('edit.copy')).toBe(true);
    const ours = Blockly.ShortcutRegistry.registry.getRegistry()[SHORTCUT_NAMES.copy]?.callback;
    editors.splice(editors.indexOf(editor), 1);
    focus(editor, 'b004');
    editor.dispose();
    expect(editor.commands.hasCommand('edit.copy')).toBe(false);
    expect(editor.commands.hasCommand('edit.paste')).toBe(false);
    expect(Blockly.ShortcutRegistry.registry.getRegistry()[SHORTCUT_NAMES.copy]?.callback).not.toBe(
      ours,
    );
    expect(
      Blockly.ShortcutRegistry.registry.getRegistry()[SHORTCUT_NAMES.duplicate],
    ).toBeUndefined();
    const { event } = clipboardEvent('copy');
    expect(event.defaultPrevented).toBe(false);
  });
});
