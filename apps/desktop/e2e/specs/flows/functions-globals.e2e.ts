/**
 * Functions and globals in the real webview (docs/plans/m3-contracts/w2.md §1.9, §5.2, package
 * C2):
 *
 * - **A global under *Everywhere*.** A *global* block taken from the Variables category onto the
 *   canvas declares `value` (`int value = 0;` in the C++ panel), and the Variables category then
 *   lists its getter under the heading *Everywhere*.
 * - **A default value typed into a parameter row.** A *define* block from the Functions category
 *   gets a parameter with its ⊕; a number from the Math category dropped at the end of that row,
 *   with `5` typed into it, is the parameter's default value: `int param = 5` in the function's
 *   declaration.
 *
 * Every click and drag is made with real pointer input, every key pressed as a person presses it;
 * what the canvas holds is read from the test hook's document.
 */
import { By, type WebElement } from 'selenium-webdriver';
import { describe, expect, it } from 'vitest';

import { type BlockNode, moduleNodes, type ProjectDocument } from '../../support/bdm';
import { bringIntoView } from '../../support/canvas';
import { centerOn, currentDocument, fieldOf, waitForDelivery } from '../../support/editor';
import { clickMiddle, middleOf, openCategory, textContent, typeIntoField } from '../../support/ui';
import { waitFor } from '../../support/wait';
import { dragValueFromToolbox, dropOnCanvas } from './lib/editor';
import { type FlowApp, startFlow } from './lib/launch';
import { newEmptyProject } from './lib/project';
import { UI_TIMEOUT_MS } from './lib/ui';

/** The toolbox entries the flows take. */
const GLOBAL = { category: 'Variables', type: 'var.global' } as const;
const DEFINE = { category: 'Functions', type: 'func.define' } as const;
const NUMBER = { category: 'Math', type: 'math.number' } as const;

/** The heading of the globals' getters in the Variables category (src/editor/toolbox/contents.ts). */
const EVERYWHERE = 'Everywhere';

/** The flyout's headings (Blockly's flyout labels). */
const FLYOUT_LABELS = '.blocklyToolboxFlyout .blocklyFlyoutLabel';

/** The top-level blocks of the first module. */
function topBlocks(doc: ProjectDocument): readonly BlockNode[] {
  return doc.modules[0]?.workspace.blocks ?? [];
}

/** A block of the canvas by ID, or `undefined`. */
function nodeOf(doc: ProjectDocument, id: string): BlockNode | undefined {
  return moduleNodes(doc).find((node) => node.id === id);
}

/** The symbol and name a block's declaration field `NAME` holds. */
function declared(node: BlockNode): { sym: string; name: string } {
  const name = node.fields?.['NAME'] as { sym: string; name: string } | undefined;
  if (name === undefined) {
    throw new Error(`block ${node.id} declares nothing`);
  }
  return name;
}

/** Drops a toolbox entry on an empty part of the canvas and waits for its new top-level block. */
async function dropNew(app: FlowApp, entry: typeof GLOBAL | typeof DEFINE): Promise<BlockNode> {
  const before = new Set(topBlocks(await currentDocument(app)).map((block) => block.id));
  await dropOnCanvas(app, entry);
  return waitFor(
    async () =>
      topBlocks(await currentDocument(app)).find(
        (block) => block.type === entry.type && !before.has(block.id),
      ) ?? null,
    { timeout: UI_TIMEOUT_MS, message: `${entry.type} from ${entry.category} on the canvas` },
  );
}

/** Waits until the C++ panel holds `text` (spaces ignored), and returns the code. */
function waitForCode(app: FlowApp, text: string): Promise<string> {
  const squeezed = text.replace(/\s+/g, '');
  return waitFor(
    async () => {
      const code = await app.hook.code();
      return code.replace(/\s+/g, '').includes(squeezed) ? code : null;
    },
    {
      timeout: UI_TIMEOUT_MS,
      message: async () => `${text} in the C++ panel (it has:\n${await app.hook.code()})`,
    },
  );
}

/** Waits for the ⊕ of block `id` (its button whose text alternative starts with "Add"). */
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

/** Clicks the ⊕ of block `id`, with the block centred and the button inside the visible canvas. */
async function clickAdd(app: FlowApp, id: string): Promise<void> {
  await centerOn(app, id);
  await bringIntoView(app.driver, async () => middleOf(app.driver, await findAddButton(app, id)));
  await clickMiddle(app.driver, await findAddButton(app, id));
}

/** The flyout's headings as it shows them now. */
async function flyoutHeadings(app: FlowApp): Promise<string[]> {
  const labels = await app.driver.findElements(By.css(FLYOUT_LABELS));
  return Promise.all(labels.map(async (label) => (await textContent(app.driver, label)).trim()));
}

describe('Functions and globals', () => {
  it('lists a global from the Variables category under Everywhere', async (context) => {
    const flow = startFlow(context);
    const app = await flow.launch();
    await newEmptyProject(app);

    const global = await dropNew(app, GLOBAL);
    const { sym, name } = declared(global);
    expect(name).toBe('value');
    await waitForCode(app, 'int value = 0;');

    // The Variables category lists its getter under the heading Everywhere.
    await openCategory(app.driver, GLOBAL.category);
    await waitFor(async () => ((await flyoutHeadings(app)).includes(EVERYWHERE) ? true : null), {
      timeout: UI_TIMEOUT_MS,
      message: async () =>
        `the heading ${EVERYWHERE} in the Variables category (it shows ${JSON.stringify(await flyoutHeadings(app))})`,
    });
    await waitFor(() => app.hook.flyoutBlockId('var.get', { VAR: sym }), {
      timeout: UI_TIMEOUT_MS,
      message: `the getter of ${name} in the Variables category`,
    });
  }, 180_000);

  it('takes a default value typed into a parameter row', async (context) => {
    const flow = startFlow(context);
    const app = await flow.launch();
    await newEmptyProject(app);

    // A function from the Functions category, given one parameter with its ⊕.
    const define = await dropNew(app, DEFINE);
    await clickAdd(app, define.id);
    await waitFor(
      async () => {
        const params = nodeOf(await currentDocument(app), define.id)?.extra?.['params'];
        return Array.isArray(params) && params.length === 1 ? true : null;
      },
      { timeout: UI_TIMEOUT_MS, message: 'the function with one parameter' },
    );
    await waitForDelivery(app.driver);

    // A number from Math at the end of the row, and 5 typed into it.
    await dragValueFromToolbox(app, NUMBER, define.id, 'DEFAULT0');
    const number = await waitFor(
      async () => {
        const block = nodeOf(await currentDocument(app), define.id)?.inputs?.['DEFAULT0'];
        return block !== undefined && 'block' in block && block.block.type === NUMBER.type
          ? block.block
          : null;
      },
      { timeout: UI_TIMEOUT_MS, message: 'a number in the parameter row' },
    );
    await typeIntoField(app.driver, await fieldOf(app, number.id, 'VALUE'), '5');
    await waitForCode(app, 'void myFunction(int param = 5);');
  }, 180_000);
});
