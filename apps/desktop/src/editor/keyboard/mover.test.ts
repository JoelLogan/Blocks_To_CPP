/**
 * Moving and adding blocks with the keyboard against the real Blockly (docs/spec/04-user-interface.md
 * §4.7): `M`, the arrow keys, Enter and Escape, adding from the toolbox, undo, and everything that
 * ends a move.
 */
import * as Blockly from 'blockly/core';
import { afterEach, describe, expect, it, vi } from 'vitest';

import { MOVING_CLASS } from './mover';
import { MOVE_KEYS_HINT } from './plugin';
import { eventsDelivered, focused, type KeyboardEditor, keyboardEditor, press } from './testing';

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

function focus(node: Blockly.IFocusableNode): void {
  Blockly.getFocusManager().focusNode(node);
}

/** The IDs of a statement list, top to bottom. */
function statements(block: Blockly.Block, input: string): string[] {
  const ids: string[] = [];
  for (let next = block.getInputTargetBlock(input); next !== null; next = next.getNextBlock()) {
    ids.push(next.type === 'io.print' || next.type === 'var.declare' ? next.id : next.type);
  }
  return ids;
}

describe('M moves a block', () => {
  it('starts where the block is, and Enter there leaves it', () => {
    const { block, keyboard } = open();
    const print = block('print');
    focus(print);

    press('m');
    expect(keyboard.mover.active).toBe(true);
    expect(print.getSvgRoot().classList.contains(MOVING_CLASS)).toBe(true);
    expect(keyboard.announcer.last).toBe(
      'Moving “print”. Arrow keys choose where it goes, Enter puts it there, Escape cancels. ' +
        'Where it is now: after “create int variable guess”, 2 of 4.',
    );

    press('Enter');
    expect(keyboard.mover.active).toBe(false);
    expect(print.getSvgRoot().classList.contains(MOVING_CLASS)).toBe(false);
    expect(statements(block('main'), 'BODY')).toEqual(['decl', 'print']);
    expect(keyboard.announcer.last).toBe('“print” stays where it was.');
  });

  it('puts the block at the chosen place in one undoable step', async () => {
    const { block, keyboard, workspace } = open();
    const print = block('print');
    focus(print);
    press('m');

    press('ArrowUp');
    expect(keyboard.announcer.last).toBe('at the top of “main”, 1 of 4.');
    const target = keyboard.mover.target;
    expect(target?.kind).toBe('statements');
    if (target?.kind === 'statements') {
      expect(target.connection.isHighlighted()).toBe(true);
    }

    press('Enter');
    expect(statements(block('main'), 'BODY')).toEqual(['print', 'decl']);
    expect(focused()).toBe(print);
    expect(keyboard.announcer.last).toBe('Moved “print” at the top of “main”.');
    if (target?.kind === 'statements') {
      expect(target.connection.isHighlighted()).toBe(false);
    }

    await eventsDelivered();
    workspace.undo(false);
    expect(statements(block('main'), 'BODY')).toEqual(['decl', 'print']);
  });

  it('moves only the block: the statements below it close up', () => {
    const { block, keyboard } = open();
    focus(block('decl'));
    press('m');
    // Places: top of main (where it is), after print, top of factorial, the canvas.
    press('ArrowDown');
    expect(keyboard.announcer.last).toBe('after “print”, 2 of 4.');
    press('Enter');
    expect(statements(block('main'), 'BODY')).toEqual(['print', 'decl']);
  });

  it('can put a block loose on the canvas', () => {
    const { block, keyboard, workspace } = open();
    const print = block('print');
    const before = print.getRelativeToSurfaceXY();
    focus(print);
    press('m');
    press('End');
    expect(keyboard.announcer.last).toBe('loose on the canvas, 4 of 4.');
    press(' ');
    expect(print.getParent()).toBeNull();
    expect(workspace.getTopBlocks(false)).toContain(print);
    const after = print.getRelativeToSurfaceXY();
    expect(after.x).toBeGreaterThan(before.x);
    expect(statements(block('main'), 'BODY')).toEqual(['decl']);
  });

  it('wraps around and goes back with Home', () => {
    const { block, keyboard } = open();
    focus(block('print'));
    press('m');
    press('ArrowDown');
    press('ArrowDown');
    press('ArrowDown');
    expect(keyboard.announcer.last).toBe('at the top of “main”, 1 of 4.');
    press('ArrowLeft');
    expect(keyboard.announcer.last).toBe('loose on the canvas, 4 of 4.');
    press('Home');
    expect(keyboard.announcer.last).toBe('at the top of “main”, 1 of 4.');
    press('ArrowRight');
    expect(keyboard.announcer.last).toBe(
      'Where it is now: after “create int variable guess”, 2 of 4.',
    );
  });

  it('Escape leaves the block where it was', () => {
    const { block, keyboard } = open();
    const print = block('print');
    focus(print);
    press('m');
    press('ArrowUp');
    press('Escape');
    expect(keyboard.mover.active).toBe(false);
    expect(statements(block('main'), 'BODY')).toEqual(['decl', 'print']);
    expect(focused()).toBe(print);
    expect(keyboard.announcer.last).toBe('“print” stays where it was.');
  });

  it('keeps every other key while a block is held', () => {
    const { block, keyboard } = open();
    const print = block('print');
    focus(print);
    press('m');
    const deleting = press('Delete');
    expect(deleting.defaultPrevented).toBe(true);
    expect(print.isDeadOrDying()).toBe(false);
    expect(keyboard.announcer.last).toBe(MOVE_KEYS_HINT);
    // Shift alone neither ends the move nor says anything.
    press('ArrowUp');
    const before = keyboard.announcer.last;
    press('Shift');
    expect(keyboard.announcer.last).toBe(before);
    expect(keyboard.mover.active).toBe(true);
  });

  it('ends without a change on Tab, a pointer press, or when the block goes', async () => {
    const { block, keyboard } = open();
    const print = block('print');

    focus(print);
    press('m');
    const tab = press('Tab');
    expect(tab.defaultPrevented).toBe(false);
    expect(keyboard.mover.active).toBe(false);

    focus(print);
    press('m');
    document.body.dispatchEvent(new PointerEvent('pointerdown', { bubbles: true }));
    expect(keyboard.mover.active).toBe(false);

    focus(print);
    press('m');
    print.dispose(false);
    await eventsDelivered();
    expect(keyboard.mover.active).toBe(false);
    expect(statements(block('main'), 'BODY')).toEqual(['decl']);
  });

  it('says when a block cannot move or has nowhere else to go', () => {
    const { block, keyboard, workspace } = open();
    focus(block('main'));
    press('m');
    expect(keyboard.mover.active).toBe(false);
    expect(keyboard.announcer.last).toBe('“main” has nowhere else to go.');

    workspace.setIsReadOnly(true);
    try {
      focus(block('print'));
      press('m');
      expect(keyboard.mover.active).toBe(false);
      expect(keyboard.announcer.last).toBe('“print” cannot be moved.');
    } finally {
      workspace.setIsReadOnly(false);
    }
  });

  it('moves the block a value slot is in', () => {
    const { block, keyboard } = open();
    const slot = block('decl').getInputTargetBlock('VALUE');
    if (!(slot instanceof Blockly.BlockSvg)) {
      throw new Error('no slot');
    }
    focus(slot);
    press('m');
    expect(keyboard.mover.block).toBe(block('decl'));
  });

  it('keeps the block held when its place has gone', () => {
    const { block, keyboard } = open();
    focus(block('print'));
    press('m');
    press('ArrowDown');
    expect(keyboard.announcer.last).toBe('at the top of “factorial”, 3 of 4.');
    block('fn').dispose(false);
    press('Enter');
    expect(keyboard.mover.active).toBe(true);
    expect(keyboard.announcer.last).toBe('“print” cannot go there any more.');
    press('Escape');
  });

  it('asks for a block when the cursor is elsewhere', () => {
    const { workspace, keyboard } = open();
    focus(workspace);
    press('m');
    expect(keyboard.mover.active).toBe(false);
    expect(keyboard.announcer.last).toBe('Go to a block first, then press M to move it.');
  });
});

