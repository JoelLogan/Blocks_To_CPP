/**
 * Which block a copy takes and where a paste goes for each thing that can have the focus on the
 * canvas: a block, a field, a connection, an expression slot, or something that is not part of a
 * project block of this canvas.
 */
import * as Blockly from 'blockly/core';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';

import { guessingGame } from '../diagnostics/testing';
import { loadModule, withoutEvents } from '../sync/bdmToWorkspace';
import { disposeWorkspaces, headlessWorkspace, renderedWorkspace } from '../sync/testing';
import {
  anchorFor,
  anchorForBlock,
  anchorIsLive,
  copyableBlock,
  isProjectBlock,
  ON_CANVAS,
  type PasteAnchor,
  pasteTarget,
} from './anchor';

let workspace: Blockly.WorkspaceSvg;

beforeEach(() => {
  workspace = renderedWorkspace();
  const doc = guessingGame();
  // A loose statement on the canvas, with nothing above it.
  doc.modules[0]?.workspace.blocks.push({
    id: 'loose',
    type: 'io.print',
    v: 1,
    x: 600,
    y: 40,
    extra: { itemCount: 1 },
    fields: { NEWLINE: true, SEP: 'none', STREAM: 'out' },
    inputs: { ITEM0: { expr: [{ str: 'hi' }] } },
  });
  loadModule(workspace, doc, 'mod_main');
});

afterEach(() => {
  disposeWorkspaces();
});

function block(id: string): Blockly.Block {
  const found = workspace.getBlockById(id);
  if (found === null) {
    throw new Error(`no block ${id}`);
  }
  return found;
}

function connectionOf(id: string, input: string): Blockly.Connection {
  const connection = block(id).getInput(input)?.connection ?? null;
  if (connection === null) {
    throw new Error(`no connection ${id}.${input}`);
  }
  return connection;
}

/** The anchor written with block IDs, for comparing. */
function described(anchor: PasteAnchor): unknown {
  switch (anchor.kind) {
    case 'after':
      return { after: anchor.block.id };
    case 'list':
    case 'value':
      return { [anchor.kind]: `${anchor.block.id}.${anchor.input}` };
    case 'canvas':
      return { canvas: anchor.near?.id ?? null, at: anchor.at };
  }
}

/** The expression slot (a shadow) in a value input. */
function slotOf(id: string, input: string): Blockly.Block {
  const shadow = connectionOf(id, input).targetBlock();
  if (shadow?.isShadow() !== true) {
    throw new Error(`${id}.${input} holds no expression slot`);
  }
  return shadow;
}

describe('copyableBlock', () => {
  it('is the focused project block, or the block of a focused field or connection', () => {
    expect(copyableBlock(workspace, block('b004'))?.id).toBe('b004');
    const field = block('b004').getField('NEWLINE');
    expect(copyableBlock(workspace, field)?.id).toBe('b004');
    expect(copyableBlock(workspace, block('b009').nextConnection)?.id).toBe('b009');
  });

  it('is nothing for an expression slot, the canvas, another canvas or a removed block', () => {
    expect(copyableBlock(workspace, slotOf('b003', 'VALUE'))).toBeNull();
    expect(copyableBlock(workspace, workspace)).toBeNull();
    expect(copyableBlock(workspace, null)).toBeNull();
    const other = headlessWorkspace().newBlock('io.print');
    expect(copyableBlock(workspace, other)).toBeNull();
    const gone = block('loose');
    withoutEvents(() => {
      gone.dispose(false);
    });
    expect(copyableBlock(workspace, gone)).toBeNull();
  });
});

describe('isProjectBlock', () => {
  it('accepts catalog blocks of this canvas and refuses slots and other canvases', () => {
    expect(isProjectBlock(workspace, block('b011'))).toBe(true);
    expect(isProjectBlock(workspace, slotOf('b004', 'ITEM0'))).toBe(false);
    expect(isProjectBlock(headlessWorkspace(), block('b011'))).toBe(false);
  });
});

