/**
 * The mutators on a rendered (Zelos) workspace: the buttons' text alternative, text-only
 * rendering, the bump of removed parts' blocks, and keyboard focus after a click.
 */
import * as Blockly from 'blockly/core';
import { afterEach, beforeAll, beforeEach, describe, expect, it } from 'vitest';

import { MutatorButton } from './buttons';
import { buttonKey } from './controller';
import { isB2cMutatorBlock, registerB2cMutators } from './register';
import { defineTestBlocks } from './test-fixtures';
import type { B2cMutatorBlock } from './types';

let host: HTMLDivElement;
let workspace: Blockly.WorkspaceSvg;

beforeAll(() => {
  registerB2cMutators();
  defineTestBlocks();
});

beforeEach(() => {
  host = document.createElement('div');
  document.body.append(host);
  workspace = Blockly.inject(host, { renderer: 'zelos', sounds: false });
});

afterEach(() => {
  workspace.dispose();
  document.body.replaceChildren();
});

function rendered(type: string): B2cMutatorBlock & Blockly.BlockSvg {
  const block = workspace.newBlock(type);
  block.initSvg();
  block.render();
  if (!isB2cMutatorBlock(block)) {
    throw new Error(`${type} has no b2c mutator`);
  }
  return block;
}

function button(block: Blockly.Block, key: string): MutatorButton {
  for (const input of block.inputList) {
    for (const field of input.fieldRow) {
      if (field instanceof MutatorButton && buttonKey(field) === key) {
        return field;
      }
    }
  }
  throw new Error(`no button ${key}`);
}

describe('buttons on a rendered block', () => {
  it('render as images with a text alternative', () => {
    const print = rendered('io.print');
    const add = button(print, 'count:add');
    const group = add.getSvgRoot();
    const image = group?.querySelector('image');
    expect(image?.getAttribute('aria-label')).toBe('Add an input');
    expect(image?.getAttribute('role')).toBe('button');
    expect(image?.querySelector('title')?.textContent).toBe('Add an input');
    expect(
      image?.getAttribute('xlink:href') ??
        image?.getAttributeNS('http://www.w3.org/1999/xlink', 'href'),
    ).toMatch(/^data:image\/svg\+xml,/);
    expect(add.getTooltip()).toBe('Add an input');
    expect(add.getText()).toBe('Add an input');
  });

  it('render no HTML: only SVG elements under the block', () => {
    const branch = rendered('control.if');
    branch.b2cSetExtra({ elseIfCount: 2, hasElse: true });
    branch.render();
    const root = branch.getSvgRoot();
    const html = [...root.querySelectorAll('*')].filter(
      (element) => element.namespaceURI !== 'http://www.w3.org/2000/svg',
    );
    expect(html).toEqual([]);
  });

  it('change the block when clicked', () => {
    const print = rendered('io.print');
    button(print, 'count:add').showEditor();
    expect(print.b2cGetExtra()).toEqual({ itemCount: 2 });
    print.render();
    expect(print.getInput('ITEM1')?.connection).not.toBeNull();
  });

  it('do nothing on blocks in a flyout or on insertion markers', () => {
    const print = rendered('io.print');
    print.isInFlyout = true;
    button(print, 'count:add').showEditor();
    expect(print.b2cGetExtra()).toEqual({ itemCount: 1 });
  });
});

describe('removing a part on a rendered block', () => {
  it('moves the unplugged block a little away', () => {
    const print = rendered('io.print');
    print.b2cSetExtra({ itemCount: 2 });
    const literal = workspace.newBlock('text.literal');
    literal.initSvg();
    literal.render();
    print.getInput('ITEM1')?.connection?.connect(literal.outputConnection);
    print.render();
    const before = literal.getRelativeToSurfaceXY();
    button(print, 'count:remove').showEditor();
    const after = literal.getRelativeToSurfaceXY();
    expect(literal.getParent()).toBeNull();
    expect(after.x - before.x).toBe(Blockly.config.snapRadius);
    expect(after.y - before.y).toBe(Blockly.config.snapRadius);
  });
});

describe('keyboard focus', () => {
  it('moves to the replacement button when the clicked one is replaced', () => {
    const print = rendered('io.print');
    const manager = Blockly.getFocusManager();
    const add = button(print, 'count:add');
    manager.focusNode(add);
    expect(manager.getFocusedNode()).toBe(add);
    add.showEditor();
    const focused = manager.getFocusedNode();
    expect(focused).toBeInstanceOf(MutatorButton);
    expect(focused).not.toBe(add);
    expect(buttonKey(focused as MutatorButton)).toBe('count:add');
  });

  it('moves to another button when the clicked kind is gone', () => {
    const branch = rendered('control.if');
    const manager = Blockly.getFocusManager();
    const addElse = button(branch, 'flag:hasElse:add');
    manager.focusNode(addElse);
    addElse.showEditor();
    expect(buttonKey(manager.getFocusedNode() as MutatorButton)).toBe('count:add');
  });

  it('stays on a button whose row did not change', () => {
    const args = rendered('func.call');
    args.b2cSetExtra({ argCount: 2 });
    args.render();
    const manager = Blockly.getFocusManager();
    const add = button(args, 'count:add');
    manager.focusNode(add);
    add.showEditor();
    expect(manager.getFocusedNode()).toBe(add);
  });
});
