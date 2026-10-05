/**
 * The module's viewport (05 §5.3: `{x, y, scale}`), captured only when the project is saved, so
 * scrolling and zooming never mark the project as changed (M2 decision "Dirty state for layout-only
 * changes").
 *
 * `x` and `y` are the workspace coordinates of the view's top-left corner, as whole numbers within
 * ±10⁷; `scale` is the zoom, within 0.1–4.0.
 */
import type { BdmViewport } from '@blocks2cpp/b2c-core-wasm';
import * as Blockly from 'blockly/core';

import { clampCoordinate, clampZoom } from './limits';

/** Where a rendered workspace is scrolled and zoomed, in Blockly's own terms. */
export interface ViewState {
  readonly scrollX: number;
  readonly scrollY: number;
  readonly scale: number;
}

/** The view state of a rendered workspace, or `null` for a headless one. */
export function viewStateOf(workspace: Blockly.Workspace): ViewState | null {
  if (!(workspace instanceof Blockly.WorkspaceSvg)) {
    return null;
  }
  return { scrollX: workspace.scrollX, scrollY: workspace.scrollY, scale: workspace.scale };
}

/** Whether two view states are the same (sub-pixel differences are ignored, as Blockly does). */
export function sameViewState(a: ViewState, b: ViewState): boolean {
  return (
    a.scale === b.scale &&
    Math.abs(a.scrollX - b.scrollX) < 1 &&
    Math.abs(a.scrollY - b.scrollY) < 1
  );
}

/** The viewport a rendered workspace shows, or `null` for a headless one. */
export function captureViewport(workspace: Blockly.Workspace): BdmViewport | null {
  const state = viewStateOf(workspace);
  if (state === null) {
    return null;
  }
  const scale = clampZoom(state.scale);
  return {
    x: clampCoordinate(-state.scrollX / scale),
    y: clampCoordinate(-state.scrollY / scale),
    scale,
  };
}

/**
 * Shows `viewport` in a rendered workspace (Blockly keeps the view near the blocks, so it may
 * scroll less far). Does nothing for a headless workspace.
 */
export function restoreViewport(workspace: Blockly.Workspace, viewport: BdmViewport): void {
  if (!(workspace instanceof Blockly.WorkspaceSvg)) {
    return;
  }
  try {
    const scale = clampZoom(viewport.scale);
    workspace.setScale(scale);
    workspace.scroll(-clampCoordinate(viewport.x) * scale, -clampCoordinate(viewport.y) * scale);
  } catch (error: unknown) {
    // A workspace without a size yet (hidden) cannot scroll; the view stays where it is.
    console.warn('The saved view could not be restored', error);
  }
}
