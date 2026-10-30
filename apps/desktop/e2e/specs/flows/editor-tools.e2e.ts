/**
 * W4's editor tools in the real webview (docs/plans/m3-contracts/w4-editor-tools.md §8):
 *
 * - **Quick Insert.** With *when program starts* selected, `Ctrl+Space`, `int total = 5` and Enter
 *   put `create int total = 5` after it, in its body; one `Ctrl+Z` takes it away.
 * - **The type picker.** A `create` block's type field opens the picker; with the keyboard alone,
 *   `map` Enter, `string` Enter, `list` Right, `int` Enter make *map from string to list of int*,
 *   and the C++ panel declares `std::map<std::string, std::vector<int>>`.
 * - **A quick fix.** With the `create` of `score` deleted, its getter shows `B2C-E0201`; the
 *   Problems panel's *Fix: Create variable “score”* brings a `create` of `score` back, and one
 *   `Ctrl+Z` takes it away again.
 * - **C++ labels.** With *Block labels* set to C++ on the Settings page, the program block reads
 *   `int main(`; back to Friendly, *when program starts*.
 *
 * Every key is pressed as a person presses it; what the canvas holds is read from the test hook's
 * document.
 */
import { By, Key, type WebDriver } from 'selenium-webdriver';
import { describe, expect, it } from 'vitest';

import {
  type BlockNode,
  declareInt,
  moduleNodes,
  NodeIds,
  statementsOf,
  onlyTopBlock,
  varGet,
} from '../../support/bdm';
import { currentDocument, fieldOf, showProblems, waitForDelivery } from '../../support/editor';
import { clickMiddle, testIdText, textContent } from '../../support/ui';
import { waitFor } from '../../support/wait';
import { clickBlock } from './lib/editor';
import { type FlowApp, startFlow } from './lib/launch';
import { newEmptyProject } from './lib/project';
import { backToEditor, openSettings, pressCtrl, UI_TIMEOUT_MS } from './lib/ui';

/** The blocks of `main`'s body, from the canvas. */
async function body(app: FlowApp): Promise<readonly BlockNode[]> {
  return statementsOf(onlyTopBlock(await currentDocument(app), 'program.main'), 'BODY');
}

/** Types `text` into the element with the keyboard focus. */
async function typeText(driver: WebDriver, ...keys: string[]): Promise<void> {
  await driver
    .switchTo()
    .activeElement()
    .sendKeys(...keys);
}

/** Waits for Quick Insert's search box to have the focus. */
async function waitForQuickInsert(driver: WebDriver): Promise<void> {
  await waitFor(
    async () => {
      const focused = await driver.switchTo().activeElement();
      return (await focused.getAttribute('aria-label')) === 'Insert a block';
    },
    { timeout: UI_TIMEOUT_MS, message: 'Quick Insert open with its box focused' },
  );
}

