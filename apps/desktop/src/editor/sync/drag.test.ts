/**
 * Block drags through the editor's block dragger, driven by Blockly's own gestures with real
 * pointer events:
 *
 * - Saving while a block is dragged (02 §2.4.1): the document is the canvas as it was just before
 *   the drag, never the canvas in flux (the dragged block detached, drag previews standing in for
 *   values or heading stacks).
 * - A drop on a delete area (03 §3.3 "Placing blocks"): it deletes only the dragged blocks and
 *   never connects them, even when a place was previewed just before, and the area counts where
 *   it is when the drag starts, even if it moved since the canvas last recorded it. happy-dom
 *   lays nothing out, so a delete area here is a component with a screen rectangle of its own, as
 *   the toolbox's flyout and the trash can are in the app; the end-to-end drops on the real
 *   toolbox are e2e/specs/flows/tray-delete.e2e.ts.
 * - The feedback: the dragged blocks are faded, and over the toolbox the whole toolbox (the
 *   category column and the flyout) is tinted, with the category names still readable.
 */
import type { BdmDocument } from '@blocks2cpp/b2c-core-wasm';
import { b2cDarkTheme, b2cLightTheme } from '@blocks2cpp/blockly-ext';
import * as Blockly from 'blockly/core';
import { afterEach, describe, expect, it } from 'vitest';

import appCss from '../../app/app.css?raw';
import { contrastOf, declarationsOf, parseRules, themeTokens } from '../../app/layout/cssAudit';
import { useAppStore } from '../../app/store';
import { editorInjectOptions } from '../EditorWorkspace';
import { B2cBlockDragger, B2cBlockDragStrategy, isOverDeleteArea } from './drag';
import dragCss from './drag.css?raw';
import {
  canonicalText,
  disposeWorkspaces,
  EXAMPLE_PROJECTS,
  loadText,
  present,
  renderedWorkspace,
  setViewSize,
  startSession,
  testCore,
  type TestSession,
} from './testing';
import { allBlocks } from './traverse';
import { readTopBlocks } from './workspaceToBdm';

const core = await testCore();

let running: TestSession | null = null;

afterEach(() => {
  running?.dispose();
  running = null;
  disposeWorkspaces();
});

const POINTER = {
  bubbles: true,
  cancelable: true,
  pointerId: 1,
  isPrimary: true,
  button: 0,
  buttons: 1,
  pointerType: 'mouse',
} as const;

/** A shown editor canvas with the editor's block dragger, holding the guessing game. */
async function guessingGame(): Promise<TestSession> {
  const wasm = present(core, 'core');
  const workspace = renderedWorkspace({
    plugins: { blockDragger: B2cBlockDragger },
    move: { scrollbars: true, drag: true },
  });
  setViewSize(workspace, 1000, 700);
  Blockly.svgResize(workspace);
  const doc = loadText(wasm, present(EXAMPLE_PROJECTS['guessing_game.b2c'], 'guessing game'));
  running = startSession(wasm, doc, { workspace });
  await running.session.flush();
  await Blockly.renderManagement.finishQueuedRenders();
  return running;
}

/** Presses on `block` and moves the pointer far enough for Blockly to start dragging it. */
function startDrag(block: Blockly.BlockSvg): void {
  block.pathObject.svgPath.dispatchEvent(
    new PointerEvent('pointerdown', { ...POINTER, clientX: 100, clientY: 100 }),
  );
  // happy-dom gives the pressed block no focus, which is what selects it in a browser.
  Blockly.common.setSelected(block);
  document.dispatchEvent(
    new PointerEvent('pointermove', { ...POINTER, clientX: 400, clientY: 300 }),
  );
}

function drop(): void {
  document.dispatchEvent(new PointerEvent('pointerup', { ...POINTER, clientX: 400, clientY: 300 }));
}

function text(doc: BdmDocument): string {
  return canonicalText(present(core, 'core'), doc);
}

