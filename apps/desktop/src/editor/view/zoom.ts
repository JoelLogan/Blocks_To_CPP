/**
 * The canvas's zoom and view (docs/spec/04-user-interface.md §4.1, §4.7; decision D10 of the M2
 * polish plan): one zoom step in or out and *Reset zoom*, both around the middle of the visible
 * canvas, and *Show all blocks*, which scrolls to the blocks and never changes the zoom.
 *
 * The canvas controls (./CanvasControls.tsx) and the keyboard's `Ctrl+=`, `Ctrl+-` and `Ctrl+0`
 * (../keyboard/shortcuts.ts) call these. Blockly's own zoom controls are off: their reset went back
 * to the start zoom (90 %), their "fit" changed the zoom, and they were SVG images without names
 * or keyboard access. `Ctrl` + wheel and a pinch zoom around the pointer, which Blockly does itself
 * with the editor's injection options (`zoom.wheel` and `zoom.pinch`, ../EditorWorkspace.tsx).
 *
 * Everything here uses Blockly's public view API (`setScale`, `scroll`, the metrics manager), so
 * a zoom is clamped to the workspace's zoom range, and a scroll to where Blockly lets the view go
 * (around the blocks), as Blockly's own zoom is. The zoom changes at once, without an animation.
 */
import type * as Blockly from 'blockly/core';

/** The zoom *Reset zoom* and `Ctrl+0` go back to: 100 % (D10), not the 90 % a new canvas starts at. */
export const RESET_SCALE = 1;

/**
 * The space *Show all blocks* leaves between the blocks' top-left corner and the edges of the
 * view when the blocks do not fit in it, in workspace units.
 */
export const SHOW_ALL_MARGIN = 24;

/** Which way a zoom step goes. */
export type ZoomDirection = 'in' | 'out';

/** A point in workspace coordinates. */
interface WorkspacePoint {
  readonly x: number;
  readonly y: number;
}

/** The middle of the visible canvas (right of the toolbox's flyout), in workspace coordinates. */
function viewCentre(workspace: Blockly.WorkspaceSvg): WorkspacePoint {
  const view = workspace.getMetricsManager().getViewMetrics(true);
  return { x: view.left + view.width / 2, y: view.top + view.height / 2 };
}

/**
 * Scrolls so that the view's top-left corner is at (`left`, `top`) in workspace coordinates, at
 * the current zoom. Blockly keeps the view within its scroll area, around the blocks.
 */
function scrollViewTo(workspace: Blockly.WorkspaceSvg, left: number, top: number): void {
  workspace.scroll(-left * workspace.scale, -top * workspace.scale);
}

/**
 * Zooms to `scale` (clamped to the workspace's zoom range) and keeps the point at the middle of the
 * view in the middle. Returns the zoom applied.
 */
function zoomAroundCentre(workspace: Blockly.WorkspaceSvg, scale: number): number {
  const centre = viewCentre(workspace);
  // `setScale` clamps, lays the flyout out and moves the view by itself; the scroll below puts the
  // view where it belongs.
  workspace.setScale(scale);
  const view = workspace.getMetricsManager().getViewMetrics(true);
  scrollViewTo(workspace, centre.x - view.width / 2, centre.y - view.height / 2);
  return workspace.scale;
}

/**
 * Zooms one step in or out (the workspace's `scaleSpeed`, 1.2, as one notch of `Ctrl` + wheel)
 * around the middle of the view. Returns the new zoom; at the end of the zoom range it stays.
 */
export function zoomStep(workspace: Blockly.WorkspaceSvg, direction: ZoomDirection): number {
  const speed = workspace.options.zoomOptions.scaleSpeed;
  return zoomAroundCentre(
    workspace,
    direction === 'in' ? workspace.scale * speed : workspace.scale / speed,
  );
}

/** *Reset zoom*: back to {@link RESET_SCALE} around the middle of the view. Returns the new zoom. */
export function resetZoom(workspace: Blockly.WorkspaceSvg): number {
  return zoomAroundCentre(workspace, RESET_SCALE);
}

/**
 * Where the view starts on one axis so that it shows the blocks spanning `start` to `end` (in
 * workspace units) in a view `size` units long: centred on them when they fit, otherwise starting
 * {@link SHOW_ALL_MARGIN} before them.
 */
export function viewStart(start: number, end: number, size: number): number {
  return end - start <= size ? (start + end - size) / 2 : start - SHOW_ALL_MARGIN;
}

/**
 * *Show all blocks*: scrolls to the blocks without changing the zoom (D10). On each axis the blocks
 * are centred when they fit in the view; where they do not, the view starts
 * {@link SHOW_ALL_MARGIN} before their top-left corner, so the start of the program is in view. An
 * empty canvas, or one without a size (the editor is hidden), is left as it is. Returns whether
 * there were blocks to scroll to.
 */
export function showAllBlocks(workspace: Blockly.WorkspaceSvg): boolean {
  if (workspace.getTopBoundedElements().length === 0) {
    return false;
  }
  const view = workspace.getMetricsManager().getViewMetrics(true);
  if (view.width <= 0 || view.height <= 0) {
    return false;
  }
  const blocks = workspace.getBlocksBoundingBox();
  scrollViewTo(
    workspace,
    viewStart(blocks.left, blocks.right, view.width),
    viewStart(blocks.top, blocks.bottom, view.height),
  );
  return true;
}

/** A zoom as people read it: `1.08` is `108%`. */
export function zoomPercent(scale: number): string {
  return `${String(Math.round(scale * 100))}%`;
}

/** What is announced after the zoom changed to `scale`. */
export function zoomAnnouncement(scale: number): string {
  return `Zoom ${zoomPercent(scale)}`;
}
