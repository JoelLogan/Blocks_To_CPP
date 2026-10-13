/**
 * Real drag and drop on Blockly's canvas with WebDriver pointer actions: press, move in small
 * steps (Blockly starts a drag once the pointer has moved a few pixels, and follows each move),
 * release. The webviews get these as native pointer events, so this proves drag and drop works in
 * both of them (WebKitGTK and WebView2).
 */
import { Button, Origin, type WebDriver } from 'selenium-webdriver';

import type { E2ePoint } from '../../src/e2e/contract';

/** How many moves a drag makes. */
const STEPS = 16;
/** How long each move takes, in milliseconds. */
const STEP_MS = 25;
/** The pause after pressing and before releasing, so Blockly sees a deliberate gesture. */
const PAUSE_MS = 120;

/** A point with whole pixels (WebDriver's pointer coordinates are integers). */
function whole(point: E2ePoint): { x: number; y: number } {
  return { x: Math.round(point.x), y: Math.round(point.y) };
}

/** The actions that press the left button at `from` and move to `to` in {@link STEPS} moves. */
function pressAndMoveActions(driver: WebDriver, from: E2ePoint, to: E2ePoint) {
  const start = whole(from);
  const end = whole(to);
  let actions = driver
    .actions({ async: true })
    .move({ x: start.x, y: start.y, origin: Origin.VIEWPORT })
    .press(Button.LEFT)
    .pause(PAUSE_MS);
  for (let step = 1; step <= STEPS; step += 1) {
    actions = actions.move({
      x: Math.round(start.x + ((end.x - start.x) * step) / STEPS),
      y: Math.round(start.y + ((end.y - start.y) * step) / STEPS),
      origin: Origin.VIEWPORT,
      duration: STEP_MS,
    });
  }
  return actions.pause(PAUSE_MS);
}

/** Drags with the left button from `from` to `to` (viewport coordinates). */
export async function drag(driver: WebDriver, from: E2ePoint, to: E2ePoint): Promise<void> {
  await pressAndMoveActions(driver, from, to).release(Button.LEFT).perform();
}

/**
 * The first half of {@link drag}: presses at `from` and moves to `to`, keeping the button down,
 * so that a test can look at the page during the drag. {@link releaseButton} drops.
 */
export async function pressAndMove(driver: WebDriver, from: E2ePoint, to: E2ePoint): Promise<void> {
  await pressAndMoveActions(driver, from, to).perform();
}

/** Releases the left button where the pointer is (after {@link pressAndMove}). */
export async function releaseButton(driver: WebDriver): Promise<void> {
  await driver.actions({ async: true }).release(Button.LEFT).perform();
}

/** Where to release a block grabbed at `grab` so that its connection at `own` lands on `target`. */
export function dropPoint(grab: E2ePoint, own: E2ePoint, target: E2ePoint): E2ePoint {
  return { x: target.x + (grab.x - own.x), y: target.y + (grab.y - own.y) };
}