describe('anchorFor', () => {
  it('pastes after a focused statement, into the first list of a hat, next to a reporter', () => {
    expect(described(anchorFor(workspace, block('b004')))).toEqual({ after: 'b004' });
    expect(described(anchorFor(workspace, block('b011')))).toEqual({ list: 'b011.BODY' });
    expect(described(anchorFor(workspace, block('b001')))).toEqual({ canvas: 'b001', at: null });
    // A field of a statement counts as the statement.
    expect(described(anchorFor(workspace, block('b004').getField('NEWLINE')))).toEqual({
      after: 'b004',
    });
  });

  it('pastes at a focused connection', () => {
    expect(described(anchorFor(workspace, block('b004').nextConnection))).toEqual({
      after: 'b004',
    });
    expect(described(anchorFor(workspace, connectionOf('b009', 'DO1')))).toEqual({
      list: 'b009.DO1',
    });
    expect(described(anchorFor(workspace, connectionOf('b002', 'VALUE')))).toEqual({
      value: 'b002.VALUE',
    });
  });

  it('pastes before a block: after the one above it, or at the start of its list', () => {
    expect(described(anchorFor(workspace, block('b004').previousConnection))).toEqual({
      after: 'b003',
    });
    expect(described(anchorFor(workspace, block('b002').previousConnection))).toEqual({
      list: 'b011.BODY',
    });
    expect(described(anchorFor(workspace, block('loose').previousConnection))).toEqual({
      canvas: 'loose',
      at: null,
    });
  });

  it('pastes into the value input of a focused expression slot', () => {
    expect(described(anchorFor(workspace, slotOf('b003', 'VALUE')))).toEqual({
      value: 'b003.VALUE',
    });
  });

  it('pastes on the canvas for anything else', () => {
    expect(anchorFor(workspace, null)).toBe(ON_CANVAS);
    expect(anchorFor(workspace, workspace)).toBe(ON_CANVAS);
    const other = headlessWorkspace().newBlock('io.print');
    expect(anchorFor(workspace, other)).toBe(ON_CANVAS);
    expect(anchorFor(workspace, other.nextConnection)).toBe(ON_CANVAS);
  });
});

describe('anchorForBlock and anchorIsLive', () => {
  it('follow the block', () => {
    const anchor = anchorForBlock(block('b006'));
    expect(described(anchor)).toEqual({ after: 'b006' });
    expect(anchorIsLive(workspace, anchor)).toBe(true);
    expect(anchorIsLive(workspace, ON_CANVAS)).toBe(true);
    expect(anchorIsLive(headlessWorkspace(), anchor)).toBe(false);
    withoutEvents(() => {
      block('b006').dispose(true);
    });
    expect(anchorIsLive(workspace, anchor)).toBe(false);
  });
});

describe('pasteTarget', () => {
  it('names the module, the block and the input for the core', () => {
    const at = (anchor: PasteAnchor) => pasteTarget(anchor, 'mod_main');
    expect(at({ kind: 'after', block: block('b004') })).toEqual({
      module: 'mod_main',
      block: 'b004',
      input: null,
    });
    expect(at({ kind: 'list', block: block('b009'), input: 'ELSE' })).toEqual({
      module: 'mod_main',
      block: 'b009',
      input: 'ELSE',
    });
    expect(at({ kind: 'value', block: block('b003'), input: 'VALUE' })).toEqual({
      module: 'mod_main',
      block: 'b003',
      input: 'VALUE',
    });
    expect(at({ kind: 'canvas', near: block('b004'), at: { x: 1, y: 2 } })).toEqual({
      module: 'mod_main',
      block: null,
      input: null,
    });
  });
});

describe('focus on a rendered canvas', () => {
  it('is what the shortcuts see', () => {
    const target = block('b005');
    expect(target).toBeInstanceOf(Blockly.BlockSvg);
    Blockly.getFocusManager().focusNode(target as Blockly.BlockSvg);
    const focused = Blockly.getFocusManager().getFocusedNode();
    expect(copyableBlock(workspace, focused)?.id).toBe('b005');
    expect(described(anchorFor(workspace, focused))).toEqual({ after: 'b005' });
  });
});
