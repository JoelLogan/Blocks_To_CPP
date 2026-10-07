/**
 * The drag benchmark's handle (drag.bench.e2e.ts): where it is, read in the page with
 * bench/page.ts `PLACE_SCRIPT`, and whether a drag moved it with the pointer. A drag that missed
 * the handle (it pressed on the canvas, or on another block) does not move it at all, and must be
 * reported, not measured.
 */
import type { E2ePoint } from '../../src/e2e/contract';
import type { Rect } from '../support/canvas';
import type { BlockPlace } from './page';

/**
 * How far from the pointer's end a drop may leave the handle, as a fraction of the drag's length,
 * before the drag counts as having missed it. Snapping or bumping moves a dropped block by a few
 * pixels; a drag that missed the handle leaves it a whole drag's length away.
 */
export const MAX_LANDING_ERROR = 0.5;

/** Which way a drag of a pair goes. */
export type DragAim = 'away' | 'back';

/**
 * Where each drag of a pair ends, as fractions of the visible canvas's width and height from its
 * top left corner (measured right before the drag): the drag away up and to the right, the drag
 * back down and to the left. Both stay clear of the flyout, whose width changes as the toolbox
 * follows the program (a drop on it deletes the handle), and of the zoom controls and the trash
 * can at the right.
 */
export const AIMS: Readonly<Record<DragAim, E2ePoint>> = {
  away: { x: 0.7, y: 0.25 },
  back: { x: 0.3, y: 0.7 },
};

/** The shortest drag that is still a drag worth timing, in CSS pixels. */
export const MIN_DRAG_DISTANCE = 80;

/**
 * How far to drag the handle from `grab` so that the drag ends at {@link AIMS}`[aim]` of the
 * visible `canvas`, in whole pixels.
 *
 * @throws Error when that is shorter than {@link MIN_DRAG_DISTANCE} (too little canvas is visible).
 */
export function aimDelta(grab: E2ePoint, canvas: Rect, aim: DragAim): E2ePoint {
  const target = AIMS[aim];
  const delta = {
    x: Math.round(canvas.left + (canvas.right - canvas.left) * target.x - grab.x),
    y: Math.round(canvas.top + (canvas.bottom - canvas.top) * target.y - grab.y),
  };
  if (Math.hypot(delta.x, delta.y) < MIN_DRAG_DISTANCE) {
    throw new Error(
      `The visible canvas (${String(Math.round(canvas.left))}, ${String(Math.round(canvas.top))} to ${String(Math.round(canvas.right))}, ${String(Math.round(canvas.bottom))}) leaves no room to drag the handle ${aim} from (${String(Math.round(grab.x))}, ${String(Math.round(grab.y))})`,
    );
  }
  return delta;
}

/** Whether `value` is a finite number. */
function finite(value: unknown): value is number {
  return typeof value === 'number' && Number.isFinite(value);
}

/** Whether `value` is a rectangle of finite numbers. */
function isRect(value: unknown): value is Rect {
  if (typeof value !== 'object' || value === null) {
    return false;
  }
  const rect = value as Record<string, unknown>;
  return (
    finite(rect['left']) && finite(rect['top']) && finite(rect['right']) && finite(rect['bottom'])
  );
}

/** Whether `value` is a {@link BlockPlace}. */
export function isBlockPlace(value: unknown): value is BlockPlace {
  if (typeof value !== 'object' || value === null) {
    return false;
  }
  const place = value as Record<string, unknown>;
  const offset = place['offset'];
  if (typeof offset !== 'object' || offset === null) {
    return false;
  }
  const point = offset as Record<string, unknown>;
  return isRect(place['box']) && finite(point['x']) && finite(point['y']);
}

/** Whole pixels, for a message. */
function px(value: number): string {
  return String(Math.round(value));
}

/** A rectangle, for a message. */
function rectText(rect: Rect): string {
  return `(${px(rect.left)}, ${px(rect.top)})–(${px(rect.right)}, ${px(rect.bottom)})`;
}

/**
 * Where a block is, for a message: its box on screen and how it lies against the visible canvas
 * (`canvas`, when known).
 */
export function describePlace(place: BlockPlace | null, canvas: Rect | null): string {
  if (place === null) {
    return 'it is not on the canvas';
  }
  const box = place.box;
  let text = `its box on screen is ${rectText(box)}`;
  if (canvas !== null) {
    const inside =
      box.left >= canvas.left &&
      box.top >= canvas.top &&
      box.right <= canvas.right &&
      box.bottom <= canvas.bottom;
    const apart =
      box.right <= canvas.left ||
      box.left >= canvas.right ||
      box.bottom <= canvas.top ||
      box.top >= canvas.bottom;
    const where = inside ? 'inside' : apart ? 'outside' : 'partly outside';
    text += `, ${where} the visible canvas ${rectText(canvas)}`;
  }
  return text;
}

/**
 * How far from where the pointer left it a drag by `delta` put the block, in CSS pixels, from its
 * places before and after the drag. Measured on the canvas, so a scroll of the canvas between the
 * two does not count.
 */
export function landingError(before: BlockPlace, after: BlockPlace, delta: E2ePoint): number {
  const moved = {
    x: after.offset.x - before.offset.x,
    y: after.offset.y - before.offset.y,
  };
  return Math.hypot(moved.x - delta.x, moved.y - delta.y);
}

/**
 * Checks that a drag by `delta` moved block `id` with the pointer, from its places before and
 * after the drag (`after` is `null` when it is no longer on the canvas), and returns its
 * {@link landingError}.
 *
 * @throws Error when the block is gone, or it landed more than {@link MAX_LANDING_ERROR} of the
 * drag's length away from the pointer's end (the drag missed it).
 */
export function checkLanding(
  id: string,
  delta: E2ePoint,
  before: BlockPlace,
  after: BlockPlace | null,
): number {
  if (after === null) {
    throw new Error(
      `The drag handle ${id} is no longer on the canvas after a drag (a drop over the toolbox or the trash can deletes it)`,
    );
  }
  const error = landingError(before, after, delta);
  if (error > MAX_LANDING_ERROR * Math.hypot(delta.x, delta.y)) {
    const moved = {
      x: after.offset.x - before.offset.x,
      y: after.offset.y - before.offset.y,
    };
    throw new Error(
      `A drag by (${px(delta.x)}, ${px(delta.y)}) moved the drag handle ${id} by ` +
        `(${px(moved.x)}, ${px(moved.y)}): the drag missed it, or the drop put it elsewhere ` +
        `(before the drag ${describePlace(before, null)}, after it ${describePlace(after, null)})`,
    );
  }
  return error;
}