function topIds(doc: BdmDocument): string[] {
  return doc.modules[0]?.workspace.blocks.map((block) => block.id) ?? [];
}

describe.skipIf(core === null)('saving during a drag', () => {
  it('writes the canvas as it was before the drag, until the block is dropped', async () => {
    const { workspace, session } = await guessingGame();
    const svg = workspace as Blockly.WorkspaceSvg;
    const before = text(session.currentDocument());

    const dragged = present(svg.getBlockById('b004'), 'b004');
    startDrag(dragged);
    expect(svg.isDragging()).toBe(true);
    expect(dragged.getParent()).toBeNull();
    // A drag preview standing in for an input's value (as with a renderer that previews values).
    const marker = svg.newBlock('math.random_int');
    marker.setInsertionMarker(true);
    marker.initSvg();
    present(
      present(svg.getBlockById('b006'), 'b006').getInput('ITEM0')?.connection,
      'ITEM0',
    ).connect(present(marker.outputConnection, 'output'));

    const during = session.currentDocument();
    expect(topIds(during)).toEqual(['b011']);
    expect(text(during)).toBe(before);

    marker.dispose(false);
    drop();
    expect(svg.isDragging()).toBe(false);
    // Dropped away from main: now a loose block, and that is what a save writes.
    expect(topIds(session.currentDocument())).toEqual(['b011', 'b004']);
  });

  it('leaves out a block the drag takes from the toolbox', async () => {
    const { workspace, session } = await guessingGame();
    const svg = workspace as Blockly.WorkspaceSvg;
    const before = text(session.currentDocument());

    // What the flyout does when a drag starts on one of its blocks: a new block on the canvas.
    const created = Blockly.serialization.blocks.append(
      { type: 'control.forever', x: 600, y: 40 },
      svg,
    ) as Blockly.BlockSvg;
    startDrag(created);
    expect(svg.isDragging()).toBe(true);
    expect(text(session.currentDocument())).toBe(before);

    drop();
    await session.flush();
    expect(topIds(session.currentDocument())).toContain(created.id);
    expect(useAppStore.getState().project?.dirty).toBe(true);
  });

  it('reads the loose stack under a drag preview that heads it', async () => {
    const { workspace } = await guessingGame();
    const svg = workspace as Blockly.WorkspaceSvg;
    const loose = Blockly.serialization.blocks.append(
      { type: 'control.forever', id: 'loose1', x: 600, y: 600 },
      svg,
    ) as Blockly.BlockSvg;
    // Previewing a statement attached above the stack: the marker becomes its top block.
    const marker = Blockly.serialization.blocks.append({ type: 'control.forever' }, svg, {
      recordUndo: false,
    }) as Blockly.BlockSvg;
    marker.setInsertionMarker(true);
    present(marker.nextConnection, 'next').connect(present(loose.previousConnection, 'previous'));
    expect(svg.getTopBlocks(false)).not.toContain(loose);

    expect(readTopBlocks(svg).map((node) => node.id)).toEqual(['b011', 'loose1']);
  });
});

/**
 * Waits until Blockly has rendered new blocks and put their connections in its connection
 * database, which it does a millisecond after a load (the editor's loader does the same).
 */
async function placed(): Promise<void> {
  await Blockly.renderManagement.finishQueuedRenders();
  await new Promise((resolve) => setTimeout(resolve, 5));
}

/** Waits until Blockly has delivered (and recorded for undo) the events fired so far. */
function eventsDelivered(): Promise<void> {
  return new Promise((resolve) => {
    requestAnimationFrame(() => {
      setTimeout(() => {
        setTimeout(resolve, 0);
      }, 0);
    });
  });
}

/** A delete area left of `edge` (screen pixels), standing in for the toolbox's flyout. */
class EdgeDeleteArea extends Blockly.DeleteArea {
  override id = 'test.deleteArea';
  /**
   * The area's right edge. A test may move it without the canvas recording its drag targets
   * again, as a layout change moves the flyout's.
   */
  edge: number;

