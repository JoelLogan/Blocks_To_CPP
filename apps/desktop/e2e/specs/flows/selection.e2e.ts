/**
 * The toolbox does not follow the selection (docs/spec/04-user-interface.md §4.2, decision D2 of
 * the M2 polish plan), in the real webview: the Variables category lists every variable of the
 * module, whatever block is selected, and selecting a block never shows the flyout again. A
 * showing keeps the blocks whose items did not change (src/editor/toolbox/inflater.ts) but builds
 * a declaring block, such as *create*, again, so the ID of the flyout's *create* block tells
 * whether the flyout was shown again. Blockly selects a block when the button is pressed, and a
 * person's click holds the button for a moment, so the blocks of the guessing game
 * (examples/guessing_game.b2c) are clicked with the button held for 100 ms, each scrolled into view
 * first through the test hook, which keeps the selection. A new variable does show the flyout
 * again, with its getter, and keeps the getters of the others.
 */
import { Button, Origin } from 'selenium-webdriver';
import { describe, expect, it } from 'vitest';

import { declareInt, NodeIds } from '../../support/bdm';
import { scrollIntoView } from '../../support/canvas';
import { blockCorner, foldCodePanel, grabPointOf } from '../../support/editor';
import { sleep, waitFor } from '../../support/wait';
import { isSelected, waitForSelected } from './lib/editor';
import { type FlowApp, startFlow } from './lib/launch';
import { copyExample, openFromStartPage } from './lib/project';
import { UI_TIMEOUT_MS } from './lib/ui';

/** The symbols of the guessing game's variables, in name order. */
const GUESS = 's_guess';
const SECRET = 's_secret';

/** The guessing game's `when program starts` block. */
const MAIN = 'b011';

/** How long a click holds the button down: about what a person's click does. */
const HOLD_MS = 100;

/**
 * Long enough for the toolbox to show its flyout again if it were to: its rebuild waits 100 ms
 * after the last change that asks for one.
 */
const SETTLE_MS = 1_000;

/**
 * The flyout block IDs of the getters of `syms` (`null` for a getter the toolbox does not list),
 * and last of its first *create* block, which each showing builds again, read in one call of the
 * hook: the toolbox shows its flyout again from a timer, which could run between separate calls
 * and mix the IDs of two showings.
 */
function getterIds(app: FlowApp, syms: readonly string[]): Promise<(string | null)[]> {
  return app.hook.flyoutBlockIds([
    ...syms.map((sym) => ({ type: 'var.get', fields: { VAR: sym } })),
    { type: 'var.declare' },
  ]);
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
  it('leaves the toolbox as it is: Variables lists every variable of the module', async (context) => {
    const flow = startFlow(context);
    const example = copyExample('guessing_game', flow.folders.projects);
    const app = await flow.launch({ dialogs: { open: [example] } });
    await openFromStartPage(app);
    await foldCodePanel(app);
    await waitFor(async () => (await getterIds(app, [GUESS, SECRET])).every((id) => id !== null), {
      timeout: UI_TIMEOUT_MS,
      message: 'getters of guess and secret in the toolbox',
    });
    // Let the toolbox's rebuilds after the project opened run, then take the flyout's blocks.
    await sleep(SETTLE_MS);
    const shown = await getterIds(app, [GUESS, SECRET]);
    expect(shown.every((id) => id !== null)).toBe(true);

    // random 1 to 100, in the first declaration (nothing is declared yet there); create secret;
    // the Correct! print inside the loop. The same getters, in the same flyout, after each.
    for (const blockId of ['b001', 'b002', 'b008']) {
      await heldClick(app, blockId);
      await sleep(SETTLE_MS);
      expect(await getterIds(app, [GUESS, SECRET]), `after a held click on ${blockId}`).toEqual(
        shown,
      );
    }

    // A new variable is listed: the flyout is shown again (a new *create* block), with its getter,
    // and keeps the getters whose items did not change.
    await app.hook.insertBlocks(MAIN, 'BODY', [
      declareInt(new NodeIds('select'), 's_e2e_tries', 'tries', 0),
    ]);
    const after = await waitFor(
      async () => {
        const ids = await getterIds(app, [GUESS, SECRET, 's_e2e_tries']);
        return ids.every((id) => id !== null) ? ids : null;
      },
      { timeout: UI_TIMEOUT_MS, message: 'the getter of the new variable in the toolbox' },
    );
    expect(after.slice(0, 2)).toEqual(shown.slice(0, 2));
    expect(after[3]).not.toBe(shown[2]);
  }, 120_000);
});
