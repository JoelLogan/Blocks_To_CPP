/**
 * Two-way highlighting on the block side: the selected and hovered blocks go to the store, and a
 * click in the code or on a problem selects its block, or the collapsed block that shows it.
 */
import * as Blockly from 'blockly/core';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import type { SelectBlockOptions } from '../../app/editor-types';
import { resetAppStore, useAppStore } from '../../app/store';
import { projectFixture } from '../../app/testing/fixtures';
import {
  blockById,
  buildBlocks,
  disposeWorkspaces,
  editorContext,
  editorHandle,
  guessingGame,
  renderedWorkspace,
  settle,
} from '../diagnostics/testing';
import {
  attachHighlightTracking,
  blockIdAt,
  documentBlockId,
  revealTarget,
  selectBlockFromCode,
  selectBlockFromProblem,
  visibleHolder,
} from '.';

function guessingGameCanvas(): Blockly.WorkspaceSvg {
  const workspace = renderedWorkspace();
  buildBlocks(workspace, guessingGame().modules[0]?.workspace.blocks ?? []);
  return workspace;
}

function ui() {
  return useAppStore.getState().ui;
}

beforeEach(() => {
  resetAppStore();
  useAppStore.getState().actions.setProject(projectFixture({ document: guessingGame() }));
});

afterEach(() => {
  disposeWorkspaces();
});

describe('following the blocks into the store', () => {
  it('records the selected block, and the block whose slot a selected shadow is', async () => {
    const workspace = guessingGameCanvas();
    const detach = attachHighlightTracking(editorContext(workspace));

    Blockly.getFocusManager().focusNode(blockById(workspace, 'b005'));
    await settle();
    expect(ui().selection).toBe('b005');

    const shadow = blockById(workspace, 'b010').getInputTargetBlock('COND');
    expect(shadow?.isShadow()).toBe(true);
    expect(documentBlockId(workspace, shadow?.id ?? null)).toBe('b010');

    Blockly.getFocusManager().focusNode(blockById(workspace, 'b009'));
    await settle();
    expect(ui().selection).toBe('b009');
    detach();
  });

  it('forgets a selected or hovered block that is deleted', async () => {
    const workspace = guessingGameCanvas();
    const detach = attachHighlightTracking(editorContext(workspace));
    useAppStore.getState().actions.setUi({ selection: 'b007', hoverBlock: 'b007' });
    blockById(workspace, 'b007').dispose(true);
    await settle();
    expect(ui().selection).toBeNull();
    expect(ui().hoverBlock).toBeNull();

    // Deleting the focused block moves Blockly's focus (and so the selection) to its parent.
    Blockly.getFocusManager().focusNode(blockById(workspace, 'b006'));
    await settle();
    expect(ui().selection).toBe('b006');
    blockById(workspace, 'b006').dispose(true);
    await settle();
    expect(ui().selection).not.toBe('b006');
    detach();
  });

  it('records the block under the pointer, and nothing once the pointer leaves', () => {
    const workspace = guessingGameCanvas();
    const detach = attachHighlightTracking(editorContext(workspace));
    const surface = workspace.getParentSvg();

    const askPath = blockById(workspace, 'b005').getSvgRoot().querySelector('path');
    askPath?.dispatchEvent(new PointerEvent('pointerover', { bubbles: true }));
    expect(ui().hoverBlock).toBe('b005');

    // A shadow's drawing stands for the block whose slot it fills.
    const shadow = blockById(workspace, 'b010').getInputTargetBlock('COND') as Blockly.BlockSvg;
    shadow.getSvgRoot().dispatchEvent(new PointerEvent('pointerover', { bubbles: true }));
    expect(ui().hoverBlock).toBe('b010');

    surface.dispatchEvent(new PointerEvent('pointerover', { bubbles: true }));
    expect(ui().hoverBlock).toBeNull();

    askPath?.dispatchEvent(new PointerEvent('pointerover', { bubbles: true }));
    surface.dispatchEvent(new PointerEvent('pointerleave'));
    expect(ui().hoverBlock).toBeNull();

    askPath?.dispatchEvent(new PointerEvent('pointerover', { bubbles: true }));
    detach();
    expect(ui().hoverBlock).toBeNull();
    askPath?.dispatchEvent(new PointerEvent('pointerover', { bubbles: true }));
    expect(ui().hoverBlock).toBeNull();
  });

  it('forgets the hovered block when another module is shown', () => {
    const workspace = guessingGameCanvas();
    const detach = attachHighlightTracking(editorContext(workspace));
    useAppStore.getState().actions.setUi({ hoverBlock: 'b005' });
    useAppStore.getState().actions.updateProject({ activeModuleId: 'mod_other' });
    expect(ui().hoverBlock).toBeNull();
    detach();
  });

  it('finds block IDs only on block drawings', () => {
    expect(blockIdAt(null)).toBeNull();
    expect(blockIdAt(document.body)).toBeNull();
    const group = document.createElementNS('http://www.w3.org/2000/svg', 'g');
    group.setAttribute('data-id', 'b001');
    const child = document.createElementNS('http://www.w3.org/2000/svg', 'path');
    group.append(child);
    expect(blockIdAt(child)).toBe('b001');
    const workspace = guessingGameCanvas();
    expect(documentBlockId(workspace, null)).toBeNull();
    expect(documentBlockId(workspace, 'nope')).toBeNull();
  });
});