  constructor(edge: number) {
    super();
    this.edge = edge;
  }

  override getClientRect(): Blockly.utils.Rect {
    return new Blockly.utils.Rect(-100_000, 100_000, -100_000, this.edge);
  }
}

/** A drag target right of x = 1000 that deletes nothing. */
class EdgeDragTarget extends Blockly.DragTarget {
  override id = 'test.dragTarget';

  override getClientRect(): Blockly.utils.Rect {
    return new Blockly.utils.Rect(-100_000, 100_000, 1000, 100_000);
  }
}

/** Where a press on a block starts in the drop tests. */
const PRESS = { x: 100, y: 100 } as const;

/** Adds a delete area left of `edge`, as the toolbox's flyout is one, and records it there. */
function addDeleteArea(svg: Blockly.WorkspaceSvg, edge: number): EdgeDeleteArea {
  const { Capability } = Blockly.ComponentManager;
  const area = new EdgeDeleteArea(edge);
  svg.getComponentManager().addComponent({
    component: area,
    weight: 1,
    capabilities: [Capability.DELETE_AREA, Capability.DRAG_TARGET],
  });
  svg.recordDragTargets();
  return area;
}

/**
 * The guessing game at the editor's start zoom (0.9), with a loose copy of `print "Guess a
 * number…"` (b004, with its text shadow) to drag.
 */
async function withLooseBlock(): Promise<{ svg: Blockly.WorkspaceSvg; dragged: Blockly.BlockSvg }> {
  const { workspace } = await guessingGame();
  const svg = workspace as Blockly.WorkspaceSvg;
  svg.setScale(0.9);
  const state = present(
    Blockly.serialization.blocks.save(present(svg.getBlockById('b004'), 'b004'), {
      addNextBlocks: false,
      saveIds: false,
    }),
    'b004 state',
  );
  const dragged = Blockly.serialization.blocks.append(
    { ...state, x: 600, y: 400 },
    svg,
  ) as Blockly.BlockSvg;
  await placed();
  return { svg, dragged };
}

/**
 * The pointer position (from a press at {@link PRESS}) that brings `block`'s top connection to
 * 4 units right of and below the next connection of block `id`: a place the drag previews.
 */
function pointerNear(
  svg: Blockly.WorkspaceSvg,
  block: Blockly.BlockSvg,
  id: string,
): { x: number; y: number } {
  const local = present(block.previousConnection, 'previous');
  const target = present(svg.getBlockById(id)?.nextConnection, `${id} next`);
  return {
    x: PRESS.x + (target.x - local.x + 4) * svg.scale,
    y: PRESS.y + (target.y - local.y + 4) * svg.scale,
  };
}

/** Presses on `block` and selects `selected` (happy-dom gives the pressed block no focus). */
function press(block: Blockly.BlockSvg, selected: Blockly.BlockSvg = block): void {
  block.pathObject.svgPath.dispatchEvent(
    new PointerEvent('pointerdown', { ...POINTER, clientX: PRESS.x, clientY: PRESS.y }),
  );
  Blockly.common.setSelected(selected);
}

function moveTo(point: { x: number; y: number }): void {
  document.dispatchEvent(
    new PointerEvent('pointermove', { ...POINTER, clientX: point.x, clientY: point.y }),
  );
}

function release(point: { x: number; y: number }): void {
  document.dispatchEvent(
    new PointerEvent('pointerup', { ...POINTER, clientX: point.x, clientY: point.y }),
  );
}

/** Whether a drag preview (an insertion marker) is on the canvas (`getAllBlocks` omits them). */
function previewShown(svg: Blockly.WorkspaceSvg): boolean {
  return allBlocks(svg).some((block) => block.isInsertionMarker());
}

