/**
 * Block drags through the editor's block dragger, driven by Blockly's own gestures with real
 * pointer events:
 *
 * - Saving while a block is dragged (02 §2.4.1): the document is the canvas as it was just before
 *   the drag, never the canvas in flux (the dragged block detached, drag previews standing in for
 *   values or heading stacks).
 * - A drop on a delete area (03 §3.3 "Placing blocks"): it deletes only the dragged blocks and
 *   never connects them, even when a place was previewed just before, and the area counts where
 *   it is when the drag starts, even if it moved since the canvas last recorded it: a drag that
 *   starts before the frame that records the moved area records it at its start, and a drag that
 *   starts after reads no area's place (./dragTargets.ts). happy-dom lays nothing out, so a delete
 *   area here is a component with a screen rectangle of its own, as the toolbox's flyout and the
 *   trash can are in the app; the end-to-end drops on the real toolbox are
 *   e2e/specs/flows/tray-delete.e2e.ts.
 * - The feedback: the dragged blocks are faded, and over the toolbox the whole toolbox (the
 *   category column and the flyout) is tinted, with the category names still readable.
 * - A drop on the canvas is settled in the drag's event group (`handleBlockDrops`): a *set*
 *   taken from the toolbox gets a variable visible where it lands, a getter keeps its variable,
 *   one Undo takes the drop back, and the drop runs one preview (04 §4.2).
 */
import type { BdmDocument, PreviewResult } from '@blocks2cpp/b2c-core-wasm';
import { b2cDarkTheme, b2cLightTheme } from '@blocks2cpp/blockly-ext';
import * as Blockly from 'blockly/core';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';

