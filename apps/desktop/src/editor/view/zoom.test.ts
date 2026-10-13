/**
 * The canvas's zoom and view (./zoom.ts) on a canvas injected with the editor's options, 1000 × 700
 * pixels: a zoom step and *Reset zoom* keep the middle of the view, *Show all blocks* never changes
 * the zoom, and the wheel scrolls while Ctrl + wheel zooms (decision D10 of the M2 polish plan).
 */
import { b2cLightTheme } from '@blocks2cpp/blockly-ext';
import * as Blockly from 'blockly/core';
import { afterEach, describe, expect, it, vi } from 'vitest';

import { editorInjectOptions } from '../EditorWorkspace';
import { disposeWorkspaces, renderedWorkspace, setViewSize } from '../sync/testing';
import {
  RESET_SCALE,
  resetZoom,
  SHOW_ALL_MARGIN,
  showAllBlocks,
  viewStart,
  zoomAnnouncement,
  zoomPercent,
  zoomStep,
} from './zoom';

afterEach(() => {
  disposeWorkspaces();
});

/** A canvas as the editor injects it, 1000 × 700 pixels, at the start zoom (0.9). */
function editorCanvas(): Blockly.WorkspaceSvg {
  const workspace = renderedWorkspace(editorInjectOptions(b2cLightTheme));
  setViewSize(workspace, 1000, 700);
  Blockly.svgResize(workspace);
  return workspace;
}

/** A rendered block with its top-left corner at (`x`, `y`) in workspace units. */
function blockAt(workspace: Blockly.WorkspaceSvg, x: number, y: number): Blockly.BlockSvg {
  const block = workspace.newBlock('control.forever');
  block.initSvg();
  block.render();
  block.moveTo(new Blockly.utils.Coordinate(x, y));
  return block;
}

/** The visible canvas in workspace units. */
function view(workspace: Blockly.WorkspaceSvg) {
  return workspace.getMetricsManager().getViewMetrics(true);
}

/** The middle of the visible canvas in workspace units. */
function centre(workspace: Blockly.WorkspaceSvg): { x: number; y: number } {
  const shown = view(workspace);
  return { x: shown.left + shown.width / 2, y: shown.top + shown.height / 2 };
}

/** Scrolls so that (`x`, `y`) is in the middle of the visible canvas. */
function centreOn(workspace: Blockly.WorkspaceSvg, x: number, y: number): void {
  const shown = view(workspace);
  workspace.scroll(
    -(x - shown.width / 2) * workspace.scale,
    -(y - shown.height / 2) * workspace.scale,
  );
}

/** Spreads blocks over a wide area, so the view can go anywhere in it. */
function spreadBlocks(workspace: Blockly.WorkspaceSvg): void {
  blockAt(workspace, -800, -600);
  blockAt(workspace, 800, 600);
}

describe('a zoom step and Reset zoom', () => {
  it('zoom by one step around the middle of the view, within the zoom range', () => {
    const workspace = editorCanvas();
    spreadBlocks(workspace);
    centreOn(workspace, 120, -80);
    const before = centre(workspace);
    expect(before.x).toBeCloseTo(120, 0);

    expect(zoomStep(workspace, 'in')).toBeCloseTo(0.9 * 1.2, 10);
    expect(zoomStep(workspace, 'in')).toBeCloseTo(0.9 * 1.44, 10);
    expect(Math.abs(centre(workspace).x - before.x)).toBeLessThan(1);
    expect(Math.abs(centre(workspace).y - before.y)).toBeLessThan(1);
    expect(zoomStep(workspace, 'out')).toBeCloseTo(0.9 * 1.2, 10);
    expect(Math.abs(centre(workspace).x - before.x)).toBeLessThan(1);

    // The zoom range of the project format (0.1–4.0).
    workspace.setScale(3.9);
    expect(zoomStep(workspace, 'in')).toBe(4);
    workspace.setScale(0.11);
    expect(zoomStep(workspace, 'out')).toBe(0.1);
  });

  it('go back to 100 % (not the 90 % start zoom), keeping the middle within 1 unit', () => {
    const workspace = editorCanvas();
    spreadBlocks(workspace);
    expect(workspace.scale).toBe(0.9);
    centreOn(workspace, 200, 150);
    zoomStep(workspace, 'in');
    zoomStep(workspace, 'in');
    const before = centre(workspace);

    expect(resetZoom(workspace)).toBe(RESET_SCALE);
    expect(workspace.scale).toBe(1);
    expect(Math.abs(centre(workspace).x - before.x)).toBeLessThan(1);
    expect(Math.abs(centre(workspace).y - before.y)).toBeLessThan(1);

    // From below 100 % too.
    workspace.setScale(0.5);
    centreOn(workspace, -300, 250);
    const zoomedOut = centre(workspace);
    resetZoom(workspace);
    expect(workspace.scale).toBe(1);
    expect(Math.abs(centre(workspace).x - zoomedOut.x)).toBeLessThan(1);
    expect(Math.abs(centre(workspace).y - zoomedOut.y)).toBeLessThan(1);
  });
});

