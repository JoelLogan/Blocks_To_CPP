/**
 * *try … catch* in the real webview (docs/plans/m3-contracts/w2.md §1.7, §5.2, package B):
 *
 * - **⊕ adds a catch part.** On a *try* block with two catch parts, a click on its ⊕ adds a third
 *   part, which catches any error (`std::exception`, as the C++ panel shows) and has a variable of
 *   its own, named `e`; one Ctrl+Z takes it away again.
 * - **A catch's variable is offered only in its own part.** The *message of* block in the first
 *   part offers that part's error and not the second part's, and the other way round.
 *
 * Every click is made with real pointer input, every key pressed as a person presses it; what the
 * canvas holds is read from the test hook's document.
 */
import { By, type WebElement } from 'selenium-webdriver';
import { describe, expect, it } from 'vitest';

import { type BlockNode, moduleNodes, NodeIds, type ProjectDocument } from '../../support/bdm';
import { bringIntoView } from '../../support/canvas';
import {
  centerOn,
  currentDocument,
  fieldOf,
  foldCodePanel,
  waitForDelivery,
} from '../../support/editor';
import { clickMiddle, middleOf } from '../../support/ui';
import { waitFor } from '../../support/wait';
import { closeMenu, menuLabels } from './lib/editor';
import { type FlowApp, startFlow } from './lib/launch';
import { newEmptyProject } from './lib/project';
import { pressCtrl, UI_TIMEOUT_MS } from './lib/ui';

/** The two caught errors, by symbol and name. */
const FIRST = { sym: 's_errors_first', name: 'first' } as const;
const SECOND = { sym: 's_errors_second', name: 'second' } as const;

/** *print* of *message of* the caught error `sym`. */
function printMessage(ids: NodeIds, sym: string): { print: BlockNode; message: BlockNode } {
  const message: BlockNode = {
    id: ids.next(),
    type: 'error.message',
    v: 1,
    fields: { ERROR: { ref: sym } },
  };
  const print: BlockNode = {
    id: ids.next(),
    type: 'io.print',
    v: 1,
    extra: { itemCount: 1 },
    inputs: { ITEM0: { block: message } },
  };
  return { print, message };
}

/** A block of the canvas by ID, or `undefined`. */
function nodeOf(doc: ProjectDocument, id: string): BlockNode | undefined {
  return moduleNodes(doc).find((node) => node.id === id);
}

/** Waits until the *try* block `id` has `count` catch parts on the canvas, and returns it. */
function waitForParts(app: FlowApp, id: string, count: number): Promise<BlockNode> {
  return waitFor(
    async () => {
      const node = nodeOf(await currentDocument(app), id);
      return node?.extra?.['catchCount'] === count ? node : null;
    },
    { timeout: UI_TIMEOUT_MS, message: `the try block with ${String(count)} catch parts` },
  );
}

/**
 * The ⊕ of block `id`: its button whose text alternative starts with "Add", with the block centred
 * and the canvas panned until the button is well inside the visible canvas, as `fieldOf` does for a
 * field. The toolbox's flyout stays open over the canvas's left part: in a window the size of the
 * Windows runner's (1024 × 768) it covers the ⊕ of a *try* block in `main`, and a click there lands
 * on the flyout.
 */
async function addButton(app: FlowApp, id: string): Promise<WebElement> {
  await centerOn(app, id);
  await bringIntoView(app.driver, async () => middleOf(app.driver, await findAddButton(app, id)));
  return findAddButton(app, id);
}

/** Waits for the ⊕ of block `id`, wherever it is on the canvas. */
function findAddButton(app: FlowApp, id: string): Promise<WebElement> {
  return waitFor(
    async () => {
      const group = await app.hook.blockElement(id);
      if (group === null) {
        return null;
      }
      for (const button of await group.findElements(By.css('[role="button"]'))) {
        const label = (await button.getAttribute('aria-label')) ?? '';
        if (label.startsWith('Add') && (await button.isDisplayed())) {
          return button;
        }
      }
      return null;
    },
    { timeout: UI_TIMEOUT_MS, message: `the ⊕ button of block ${id}` },
  );
}

/** The labels of the menu of field `ERROR` of the *message of* block `id`. */
async function errorMenu(app: FlowApp, id: string): Promise<string[]> {
  await clickMiddle(app.driver, await fieldOf(app, id, 'ERROR'));
  const labels = await menuLabels(app.driver);
  await closeMenu(app.driver);
  return labels;
}

describe('Errors', () => {
  it('adds a catch part with ⊕ and offers each part’s error in that part only', async (context) => {
    const flow = startFlow(context);
    const app = await flow.launch();
    const mainId = await newEmptyProject(app);
    await foldCodePanel(app);
    const ids = new NodeIds('errors');
    const inFirst = printMessage(ids, FIRST.sym);
    const inSecond = printMessage(ids, SECOND.sym);
    const tryBlock: BlockNode = {
      id: ids.next(),
      type: 'error.try',
      v: 1,
      extra: { catchCount: 2 },
      fields: {
        ERROR0: 'std::invalid_argument',
        NAME0: FIRST,
        ERROR1: 'std::out_of_range',
        NAME1: SECOND,
      },
      statements: { CATCH0: [inFirst.print], CATCH1: [inSecond.print] },
    };
    await app.hook.insertBlocks(mainId, 'BODY', [tryBlock]);
    await waitForParts(app, tryBlock.id, 2);

    // Each part's message block offers its own error, not the other part's.
    const firstMenu = await errorMenu(app, inFirst.message.id);
    expect(firstMenu).toContain(FIRST.name);
    expect(firstMenu).not.toContain(SECOND.name);
    const secondMenu = await errorMenu(app, inSecond.message.id);
    expect(secondMenu).toContain(SECOND.name);
    expect(secondMenu).not.toContain(FIRST.name);

    // ⊕ adds a third part for any error, with a variable of its own. The document leaves out a
    // field the block's file did not hold while it has its default (sync/blockState.ts
    // `readFields`), so the part's error is read from the C++.
    await clickMiddle(app.driver, await addButton(app, tryBlock.id));
    const grown = await waitForParts(app, tryBlock.id, 3);
    expect(grown.fields?.['ERROR2']).toBeUndefined();
    await waitFor(
      async () =>
        (await app.hook.code()).includes('catch (const std::exception& e) {') ? true : null,
      {
        timeout: UI_TIMEOUT_MS,
        message: async () =>
          `a third catch part for any error, as e, in the C++ panel (it has:\n${await app.hook.code()})`,
      },
    );
    const third = grown.fields?.['NAME2'] as { sym?: string; name?: string } | undefined;
    expect(third?.name).toBe('e');
    expect([FIRST.sym, SECOND.sym]).not.toContain(third?.sym);
    expect(grown.fields?.['NAME0']).toEqual(FIRST);
    expect(grown.fields?.['NAME1']).toEqual(SECOND);

    // One Ctrl+Z takes the part away again; the others keep their errors.
    await waitForDelivery(app.driver);
    await pressCtrl(app.driver, 'z');
    const undone = await waitForParts(app, tryBlock.id, 2);
    expect(undone.fields?.['NAME1']).toEqual(SECOND);
    expect(undone.fields?.['ERROR1']).toBe('std::out_of_range');
  }, 180_000);
});
