/**
 * Keyboard navigation of the block editor against the real Blockly (docs/spec/04-user-interface.md
 * §4.7): the arrow keys on the canvas, Enter on fields and on the canvas, the toolbox and its
 * blocks, and Escape.
 */
import * as Blockly from 'blockly/core';
import { afterEach, describe, expect, it, vi } from 'vitest';

import { guessDocument } from '../toolbox/testing';
import { focused, type KeyboardEditor, keyboardEditor, press } from './testing';

let editor: KeyboardEditor | null = null;

function open(): KeyboardEditor {
  editor = keyboardEditor();
  return editor;
}

afterEach(() => {
  editor?.dispose();
  editor = null;
  Blockly.keyboardNavigationController.setIsActive(false);
});

/** Focuses a node as the keyboard would reach it. */
function focus(node: Blockly.IFocusableNode): void {
  Blockly.getFocusManager().focusNode(node);
}

function nameOf(node: Blockly.IFocusableNode | null): string {
  if (node instanceof Blockly.BlockSvg) {
    return `block:${node.id}`;
  }
  if (node instanceof Blockly.Field) {
    return `field:${node.name ?? ''}@${node.getSourceBlock()?.type ?? ''}`;
  }
  if (node instanceof Blockly.FlyoutButton) {
    return `button:${node.getButtonText()}`;
  }
  if (node instanceof Blockly.WorkspaceSvg) {
    return node.isFlyout ? 'flyout' : 'canvas';
  }
  return node === null ? 'nothing' : typeof node;
}

describe('the arrow keys on the canvas', () => {
  it('go from block to block with ↓ and ↑, and wrap around', () => {
    const { workspace, block } = open();
    focus(workspace);

    const down = press('ArrowDown');
    expect(down.defaultPrevented).toBe(true);
    expect(nameOf(focused())).toBe('block:main');
    press('ArrowDown');
    expect(nameOf(focused())).toBe('block:decl');
    press('ArrowDown');
    expect(nameOf(focused())).toBe('block:print');
    press('ArrowUp');
    expect(focused()).toBe(block('decl'));
    expect(Blockly.keyboardNavigationController.getIsActive()).toBe(true);
  });

  it('go through a block’s fields and inputs with → and ←', () => {
    const { block } = open();
    focus(block('decl'));

    press('ArrowRight');
    expect(nameOf(focused())).toBe('field:TYPE@var.declare');
    press('ArrowRight');
    expect(nameOf(focused())).toBe('field:NAME@var.declare');
    press('ArrowLeft');
    expect(nameOf(focused())).toBe('field:TYPE@var.declare');
  });

  it('start at the last block with ↑ when only the canvas has the focus', () => {
    const { workspace } = open();
    focus(workspace);
    press('ArrowUp');
    expect(focused()).toBeInstanceOf(Blockly.BlockSvg);
    expect(focused()).not.toBe(workspace);
  });

  it('say what the cursor reached', () => {
    const { block, keyboard } = open();
    focus(block('main'));
    press('ArrowDown');
    expect(keyboard.announcer.last).toBe('block “create int variable guess”');
    press('ArrowRight');
    expect(keyboard.announcer.last).toBe('field “int” of “create int variable guess”');
  });

  it('say that an empty canvas is empty', () => {
    editor = keyboardEditor({ document: emptyDocument() });
    const { workspace, keyboard } = editor;
    focus(workspace);
    press('ArrowDown');
    expect(keyboard.announcer.last).toBe('The canvas is empty. Press T to add a block.');
  });

  it('leave keys with a modifier alone', () => {
    const { block } = open();
    focus(block('decl'));
    const shifted = press('ArrowDown', { shiftKey: true });
    expect(shifted.defaultPrevented).toBe(false);
    expect(focused()).toBe(block('decl'));
  });
});

describe('Enter on the canvas', () => {
  it('opens a field’s editor', () => {
    const { block } = open();
    const field = block('decl').getField('NAME');
    if (field === null) {
      throw new Error('no NAME field');
    }
    const show = vi.spyOn(field, 'showEditor').mockImplementation(() => undefined);
    focus(field);
    const event = press('Enter');
    expect(show).toHaveBeenCalledTimes(1);
    expect(event.defaultPrevented).toBe(true);
  });

  it('edits a number slot as a whole', () => {
    const { block } = open();
    const slot = block('decl').getInputTargetBlock('VALUE');
    if (!(slot instanceof Blockly.BlockSvg)) {
      throw new Error('no value slot');
    }
    const field = slot.inputList
      .flatMap((input) => input.fieldRow)
      .find((f) => f.isFullBlockField());
    if (field === undefined) {
      throw new Error('no full-block field');
    }
    const show = vi.spyOn(field, 'showEditor').mockImplementation(() => undefined);
    focus(slot);
    press(' ');
    expect(show).toHaveBeenCalledTimes(1);
  });

  it('on a block explains the keys', () => {
    const { block, keyboard } = open();
    focus(block('print'));
    press('Enter');
    expect(keyboard.announcer.last).toBe(
      'Right arrow goes to the block’s fields, M moves it, Delete removes it.',
    );
  });

  it('on the canvas itself opens the toolbox', () => {
    const { workspace, keyboard } = open();
    focus(workspace);
    press('Enter');
    expect(keyboard.area()).toBe('toolbox');
  });
});

describe('the toolbox and its blocks', () => {
  it('T opens the toolbox, → goes to the category’s blocks, ← back, Escape to the canvas', () => {
    const { block, keyboard } = open();
    focus(block('print'));

    press('t');
    expect(keyboard.area()).toBe('toolbox');
    expect(keyboard.announcer.last).toContain('Toolbox.');

    press('ArrowRight');
    expect(keyboard.area()).toBe('flyout');
    expect(nameOf(focused())).toBe('button:Program');
    expect(keyboard.announcer.last).toBe('category “Program”');

    press('ArrowDown');
    expect(nameOf(focused())).toMatch(/^block:/);
    expect((focused() as Blockly.BlockSvg).type).toBe('program.main');

    press('ArrowLeft');
    expect(keyboard.area()).toBe('toolbox');

    press('Escape');
    expect(keyboard.area()).toBe('canvas');
    expect(focused()).toBe(block('print'));
  });

  it('Escape in the toolbox’s blocks goes back to the canvas', () => {
    const { block, keyboard } = open();
    focus(block('print'));
    press('t');
    press('Enter');
    expect(keyboard.area()).toBe('flyout');
    press('Escape');
    expect(keyboard.area()).toBe('canvas');
  });

  it('Enter on a category label says how to reach its blocks', () => {
    const { block, keyboard } = open();
    focus(block('print'));
    press('t');
    press('ArrowRight');
    press('Enter');
    expect(keyboard.announcer.last).toBe('Down arrow goes to the category’s blocks.');
  });

  it('Escape on the canvas is still Blockly’s', () => {
    const { workspace, block } = open();
    const hide = vi.spyOn(workspace, 'hideChaff');
    focus(block('print'));
    press('Escape');
    expect(hide).toHaveBeenCalled();
  });
});

/** A document whose only module is empty. */
function emptyDocument() {
  const doc = guessDocument();
  const module = doc.modules[0];
  if (module !== undefined) {
    module.workspace.blocks = [];
  }
  return doc;
}
