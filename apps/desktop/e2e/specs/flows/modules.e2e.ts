/**
 * Modules in the real webview (docs/plans/m3-contracts/w3-modules.md §1.2, §2.5, §2.6, §5.2 E):
 *
 * - ⊕ *Add module* names a module `utils` in the app's dialog; the strip shows `main.cpp` and
 *   `utils.cpp`, and the canvas keeps its room below the strip at every window size.
 * - In `utils` a *define* from Functions, given a print and *share with other modules*, is offered
 *   in `main`'s *Your functions* under *Module: utils*; its call in *when program starts* runs.
 * - The C++ tab lists `main.cpp`, then `utils.cpp` and `utils.hpp` under *Module utils*; clicking
 *   the header's declaration selects the *define* in `utils`, which is shown.
 * - Renaming `utils` to `tools` (its actions, opened with Shift+F10 on its button) keeps the call:
 *   `#include "tools.hpp"`, and it runs again.
 * - Unticking *share* gives `B2C-E0206` in Problems under module `main`; choosing it shows `main`
 *   and selects the call. Ticked again, deleting the module (confirmed) gives `B2C-E0201` on the
 *   call; *Undo delete* brings it back, and the program runs again.
 *
 * Needs package F (functions shared across modules) for the call, the header and the run. Every
 * click is real pointer input, every key pressed as a person presses it; what the canvas holds is
 * read from the test hook's document.
 */
import { By, Key, type WebDriver, type WebElement } from 'selenium-webdriver';
import { describe, expect, it } from 'vitest';

import { allNodes, type BlockNode, NodeIds, print, type ProjectDocument } from '../../support/bdm';
import {
  centerOn,
  currentDocument,
  dragFromToolbox,
  fieldOf,
  foldCodePanel,
  showCodePanel,
  showProblems,
  waitForDelivery,
  waitForErrors,
} from '../../support/editor';
import { clickMiddle, clickTestId, openCategory, textContent } from '../../support/ui';
import { waitFor } from '../../support/wait';
import { waitForTranscript } from './lib/console';
import { dropOnCanvas, waitForSelected } from './lib/editor';
import { type FlowApp, startFlow } from './lib/launch';
import { newEmptyProject } from './lib/project';
import {
  answerDialog,
  BUILD_TIMEOUT_MS,
  DIALOG,
  UI_TIMEOUT_MS,
  waitForDialog,
  waitForToolchain,
} from './lib/ui';

/** What the shared function prints. */
const GREETING = 'B2C-MODULES-HELLO';

/** The toolbox entry of *define*. */
const DEFINE = { category: 'Functions', type: 'func.define' } as const;

/** The flyout's labels (headings and notes). */
const FLYOUT_LABELS = '.blocklyToolboxFlyout .blocklyFlyoutLabel';

/** The least height the canvas keeps below the strip, in CSS pixels (800 × 560 included). */
const CANVAS_MIN_HEIGHT = 200;

/** Every block of every module of `doc`. */
function everyNode(doc: ProjectDocument): BlockNode[] {
  return doc.modules.flatMap((module) => allNodes(module.workspace.blocks));
}

/** The block `id` of any module, or `undefined`. */
function nodeOf(doc: ProjectDocument, id: string): BlockNode | undefined {
  return everyNode(doc).find((node) => node.id === id);
}

/** The module buttons of the strip: each one's file label and whether it is pressed. */
async function strip(driver: WebDriver): Promise<{ file: string; pressed: boolean }[]> {
  const buttons = await driver.findElements(By.css('[data-testid="module-switch"]'));
  return Promise.all(
    buttons.map(async (button) => ({
      file: (await textContent(driver, button)).trim(),
      pressed: (await button.getAttribute('aria-pressed')) === 'true',
    })),
  );
}

/** Waits until the strip shows `files`, with `shown` pressed. */
async function waitForStrip(driver: WebDriver, files: string[], shown: string): Promise<void> {
  await waitFor(
    async () => {
      const now = await strip(driver);
      return JSON.stringify(now.map((button) => button.file)) === JSON.stringify(files) &&
        now.find((button) => button.pressed)?.file === shown
        ? true
        : null;
    },
    {
      timeout: UI_TIMEOUT_MS,
      message: async () =>
        `the strip to show ${files.join(', ')} with ${shown} shown (it shows ${JSON.stringify(await strip(driver))})`,
    },
  );
}

