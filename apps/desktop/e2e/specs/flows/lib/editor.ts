/**
 * The block editor in the flow tests, beyond ../../../support/editor.ts: dragging a reporter from
 * the toolbox by its output connection onto a value input, dropping a block on an empty part of
 * the canvas, clicking a block with the pointer, and reading Blockly's dropdown menu.
 */
import { By, Key, Origin, type WebDriver } from 'selenium-webdriver';

import type { E2ePoint } from '../../../../src/e2e/contract';
import type { App } from '../../../support/app';
import { bringIntoView, visibleCanvas } from '../../../support/canvas';
import { drag, dropPoint } from '../../../support/drag';
import { centerOn, type ToolboxEntry } from '../../../support/editor';
import { openCategory, textContent } from '../../../support/ui';
import { sleep, waitFor } from '../../../support/wait';

/** How long Blockly may take to show a block, a connection or a menu. */
const EDITOR_TIMEOUT_MS = 10_000;

/** Waits for a point the hook reports (`null` while it is not on screen). */
function pointOf(locate: () => Promise<E2ePoint | null>, what: string): Promise<E2ePoint> {
  return waitFor(locate, { timeout: EDITOR_TIMEOUT_MS, interval: 200, message: what });
}

/** The ID of a toolbox entry's block in the flyout, once its category is open. */
async function flyoutBlock(app: App, entry: ToolboxEntry): Promise<string> {
  await openCategory(app.driver, entry.category);
  return waitFor(() => app.hook.flyoutBlockId(entry.type, entry.fields ?? {}), {
    timeout: EDITOR_TIMEOUT_MS,
    message: `${entry.type} in the toolbox's ${entry.category} category`,
  });
}

/**
 * Drags a reporter or predicate from the toolbox so that its output connection lands on value
 * input `input` of block `targetId`, where Blockly connects it if the type rule allows (the
 * target is centred first; the toolbox follows the selection, so the flyout block is looked up
 * after that).
 */
export async function dragValueFromToolbox(
  app: App,
  entry: ToolboxEntry,
  targetId: string,
  input: string,
): Promise<void> {
  await centerOn(app, targetId);
  await bringIntoView(app.driver, () =>
    pointOf(() => app.hook.connectionPoint(targetId, input), `input ${input} of ${targetId}`),
  );
  const blockId = await flyoutBlock(app, entry);
  const grab = await pointOf(() => app.hook.grabPoint(blockId), `${entry.type} to be on screen`);
  const own = await pointOf(
    () => app.hook.connectionPoint(blockId, 'output'),
    `the output connection of ${entry.type}`,
  );
  const target = await pointOf(
    () => app.hook.connectionPoint(targetId, input),
    `input ${input} of ${targetId}`,
  );
  await drag(app.driver, grab, dropPoint(grab, own, target));
  // Blockly finishes the drop (connecting, or bumping a refused block away) in the next frames.
  await sleep(400);
}

/**
 * Runs in the webview: a point of the visible canvas (`arguments[0..3]`: left, top, right,
 * bottom) at the corner of a `arguments[4]` × `arguments[5]` box of empty canvas background,
 * searched from the bottom left (away from the trash can and the zoom buttons on the right);
 * `null` when there is none. Only the pointer must be over the background: a dropped block may
 * overlap others, and a definition has no connection that could snap to them.
 */
const EMPTY_AREA = `
  const [left, top, right, bottom, width, height] = arguments;
  const empty = (x, y) => {
    const element = document.elementFromPoint(x, y);
    return element !== null && element.classList.contains('blocklyMainBackground');
  };
  for (let y = bottom - height; y >= top; y -= 16) {
    for (let x = left; x + width <= right; x += 16) {
      let free = true;
      for (let dy = 0; dy <= height && free; dy += 16) {
        for (let dx = 0; dx <= width && free; dx += 16) {
          free = empty(x + dx, y + dy);
        }
      }
      if (free) {
        return [x + 24, y + 24];
      }
    }
  }
  return null;
`;

/** Drags a block from the toolbox to an empty part of the visible canvas (not onto a block). */
export async function dropOnCanvas(app: App, entry: ToolboxEntry): Promise<void> {
  const blockId = await flyoutBlock(app, entry);
  const grab = await pointOf(() => app.hook.grabPoint(blockId), `${entry.type} to be on screen`);
  const rect = await visibleCanvas(app.driver);
  const found: unknown = await app.driver.executeScript(
    EMPTY_AREA,
    rect.left + 8,
    rect.top + 8,
    rect.right - 8,
    rect.bottom - 8,
    160,
    80,
  );
  if (!Array.isArray(found) || found.length !== 2 || !found.every((n) => typeof n === 'number')) {
    throw new Error('The visible canvas has no empty area to drop a block on');
  }
  const [x, y] = found as [number, number];
  await drag(app.driver, grab, { x, y });
  await sleep(400);
}

/** Clicks a block with the pointer at its grab point (which selects it and gives it the focus). */
export async function clickBlock(app: App, blockId: string): Promise<void> {
  await centerOn(app, blockId);
  const point = await pointOf(() => app.hook.grabPoint(blockId), `block ${blockId} on screen`);
  await app.driver
    .actions({ async: true })
    .move({ x: Math.round(point.x), y: Math.round(point.y), origin: Origin.VIEWPORT })
    .click()
    .perform();
  await waitForSelected(app, blockId);
}

/** Whether a block is Blockly's selected block (its SVG group has `blocklySelected`). */
export async function isSelected(app: App, blockId: string): Promise<boolean> {
  const element = await app.hook.blockElement(blockId);
  if (element === null) {
    return false;
  }
  const classes: string | null = await element.getAttribute('class');
  return (classes ?? '').split(/\s+/).includes('blocklySelected');
}

/** Waits until block `blockId` is selected. */
export async function waitForSelected(app: App, blockId: string): Promise<void> {
  await waitFor(() => isSelected(app, blockId), {
    timeout: EDITOR_TIMEOUT_MS,
    message: `block ${blockId} to be selected`,
  });
}

/** The labels of the open dropdown menu's items, once it is shown. */
export async function menuLabels(driver: WebDriver): Promise<string[]> {
  return waitFor(
    async () => {
      // Every item of the open menu, also those scrolled out of its view: a menu near the
      // window's edge is shortened and scrolled to the current choice.
      const menus = await driver.findElements(By.css('.blocklyDropDownDiv'));
      const open = [];
      for (const menu of menus) {
        if (await menu.isDisplayed()) {
          open.push(menu);
        }
      }
      if (open.length !== 1 || open[0] === undefined) {
        return null;
      }
      const items = await open[0].findElements(By.css('.blocklyMenuItem'));
      const labels: string[] = [];
      for (const item of items) {
        labels.push(await textContent(driver, item));
      }
      return labels.length > 0 ? labels : null;
    },
    { timeout: EDITOR_TIMEOUT_MS, message: 'the dropdown menu' },
  );
}

/** Closes Blockly's open dropdown menu with Escape. */
export async function closeMenu(driver: WebDriver): Promise<void> {
  await driver.actions({ async: true }).sendKeys(Key.ESCAPE).perform();
  await waitFor(
    async () => {
      for (const item of await driver.findElements(
        By.css('.blocklyDropDownDiv .blocklyMenuItem'),
      )) {
        if (await item.isDisplayed()) {
          return false;
        }
      }
      return true;
    },
    { timeout: EDITOR_TIMEOUT_MS, message: 'the dropdown menu to close' },
  );
}