import appCss from '../../app/app.css?raw';
import { contrastOf, declarationsOf, parseRules, themeTokens } from '../../app/layout/cssAudit';
import { useAppStore } from '../../app/store';
import { editorInjectOptions } from '../EditorWorkspace';
import type { PreviewService } from '../preview/service';
import { createToolboxPlugin, toolboxInjectOptions } from '../toolbox';
import { B2cBlockDragger, B2cBlockDragStrategy, handleBlockDrops, isOverDeleteArea } from './drag';
import dragCss from './drag.css?raw';
import { markDragTargetsStale } from './dragTargets';
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

  /**
   * How often the area's place on screen was read (in a browser, a read after any change to the
   * page lays it out at once): once for each record of the canvas's drag targets.
   */
  reads = 0;

  constructor(edge: number) {
    super();
    this.edge = edge;
  }

  override getClientRect(): Blockly.utils.Rect {
    this.reads += 1;
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
    // Recorded with its edge far left of the pointer's way, then moved, and the record marked
    // stale: the toolbox's flyout after a rebuild of its contents, which Blockly records while the
    // flyout is not displayed (its edge then at the window's left edge). The drag starts in the
    // same task, before the frame that would record it.
    const area = addDeleteArea(svg, over.x - 100);
    await eventsDelivered();
    area.edge = near.x - 2;
    markDragTargetsStale(svg);
    area.reads = 0;

    press(dragged);
    moveTo(near);
    // Recorded once, at the drag's start.
    expect(area.reads).toBe(1);
    expect(previewShown(svg)).toBe(true);
    moveTo(over);
    expect(fadedForDelete(dragged)).toBe(true);
    expect(previewShown(svg)).toBe(false);
    release(over);

    expect(svg.getBlockById(id)).toBeNull();
    expect(mainList(svg)).toEqual(MAIN);
    await eventsDelivered();
    // The frame then had nothing left to record.
    expect(area.reads).toBe(1);

    svg.undo(false);
    const back = present(svg.getBlockById(id), 'the dragged block');
    expect(back.getParent()).toBeNull();
    expect(back.getInputTargetBlock('ITEM0')?.isShadow()).toBe(true);
    expect(mainList(svg)).toEqual(MAIN);
  });

  it('reads no area’s place when the drag starts after the frame that recorded it', async () => {
    const { svg, dragged } = await withLooseBlock();
    const id = dragged.id;
    press(dragged);
    release(PRESS);
    const near = pointerNear(svg, dragged, 'b003');
    const over = { x: near.x - 6, y: near.y };
    const area = addDeleteArea(svg, over.x - 100);
    area.edge = near.x - 2;
    markDragTargetsStale(svg);
    area.reads = 0;
    await eventsDelivered();
    // Recorded once, in the frame.
    expect(area.reads).toBe(1);

    press(dragged);
    moveTo(near);
    moveTo(over);
    expect(fadedForDelete(dragged)).toBe(true);
    release(over);

    expect(svg.getBlockById(id)).toBeNull();
    expect(area.reads).toBe(1);
  });

  it('reads no area’s place for a drag when nothing moved', async () => {
    const { svg, dragged } = await withLooseBlock();
    const near = pointerNear(svg, dragged, 'b003');
    const area = addDeleteArea(svg, near.x - 20);
    press(dragged);
    release(PRESS);
    await eventsDelivered();
    area.reads = 0;

    for (let drag = 0; drag < 3; drag += 1) {
      press(dragged);
      moveTo({ x: PRESS.x + 200, y: PRESS.y + 40 * drag });
      release({ x: PRESS.x + 200, y: PRESS.y + 40 * drag });
    }
    await eventsDelivered();
    expect(dragged.isDeadOrDying()).toBe(false);
    expect(area.reads).toBe(0);
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

describe.skipIf(core === null)('a block dropped on the canvas', () => {
  afterEach(() => {
    const workspace = running?.workspace;
    if (workspace instanceof Blockly.WorkspaceSvg) {
      workspace.cancelCurrentGesture();
    }
  });

  it('is settled by the drop handler in the drag’s event group, and one Undo takes all back', async () => {
    const { svg, dragged } = await withLooseBlock();
    const id = dragged.id;
    const settled: { block: string; group: string }[] = [];
    const stop = handleBlockDrops(svg, (block) => {
      settled.push({ block: block.id, group: Blockly.Events.getGroup() });
      block.setFieldValue(false, 'NEWLINE');
    });
    await eventsDelivered();
    const groups = new Set<string>();
    const record = (event: Blockly.Events.Abstract): void => {
      if (!event.isUiEvent) {
        groups.add(event.group);
      }
    };
    svg.addChangeListener(record);

    const near = pointerNear(svg, dragged, 'b003');
    press(dragged);
    moveTo(near);
    release(near);
    stop();
    expect(Blockly.Events.getGroup()).toBe('');
    expect(settled).toHaveLength(1);
    expect(settled[0]?.block).toBe(id);
    expect(settled[0]?.group).not.toBe('');
    expect(mainList(svg)).toEqual(['b002', 'b003', id, 'b004', 'b010']);
    expect(dragged.getFieldValue('NEWLINE')).toBe('FALSE');
    await eventsDelivered();
    svg.removeChangeListener(record);
    expect([...groups]).toEqual([settled[0]?.group]);

    svg.undo(false);
    await eventsDelivered();
    expect(mainList(svg)).toEqual(['b002', 'b003', 'b004', 'b010']);
    expect(svg.getBlockById(id)?.getFieldValue('NEWLINE')).toBe('TRUE');
  });

  it('is not settled when the drop deletes it', async () => {
    const { svg, dragged } = await withLooseBlock();
    const near = pointerNear(svg, dragged, 'b003');
    addDeleteArea(svg, near.x - 2);
    const settled: string[] = [];
    const stop = handleBlockDrops(svg, (block) => settled.push(block.id));
    press(dragged);
    moveTo(near);
    release({ x: near.x - 6, y: near.y });
    stop();
    expect(dragged.isDeadOrDying()).toBe(true);
    expect(settled).toEqual([]);
  });
});

/**
 * The guessing game with `define greet (times)` beside main: main declares `secret` and `guess`
 * (b002, b003), greet a parameter `times` and a `create int count` (b_count) in its body.
 */
function gameWithGreet(): string {
  const doc = JSON.parse(
    present(EXAMPLE_PROJECTS['guessing_game.b2c'], 'guessing game'),
  ) as BdmDocument;
  doc.modules[0]?.workspace.blocks.push({
    id: 'greet',
    type: 'func.define',
    v: 1,
    x: 700,
    y: 40,
    extra: { params: [{ sym: 's_times', name: 'times', type: 'int', mode: 'copy' }] },
    fields: { NAME: { sym: 's_greet', name: 'greet' }, RETURNS: 'void' },
    statements: {
      BODY: [
        {
          id: 'b_count',
          type: 'var.declare',
          v: 1,
          fields: { TYPE: 'int', NAME: { sym: 's_count', name: 'count' }, CONST: false },
          inputs: { VALUE: { expr: [{ num: '0' }] } },
        },
      ],
    },
  });
  return JSON.stringify(doc);
}

/** A preview service on the core that counts the previews it runs. */
function countingPreviews(): PreviewService & { readonly runs: () => number } {
  const wasm = present(core, 'core');
  let runs = 0;
  return {
    runs: () => runs,
    preview(json, options): Promise<PreviewResult> {
      runs += 1;
      return Promise.resolve(wasm.preview(json, options));
    },
  };
}

/** Waits until the toolbox has rebuilt its flyout after the latest analysis. */
function toolboxSettled(): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, 200));
}

