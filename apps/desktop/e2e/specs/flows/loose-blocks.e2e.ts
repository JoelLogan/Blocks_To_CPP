/**
 * Blocks left loose on the canvas (docs/spec/03-block-language.md §3.6, 04 §4.2), in the real
 * webview: a block dropped on an empty part of the canvas stays selected, and the analysis has no
 * scope for it, yet
 *
 * - the Variables category still lists the variables of `main` (its getters), and
 * - the variable menu of a loose *set* lists `main`'s variables, `secret` and `guess`.
 */
import { describe, expect, it } from 'vitest';

import type { BlockNode, ProjectDocument } from '../../support/bdm';
import { panCanvas, visibleCanvas } from '../../support/canvas';
import { currentDocument, fieldOf, foldCodePanel } from '../../support/editor';
import { clickMiddle } from '../../support/ui';
import { sleep, waitFor } from '../../support/wait';
import { closeMenu, dropOnCanvas, menuLabels, waitForSelected } from './lib/editor';
import { type FlowApp, startFlow } from './lib/launch';
import { copyExample, openFromStartPage } from './lib/project';
import { UI_TIMEOUT_MS } from './lib/ui';

/** The symbols of the guessing game's variables (examples/guessing_game.b2c). */
const SECRET = 's_secret';
const GUESS = 's_guess';

/**
 * Long enough for the live preview of a change (a 50 ms debounce, then the analysis) and the
 * toolbox's rebuild after it (a 100 ms debounce) to have run.
 */
const SETTLE_MS = 1_500;

/** Waits for a loose (top-level) block of `type` on the canvas, and returns it. */
function looseBlock(app: FlowApp, type: string): Promise<BlockNode> {
  const find = (doc: ProjectDocument) =>
    (doc.modules[0]?.workspace.blocks ?? []).find((block) => block.type === type) ?? null;
  return waitFor(async () => find(await currentDocument(app)), {
    timeout: UI_TIMEOUT_MS,
    message: `a loose ${type} block on the canvas`,
  });
}

/** Whether the toolbox's flyout shows a getter of `sym`. */
async function listsGetter(app: FlowApp, sym: string): Promise<boolean> {
  return (await app.hook.flyoutBlockId('var.get', { VAR: sym })) !== null;
}

describe('Loose blocks', () => {
  it('keep main’s variables in the toolbox and in their own menus', async (context) => {
    const flow = startFlow(context);
    const example = copyExample('guessing_game', flow.folders.projects);
    const app = await flow.launch({ dialogs: { open: [example] } });
    await openFromStartPage(app);
    await foldCodePanel(app);
    // Nothing selected: the getters of main's variables are listed.
    await waitFor(async () => (await listsGetter(app, SECRET)) && (await listsGetter(app, GUESS)), {
      timeout: UI_TIMEOUT_MS,
      message: 'getters of secret and guess in the toolbox',
    });

    // The guessing game fills the canvas beside the open flyout. Pan it to the left, under the
    // flyout, so the blocks below are dropped on empty canvas, out of reach of main's connections.
    const canvas = await visibleCanvas(app.driver);
    await panCanvas(app.driver, { x: -(canvas.right - canvas.left - 40), y: 0 });

    // A print dropped loose stays selected; once the analysis and the toolbox's rebuild have run,
    // the getters are still listed.
    await dropOnCanvas(app, { category: 'Input / Output', type: 'io.print' });
    const print = await looseBlock(app, 'io.print');
    await waitForSelected(app, print.id);
    await sleep(SETTLE_MS);
    expect(await listsGetter(app, SECRET)).toBe(true);
    expect(await listsGetter(app, GUESS)).toBe(true);

    // A set dropped loose: its variable menu lists main's variables. The category lists each
    // variable's blocks in name order, and only the first variable's (guess) are sure to be in
    // view without scrolling the flyout.
    await dropOnCanvas(app, { category: 'Variables', type: 'var.set', fields: { VAR: GUESS } });
    const set = await looseBlock(app, 'var.set');
    await sleep(SETTLE_MS);
    await clickMiddle(app.driver, await fieldOf(app, set.id, 'VAR'));
    expect((await menuLabels(app.driver)).sort()).toEqual(['guess', 'secret']);
    await closeMenu(app.driver);
  }, 180_000);
});