/** The strip's button of the module whose file is `file`. */
async function moduleButton(driver: WebDriver, file: string): Promise<WebElement> {
  return waitFor(
    async () => {
      for (const button of await driver.findElements(By.css('[data-testid="module-switch"]'))) {
        if ((await textContent(driver, button)).trim() === file) {
          return button;
        }
      }
      return null;
    },
    { timeout: UI_TIMEOUT_MS, message: `the strip's button of ${file}` },
  );
}

/** Shows the module whose file is `file` with a click on its button. */
async function showModule(driver: WebDriver, file: string, files: string[]): Promise<void> {
  await clickMiddle(driver, await moduleButton(driver, file));
  await waitForStrip(driver, files, file);
  await waitForDelivery(driver);
}

/** Replaces the text of the open dialog's field with `text` and presses `button`. */
async function nameInDialog(
  driver: WebDriver,
  title: RegExp,
  text: string,
  button: string,
): Promise<void> {
  await waitForDialog(driver, title);
  const field = await driver.findElement(By.css(`${DIALOG} input`));
  await clickMiddle(driver, field);
  await driver
    .actions({ async: true })
    .keyDown(Key.CONTROL)
    .sendKeys('a')
    .keyUp(Key.CONTROL)
    .sendKeys(text)
    .perform();
  await waitFor(async () => (await field.getAttribute('value')) === text, {
    timeout: UI_TIMEOUT_MS,
    message: `the dialog's field to hold "${text}"`,
  });
  await answerDialog(driver, button);
}

/** Waits until the C++ panel's shown file holds `text` (spaces ignored), and returns it. */
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

/** Ticks or unticks *share with other modules* on block `id`, and waits for the document. */
async function setShared(app: FlowApp, id: string, shared: boolean): Promise<void> {
  const box = await fieldOf(app, id, 'SHARED');
  await clickMiddle(app.driver, box);
  await waitFor(
    async () => {
      const value = nodeOf(await currentDocument(app), id)?.fields?.['SHARED'];
      return (value === true) === shared ? true : null;
    },
    {
      timeout: UI_TIMEOUT_MS,
      message: `share with other modules ${shared ? 'ticked' : 'unticked'}`,
    },
  );
  await waitForDelivery(app.driver);
}

/** Runs the program with F5 and waits until the console shows the greeting `count` times in all. */
async function runAndSee(app: FlowApp, count: number): Promise<void> {
  await app.driver.actions({ async: true }).sendKeys(Key.F5).perform();
  await waitForTranscript(
    app,
    (text) => text.split(GREETING).length - 1 >= count,
    BUILD_TIMEOUT_MS,
    `the greeting ${String(count)} times`,
  );
}

/** The C++ tab's file switcher: its options' paths and groups, in order. */
async function fileList(driver: WebDriver): Promise<string[]> {
  const listed: unknown = await driver.executeScript(`
    const select = document.querySelector('[data-testid="code-panel"] select');
    if (select === null) { return []; }
    return Array.from(select.children, (child) =>
      child.tagName === 'OPTGROUP'
        ? child.label + ': ' + Array.from(child.children, (option) => option.value).join(', ')
        : child.value);
  `);
  return Array.isArray(listed) ? listed.map(String) : [];
}

