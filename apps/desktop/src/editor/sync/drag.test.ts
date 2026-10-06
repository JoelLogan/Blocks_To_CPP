/**
 * Saving while a block is dragged (02 §2.4.1): the document is the canvas as it was just before
 * the drag, never the canvas in flux (the dragged block detached, drag previews standing in for
 * values or heading stacks). Blockly's own gestures drive the drags, through the editor's block
 * dragger.
 */
import type { BdmDocument } from '@blocks2cpp/b2c-core-wasm';
import * as Blockly from 'blockly/core';
import { afterEach, describe, expect, it } from 'vitest';

import { useAppStore } from '../../app/store';
import { B2cBlockDragger } from './drag';
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
