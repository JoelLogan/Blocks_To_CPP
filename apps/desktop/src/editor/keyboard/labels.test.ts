/**
 * The names the keyboard plugin announces for blocks, fields, inputs, toolbox items and places,
 * against the real Blockly blocks.
 */
import * as Blockly from 'blockly/core';
import { afterEach, describe, expect, it, vi } from 'vitest';

import { FLYOUT_TEST_TIMEOUT_MS } from '../toolbox/testing';
import { describeBlock, describeNode, describeTarget } from './labels';
import { type KeyboardEditor, keyboardEditor } from './testing';

// Each test here injects the editor with the continuous toolbox, whose first showing draws every
// catalog block (toolbox/testing.ts): the slowest test took 4.9 to 5.5 s under coverage with two
// workers on a loaded machine, with happy-dom 20 and 21 alike, and 5.1 to 5.8 s with W2's control
// and file blocks (2.2 s alone before them), against Vitest's 5 s default.
vi.setConfig({ testTimeout: FLYOUT_TEST_TIMEOUT_MS });

let editor: KeyboardEditor | null = null;

function open(): KeyboardEditor {
  editor = keyboardEditor();
  return editor;
}

afterEach(() => {
  editor?.dispose();
  editor = null;
});

function newBlock(workspace: Blockly.WorkspaceSvg, type: string): Blockly.BlockSvg {
  const block = workspace.newBlock(type);
  block.initSvg();
  block.render();
  return block;
}

describe('block names', () => {
  it('use the block-path label, with a declared name', () => {
    const { block } = open();
    expect(describeBlock(block('main'))).toBe('when program starts');
    expect(describeBlock(block('fn'))).toBe('factorial');
    expect(describeBlock(block('decl'))).toBe('create int variable guess');
    expect(describeBlock(block('print'))).toBe('print');
  });

  it('show a value slot by its value', () => {
    const { block } = open();
    const slot = block('decl').getInputTargetBlock('VALUE');
    if (slot === null) {
      throw new Error('no value slot');
    }
    expect(describeBlock(slot)).toBe('0');
  });

  it('fall back to the block type when naming fails', () => {
    const { block } = open();
    const decl = block('decl');
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    vi.spyOn(decl, 'getField').mockImplementation(() => {
      throw new Error('broken');
    });
    expect(describeBlock(decl)).toBe('var.declare');
    expect(warn).toHaveBeenCalled();
  });
});

describe('node names', () => {
  it('name fields, values, blocks, disabled blocks and the canvas', () => {
    const { block, workspace } = open();
    const decl = block('decl');
    expect(describeNode(decl.getField('TYPE'))).toBe('field “int” of “create int variable guess”');
    const slot = decl.getInputTargetBlock('VALUE') as Blockly.BlockSvg;
    expect(describeNode(slot)).toBe('value “0” in “create int variable guess”');
    const slotField = slot.inputList[0]?.fieldRow.find((field) => field.isFullBlockField());
    expect(describeNode(slotField)).toBe('field “0” in “create int variable guess”');
    expect(describeNode(block('print'))).toBe('block “print”');
    block('print').setDisabledReason(true, 'test');
    expect(describeNode(block('print'))).toBe('block “print”, disabled');
    expect(describeNode(workspace)).toBe('the canvas');
    expect(describeNode(workspace.getFlyout()?.getWorkspace())).toBe('the toolbox’s blocks');
    expect(describeNode(null)).toBe('');
    expect(describeNode('something else')).toBe('');
  });

  it('name an empty input by its block', () => {
    const { workspace } = open();
    const compare = newBlock(workspace, 'math.compare');
    const input = compare.inputList.find((candidate) => candidate.connection !== null)?.connection;
    expect(describeNode(input)).toMatch(/^empty input of “/);
  });

  it('name toolbox labels as categories', () => {
    const { workspace } = open();
    const flyout = workspace.getFlyout() as Blockly.Flyout;
    const label = flyout
      .getContents()
      .map((item) => item.getElement())
      .find((element) => element instanceof Blockly.FlyoutButton && element.isLabel());
    expect(describeNode(label)).toBe('category “Program”');
  });
});

describe('place names', () => {
  it('say which input of a block with several', () => {
    const { workspace } = open();
    const compare = newBlock(workspace, 'math.compare');
    const values = compare.inputList.filter(
      (input) => input.connection?.type === Blockly.ConnectionType.INPUT_VALUE,
    );
    expect(values.length).toBeGreaterThan(1);
    const second = values[1];
    if (second?.connection === null || second === undefined) {
      throw new Error('no second input');
    }
    const text = describeTarget({
      kind: 'value',
      owner: compare,
      input: second.name,
      connection: second.connection as Blockly.RenderedConnection,
    });
    expect(text).toMatch(/^into “.*” \((?:part 2|“.+” part)\)$/);
    expect(describeTarget({ kind: 'canvas' })).toBe('loose on the canvas');
  });
});