describe('Modules', () => {
  it('adds, shares, calls, renames, deletes and brings back a module', async (context) => {
    const flow = startFlow(context);
    const app = await flow.launch();
    const { driver } = app;
    await waitForToolchain(app);
    const mainId = await newEmptyProject(app);

    // (0) The strip with one module, and the canvas below it with room to work.
    await waitForStrip(driver, ['main.cpp'], 'main.cpp');
    const room: unknown = await driver.executeScript(`
      const strip = document.querySelector('[data-testid="module-strip"]').getBoundingClientRect();
      const canvas = document.querySelector('.blockly-host').getBoundingClientRect();
      return [strip.bottom, canvas.top, canvas.height];
    `);
    const [stripBottom, canvasTop, canvasHeight] = room as [number, number, number];
    expect(canvasTop).toBeGreaterThanOrEqual(stripBottom - 1);
    expect(canvasHeight).toBeGreaterThanOrEqual(CANVAS_MIN_HEIGHT);
    // The toolbox's flyout stays open over the canvas's left part: the C++ panel folds away, as
    // in the other flows, so the drags have room; (4) shows it again.
    await foldCodePanel(app);

    // (1) Add module utils: it is shown.
    await clickTestId(driver, 'module-add');
    await nameInDialog(driver, /Add module/, 'utils', 'Add');
    await waitForStrip(driver, ['main.cpp', 'utils.cpp'], 'utils.cpp');

    // (2) A function in utils that prints, shared with the other modules.
    await dropOnCanvas(app, DEFINE);
    const define = await waitFor(
      async () =>
        (await currentDocument(app)).modules[1]?.workspace.blocks.find(
          (block) => block.type === DEFINE.type,
        ) ?? null,
      { timeout: UI_TIMEOUT_MS, message: 'define in utils' },
    );
    const sym = (define.fields?.['NAME'] as { sym: string; name: string }).sym;
    const name = (define.fields?.['NAME'] as { sym: string; name: string }).name;
    await app.hook.insertBlocks(define.id, 'BODY', [print(new NodeIds('mod'), GREETING)]);
    await waitForDelivery(driver);
    await setShared(app, define.id, true);

    // (3) In main, Your functions lists it under Module: utils; its call goes into main.
    await showModule(driver, 'main.cpp', ['main.cpp', 'utils.cpp']);
    await openCategory(driver, 'Functions');
    await waitFor(
      async () => {
        const labels = await driver.findElements(By.css(FLYOUT_LABELS));
        const texts = await Promise.all(labels.map((label) => textContent(driver, label)));
        return texts.some((text) => text.trim() === 'Module: utils') ? true : null;
      },
      { timeout: UI_TIMEOUT_MS, message: 'the heading Module: utils in Your functions' },
    );
    await dragFromToolbox(
      app,
      { category: 'Functions', type: 'func.call_stmt', fields: { FUNC: sym } },
      mainId,
      'BODY',
    );
    const call = await waitFor(
      async () =>
        everyNode(await currentDocument(app)).find(
          (node) =>
            node.type === 'func.call_stmt' &&
            (node.fields?.['FUNC'] as { ref?: string } | undefined)?.ref === sym,
        ) ?? null,
      { timeout: UI_TIMEOUT_MS, message: `the call of ${name} in main` },
    );
    await waitForDelivery(driver);
    await waitForCode(app, '#include "utils.hpp"');
    await runAndSee(app, 1);

    // (4) The C++ tab lists main.cpp, then utils.cpp and utils.hpp under utils (no
    // b2c_support.hpp: a print of a text uses no helper); clicking the header's declaration
    // selects the define in utils.
    await showCodePanel(app);
    await waitFor(
      async () =>
        JSON.stringify(await fileList(driver)) ===
        JSON.stringify(['main.cpp', 'Module utils: utils.cpp, utils.hpp'])
          ? true
          : null,
      {
        timeout: UI_TIMEOUT_MS,
        message: async () =>
          `the C++ tab's files (they are ${JSON.stringify(await fileList(driver))})`,
      },
    );
    await (
      await driver.findElement(
        By.css('[data-testid="code-panel"] select option[value="utils.hpp"]'),
      )
    ).click();
    // The click lands on the declaration's name: the middle of the whole line is past its end,
    // where the caret is after the declaration's range.
    const declaration = await waitFor(
      async () => {
        for (const line of await driver.findElements(
          By.css('[data-testid="code-panel"] .cm-line'),
        )) {
          if ((await textContent(driver, line)).includes(`${name}(`)) {
            for (const token of await line.findElements(By.css('span'))) {
              if ((await textContent(driver, token)) === name) {
                return token;
              }
            }
          }
        }
        return null;
      },
      { timeout: UI_TIMEOUT_MS, message: `the declaration of ${name} in utils.hpp` },
    );
    await clickMiddle(driver, declaration);
    await waitForSelected(app, define.id);
    await waitForStrip(driver, ['main.cpp', 'utils.cpp'], 'utils.cpp');

    // (5) Rename utils to tools from its own actions: the call keeps working.
    const utils = await moduleButton(driver, 'utils.cpp');
    await clickMiddle(driver, utils);
    await driver
      .actions({ async: true })
      .keyDown(Key.SHIFT)
      .sendKeys(Key.F10)
      .keyUp(Key.SHIFT)
      .perform();
    const rename = await waitFor(
      async () => {
        for (const item of await driver.findElements(By.css('[role="menuitem"]'))) {
          if ((await textContent(driver, item)).trim() === 'Rename…') {
            return item;
          }
        }
        return null;
      },
      { timeout: UI_TIMEOUT_MS, message: 'Rename… in the module’s actions' },
    );
    await rename.click();
    await nameInDialog(driver, /Rename module “utils”/, 'tools', 'Rename');
    await waitForStrip(driver, ['main.cpp', 'tools.cpp'], 'tools.cpp');
    await showModule(driver, 'main.cpp', ['main.cpp', 'tools.cpp']);
    await waitForCode(app, '#include "tools.hpp"');
    await runAndSee(app, 2);

    // (6) Not shared any more: B2C-E0206 on the call, under module main; choosing it lands there.
    await showModule(driver, 'tools.cpp', ['main.cpp', 'tools.cpp']);
    await centerOn(app, define.id);
    await setShared(app, define.id, false);
    await waitForErrors(app, 1);
    const row = await waitFor(
      async () => {
        for (const candidate of await driver.findElements(By.css('[data-testid="problem-row"]'))) {
          const text = await textContent(driver, candidate);
          if (text.includes('B2C-E0206') || text.includes('does not share it')) {
            return candidate;
          }
        }
        return null;
      },
      { timeout: UI_TIMEOUT_MS, message: 'the B2C-E0206 problem' },
    );
    expect(await textContent(driver, row)).toContain('main');
    await clickMiddle(driver, await row.findElement(By.css('[role="gridcell"]')));
    await waitForStrip(driver, ['main.cpp', 'tools.cpp'], 'main.cpp');
    await waitForSelected(app, call.id);

    // (7) Shared again, then the module deleted (it holds blocks, so it asks): B2C-E0201 on the
    // call; Undo delete brings it back, and the program runs again.
    await showModule(driver, 'tools.cpp', ['main.cpp', 'tools.cpp']);
    await centerOn(app, define.id);
    await setShared(app, define.id, true);
    await waitForErrors(app, 0);
    await clickTestId(driver, 'module-actions');
    const remove = await waitFor(
      async () => {
        for (const item of await driver.findElements(By.css('[role="menuitem"]'))) {
          if ((await textContent(driver, item)).trim().startsWith('Delete…')) {
            return item;
          }
        }
        return null;
      },
      { timeout: UI_TIMEOUT_MS, message: 'Delete… in Module actions' },
    );
    await remove.click();
    await waitForDialog(driver, /Delete module “tools”\?/);
    await answerDialog(driver, 'Delete module');
    await waitForStrip(driver, ['main.cpp'], 'main.cpp');
    await showProblems(driver);
    await waitFor(
      async () => {
        for (const candidate of await driver.findElements(By.css('[data-testid="problem-row"]'))) {
          if ((await textContent(driver, candidate)).includes('B2C-E0201')) {
            return true;
          }
        }
        return null;
      },
      { timeout: UI_TIMEOUT_MS, message: 'B2C-E0201 on the call' },
    );
    await clickTestId(driver, 'module-undo-delete');
    await waitForStrip(driver, ['main.cpp', 'tools.cpp'], 'tools.cpp');
    await waitForErrors(app, 0);
    await runAndSee(app, 3);
  }, 300_000);
});