/** The rules of ./drag.css that show a drop on a delete area. */
const FADED = '.blocklyDragging.blocklyDraggingDelete';
const TINTED = '.blocklyToolboxDelete ~ .blocklyFlyout > .blocklyFlyoutBackground';
const COLUMN_TINTED = '.blocklyToolbox.blocklyToolboxDelete';

/** Whether `block` is drawn faded, as a block a drop will delete. */
function fadedForDelete(block: Blockly.BlockSvg): boolean {
  return block.getSvgRoot().matches(FADED);
}

/** The IDs of `main`'s statements, in order. */
function mainList(svg: Blockly.WorkspaceSvg): string[] {
  const ids: string[] = [];
  let block = present(svg.getBlockById('b011'), 'b011').getInputTargetBlock('BODY');
  while (block !== null) {
    ids.push(block.id);
    block = block.getNextBlock();
  }
  return ids;
}

describe.skipIf(core === null)('a drop on a delete area', () => {
  /** `main`'s statements in the guessing game. */
  const MAIN = ['b002', 'b003', 'b004', 'b010'];

  afterEach(() => {
    // A test that failed during a drag leaves it running: end it while its blocks still exist, so
    // that the next test starts afresh.
    const workspace = running?.workspace;
    if (workspace instanceof Blockly.WorkspaceSvg) {
      workspace.cancelCurrentGesture();
    }
  });

  it('deletes only the dragged block after the pointer moved there from a preview', async () => {
    const { svg, dragged } = await withLooseBlock();
    const near = pointerNear(svg, dragged, 'b003');
    addDeleteArea(svg, near.x - 2);
    const blocks = allBlocks(svg).length;

    press(dragged);
    moveTo(near);
    expect(previewShown(svg)).toBe(true);
    expect(fadedForDelete(dragged)).toBe(false);
    // Still within snapping distance of b003, but the pointer is over the delete area.
    const over = { x: near.x - 6, y: near.y };
    moveTo(over);
    expect(previewShown(svg)).toBe(false);
    expect(fadedForDelete(dragged)).toBe(true);
    release(over);

    expect(dragged.isDeadOrDying()).toBe(true);
    expect(mainList(svg)).toEqual(MAIN);
    // The block and its text shadow.
    expect(allBlocks(svg)).toHaveLength(blocks - 2);
  });

  it('deletes only the dragged block when released there without a last move', async () => {
    const { svg, dragged } = await withLooseBlock();
    const near = pointerNear(svg, dragged, 'b003');
    addDeleteArea(svg, near.x - 2);

    press(dragged);
    moveTo(near);
    expect(previewShown(svg)).toBe(true);
    release({ x: near.x - 6, y: near.y });

    expect(dragged.isDeadOrDying()).toBe(true);
    expect(mainList(svg)).toEqual(MAIN);
    expect(previewShown(svg)).toBe(false);
  });

  it('treats a dragged shadow as its block', async () => {
    const { svg, dragged } = await withLooseBlock();
    const shadow = dragged.getInputTargetBlock('ITEM0');
    if (!(shadow instanceof Blockly.BlockSvg) || !shadow.isShadow()) {
      throw new Error('print has no shadow in ITEM0');
    }
    const near = pointerNear(svg, dragged, 'b003');
    addDeleteArea(svg, near.x - 2);

    press(shadow, shadow);
    moveTo(near);
    expect(previewShown(svg)).toBe(true);
    moveTo({ x: near.x - 6, y: near.y });
    expect(previewShown(svg)).toBe(false);
    release({ x: near.x - 6, y: near.y });

    expect(dragged.isDeadOrDying()).toBe(true);
    expect(mainList(svg)).toEqual(MAIN);
  });

  it('is undone with one Ctrl+Z', async () => {
    const { svg, dragged } = await withLooseBlock();
    const id = dragged.id;
    const near = pointerNear(svg, dragged, 'b003');
    addDeleteArea(svg, near.x - 2);
    await eventsDelivered();

    press(dragged);
    moveTo(near);
    release({ x: near.x - 6, y: near.y });
    expect(svg.getBlockById(id)).toBeNull();
    await eventsDelivered();

    svg.undo(false);
    const back = present(svg.getBlockById(id), 'the dragged block');
    expect(back.getParent()).toBeNull();
    expect(back.getInputTargetBlock('ITEM0')?.isShadow()).toBe(true);
    expect(mainList(svg)).toEqual(MAIN);
  });

  it('finds the area where it is when the drag starts, not where it was recorded', async () => {
    const { svg, dragged } = await withLooseBlock();
    const id = dragged.id;
    // A click first: Blockly's first gesture on a canvas records its drag targets itself.
    press(dragged);
    release(PRESS);
    const near = pointerNear(svg, dragged, 'b003');
    const over = { x: near.x - 6, y: near.y };
    // Recorded with its edge far left of the pointer's way, then moved without a new record: the
    // toolbox's flyout after a rebuild of its contents, which Blockly records while the flyout is
    // not displayed (its edge then at the window's left edge).
    const area = addDeleteArea(svg, over.x - 100);
    area.edge = near.x - 2;
    await eventsDelivered();

    press(dragged);
    moveTo(near);
    expect(previewShown(svg)).toBe(true);
    moveTo(over);
    expect(fadedForDelete(dragged)).toBe(true);
    expect(previewShown(svg)).toBe(false);
    release(over);

    expect(svg.getBlockById(id)).toBeNull();
    expect(mainList(svg)).toEqual(MAIN);
    await eventsDelivered();

    svg.undo(false);
    const back = present(svg.getBlockById(id), 'the dragged block');
    expect(back.getParent()).toBeNull();
    expect(back.getInputTargetBlock('ITEM0')?.isShadow()).toBe(true);
    expect(mainList(svg)).toEqual(MAIN);
  });

  it('still connects a block dropped over the canvas', async () => {
    const { svg, dragged } = await withLooseBlock();
    const near = pointerNear(svg, dragged, 'b003');
    addDeleteArea(svg, near.x - 20);

    press(dragged);
    moveTo(near);
    expect(previewShown(svg)).toBe(true);
    release(near);

    expect(dragged.isDeadOrDying()).toBe(false);
    expect(mainList(svg)).toEqual(['b002', 'b003', dragged.id, 'b004', 'b010']);
  });

  it('counts only delete areas as places that delete', async () => {
    const { svg } = await withLooseBlock();
    svg.getComponentManager().addComponent({
      component: new EdgeDragTarget(),
      weight: 1,
      capabilities: [Blockly.ComponentManager.Capability.DRAG_TARGET],
    });
    addDeleteArea(svg, 400);
    const at = (x: number) => new PointerEvent('pointermove', { clientX: x, clientY: 300 });

    expect(isOverDeleteArea(svg, at(10))).toBe(true);
    expect(isOverDeleteArea(svg, at(600))).toBe(false);
    expect(svg.getDragTarget(at(1100))).not.toBeNull();
    expect(isOverDeleteArea(svg, at(1100))).toBe(false);
  });

  it('searches for no connection while the pointer is over the area', async () => {
    const { svg, dragged } = await withLooseBlock();
    const near = pointerNear(svg, dragged, 'b003');
    addDeleteArea(svg, near.x - 2);

    /** Exposes the protected search radius. */
    class Probe extends B2cBlockDragStrategy {
      radius(): number {
        return this.getSearchRadius();
      }
    }
    const probe = new Probe(dragged);
    const at = dragged.getRelativeToSurfaceXY();
    probe.startDrag();
    probe.drag(at, new PointerEvent('pointermove', { clientX: near.x - 6, clientY: near.y }));
    expect(probe.radius()).toBe(-1);
    probe.drag(at, new PointerEvent('pointermove', { clientX: near.x + 6, clientY: near.y }));
    expect(probe.radius()).toBeGreaterThan(0);
    probe.drag(at, new PointerEvent('pointermove', { clientX: near.x - 6, clientY: near.y }));
    // A move without a pointer event (a programmatic drag) is never over a delete area.
    probe.drag(at);
    expect(probe.radius()).toBeGreaterThan(0);
    probe.endDrag();
  });
});