describe('Show all blocks', () => {
  it('keeps the zoom and centres blocks that fit in the view', () => {
    const workspace = editorCanvas();
    const block = blockAt(workspace, 1500, 1200);
    zoomStep(workspace, 'in');
    zoomStep(workspace, 'in');
    const scale = workspace.scale;
    centreOn(workspace, 1000, 800);

    expect(showAllBlocks(workspace)).toBe(true);
    expect(workspace.scale).toBe(scale);
    const box = block.getBoundingRectangle();
    expect(Math.abs(centre(workspace).x - (box.left + box.right) / 2)).toBeLessThan(1);
    expect(Math.abs(centre(workspace).y - (box.top + box.bottom) / 2)).toBeLessThan(1);
  });

  it('keeps the zoom and shows the top-left corner, with a margin, of blocks that do not fit', () => {
    const workspace = editorCanvas();
    blockAt(workspace, -100, -50);
    blockAt(workspace, 3000, 2500);
    zoomStep(workspace, 'in');
    const scale = workspace.scale;
    centreOn(workspace, 2000, 2000);

    showAllBlocks(workspace);
    expect(workspace.scale).toBe(scale);
    const box = workspace.getBlocksBoundingBox();
    expect(Math.abs(view(workspace).left - (box.left - SHOW_ALL_MARGIN))).toBeLessThan(1);
    expect(Math.abs(view(workspace).top - (box.top - SHOW_ALL_MARGIN))).toBeLessThan(1);
  });

  it('centres each way the blocks fit: a tall, narrow program is centred across and shown from its top', () => {
    const workspace = editorCanvas();
    blockAt(workspace, 400, 0);
    blockAt(workspace, 400, 3000);
    const scale = workspace.scale;

    showAllBlocks(workspace);
    expect(workspace.scale).toBe(scale);
    const box = workspace.getBlocksBoundingBox();
    expect(Math.abs(centre(workspace).x - (box.left + box.right) / 2)).toBeLessThan(1);
    expect(Math.abs(view(workspace).top - (box.top - SHOW_ALL_MARGIN))).toBeLessThan(1);
  });

  it('leaves an empty canvas, or one without a size, as it is', () => {
    const workspace = editorCanvas();
    const before = { x: workspace.scrollX, y: workspace.scrollY, scale: workspace.scale };
    expect(showAllBlocks(workspace)).toBe(false);
    expect({ x: workspace.scrollX, y: workspace.scrollY, scale: workspace.scale }).toEqual(before);

    blockAt(workspace, 5000, 5000);
    setViewSize(workspace, 0, 0);
    Blockly.svgResize(workspace);
    const hidden = { x: workspace.scrollX, y: workspace.scrollY };
    expect(showAllBlocks(workspace)).toBe(false);
    expect({ x: workspace.scrollX, y: workspace.scrollY }).toEqual(hidden);
  });

  it('starts the view on one axis: centred when the blocks fit, else a margin before them', () => {
    expect(viewStart(100, 300, 600)).toBe(-100);
    expect(viewStart(100, 800, 600)).toBe(100 - SHOW_ALL_MARGIN);
    // Exactly as long as the view: centred, edge to edge.
    expect(viewStart(0, 600, 600)).toBe(0);
  });
});

describe('the wheel', () => {
  /** A wheel event over the canvas at (500, 300), as a browser sends it. */
  function wheel(deltaY: number, ctrlKey: boolean): WheelEvent {
    const event = new WheelEvent('wheel', { deltaY, bubbles: true, cancelable: true });
    // happy-dom's WheelEvent leaves out the pointer's place and the modifier keys it is given.
    Object.defineProperties(event, {
      clientX: { value: 500 },
      clientY: { value: 300 },
      ctrlKey: { value: ctrlKey },
    });
    return event;
  }

  it('zooms around the pointer with Ctrl held, and keeps the webview from zooming the page', () => {
    const workspace = editorCanvas();
    // happy-dom's SVG points cannot be transformed; Blockly maps the pointer onto the canvas.
    vi.spyOn(workspace.getParentSvg(), 'createSVGPoint').mockImplementation(() => new DOMPoint());
    const zoom = vi.spyOn(workspace, 'zoom');
    const event = wheel(-100, true);
    workspace.getSvgGroup().dispatchEvent(event);

    // Two notches of 50 pixels: two zoom steps around the pointer.
    expect(zoom).toHaveBeenCalledWith(500, 300, 2);
    expect(workspace.scale).toBeCloseTo(0.9 * 1.44, 10);
    expect(event.defaultPrevented).toBe(true);
  });

  it('scrolls without Ctrl and leaves the zoom', () => {
    const workspace = editorCanvas();
    spreadBlocks(workspace);
    const zoom = vi.spyOn(workspace, 'zoom');
    const before = workspace.scrollY;
    workspace.getSvgGroup().dispatchEvent(wheel(100, false));

    expect(zoom).not.toHaveBeenCalled();
    expect(workspace.scale).toBe(0.9);
    expect(workspace.scrollY).toBe(before - 100);
  });
});

describe('the zoom as people read it', () => {
  it('is a whole percentage', () => {
    expect(zoomPercent(1)).toBe('100%');
    expect(zoomPercent(0.9 * 1.2)).toBe('108%');
    expect(zoomPercent(0.9 * 1.44)).toBe('130%');
    expect(zoomAnnouncement(0.75)).toBe('Zoom 75%');
  });
});
