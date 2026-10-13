/**
 * When the canvas records where its drag targets are (./dragTargets.ts): a stale record is made
 * again once, in the next animation frame after the renders Blockly queued, or at the start of a
 * drag that comes before that frame; marking reads nothing, and a current record is not made
 * again.
 */
import * as Blockly from 'blockly/core';
import { afterEach, describe, expect, it, vi } from 'vitest';

import { markDragTargetsStale, recordDragTargetsIfStale } from './dragTargets';
import { disposeWorkspaces, renderedWorkspace } from './testing';

afterEach(() => {
  vi.restoreAllMocks();
  disposeWorkspaces();
});

/** Resolves in the next animation frame. */
function nextFrame(): Promise<void> {
  return new Promise<void>((resolve) => {
    requestAnimationFrame(() => {
      resolve();
    });
  });
}

/** Waits for the next animation frame, for Blockly's queued renders, and for what follows them. */
async function afterNextFrame(): Promise<void> {
  await nextFrame();
  await Blockly.renderManagement.finishQueuedRenders();
  await Promise.resolve();
}

/** A shown canvas, its records of its drag targets counted from now on. */
function countedCanvas(): { workspace: Blockly.WorkspaceSvg; record: { calls: number } } {
  const workspace = renderedWorkspace({});
  const record = { calls: 0 };
  vi.spyOn(workspace, 'recordDragTargets').mockImplementation(() => {
    record.calls += 1;
  });
  return { workspace, record };
}

describe('a stale record of the drag targets', () => {
  it('is made once in the next animation frame, however often it was marked', async () => {
    const { workspace, record } = countedCanvas();
    markDragTargetsStale(workspace);
    markDragTargetsStale(workspace);
    markDragTargetsStale(workspace);
    expect(record.calls).toBe(0);
    await afterNextFrame();
    expect(record.calls).toBe(1);
    await afterNextFrame();
    expect(record.calls).toBe(1);
    // Marked again: one more record, in the frame after.
    markDragTargetsStale(workspace);
    await afterNextFrame();
    expect(record.calls).toBe(2);
  });

  it('is made after the renders Blockly queued for that frame', async () => {
    const workspace = renderedWorkspace({});
    // Appending renders the block at once.
    const block = Blockly.serialization.blocks.append(
      { type: 'control.forever' },
      workspace,
    ) as Blockly.BlockSvg;
    const order: string[] = [];
    vi.spyOn(workspace, 'recordDragTargets').mockImplementation(() => {
      order.push('record');
    });
    vi.spyOn(block, 'renderEfficiently').mockImplementation(() => {
      order.push('render');
    });
    markDragTargetsStale(workspace);
    // Queued after the mark, so Blockly's animation frame comes after the editor's.
    void block.queueRender();
    await afterNextFrame();
    expect(order).toEqual(['render', 'record']);
  });

  it('is made at the start of a drag before that frame, and the frame then records nothing', async () => {
    const { workspace, record } = countedCanvas();
    expect(recordDragTargetsIfStale(workspace)).toBe(false);
    markDragTargetsStale(workspace);
    expect(recordDragTargetsIfStale(workspace)).toBe(true);
    expect(recordDragTargetsIfStale(workspace)).toBe(false);
    expect(record.calls).toBe(1);
    await afterNextFrame();
    expect(record.calls).toBe(1);
  });

  it('is current for a drag that starts after the frame', async () => {
    const { workspace, record } = countedCanvas();
    markDragTargetsStale(workspace);
    await afterNextFrame();
    expect(recordDragTargetsIfStale(workspace)).toBe(false);
    expect(record.calls).toBe(1);
  });

  it('is kept for each canvas apart', async () => {
    const first = countedCanvas();
    const second = countedCanvas();
    markDragTargetsStale(first.workspace);
    expect(recordDragTargetsIfStale(second.workspace)).toBe(false);
    await afterNextFrame();
    expect(first.record.calls).toBe(1);
    expect(second.record.calls).toBe(0);
  });

  it('is not made for a canvas disposed before the frame', async () => {
    // A canvas of the test's own, which it disposes itself.
    const host = document.createElement('div');
    document.body.append(host);
    const workspace = Blockly.inject(host, { renderer: 'zelos', sounds: false });
    const record = vi.spyOn(workspace, 'recordDragTargets');
    markDragTargetsStale(workspace);
    workspace.dispose();
    host.remove();
    await afterNextFrame();
    expect(record).not.toHaveBeenCalled();
  });
});
