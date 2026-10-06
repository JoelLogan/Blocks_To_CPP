/**
 * Working with the block editor in the tests: reading the open project, dragging blocks from the
 * toolbox onto connections, and waiting for the live analysis (Problems, the C++ panel).
 */
import { By, type WebDriver, type WebElement } from 'selenium-webdriver';

import type { App } from './app';
import { parseDocument, type ProjectDocument } from './bdm';
import { drag, dropPoint } from './drag';
import { bringIntoView } from './canvas';
import { byTestId, clickTestId, errorCount, middleOf, openCategory, testIdText } from './ui';
import { sleep, waitFor } from './wait';

/** The open project as the canvas has it now. */
export async function currentDocument(app: App): Promise<ProjectDocument> {
  return parseDocument(await app.hook.documentText());
}

/** The Problems panel's summary (`No problems`, `1 error`, …), also while its tab is hidden. */
export function problemsSummary(driver: WebDriver): Promise<string> {
  return testIdText(driver, 'problems-summary');
}

/** Waits until the Problems panel names `count` errors (or, with `atLeast`, that many or more). */
export async function waitForErrors(
  app: App,
  count: number,
  options: { readonly atLeast?: boolean; readonly timeout?: number } = {},
): Promise<string> {
  return waitFor(
    async () => {
      const summary = await problemsSummary(app.driver);
      const errors = errorCount(summary);
      const matches = options.atLeast === true ? errors >= count : errors === count;
      return summary !== '' && matches ? summary : null;
    },
    {
      timeout: options.timeout ?? 15_000,
      message: async () =>
        `${options.atLeast === true ? 'at least ' : ''}${String(count)} errors in Problems (it says "${await problemsSummary(app.driver)}")`,
    },
  );
}

/** Whether Run is held back (`aria-disabled`), as the run gate decides. */
export async function runHeldBack(driver: WebDriver): Promise<boolean> {
  const run = await driver.findElement(byTestId('toolbar-run'));
  return (await run.getAttribute('aria-disabled')) === 'true';
}

/**
 * Gives the canvas room: the toolbox's flyout stays open over the canvas's left part, so the C++
 * panel is folded away and only a narrow strip would be left otherwise.
 */
export async function foldCodePanel(app: App): Promise<void> {
  await clickTestId(app.driver, 'right-dock-toggle');
  await waitFor(
    async () => {
      const dock = await app.driver.findElements(By.css('aside.right-dock[data-collapsed="true"]'));
      return dock.length > 0;
    },
    { timeout: 5_000, message: 'the C++ panel to fold away' },
  );
  await sleep(300);
}

/** Scrolls the canvas so block `blockId` is in the middle of what is visible, and selects it. */
export async function centerOn(app: App, blockId: string): Promise<void> {
  await app.hook.selectBlock(blockId);
  // Blockly renders the scrolled canvas in the next frame.
  await sleep(250);
}

/**
 * The SVG group of a field (of the block, or of the block in `input`), with the block centred and
 * the canvas panned until the field is well inside the visible canvas.
 */
export async function fieldOf(
  app: App,
  blockId: string,
  field: string,
  input: string | null = null,
): Promise<WebElement> {
  const find = () =>
    waitFor(() => app.hook.fieldElement(blockId, field, input), {
      timeout: 5_000,
      message: `field ${field}${input === null ? '' : ` in ${input}`} of block ${blockId}`,
    });
  await centerOn(app, blockId);
  await bringIntoView(app.driver, async () => middleOf(app.driver, await find()));
  return find();
}

/** Waits for a block's grab point: the block must be on screen and not covered. */
function grabPointOf(app: App, blockId: string) {
  return waitFor(() => app.hook.grabPoint(blockId), {
    timeout: 10_000,
    interval: 200,
    message: `block ${blockId} to be on screen`,
  });
}

/** A connection's point on screen. */
function connectionOf(app: App, blockId: string, connection: string) {
  return waitFor(() => app.hook.connectionPoint(blockId, connection), {
    timeout: 10_000,
    message: `the ${connection} connection of block ${blockId}`,
  });
}

/** A block of the toolbox: its category, its type and (to tell entries apart) field values. */
export interface ToolboxEntry {
  readonly category: string;
  readonly type: string;
  readonly fields?: Readonly<Record<string, string>>;
}

/**
 * Drags a block from the toolbox so that its `previous` connection lands on connection `target` of
 * block `targetId`, where Blockly snaps it in.
 *
 * The target is centred (and selected) first. The toolbox follows the selection (the Variables
 * category lists what is in scope there), so it may build its flyout again; only then is the
 * category opened and the flyout block looked up.
 */
export async function dragFromToolbox(
  app: App,
  entry: ToolboxEntry,
  targetId: string,
  target: string,
): Promise<void> {
  await centerOn(app, targetId);
  await bringIntoView(app.driver, () => connectionOf(app, targetId, target));
  await openCategory(app.driver, entry.category);
  const blockId = await waitFor(() => app.hook.flyoutBlockId(entry.type, entry.fields ?? {}), {
    timeout: 10_000,
    message: `${entry.type} in the toolbox's ${entry.category} category`,
  });
  const grab = await grabPointOf(app, blockId);
  const own = await connectionOf(app, blockId, 'previous');
  const targetPoint = await connectionOf(app, targetId, target);
  await drag(app.driver, grab, dropPoint(grab, own, targetPoint));
  // Blockly finishes the drop (snapping, bumping) in the next frames.
  await sleep(300);
}

/** Drags block `blockId` by `dx`, `dy` pixels on the canvas. */
export async function dragBy(app: App, blockId: string, dx: number, dy: number): Promise<void> {
  await centerOn(app, blockId);
  const grab = await grabPointOf(app, blockId);
  await drag(app.driver, grab, { x: grab.x + dx, y: grab.y + dy });
  await sleep(300);
}

/** The function `int main() { … }` of generated C++ (to the end of the file), trimmed. */
export function mainFunction(cpp: string): string {
  const start = cpp.indexOf('int main() {');
  if (start === -1) {
    throw new Error('The C++ has no main function');
  }
  return cpp.slice(start).trim();
}
