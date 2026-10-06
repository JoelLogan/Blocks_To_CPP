/** Finding blocks, fields, connections and grab points on screen for the end-to-end tests. */
import * as Blockly from 'blockly/core';
import { afterEach, describe, expect, it } from 'vitest';

import { guessingGame } from '../editor/diagnostics/testing';
import { loadModule } from '../editor/sync/bdmToWorkspace';
import { disposeWorkspaces, renderedWorkspace } from '../editor/sync/testing';
import {
  blockElement,
  connectionPoint,
  fieldElement,
  findBlock,
  flyoutBlockId,
  grabPoint,
} from './locate';

afterEach(() => {
  disposeWorkspaces();
});

/** A canvas with the guessing game, and a flyout with two loops and a print. */
function canvas(): Blockly.WorkspaceSvg {
  const workspace = renderedWorkspace({
    toolbox: {
      kind: 'flyoutToolbox',
      contents: [
        { kind: 'block', type: 'control.while' },
        { kind: 'block', type: 'control.while', fields: { MODE: 'until' } },
        { kind: 'block', type: 'io.print' },
      ],
    },
  });
  loadModule(workspace, guessingGame(), 'mod_main');
  return workspace;
}

function isFinitePoint(point: { x: number; y: number } | null): boolean {
  return point !== null && Number.isFinite(point.x) && Number.isFinite(point.y);
}

describe('finding blocks and fields', () => {
  it('finds blocks on the canvas and in the flyout, and their SVG groups', () => {
    const workspace = canvas();
    expect(findBlock(workspace, 'b005')?.type).toBe('io.ask');
    expect(blockElement(workspace, 'b005')).toBe(findBlock(workspace, 'b005')?.getSvgRoot());
    expect(blockElement(workspace, 'missing')).toBeNull();

    const until = flyoutBlockId(workspace, 'control.while', { MODE: 'until' });
    const plain = flyoutBlockId(workspace, 'control.while');
    expect(until).not.toBeNull();
    expect(plain).not.toBeNull();
    expect(until).not.toBe(plain);
    const flyoutBlock = findBlock(workspace, until ?? '');
    expect(flyoutBlock?.isInFlyout).toBe(true);
    expect(flyoutBlock?.getFieldValue('MODE')).toBe('until');
    expect(flyoutBlockId(workspace, 'io.ask')).toBeNull();
    expect(flyoutBlockId(workspace, 'io.print', { SEP: 'comma' })).toBeNull();
  });

  it('has no flyout blocks without a toolbox', () => {
    const workspace = renderedWorkspace();
    expect(flyoutBlockId(workspace, 'io.print')).toBeNull();
  });

  it('finds a field of a block, or of the block in one of its inputs', () => {
    const workspace = canvas();
    const ask = findBlock(workspace, 'b005');
    expect(fieldElement(workspace, 'b005', 'VAR')).toBe(ask?.getField('VAR')?.getSvgRoot());
    const slot = ask?.getInputTargetBlock('PROMPT');
    expect(fieldElement(workspace, 'b005', 'VALUE', 'PROMPT')).toBe(
      slot?.getField('VALUE')?.getSvgRoot(),
    );
    expect(fieldElement(workspace, 'b005', 'NOPE')).toBeNull();
    expect(fieldElement(workspace, 'b005', 'VALUE', 'NOPE')).toBeNull();
    expect(fieldElement(workspace, 'missing', 'VAR')).toBeNull();
  });
});

describe('connectionPoint', () => {
  it('locates previous, next, output and input connections', () => {
    const workspace = canvas();
    for (const name of ['previous', 'next', 'BODY']) {
      expect(isFinitePoint(connectionPoint(workspace, 'b010', name)), name).toBe(true);
    }
    expect(isFinitePoint(connectionPoint(workspace, 'b001', 'output'))).toBe(true);
    expect(connectionPoint(workspace, 'b010', 'output')).toBeNull();
    expect(connectionPoint(workspace, 'b011', 'previous')).toBeNull();
    expect(connectionPoint(workspace, 'b010', 'NOPE')).toBeNull();
    expect(connectionPoint(workspace, 'missing', 'next')).toBeNull();
  });

  it('follows the workspace coordinates of the connection', () => {
    const workspace = canvas();
    const loop = findBlock(workspace, 'b011');
    const before = connectionPoint(workspace, 'b011', 'BODY');
    loop?.moveBy(40, 20);
    const after = connectionPoint(workspace, 'b011', 'BODY');
    const scale = workspace.scale;
    expect((after?.x ?? 0) - (before?.x ?? 0)).toBeCloseTo(40 * scale);
    expect((after?.y ?? 0) - (before?.y ?? 0)).toBeCloseTo(20 * scale);
  });
});

describe('grabPoint', () => {
  /** Lays the block's outline out at 10..40 × 20..50 (happy-dom lays nothing out itself). */
  function outlineOf(workspace: Blockly.WorkspaceSvg, id: string): SVGElement {
    const block = findBlock(workspace, id);
    if (block === null) {
      throw new Error(`no block ${id}`);
    }
    const outline = block.pathObject.svgPath;
    outline.getBoundingClientRect = () => new DOMRect(10, 20, 30, 30);
    return outline;
  }

  it('returns the first point, row by row, where the block itself is on top', () => {
    const workspace = canvas();
    const outline = outlineOf(workspace, 'b010');
    const point = grabPoint(workspace, 'b010', (x, y) => (x >= 16 && y >= 26 ? outline : null));
    expect(point).toEqual({ x: 16, y: 26 });
  });

  it('returns null when the block is covered everywhere, or not there', () => {
    const workspace = canvas();
    const outline = outlineOf(workspace, 'b010');
    const cover = document.createElement('div');
    expect(grabPoint(workspace, 'b010', () => cover)).toBeNull();
    expect(grabPoint(workspace, 'missing', () => outline)).toBeNull();
  });

  it('gives up after a bounded number of tries on a huge block', () => {
    const workspace = canvas();
    const block = findBlock(workspace, 'b011');
    if (block === null) {
      throw new Error('no main');
    }
    block.pathObject.svgPath.getBoundingClientRect = () => new DOMRect(0, 0, 100_000, 100_000);
    let calls = 0;
    expect(
      grabPoint(workspace, 'b011', () => {
        calls += 1;
        return null;
      }),
    ).toBeNull();
    expect(calls).toBe(4000);
  });
});