/**
 * The game with greet on an editor canvas with the editor's block dragger and the toolbox (its
 * plugin attached to the session's core and store), the toolbox's flyout built.
 */
async function withToolbox(): Promise<{
  svg: Blockly.WorkspaceSvg;
  flyout: Blockly.IFlyout;
  previews: ReturnType<typeof countingPreviews>;
}> {
  const wasm = present(core, 'core');
  const toolbox = toolboxInjectOptions();
  const svg = renderedWorkspace({
    toolbox: toolbox.toolbox,
    plugins: { ...toolbox.plugins, blockDragger: B2cBlockDragger },
    move: { scrollbars: true, drag: true },
  });
  setViewSize(svg, 1600, 900);
  Blockly.svgResize(svg);
  const previews = countingPreviews();
  const session = startSession(wasm, loadText(wasm, gameWithGreet()), {
    workspace: svg,
    service: previews,
  });
  const detach = createToolboxPlugin().attach({
    workspace: svg,
    store: useAppStore,
    core: () => wasm,
    selectBlock: () => undefined,
    activeModuleId: () => useAppStore.getState().project?.activeModuleId ?? '',
  });
  running = {
    ...session,
    dispose() {
      detach();
      session.dispose();
    },
  };
  await session.session.flush();
  await Blockly.renderManagement.finishQueuedRenders();
  await toolboxSettled();
  await placed();
  return { svg, flyout: present(svg.getFlyout(), 'flyout'), previews };
}

/** The flyout block of `type` (whose VAR is `ref`, when given). */
function flyoutBlock(flyout: Blockly.IFlyout, type: string, ref?: string): Blockly.BlockSvg {
  return present(
    flyout
      .getWorkspace()
      .getTopBlocks(true)
      .find(
        (block) => block.type === type && (ref === undefined || block.getFieldValue('VAR') === ref),
      ),
    `${type} in the flyout`,
  );
}

/**
 * Drags `original` out of the toolbox's flyout with the pointer and drops it with its `own`
 * connection 4 units right of and below connection `target`, where Blockly connects it. Returns
 * the block created on the canvas.
 */
