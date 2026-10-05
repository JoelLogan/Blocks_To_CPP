/**
 * Two-way highlighting, block side (docs/spec/04-user-interface.md §4.3): the block under the
 * pointer and the selected block are kept in the app's store (`ui.hoverBlock`, `ui.selection`),
 * and the code panel highlights their C++.
 *
 * Shadow blocks (the expression slots) are not document blocks, so they stand for the block whose
 * input holds them.
 */
import * as Blockly from 'blockly/core';

import type { EditorContext } from '../../app/editor-types';

/** The document block a workspace element ID stands for: shadows give their owner; else null. */
export function documentBlockId(workspace: Blockly.Workspace, id: string | null): string | null {
  if (id === null) {
    return null;
  }
  let block = workspace.getBlockById(id);
  while (block?.isShadow() === true) {
    block = block.getParent();
  }
  return block?.id ?? null;
}

/** The ID of the block whose drawing contains `target` (Blockly marks block groups with `data-id`). */
export function blockIdAt(target: EventTarget | null): string | null {
  if (!(target instanceof Element)) {
    return null;
  }
  return target.closest('[data-id]')?.getAttribute('data-id') ?? null;
}

/**
 * Follows the workspace's selection and the block under the pointer into the store. Returns the
 * function that stops following (and clears the hovered block).
 */
export function attachHighlightTracking(ctx: EditorContext): () => void {
  const { workspace, store } = ctx;

  const setSelection = (id: string | null): void => {
    if (store.getState().ui.selection !== id) {
      store.getState().actions.setUi({ selection: id });
    }
  };
  const setHover = (id: string | null): void => {
    if (store.getState().ui.hoverBlock !== id) {
      store.getState().actions.setUi({ hoverBlock: id });
    }
  };

  const onEvent = (event: Blockly.Events.Abstract): void => {
    if (event instanceof Blockly.Events.Selected) {
      setSelection(documentBlockId(workspace, event.newElementId ?? null));
    } else if (event instanceof Blockly.Events.BlockDelete) {
      const deleted = new Set(event.ids ?? []);
      const { selection, hoverBlock } = store.getState().ui;
      if (selection !== null && deleted.has(selection)) {
        setSelection(null);
      }
      if (hoverBlock !== null && deleted.has(hoverBlock)) {
        setHover(null);
      }
    }
  };
  workspace.addChangeListener(onEvent);

  // A module switch replaces every block without events: forget the hovered block then.
  const unsubscribe = store.subscribe((state, previous) => {
    if (state.project?.activeModuleId !== previous.project?.activeModuleId) {
      setHover(null);
    }
  });

  const surface = workspace instanceof Blockly.WorkspaceSvg ? workspace.getParentSvg() : null;
  const onPointerOver = (event: PointerEvent): void => {
    setHover(documentBlockId(workspace, blockIdAt(event.target)));
  };
  const onPointerLeave = (): void => {
    setHover(null);
  };
  surface?.addEventListener('pointerover', onPointerOver);
  surface?.addEventListener('pointerleave', onPointerLeave);

  return () => {
    workspace.removeChangeListener(onEvent);
    unsubscribe();
    surface?.removeEventListener('pointerover', onPointerOver);
    surface?.removeEventListener('pointerleave', onPointerLeave);
    setHover(null);
  };
}
