/**
 * The places a block can be moved to with the keyboard, against the real Blockly with the app's
 * blocks and type-aware connection checker.
 */
import * as Blockly from 'blockly/core';
import { afterEach, describe, expect, it } from 'vitest';

import { describeTarget } from './labels';
import {
  currentConnection,
  dropTargets,
  type DropTarget,
  isCurrentPlace,
  isStillValid,
  movingBlocks,
  movingConnection,
} from './targets';
import { type KeyboardEditor, keyboardEditor } from './testing';

let editor: KeyboardEditor | null = null;

function open(): KeyboardEditor {
  editor = keyboardEditor();
  return editor;
}

afterEach(() => {
  editor?.dispose();
  editor = null;
});

/** A new rendered block on the canvas. */
function newBlock(workspace: Blockly.WorkspaceSvg, type: string): Blockly.BlockSvg {
  const block = workspace.newBlock(type);
  block.initSvg();
  block.render();
  return block;
}

function summary(targets: readonly DropTarget[]): string[] {
  return targets.map((target) => describeTarget(target));
}

describe('the places of a statement', () => {
  it('are the tops of statement lists and the places after statements, then the canvas', () => {
    const { block } = open();
    const print = block('print');
    const targets = dropTargets(print);

    expect(summary(targets)).toEqual([
      'at the top of “main”',
      'after “create int variable guess”',
      'at the top of “factorial”',
      'loose on the canvas',
    ]);
    expect(targets.every((target) => target.kind !== 'value')).toBe(true);
    // Where it is now is one of them, so the keyboard can start there.
    expect(targets.findIndex((target) => isCurrentPlace(print, target))).toBe(1);
  });

  it('never include the block itself or what is inside it', () => {
    const { block, workspace } = open();
    const main = block('main');
    const loop = newBlock(workspace, 'control.forever');
    const below = loop.getInput('BODY')?.connection;
    if (below === null || below === undefined) {
      throw new Error('no BODY');
    }
    const inner = newBlock(workspace, 'io.print');
    below.connect(inner.previousConnection);

    const places = dropTargets(loop);
    const owners = places.flatMap((target) => (target.kind === 'canvas' ? [] : [target.owner]));
    expect(owners).not.toContain(loop);
    expect(owners).not.toContain(inner);
    expect(owners).toContain(main);
    expect(movingBlocks(loop).has(inner)).toBe(true);
  });

  it('keep the statements below a moved block out of what moves', () => {
    const { block } = open();
    const decl = block('decl');
    const moving = movingBlocks(decl);
    expect(moving.has(decl)).toBe(true);
    expect(moving.has(block('print'))).toBe(false);
    // The statement below stays, so the block can go after it.
    expect(summary(dropTargets(decl))).toContain('after “print”');
  });

  it('leave out places where the statements there could not hang below the block', () => {
    const { workspace } = open();
    // Every Blocks2Cpp statement can have a statement below it; a test block that cannot shows
    // that a move never knocks the statements at a place loose.
    Blockly.common.defineBlocksWithJsonArray([
      { type: 'kb_test_last', message0: 'last', previousStatement: null },
    ]);
    const last = newBlock(workspace, 'kb_test_last');
    expect(last.nextConnection).toBeNull();
    const places = dropTargets(last);
    const owners = places.flatMap((target) =>
      target.kind === 'canvas' ? [] : [`${target.kind}:${target.owner.id}`],
    );
    expect(owners).toContain('after:print');
    expect(owners).not.toContain('after:decl');
    expect(owners).not.toContain('statements:main');
    delete Blockly.Blocks['kb_test_last'];
  });
});

describe('the places of a value', () => {
  it('are the value inputs that hold an expression slot or nothing', () => {
    const { workspace } = open();
    const sum = newBlock(workspace, 'math.arithmetic');
    expect(movingConnection(sum)).toBe(sum.outputConnection);

    const places = dropTargets(sum);
    const values = places.filter((target) => target.kind === 'value');
    expect(values.length).toBeGreaterThan(0);
    expect(summary(values)).toContain('into “create int variable guess”');
    expect(places.at(-1)?.kind).toBe('canvas');
    expect(places.some((target) => target.kind === 'after')).toBe(false);
  });

  it('include the input the value is in now', () => {
    const { block, workspace } = open();
    const sum = newBlock(workspace, 'math.arithmetic');
    const value = block('decl').getInput('VALUE')?.connection;
    if (value === null || value === undefined) {
      throw new Error('no VALUE');
    }
    value.connect(sum.outputConnection);
    expect(currentConnection(sum)).toBe(value);

    const places = dropTargets(sum);
    const now = places.find((target) => isCurrentPlace(sum, target));
    expect(now?.kind).toBe('value');
    expect(isCurrentPlace(sum, { kind: 'canvas' })).toBe(false);
  });
});

describe('the places of a block without a connection', () => {
  it('are only the canvas, where it is', () => {
    const { block } = open();
    const main = block('main');
    const places = dropTargets(main);
    expect(places).toEqual([{ kind: 'canvas' }]);
    expect(isCurrentPlace(main, { kind: 'canvas' })).toBe(true);
  });
});

describe('the places', () => {
  it('stop at the limit, keeping the canvas', () => {
    const { block } = open();
    const places = dropTargets(block('print'), 1);
    expect(summary(places)).toEqual(['at the top of “main”', 'loose on the canvas']);
  });

  it('are checked again before a drop', () => {
    const { block, workspace } = open();
    const print = block('print');
    const places = dropTargets(print);
    const intoFunction = places.find(
      (target) => target.kind === 'statements' && target.owner.id === 'fn',
    );
    if (intoFunction === undefined) {
      throw new Error('no place in the function');
    }
    expect(isStillValid(print, intoFunction)).toBe(true);
    expect(isStillValid(print, { kind: 'canvas' })).toBe(true);
    block('fn').dispose(false);
    expect(isStillValid(print, intoFunction)).toBe(false);
    expect(workspace.getBlockById('fn')).toBeNull();
  });
});