async function dragFromFlyout(
  svg: Blockly.WorkspaceSvg,
  original: Blockly.BlockSvg,
  own: 'previous' | 'output',
  target: Blockly.RenderedConnection,
): Promise<Blockly.BlockSvg> {
  const before = new Set(svg.getAllBlocks(false).map((block) => block.id));
  original.pathObject.svgPath.dispatchEvent(
    new PointerEvent('pointerdown', { ...POINTER, clientX: PRESS.x, clientY: PRESS.y }),
  );
  // Past the drag radius: Blockly creates the block on the canvas and drags it.
  const started = { x: PRESS.x + 40, y: PRESS.y };
  moveTo(started);
  const created = present(
    svg.getAllBlocks(false).find((block) => !before.has(block.id) && block.type === original.type),
    'the block taken from the toolbox',
  );
  const connection = present(
    own === 'previous' ? created.previousConnection : created.outputConnection,
    `${own} connection`,
  );
  const at = created.getRelativeToSurfaceXY();
  const offset = connection.getOffsetInBlock();
  const goal = {
    x: started.x + (target.x - offset.x + 4 - at.x) * svg.scale,
    y: started.y + (target.y - offset.y + 4 - at.y) * svg.scale,
  };
  moveTo(goal);
  release(goal);
  await placed();
  return created;
}

describe.skipIf(core === null)('a block dragged from the toolbox', () => {
  /**
   * Blockly ripples a connection it makes with an SVG animation; happy-dom has no SMIL, so the
   * animation's `beginElement` is a no-op here.
   */
  const svgElement = SVGElement.prototype as SVGElement & { beginElement?: () => void };
  const hadBeginElement = typeof svgElement.beginElement === 'function';

  beforeEach(() => {
    if (!hadBeginElement) {
      svgElement.beginElement = () => undefined;
    }
  });

  afterEach(() => {
    const workspace = running?.workspace;
    if (workspace instanceof Blockly.WorkspaceSvg) {
      workspace.cancelCurrentGesture();
    }
    if (!hadBeginElement) {
      delete svgElement.beginElement;
    }
  });

  it('is a set preset to main’s guess that takes a variable of greet, in one drop and one preview', async () => {
    const { svg, flyout, previews } = await withToolbox();
    const preset = flyoutBlock(flyout, 'var.set');
    expect(preset.getFieldValue('VAR')).toBe('s_guess');
    await eventsDelivered();
    svg.clearUndo();
    const runs = previews.runs();

    const set = await dragFromFlyout(
      svg,
      preset,
      'previous',
      present(svg.getBlockById('b_count')?.nextConnection, 'below create count'),
    );
    expect(set.getPreviousBlock()?.id).toBe('b_count');
    // In greet, count and times are visible (and fit): count comes first by name.
    expect(set.getFieldValue('VAR')).toBe('s_count');
    await running?.session.flush();
    await eventsDelivered();
    // One preview for the drop and its new variable.
    expect(previews.runs() - runs).toBe(1);
    const diagnostics = useAppStore.getState().analysis.preview?.diagnostics ?? [];
    expect(diagnostics.filter((diagnostic) => diagnostic.primary.block === set.id)).toEqual([]);

    // One Undo takes the drop back: the block is gone, greet's body as it was.
    svg.undo(false);
    await eventsDelivered();
    expect(svg.getBlockById(set.id)).toBeNull();
    expect(svg.getBlockById('b_count')?.getNextBlock()).toBeNull();
  });

  it('is a getter of greet’s times that keeps it in main, where it is B2C-E0203', async () => {
    const { svg, flyout } = await withToolbox();
    const getter = flyoutBlock(flyout, 'var.get', 's_times');
    // The print of main (b004): its text slot.
    const print = present(svg.getBlockById('b004'), 'b004');
    const dropped = await dragFromFlyout(
      svg,
      getter,
      'output',
      present(print.getInput('ITEM0')?.connection as Blockly.RenderedConnection, 'ITEM0'),
    );
    expect(dropped.getParent()?.id).toBe('b004');
    expect(dropped.getFieldValue('VAR')).toBe('s_times');
    await running?.session.flush();
    const codes = (useAppStore.getState().analysis.preview?.diagnostics ?? [])
      .filter((diagnostic) => diagnostic.primary.block === dropped.id)
      .map((diagnostic) => diagnostic.code);
    expect(codes).toEqual(['B2C-E0203']);
  });
});
