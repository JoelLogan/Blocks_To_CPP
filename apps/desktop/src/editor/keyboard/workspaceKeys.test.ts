/**
 * The workspace keys of 04 §4.7 with the keyboard focus on blocks, in the editor as the app builds
 * it (the editing session, the real compiler core, the clipboard plugin and the keyboard plugin):
 * Delete, Ctrl+Z and Ctrl+Y, Ctrl+C and Ctrl+V through the validated clipboard, and a keyboard
 * move that reaches the document.
 */
import { b2cLightTheme } from '@blocks2cpp/blockly-ext';
import * as Blockly from 'blockly/core';
import { afterEach, describe, expect, it } from 'vitest';

import { editorInjectOptions } from '../EditorWorkspace';
import {
  type ClipboardTestEditor,
  clipboardEditor,
  contextFor,
  loadGuessingGame,
  requireCore,
  settle,
  WITH_CORE,
} from '../clipboard/testing';
import { disposeWorkspaces, renderedWorkspace } from '../sync/testing';
import { registerToolboxComponents } from '../toolbox/register';
import { createKeyboardPlugin } from './plugin';
import { eventsDelivered, press } from './testing';

let editor: ClipboardTestEditor | null = null;
let detachKeyboard: (() => void) | null = null;

afterEach(() => {
  detachKeyboard?.();
  detachKeyboard = null;
  editor?.dispose();
  editor = null;
  disposeWorkspaces();
});

/** The guessing game in the app's editor, with the clipboard and keyboard plugins attached. */
async function openGame(): Promise<ClipboardTestEditor> {
  const core = await requireCore();
  registerToolboxComponents();
  const workspace = renderedWorkspace(editorInjectOptions(b2cLightTheme));
  editor = clipboardEditor(core, loadGuessingGame(core), workspace);
  detachKeyboard = createKeyboardPlugin({ matchMedia: null }).attach(
    contextFor(editor.session, workspace),
  );
  return editor;
}

/** The program block's statements, top to bottom (their block types). */
function bodyTypes(workspace: Blockly.Workspace): string[] {
  const main = workspace.getTopBlocks(true).find((block) => block.type === 'program.main');
  const types: string[] = [];
  for (let next = main?.getInputTargetBlock('BODY') ?? null; next !== null;) {
    types.push(next.type);
    next = next.getNextBlock();
  }
  return types;
}

/** The program block's statement at `index`. */
function statementAt(workspace: Blockly.Workspace, index: number): Blockly.BlockSvg {
  const main = workspace.getTopBlocks(true).find((block) => block.type === 'program.main');
  let block = main?.getInputTargetBlock('BODY') ?? null;
  for (let step = 0; step < index && block !== null; step++) {
    block = block.getNextBlock();
  }
  if (!(block instanceof Blockly.BlockSvg)) {
    throw new Error(`no statement ${String(index)}`);
  }
  return block;
}

function focus(node: Blockly.IFocusableNode): void {
  Blockly.getFocusManager().focusNode(node);
}

describe.skipIf(!WITH_CORE)('the workspace keys with the keyboard focus on a block', () => {
  it('Delete removes the block, Ctrl+Z brings it back and Ctrl+Y removes it again', async () => {
    const { workspace } = await openGame();
    const before = bodyTypes(workspace);
    const second = statementAt(workspace, 1);
    focus(second);

    press('Delete');
    await eventsDelivered();
    expect(second.isDeadOrDying()).toBe(true);
    expect(bodyTypes(workspace)).toHaveLength(before.length - 1);

    focus(statementAt(workspace, 0));
    press('z', { ctrlKey: true });
    await eventsDelivered();
    expect(bodyTypes(workspace)).toEqual(before);

    focus(statementAt(workspace, 0));
    press('y', { ctrlKey: true });
    await eventsDelivered();
    expect(bodyTypes(workspace)).toHaveLength(before.length - 1);
  });

  it('Ctrl+C and Ctrl+V copy and paste the focused block through the validated clipboard', async () => {
    const { workspace, memory } = await openGame();
    const count = workspace.getAllBlocks(false).length;
    const first = statementAt(workspace, 0);
    focus(first);

    press('c', { ctrlKey: true });
    expect(memory.get()).not.toBeNull();

    focus(first);
    press('v', { ctrlKey: true });
    await settle();
    await eventsDelivered();
    expect(workspace.getAllBlocks(false).length).toBeGreaterThan(count);
  });

  it('Ctrl+X on a loose block keeps the keyboard focus on the canvas', async () => {
    const { workspace, memory } = await openGame();
    const loose = statementAt(workspace, 1);
    focus(loose);
    press('m');
    press('End');
    press('Enter');
    expect(loose.getParent()).toBeNull();
    expect(loose.getChildren(false).length).toBeGreaterThan(0);
    focus(loose);

    press('x', { ctrlKey: true });
    await settle();
    await eventsDelivered();
    expect(loose.isDeadOrDying()).toBe(true);
    expect(memory.get()).not.toBeNull();
    const node = Blockly.getFocusManager().getFocusedNode();
    expect(node).toBeInstanceOf(Blockly.BlockSvg);
    expect((node as Blockly.BlockSvg).isDeadOrDying()).toBe(false);
    expect(document.activeElement).toBe(node?.getFocusableElement());
  });

  it('a keyboard add from the toolbox is one undo step for the document too', async () => {
    const game = await openGame();
    const { workspace } = game;
    const before = bodyTypes(workspace);
    focus(statementAt(workspace, 0));
    await eventsDelivered();
    workspace.clearUndo();

    press('t');
    press('ArrowRight');
    for (let step = 0; step < 80; step++) {
      const node = Blockly.getFocusManager().getFocusedNode();
      if (node instanceof Blockly.BlockSvg && node.type === 'program.exit') {
        break;
      }
      press('ArrowDown');
    }
    press('Enter');
    await eventsDelivered();
    press('Enter');
    await eventsDelivered();
    expect(bodyTypes(workspace)).toEqual([before[0], 'program.exit', ...before.slice(1)]);

    press('z', { ctrlKey: true });
    await eventsDelivered();
    expect(bodyTypes(workspace)).toEqual(before);
    expect(workspace.getAllBlocks(false).some((block) => block.type === 'program.exit')).toBe(
      false,
    );
    const main = game
      .document()
      .modules[0]?.workspace.blocks.find((block) => block.type === 'program.main');
    expect(main?.statements?.['BODY']?.map((node) => node.type)).toEqual(before);
  });

  it('a keyboard move reaches the project document', async () => {
    const game = await openGame();
    const { workspace } = game;
    const before = bodyTypes(workspace);
    focus(statementAt(workspace, 1));

    press('m');
    press('ArrowUp');
    press('Enter');
    await eventsDelivered();

    const after = bodyTypes(workspace);
    expect(after[0]).toBe(before[1]);
    expect(after[1]).toBe(before[0]);
    const main = game
      .document()
      .modules[0]?.workspace.blocks.find((block) => block.type === 'program.main');
    expect(main?.statements?.['BODY']?.map((node) => node.type)).toEqual(after);
  });
});
