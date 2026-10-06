/**
 * Keyboard navigation of the block editor against the real Blockly (docs/spec/04-user-interface.md
 * §4.7): the arrow keys on the canvas, Enter on fields and on the canvas, the toolbox and its
 * blocks (also when they are rebuilt under the keyboard), and Escape.
 */
import * as Blockly from 'blockly/core';
import { afterEach, describe, expect, it, vi } from 'vitest';

import { useAppStore } from '../../app/store';
import { type B2cContinuousToolbox, CONTINUOUS_REFRESH_DELAY_MS } from '../toolbox/continuous';
import { createToolboxPlugin } from '../toolbox/plugin';
import { guessDocument, symbolFixture } from '../toolbox/testing';
import { keepFlyoutFocus } from './keepFocus';
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

describe('the toolbox’s blocks rebuilt while the keyboard is in them', () => {
  /** Whether the keyboard focus is on a live node, with the DOM focus on its element. */
  function onLiveNode(): boolean {
    const node = focused();
    if (node === null) {
      return false;
    }
    const element = node.getFocusableElement();
    const dead = node instanceof Blockly.BlockSvg && node.isDeadOrDying();
    return !dead && element.isConnected && document.activeElement === element;
  }

  it('keep the keyboard on the same item, and ↓ goes on from there', () => {
    const { block, keyboard, workspace } = open();
    focus(block('print'));
    press('t');
    press('ArrowRight');
    press('ArrowDown');
    press('ArrowDown');
    const before = focused();
    expect(before).toBeInstanceOf(Blockly.BlockSvg);
    const type = (before as Blockly.BlockSvg).type;
    const toolbox = workspace.getToolbox() as B2cContinuousToolbox;

    expect(toolbox.showAllCategories(true)).toBe(true);
    expect((before as Blockly.BlockSvg).isDeadOrDying()).toBe(true);
    expect(onLiveNode()).toBe(true);
    expect(keyboard.area()).toBe('flyout');
    expect((focused() as Blockly.BlockSvg).type).toBe(type);

    const at = focused();
    press('ArrowDown');
    expect(onLiveNode()).toBe(true);
    expect(focused()).not.toBe(at);
  });

  it('keep the keyboard on a toolbox button that is rebuilt', () => {
    const { block, keyboard, workspace } = open();
    focus(block('print'));
    press('t');
    press('ArrowRight');
    const label = focused();
    expect(nameOf(label)).toBe('button:Program');

    (workspace.getToolbox() as B2cContinuousToolbox).showAllCategories(true);
    expect(focused()).not.toBe(label);
    expect(nameOf(focused())).toBe('button:Program');
    expect(onLiveNode()).toBe(true);
    expect(keyboard.area()).toBe('flyout');
  });

  it('keep the keyboard at the same place when its item is no longer there', () => {
    const { block, workspace } = open();
    focus(block('print'));
    press('t');
    press('ArrowRight');
    press('ArrowDown');
    press('ArrowDown');
    const flyout = workspace.getFlyout();
    if (flyout === null) {
      throw new Error('no flyout');
    }
    const before = focused();
    // Past the end of the contents shown next.
    expect(flyout.getContents().findIndex((item) => item.getElement() === before)).toBeGreaterThan(
      2,
    );

    // Fewer items, none of them the focused one: the last item is nearest its place.
    keepFlyoutFocus(flyout, () => {
      flyout.show([
        { kind: 'label', text: 'Only' },
        { kind: 'block', type: 'program.exit' },
      ]);
    });
    const now = focused();
    expect(now).toBeInstanceOf(Blockly.BlockSvg);
    expect((now as Blockly.BlockSvg).type).toBe('program.exit');
    expect(onLiveNode()).toBe(true);
  });

  it('leave a keyboard that is elsewhere alone', () => {
    const { block, workspace } = open();
    const print = block('print');
    focus(print);
    (workspace.getToolbox() as B2cContinuousToolbox).showAllCategories(true);
    expect(focused()).toBe(print);
  });

  // The toolbox plugin rebuilds the flyout twice here, which is slow on a busy machine.
  it(
    'keep the keyboard when an analysis adds a category’s block',
    { timeout: 20_000 },
    async () => {
      editor = keyboardEditor({ plugins: [createToolboxPlugin()] });
      const { block, keyboard } = editor;
      focus(block('print'));
      press('t');
      press('ArrowRight');
      press('ArrowDown');
      press('ArrowDown');
      expect(keyboard.area()).toBe('flyout');
      const before = focused();

      // The preview of an earlier edit lands: My Blocks now lists factorial.
      const factorial = symbolFixture('s_fact', 'factorial', {
        kind: 'function',
        params: ['s_n'],
        returns: 'int',
        declBlock: 'fn',
      });
      const state = useAppStore.getState();
      useAppStore.setState({
        analysis: {
          ...state.analysis,
          seq: state.analysis.seq + 1,
          preview: {
            stage: 'generate',
            diagnostics: [],
            files: [],
            sourceMap: null,
            buildable: true,
            placeholders: 0,
            contentHash: 'a'.repeat(64),
            blockTypes: {},
            symbols: [factorial],
          },
        },
      });
      await new Promise((resolve) => setTimeout(resolve, CONTINUOUS_REFRESH_DELAY_MS + 50));

      expect((before as Blockly.BlockSvg).isDeadOrDying()).toBe(true);
      expect(onLiveNode()).toBe(true);
      expect(keyboard.area()).toBe('flyout');
      press('ArrowDown');
      expect(onLiveNode()).toBe(true);
    },
  );
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
