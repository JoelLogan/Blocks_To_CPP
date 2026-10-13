/**
 * The sizes of the editor's docks (docs/spec/04-user-interface.md §4.1), in CSS pixels:
 *
 * - a dock starts at a share of the window ({@link startSize}) and follows the window's size until
 *   the user resizes it;
 * - the user can make it anything from its `min` to its largest size now ({@link dockMaximum}):
 *   its `max`, and at most {@link MAX_DOCK_SHARE} of the editor's area; the C++ dock also leaves
 *   the canvas {@link CANVAS_MIN_WIDTH} beside the toolbox's flyout at its narrowest
 *   ({@link rightDockMaximum});
 * - the size shown is the user's (or the start size) kept within those limits, so the splitter
 *   moves at once from wherever the dock is (no dead zone), and the same largest size is the
 *   splitter's `aria-valuemax`.
 */
import { CANVAS_MIN_WIDTH } from './flyoutLayout';

/** The thickness of a splitter bar (app.css `.splitter-vertical`, `.splitter-horizontal`). */
export const SPLITTER_SIZE = 6;

/** The largest share of the editor's area a dock may take, so the canvas always stays visible. */
export const MAX_DOCK_SHARE = 0.65;

/** How a dock is sized; see the module comment. */
export interface DockSizes {
  /** The smallest and the largest size the user can give it. */
  readonly min: number;
  readonly max: number;
  /** Its start size: this share of the window's width or height, within `start.min`–`start.max`. */
  readonly share: number;
  readonly start: { readonly min: number; readonly max: number };
}

/** The C++ dock's width: it starts at 30 % of the window's width, 280–480 px. */
export const RIGHT_DOCK: DockSizes = Object.freeze({
  min: 200,
  max: 960,
  share: 0.3,
  start: Object.freeze({ min: 280, max: 480 }),
});

/** The bottom dock's height: it starts at 28 % of the window's height, 120–280 px. */
export const BOTTOM_DOCK: DockSizes = Object.freeze({
  min: 96,
  max: 720,
  share: 0.28,
  start: Object.freeze({ min: 120, max: 280 }),
});

/** A dock's start size in a window `window` pixels wide (or high, for the bottom dock). */
export function startSize(dock: DockSizes, window: number): number {
  return Math.min(dock.start.max, Math.max(dock.start.min, Math.round(dock.share * window)));
}

/**
 * The largest size of a dock in an editor area `area` pixels wide (or high): its `max`, and at
 * most {@link MAX_DOCK_SHARE} of the area, but never below its `min`.
 */
export function dockMaximum(dock: DockSizes, area: number): number {
  return Math.max(dock.min, Math.min(dock.max, Math.floor(MAX_DOCK_SHARE * area)));
}

/** Where the toolbox's flyout starts and how narrow it can be made (./flyoutLayout.ts). */
export interface ToolboxExtent {
  /** The width of the toolbox's category column, left of the flyout. */
  readonly left: number;
  /** The narrowest the flyout can be. */
  readonly min: number;
}

/**
 * The widest the C++ dock may be in an editor area `area` pixels wide beside `toolbox` (unknown
 * until the flyout is laid out): its `max`, at most {@link MAX_DOCK_SHARE} of the area, and never
 * so wide that the canvas has less than {@link CANVAS_MIN_WIDTH} beside the narrowest flyout.
 * Below {@link RIGHT_DOCK}.min the dock does not fit at all, and it is hidden.
 */
export function rightDockMaximum(area: number, toolbox: ToolboxExtent | null): number {
  const share = Math.min(RIGHT_DOCK.max, Math.floor(MAX_DOCK_SHARE * area));
  if (toolbox === null) {
    return share;
  }
  const room = area - SPLITTER_SIZE - toolbox.left - toolbox.min - CANVAS_MIN_WIDTH;
  return Math.floor(Math.min(share, room));
}

/** `value` kept within `min`–`max` (`max` wins when it is below `min`), as a whole number. */
export function clampSize(value: number, min: number, max: number): number {
  return Math.round(Math.min(Math.max(value, min), max));
}
