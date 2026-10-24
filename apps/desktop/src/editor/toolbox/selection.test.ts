import * as Blockly from 'blockly/core';
import { afterEach, beforeAll, describe, expect, it } from 'vitest';

import { SelectionTracker, canvasFocus, selectedBlock } from './selection';
import {
  disposeTestWorkspaces,
  guessDocument,
  injectedWorkspace,
  loadModule,
  setUpToolboxBlocks,
} from './testing';

beforeAll(() => {
  setUpToolboxBlocks();
});

afterEach(() => {
  disposeTestWorkspaces();
});

/** A rendered canvas with the guess document. */
function canvas(): Blockly.WorkspaceSvg {
  const workspace = injectedWorkspace('category');
  loadModule(workspace, guessDocument());
  return workspace;
}

/** A rendered block of the canvas. */
function block(workspace: Blockly.WorkspaceSvg, id: string): Blockly.BlockSvg {
  const found = workspace.getBlockById(id);
  if (!(found instanceof Blockly.BlockSvg)) {
    throw new Error(`no block ${id}`);
  }
  return found;
}

/** A selection event, as Blockly fires it on the canvas. */
function selectedEvent(workspace: Blockly.WorkspaceSvg, id: string | null) {
  return new Blockly.Events.Selected(null, id, workspace.id);
}

describe('canvasFocus', () => {
  it('names the focused block, or the block of a focused field', () => {
    const workspace = canvas();
    const decl = block(workspace, 'decl');
    Blockly.getFocusManager().focusNode(decl);
    expect(canvasFocus(workspace)).toEqual({ kind: 'block', block: decl });
    expect(selectedBlock(workspace)).toBe(decl);

    const field = decl.getField('NAME');
    if (field === null) {
      throw new Error('no NAME field');
    }
    Blockly.getFocusManager().focusNode(field);
    expect(canvasFocus(workspace)).toEqual({ kind: 'block', block: decl });
  });

  it('tells the canvas background from everywhere else', () => {
    const workspace = canvas();
    Blockly.getFocusManager().focusNode(workspace);
    expect(canvasFocus(workspace)).toEqual({ kind: 'canvas' });
    expect(selectedBlock(workspace)).toBeNull();

    const toolbox = workspace.getToolbox() as Blockly.Toolbox;
    Blockly.getFocusManager().focusTree(toolbox);
    expect(canvasFocus(workspace)).toEqual({ kind: 'elsewhere' });

    // Another canvas's block is not this canvas's selection.
    const other = canvas();
    Blockly.getFocusManager().focusNode(block(other, 'decl'));
    expect(canvasFocus(workspace)).toEqual({ kind: 'elsewhere' });
    expect(selectedBlock(workspace)).toBeNull();
  });
});

describe('SelectionTracker', () => {
  it('keeps the selected block while the focus is in the toolbox or outside the canvas', () => {
    const workspace = canvas();
    const tracker = new SelectionTracker(workspace);
    const print = block(workspace, 'print');
    Blockly.common.setSelected(print);
    expect(tracker.current()).toBe(print);

    Blockly.getFocusManager().focusTree(workspace.getToolbox() as Blockly.Toolbox);
    expect(Blockly.getSelected()).toBeNull();
    expect(tracker.current()).toBe(print);

    const elsewhere = document.createElement('button');
    document.body.append(elsewhere);
    elsewhere.focus();
    expect(tracker.current()).toBe(print);
  });

  it('forgets the block when the canvas background gets the focus', () => {
    const workspace = canvas();
    const tracker = new SelectionTracker(workspace);
    Blockly.common.setSelected(block(workspace, 'print'));
    expect(tracker.current()?.id).toBe('print');
    Blockly.getFocusManager().focusNode(workspace);
    expect(tracker.current()).toBeNull();
    Blockly.getFocusManager().focusTree(workspace.getToolbox() as Blockly.Toolbox);
    expect(tracker.current()).toBeNull();
  });

  it('forgets a block that was deleted', () => {
    const workspace = canvas();
    const tracker = new SelectionTracker(workspace);
    tracker.noteSelected(selectedEvent(workspace, 'print'));
    expect(tracker.current()?.id).toBe('print');
    block(workspace, 'print').dispose(false);
    expect(tracker.current()).toBeNull();
  });

  it('remembers a selection event at once, and ignores IDs that are not canvas blocks', () => {
    const workspace = canvas();
    const tracker = new SelectionTracker(workspace);
    tracker.noteSelected(selectedEvent(workspace, 'nope'));
    tracker.noteSelected(selectedEvent(workspace, null));
    expect(tracker.current()).toBeNull();
    tracker.noteSelected(selectedEvent(workspace, 'decl'));
    expect(tracker.current()?.id).toBe('decl');
    tracker.forget();
    expect(tracker.current()).toBeNull();
  });
});