describe('the editor tools', () => {
  it('insert a typed line at the selection with Quick Insert, as one undo step', async (context) => {
    const flow = startFlow(context);
    const app = await flow.launch();
    const main = await newEmptyProject(app);
    await clickBlock(app, main);
    await pressCtrl(app.driver, ' ');
    await waitForQuickInsert(app.driver);
    await typeText(app.driver, 'int total = 5');
    await waitFor(
      async () => {
        const [first] = await app.driver.findElements(
          By.css('[data-testid="quick-insert"] [role="option"]'),
        );
        return (
          first !== undefined && (await textContent(app.driver, first)).includes('create total')
        );
      },
      { timeout: UI_TIMEOUT_MS, message: 'the typed line as the first result' },
    );
    await typeText(app.driver, Key.ENTER);
    const created = await waitFor(
      async () => (await body(app)).find((block) => block.type === 'var.declare') ?? null,
      { timeout: UI_TIMEOUT_MS, message: 'create total in main' },
    );
    expect(created.fields?.['NAME']).toMatchObject({ name: 'total' });
    expect(created.fields?.['TYPE']).toBe('int');
    await waitForDelivery(app.driver);
    await pressCtrl(app.driver, 'z');
    await waitFor(async () => (await body(app)).length === 0, {
      timeout: UI_TIMEOUT_MS,
      message: 'the inserted block undone',
    });
  });

  it('build a map type with the type picker and the keyboard alone', async (context) => {
    const flow = startFlow(context);
    const app = await flow.launch();
    const main = await newEmptyProject(app);
    const ids = new NodeIds('w4t');
    const decl = declareInt(ids, 'sym_w4t_table', 'table', 0);
    await app.hook.insertBlocks(main, 'BODY', [decl]);
    await clickMiddle(app.driver, await fieldOf(app, decl.id, 'TYPE'));
    await waitFor(
      async () => {
        const focused = await app.driver.switchTo().activeElement();
        return (await focused.getAttribute('role')) === 'combobox';
      },
      { timeout: UI_TIMEOUT_MS, message: 'the type picker open with its box focused' },
    );
    for (const [text, key] of [
      ['map', Key.ENTER],
      ['string', Key.ENTER],
      ['list', Key.ARROW_RIGHT],
      ['int', Key.ENTER],
    ] as const) {
      await typeText(app.driver, text);
      await typeText(app.driver, key);
    }
    await waitFor(
      async () => {
        const node = moduleNodes(await currentDocument(app)).find((block) => block.id === decl.id);
        return JSON.stringify(node?.fields?.['TYPE']) ===
          JSON.stringify({ 'std::map': ['std::string', { 'std::vector': 'int' }] })
          ? node
          : null;
      },
      { timeout: UI_TIMEOUT_MS, message: 'the map type on the create block' },
    );
    await waitFor(
      async () => (await app.hook.code()).includes('std::map<std::string, std::vector<int>> table'),
      {
        timeout: UI_TIMEOUT_MS,
        message: 'the map in the C++',
      },
    );
  });

  it('bring a deleted variable back with its quick fix, as one undo step', async (context) => {
    const flow = startFlow(context);
    const app = await flow.launch();
    const main = await newEmptyProject(app);
    const ids = new NodeIds('w4f');
    const decl = declareInt(ids, 'sym_w4f_score', 'score', 3);
    const show: BlockNode = {
      id: ids.next(),
      type: 'io.print',
      v: 1,
      extra: { itemCount: 1 },
      fields: { NEWLINE: true, SEP: 'none', STREAM: 'out' },
      inputs: { ITEM0: { block: varGet(ids, 'sym_w4f_score') } },
    };
    await app.hook.insertBlocks(main, 'BODY', [decl, show]);
    await waitFor(async () => (await app.hook.code()).includes('int score = 3;'), {
      timeout: UI_TIMEOUT_MS,
      message: 'score in the C++',
    });
    // Delete the create block with the keyboard.
    await clickBlock(app, decl.id);
    await app.driver.actions({ async: true }).sendKeys(Key.DELETE).perform();
    await waitFor(async () => !(await body(app)).some((block) => block.type === 'var.declare'), {
      timeout: UI_TIMEOUT_MS,
      message: 'the create block deleted',
    });
    await showProblems(app.driver);
    const fix = await waitFor(
      async () => {
        for (const button of await app.driver.findElements(
          By.css('[data-testid="problems-panel"] button'),
        )) {
          if ((await textContent(app.driver, button)) === 'Fix: Create variable “score”') {
            return button;
          }
        }
        return null;
      },
      { timeout: 15_000, message: 'the quick fix in Problems' },
    );
    await fix.click();
    const back = await waitFor(
      async () => (await body(app)).find((block) => block.type === 'var.declare') ?? null,
      { timeout: UI_TIMEOUT_MS, message: 'the create of score again' },
    );
    expect(back.fields?.['NAME']).toMatchObject({ sym: 'sym_w4f_score', name: 'score' });
    await waitForDelivery(app.driver);
    await clickBlock(app, main);
    await pressCtrl(app.driver, 'z');
    await waitFor(async () => !(await body(app)).some((block) => block.type === 'var.declare'), {
      timeout: UI_TIMEOUT_MS,
      message: 'the fix undone',
    });
  });

  it('show the blocks in C++ labels and back', async (context) => {
    const flow = startFlow(context);
    const app = await flow.launch();
    const main = await newEmptyProject(app);
    const label = async () => {
      const group = await app.hook.blockElement(main);
      return group === null ? '' : textContent(app.driver, group);
    };
    expect(await label()).toContain('when program starts');
    /** Whether the title shows unsaved changes (its `•`). */
    const unsaved = async () => (await testIdText(app.driver, 'project-name')).includes('•');
    await waitFor(async () => !(await unsaved()), {
      timeout: UI_TIMEOUT_MS,
      message: 'the new project to have no unsaved changes',
    });
    for (const [mode, expected] of [
      ['cpp', 'int main('],
      ['friendly', 'when program starts'],
    ] as const) {
      await openSettings(app.driver);
      await app.driver
        .findElement(
          By.css(`[data-testid="settings-block-labels"] input[type="radio"][value="${mode}"]`),
        )
        .click();
      await backToEditor(app.driver);
      await waitFor(async () => (await label()).includes(expected), {
        timeout: UI_TIMEOUT_MS,
        message: async () =>
          `the program block to read “${expected}” (it reads “${await label()}”)`,
      });
    }
    // A label change is no edit: the project has nothing to save.
    await waitForDelivery(app.driver);
    expect(await unsaved()).toBe(false);
    const doc = await currentDocument(app);
    expect(onlyTopBlock(doc, 'program.main').id).toBe(main);
  });
});
