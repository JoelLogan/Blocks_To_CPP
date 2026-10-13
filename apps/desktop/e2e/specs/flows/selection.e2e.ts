/**
 * The Variables category follows a selection made with the mouse (docs/spec/04-user-interface.md
 * §4.2), in the real webview. Blockly selects a block when the button is pressed and refreshes no
 * toolbox until it is released, so the rebuild a click asks for comes while the button is held. A
 * person's click holds it for a moment: the toolbox must still list the new block's scope once
 * the button is up. The blocks of the guessing game (examples/guessing_game.b2c) are clicked with
 * the button held for 100 ms, each scrolled into view first through the test hook, which keeps
 * the selection (centring a block through the hook would select it, and so rebuild the toolbox).
 */
import { Button, Origin } from 'selenium-webdriver';
import { describe, it } from 'vitest';

import { scrollIntoView } from '../../support/canvas';
import { blockCorner, foldCodePanel, grabPointOf } from '../../support/editor';
import { waitFor } from '../../support/wait';
import { isSelected, waitForSelected } from './lib/editor';
import { type FlowApp, startFlow } from './lib/launch';
import { copyExample, openFromStartPage } from './lib/project';
import { UI_TIMEOUT_MS } from './lib/ui';

/** The symbols of the guessing game's variables. */
const SECRET = 's_secret';
const GUESS = 's_guess';

/** How long a click holds the button down: about what a person's click does. */
const HOLD_MS = 100;

/** Which of the guessing game's variables the toolbox lists getters of, in name order. */
async function listedGetters(app: FlowApp): Promise<string[]> {
  const listed: string[] = [];
  for (const sym of [GUESS, SECRET]) {
    if ((await app.hook.flyoutBlockId('var.get', { VAR: sym })) !== null) {
      listed.push(sym);
    }
  }
  return listed;
}

/** Waits until the toolbox lists getters of exactly `expected` (in name order). */
async function waitForListing(app: FlowApp, expected: readonly string[], after: string) {
  let last: string[] = [];
  await waitFor(
    async () => {
      last = await listedGetters(app);
      return last.join(',') === expected.join(',');
    },
    {
      timeout: UI_TIMEOUT_MS,
      message: () =>
        `getters of [${expected.join(', ')}] in the toolbox after ${after} (it lists [${last.join(', ')}])`,
    },
  );
}

/**
 * Clicks block `blockId` with the pointer, holding the button for {@link HOLD_MS}, after
 * scrolling it into view without selecting it.
 */
async function heldClick(app: FlowApp, blockId: string): Promise<void> {
  await scrollIntoView(app.driver, app.hook, () => blockCorner(app, blockId));
  const point = await scrollIntoView(app.driver, app.hook, () => grabPointOf(app, blockId));
  if (await isSelected(app, blockId)) {
    throw new Error(`Block ${blockId} is selected already, so the click would change nothing`);
  }
  await app.driver
    .actions({ async: true })
    .move({ x: Math.round(point.x), y: Math.round(point.y), origin: Origin.VIEWPORT })
    .press(Button.LEFT)
    .pause(HOLD_MS)
    .release(Button.LEFT)
    .perform();
  await waitForSelected(app, blockId);
}

describe('Selecting blocks with the mouse', () => {
  it('rebuilds the Variables category after a click that holds the button', async (context) => {
    const flow = startFlow(context);
    const example = copyExample('guessing_game', flow.folders.projects);
    const app = await flow.launch({ dialogs: { open: [example] } });
    await openFromStartPage(app);
    await foldCodePanel(app);
    // The Too low! print, inside the loop: both variables are visible.
    await app.hook.selectBlock('b006');
    await waitForListing(app, [GUESS, SECRET], 'selecting b006 through the hook');

    // random 1 to 100, in the first declaration: nothing is declared yet.
    await heldClick(app, 'b001');
    await waitForListing(app, [], 'a held click on b001');
    // create secret: secret is visible after it.
    await heldClick(app, 'b002');
    await waitForListing(app, [SECRET], 'a held click on b002');
    // The Correct! print: both again.
    await heldClick(app, 'b008');
    await waitForListing(app, [GUESS, SECRET], 'a held click on b008');
  }, 120_000);
});