describe('selecting blocks from the code and from Problems', () => {
  function handleWithSpy(workspace: Blockly.WorkspaceSvg) {
    const calls: [string, SelectBlockOptions | undefined][] = [];
    const handle = editorHandle(workspace, (id, opts) => {
      calls.push([id, opts]);
    });
    return { handle, calls };
  }

  it('selects the clicked block and scrolls it into view', () => {
    const workspace = guessingGameCanvas();
    const scroll = vi.spyOn(workspace, 'scrollBoundsIntoView');
    const { handle, calls } = handleWithSpy(workspace);

    selectBlockFromCode(handle, 'b005');
    expect(calls).toEqual([['b005', undefined]]);
    expect(scroll).toHaveBeenCalled();
  });

  it('selects the outermost collapsed block around a hidden block', () => {
    const workspace = guessingGameCanvas();
    blockById(workspace, 'b009').setCollapsed(true);
    blockById(workspace, 'b010').setCollapsed(true);
    const { handle, calls } = handleWithSpy(workspace);

    expect(visibleHolder(blockById(workspace, 'b006')).id).toBe('b010');
    expect(revealTarget(workspace, 'b006')?.id).toBe('b010');
    // A collapsed block itself shows.
    expect(revealTarget(workspace, 'b010')?.id).toBe('b010');
    // Blocks above in the same list do not hide the ones below.
    expect(revealTarget(workspace, 'b004')?.id).toBe('b004');

    selectBlockFromCode(handle, 'b006');
    selectBlockFromProblem(handle, 'b008');
    expect(calls).toEqual([
      ['b010', undefined],
      ['b010', { center: true }],
    ]);
  });

  it('leaves blocks the canvas does not show to the editor, centred', () => {
    const workspace = guessingGameCanvas();
    const { handle, calls } = handleWithSpy(workspace);
    expect(revealTarget(workspace, 'b_other_module')).toBeNull();

    selectBlockFromCode(handle, 'b_other_module');
    selectBlockFromProblem(handle, 'b_other_module');
    selectBlockFromProblem(handle, 'b009');
    expect(calls).toEqual([
      ['b_other_module', { center: true }],
      ['b_other_module', { center: true }],
      ['b009', { center: true }],
    ]);
  });

  it('does nothing without an editor, and survives a failed scroll', () => {
    expect(() => {
      selectBlockFromCode(null, 'b005');
      selectBlockFromProblem(null, 'b005');
    }).not.toThrow();

    const workspace = guessingGameCanvas();
    vi.spyOn(workspace, 'scrollBoundsIntoView').mockImplementation(() => {
      throw new Error('no layout');
    });
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    const { handle, calls } = handleWithSpy(workspace);
    selectBlockFromCode(handle, 'b005');
    expect(calls).toEqual([['b005', undefined]]);
    expect(warn).toHaveBeenCalled();
  });
});