describe('adding a block from the toolbox', () => {
  /** Opens the toolbox from `node` and goes down to the first flyout block of `type`. */
  function reachFlyoutBlock(type: string): void {
    press('t');
    press('ArrowRight');
    for (let step = 0; step < 80; step++) {
      const node = focused();
      if (node instanceof Blockly.BlockSvg && node.type === type) {
        return;
      }
      press('ArrowDown');
    }
    throw new Error(`no ${type} in the toolbox`);
  }

  it('adds it where the cursor was and lets the keyboard choose the place', () => {
    const { block, keyboard, workspace } = open();
    focus(block('print'));
    const before = workspace.getAllBlocks(false).length;

    reachFlyoutBlock('program.exit');
    press('Enter');
    expect(keyboard.area()).toBe('canvas');
    expect(keyboard.mover.active).toBe(true);
    const added = keyboard.mover.block;
    expect(added?.type).toBe('program.exit');
    expect(keyboard.announcer.last).toMatch(
      /^Adding “stop program[^”]*”\. .* after “print”, \d+ of \d+\.$/,
    );

    press('Enter');
    expect(keyboard.mover.active).toBe(false);
    expect(statements(block('main'), 'BODY')).toEqual(['decl', 'print', 'program.exit']);
    expect(workspace.getAllBlocks(false).length).toBeGreaterThan(before);
    expect(keyboard.announcer.last).toMatch(/^Added “stop program[^”]*” after “print”\.$/);
  });

  it('Escape takes the added block away again', () => {
    const { block, keyboard, workspace } = open();
    focus(block('print'));
    const before = workspace.getAllBlocks(false).length;
    reachFlyoutBlock('program.exit');
    press('Enter');
    press('Escape');
    expect(keyboard.mover.active).toBe(false);
    expect(workspace.getAllBlocks(false).length).toBe(before);
    expect(keyboard.announcer.last).toBe('Nothing was added.');
  });

  it('adds a block that stands alone straight onto the canvas', () => {
    const { block, keyboard, workspace } = open();
    focus(block('print'));
    const tops = workspace.getTopBlocks(false).length;
    reachFlyoutBlock('program.main');
    press('Enter');
    expect(keyboard.mover.active).toBe(false);
    expect(workspace.getTopBlocks(false).length).toBe(tops + 1);
    expect(keyboard.announcer.last).toMatch(/^Added “main” loose on the canvas\.$/);
  });

  it('presses a toolbox button with Enter', () => {
    const { block, workspace } = open();
    let pressed = 0;
    workspace.registerButtonCallback('KB_TEST', () => {
      pressed += 1;
    });
    const flyout = workspace.getFlyout() as Blockly.Flyout;
    const button = new Blockly.FlyoutButton(
      flyout.getWorkspace(),
      workspace,
      { kind: 'button', text: 'Make a test', callbackkey: 'KB_TEST' },
      false,
    );
    focus(block('print'));
    press('t');
    press('ArrowRight');
    const editorFocus = Blockly.getFocusManager();
    const spy = vi.spyOn(editorFocus, 'getFocusedNode').mockReturnValue(button);
    press('Enter');
    spy.mockRestore();
    expect(pressed).toBe(1);
  });
});