describe('the delete-area feedback', () => {
  const rules = parseRules(dragCss);
  const tokens = themeTokens(rules);
  /** The tokens of the page (its text colour) and of ./drag.css. */
  const pageTokens = themeTokens([...parseRules(appCss), ...rules]);

  it('fades the dragged blocks', () => {
    const opacity = Number(declarationsOf(rules, FADED).get('opacity'));
    expect(opacity).toBeGreaterThan(0.2);
    expect(opacity).toBeLessThanOrEqual(0.6);
  });

  it.each(['light', 'dark'] as const)(
    'tints the flyout in the %s theme, keeping its headings readable',
    (theme) => {
      expect(declarationsOf(rules, TINTED).get('fill')).toBe('var(--b2c-flyout-delete)');
      // Important, to outweigh the fill the Blockly theme sets inline.
      expect(dragCss).toContain('fill: var(--b2c-flyout-delete) !important;');
      // The flyout's heading text: Zelos's label colour, or the dark theme's flyout foreground.
      const heading = theme === 'light' ? '#575e75' : '#e6e9f0';
      expect(contrastOf(tokens[theme], heading, 'var(--b2c-flyout-delete)')).toBeGreaterThanOrEqual(
        4.5,
      );
    },
  );

  it('tints the category column as it tints the flyout', () => {
    expect(declarationsOf(rules, COLUMN_TINTED).get('background-color')).toBe(
      'var(--b2c-flyout-delete)',
    );
    // Important, to outweigh the background the dark theme sets inline.
    expect(b2cDarkTheme.getComponentStyle('toolboxBackgroundColour')).not.toBeNull();
    expect(dragCss).toContain('background-color: var(--b2c-flyout-delete) !important;');
  });

  it.each([
    ['light', b2cLightTheme],
    ['dark', b2cDarkTheme],
  ] as const)('keeps the category names readable on the tint in the %s theme', (theme, blockly) => {
    // The theme's toolbox foreground, or (the light theme sets none) the page's text, which the
    // names inherit. A selected category's name is white on the category's own colour.
    const name = blockly.getComponentStyle('toolboxForegroundColour') ?? 'var(--b2c-text)';
    expect(contrastOf(pageTokens[theme], name, 'var(--b2c-flyout-delete)')).toBeGreaterThanOrEqual(
      4.5,
    );
  });

  it('marks the editor’s category column and flyout while a block over them would be deleted', () => {
    const workspace = renderedWorkspace(editorInjectOptions(Blockly.Themes.Zelos));
    const toolbox = present(workspace.getToolbox(), 'toolbox');
    if (!(toolbox instanceof Blockly.Toolbox)) {
      throw new Error('the editor has no Blockly toolbox');
    }
    const block = Blockly.serialization.blocks.append(
      { type: 'control.forever' },
      workspace,
    ) as Blockly.BlockSvg;
    const injection = workspace.getInjectionDiv();
    expect(injection.querySelector(TINTED)).toBeNull();
    expect(injection.querySelector(COLUMN_TINTED)).toBeNull();

    // What Blockly's dragger does while a block is dragged over the toolbox.
    expect(toolbox.wouldDelete(block)).toBe(true);
    toolbox.onDragEnter(block);
    expect(injection.querySelector(TINTED)).not.toBeNull();
    expect(injection.querySelector(COLUMN_TINTED)).not.toBeNull();
    toolbox.onDragExit(block);
    expect(injection.querySelector(TINTED)).toBeNull();
    expect(injection.querySelector(COLUMN_TINTED)).toBeNull();
  });
});
