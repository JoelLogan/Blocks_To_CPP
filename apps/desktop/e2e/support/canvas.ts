/**
 * The visible part of Blockly's canvas, and scrolling it the way a person does: by dragging the
 * empty background. The toolbox's flyout stays open over the canvas's left part, so a block that
 * Blockly centres can still reach under it when it is wide; the tests then pan the canvas until
 * the point they need (a connection, a field) is well inside the visible part.
 */
import type { WebDriver } from 'selenium-webdriver';

import type { E2ePoint } from '../../src/e2e/contract';
import { drag } from './drag';
import { sleep } from './wait';

/** A rectangle in viewport coordinates. */
export interface Rect {
  readonly left: number;
  readonly top: number;
  readonly right: number;
  readonly bottom: number;
}

/** How far inside the visible canvas a point must be to count as reachable, in CSS pixels. */
const MARGIN = 60;

/** The most pans {@link bringIntoView} makes before it gives up. */
const MAX_PANS = 6;

/**
 * Runs in the webview: the canvas's visible rectangle, right of the flyout and inside the
 * scrollbars.
 */
const VISIBLE_CANVAS = `
  const svg = document.querySelector('[data-testid="workspace"] svg.blocklySvg');
  if (svg === null) {
    return null;
  }
  const canvas = svg.getBoundingClientRect();
  const flyout = document.querySelector('[data-testid="workspace"] svg.blocklyToolboxFlyout');
  const flyoutRight = flyout === null ? canvas.left : flyout.getBoundingClientRect().right;
  return [Math.max(canvas.left, flyoutRight), canvas.top, canvas.right - 16, canvas.bottom - 16];
`;

/**
 * Runs in the webview: a point of the empty canvas background inside the given rectangle,
 * searched from the given corner, or `null`.
 */
const EMPTY_POINT = `
  const [left, top, right, bottom, fromLeft, fromTop] = arguments;
  const xs = [];
  for (let x = Math.ceil(left); x <= right; x += 8) {
    xs.push(x);
  }
  const ys = [];
  for (let y = Math.ceil(top); y <= bottom; y += 8) {
    ys.push(y);
  }
  if (!fromLeft) {
    xs.reverse();
  }
  if (!fromTop) {
    ys.reverse();
  }
  for (const y of ys) {
    for (const x of xs) {
      const element = document.elementFromPoint(x, y);
      if (element !== null && element.classList.contains('blocklyMainBackground')) {
        return [x, y];
      }
    }
  }
  return null;
`;

/** Whether `value` is a list of `length` numbers. */
function isNumbers(value: unknown, length: number): value is number[] {
  return (
    Array.isArray(value) &&
    value.length === length &&
    value.every((item) => typeof item === 'number' && Number.isFinite(item))
  );
}

/** The canvas's visible rectangle (right of the flyout, inside the scrollbars). */
export async function visibleCanvas(driver: WebDriver): Promise<Rect> {
  const found: unknown = await driver.executeScript(VISIBLE_CANVAS);
  if (!isNumbers(found, 4)) {
    throw new Error('The canvas is not on screen');
  }
  const [left, top, right, bottom] = found as [number, number, number, number];
  return { left, top, right, bottom };
}

/** Whether `point` is at least {@link MARGIN} inside `rect`. */
export function wellInside(point: E2ePoint, rect: Rect): boolean {
  return (
    point.x >= rect.left + MARGIN &&
    point.x <= rect.right - MARGIN &&
    point.y >= rect.top + MARGIN &&
    point.y <= rect.bottom - MARGIN
  );
}

/**
 * How far to pan so that `point` comes to a comfortable place: a third from the visible canvas's
 * left and two fifths from its top, at most what one drag across it can do.
 */
export function panTowards(point: E2ePoint, rect: Rect): E2ePoint {
  const width = rect.right - rect.left;
  const height = rect.bottom - rect.top;
  const clamp = (value: number, limit: number) => Math.max(-limit, Math.min(limit, value));
  return {
    x: Math.round(clamp(rect.left + width / 3 - point.x, width - 2 * MARGIN)),
    y: Math.round(clamp(rect.top + (height * 2) / 5 - point.y, height - 2 * MARGIN)),
  };
}

/** Pans the canvas by `delta` by dragging its empty background. */
export async function panCanvas(driver: WebDriver, delta: E2ePoint): Promise<void> {
  const rect = await visibleCanvas(driver);
  // Start where the drag has room: on the left for a pan to the right, and so on.
  const found: unknown = await driver.executeScript(
    EMPTY_POINT,
    rect.left + 4,
    rect.top + 4,
    rect.right - 4,
    rect.bottom - 4,
    delta.x >= 0,
    delta.y >= 0,
  );
  if (!isNumbers(found, 2)) {
    throw new Error('The canvas has no empty spot to drag it by');
  }
  const [x, y] = found as [number, number];
  const end = {
    x: Math.max(rect.left + 2, Math.min(rect.right - 2, x + delta.x)),
    y: Math.max(rect.top + 2, Math.min(rect.bottom - 2, y + delta.y)),
  };
  await drag(driver, { x, y }, end);
  await sleep(200);
}

/**
 * Pans the canvas until the point `locate` gives is well inside the visible canvas, and returns
 * that point.
 */
export async function bringIntoView(
  driver: WebDriver,
  locate: () => Promise<E2ePoint>,
): Promise<E2ePoint> {
  let point = await locate();
  for (let pan = 0; pan < MAX_PANS; pan += 1) {
    const rect = await visibleCanvas(driver);
    if (wellInside(point, rect)) {
      return point;
    }
    await panCanvas(driver, panTowards(point, rect));
    point = await locate();
  }
  throw new Error(`Could not bring (${String(point.x)}, ${String(point.y)}) into view`);
}
