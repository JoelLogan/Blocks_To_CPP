/**
 * Types shared across modules in the real webview (docs/plans/m3-contracts/w3-modules.md §1.3,
 * §5.2 T), on a copy of the `gradebook` example (`records` shares the struct `Student`, which
 * `main` and `report` use and `records`' shared `grade_of` takes):
 *
 * - In `main`, a type field's picker offers `Student` under the heading *Module: records*, after
 *   the module's own types.
 * - In `records`, *share with other modules* is shown on the struct; unticking it gives
 *   `B2C-E0206` on its use in `main` and `B2C-E0214` on `grade_of`, which takes it; ticking it
 *   again takes both away.
 *
 * Every click is real pointer input, every key pressed as a person presses it; what the canvas
 * holds is read from the test hook's document. The C++ panel is folded away, and the bottom dock
 * too while the canvas is clicked: at 800 × 560, below the module strip and the Restricted Mode
 * banner, the canvas is otherwise too short for the flyout and a block side by side.
 */
import { By, Key, type WebDriver, type WebElement } from 'selenium-webdriver';
import { describe, expect, it } from 'vitest';

import { allNodes, type BlockNode, NodeIds, type ProjectDocument } from '../../support/bdm';
import {
  centerOn,
  currentDocument,
  fieldOf,
  foldCodePanel,
  showProblems,
  waitForDelivery,
  waitForErrors,
} from '../../support/editor';
import { clickMiddle, clickTestId, textContent } from '../../support/ui';
import { waitFor } from '../../support/wait';
import { type FlowApp, startFlow } from './lib/launch';
import { copyExample, declare, openFromStartPage } from './lib/project';
import { UI_TIMEOUT_MS } from './lib/ui';

/** The example's modules, as the strip shows their files. */
const FILES = ['main.cpp', 'records.cpp', 'report.cpp'];

/** Every block of every module of `doc`. */
function everyNode(doc: ProjectDocument): BlockNode[] {
  return doc.modules.flatMap((module) => allNodes(module.workspace.blocks));
}

/** The top-level block of module `module` of type `type` named `name`. */
function definition(
  doc: ProjectDocument,
  module: string,
  type: string,
  name: string,
): BlockNode | undefined {
  return doc.modules
    .find((candidate) => candidate.name === module)
    ?.workspace.blocks.find(
      (block) =>
        block.type === type &&
        (block.fields?.['NAME'] as { name?: string } | undefined)?.name === name,
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

/** Shows the module whose file is `file` with a click on its button, and waits until it is. */
async function showModule(driver: WebDriver, file: string): Promise<void> {
  await clickMiddle(driver, await moduleButton(driver, file));
  await waitFor(
    async () => {
      const button = await moduleButton(driver, file);
      return (await button.getAttribute('aria-pressed')) === 'true' ? true : null;
    },
    { timeout: UI_TIMEOUT_MS, message: `${file} shown` },
  );
  await waitForDelivery(driver);
}

/** The open type picker's groups: each heading with its options, in order. */
async function pickerGroups(driver: WebDriver): Promise<string[][]> {
  const groups = await driver.findElements(By.css('.b2c-type-picker [role="group"]'));
  return Promise.all(
    groups.map(async (group) => {
      const heading = await group.findElement(By.css('.b2c-type-picker-heading'));
      const options = await group.findElements(By.css('[role="option"]'));
      return [
        (await textContent(driver, heading)).trim(),
        ...(await Promise.all(
          options.map(async (option) => (await textContent(driver, option)).trim()),
        )),
      ];
    }),
  );
}

/** Ticks or unticks *share with other modules* on block `id`, and waits for the document. */
async function setShared(app: FlowApp, id: string, shared: boolean): Promise<void> {
  const box = await fieldOf(app, id, 'SHARED');
  await clickMiddle(app.driver, box);
  await waitFor(
    async () => {
      const node = everyNode(await currentDocument(app)).find((candidate) => candidate.id === id);
      return (node?.fields?.['SHARED'] === true) === shared ? true : null;
    },
    {
      timeout: UI_TIMEOUT_MS,
      message: `share with other modules ${shared ? 'ticked' : 'unticked'}`,
    },
  );
  await waitForDelivery(app.driver);
}

/**
 * Folds the bottom dock away, if it is open, to give the canvas room; {@link showProblems} opens
 * it again.
 */
async function foldBottomDock(driver: WebDriver): Promise<void> {
  const open = await driver.findElements(By.css('section.bottom-dock[data-collapsed="false"]'));
  if (open.length > 0) {
    await clickTestId(driver, 'bottom-dock-toggle');
  }
  await waitFor(
    async () =>
      (await driver.findElements(By.css('section.bottom-dock[data-collapsed="true"]'))).length > 0,
    { timeout: UI_TIMEOUT_MS, message: 'the bottom dock to fold away' },
  );
}

/** The Problems panel's rows' texts. */
async function problemRows(driver: WebDriver): Promise<string[]> {
  const rows = await driver.findElements(By.css('[data-testid="problem-row"]'));
  return Promise.all(rows.map((row) => textContent(driver, row)));
}

describe('Types across modules', () => {
  it('offers another module’s shared struct under its module, and reports it unshared', async (context) => {
    const flow = startFlow(context);
    const example = copyExample('gradebook', flow.folders.projects);
    const app = await flow.launch({ dialogs: { open: [example] } });
    const { driver } = app;
    await openFromStartPage(app);
    await foldCodePanel(app);
    await moduleButton(driver, 'main.cpp');
    const doc = await currentDocument(app);
    expect(doc.modules.map((module) => `${module.name}.cpp`)).toEqual(FILES);
    const main = doc.modules[0]?.workspace.blocks.find((block) => block.type === 'program.main');
    const student = definition(doc, 'records', 'type.struct', 'Student');
    if (main === undefined || student === undefined) {
      throw new Error('the gradebook example has changed');
    }
    await waitForErrors(app, 0);
    await foldBottomDock(driver);

    // (1) In main, a type field's picker lists Student under Module: records.
    const probe = declare(new NodeIds('mt'), {
      sym: 'sym_mt_probe',
      name: 'probe',
      type: 'int',
      value: { expr: [{ num: '0' }] },
    });
    await app.hook.insertBlocks(main.id, 'BODY', [probe]);
    await waitForDelivery(driver);
    await centerOn(app, probe.id);
    await clickMiddle(driver, await fieldOf(app, probe.id, 'TYPE'));
    const groups = await waitFor(
      async () => {
        const found = await pickerGroups(driver);
        return found.some((group) => group[0] === 'Module: records') ? found : null;
      },
      {
        timeout: UI_TIMEOUT_MS,
        message: async () =>
          `the picker's Module: records group (it shows ${JSON.stringify(await pickerGroups(driver))})`,
      },
    );
    const records = groups.find((group) => group[0] === 'Module: records') ?? [];
    expect(records.slice(1).sort()).toEqual(['Grade', 'Student']);
    const headings = groups.map((group) => group[0]);
    expect(headings.indexOf('Module: records')).toBeGreaterThan(headings.indexOf('Common'));
    await driver.actions({ async: true }).sendKeys(Key.ESCAPE).perform();
    await waitFor(
      async () => (await driver.findElements(By.css('.b2c-type-picker'))).length === 0,
      { timeout: UI_TIMEOUT_MS, message: 'the type picker to close' },
    );

    // (2) In records, Student's share with other modules unticked: B2C-E0206 where main uses it,
    // B2C-E0214 on grade_of, which takes it.
    await showModule(driver, 'records.cpp');
    await centerOn(app, student.id);
    await setShared(app, student.id, false);
    await showProblems(driver);
    const rows = await waitFor(
      async () => {
        const found = await problemRows(driver);
        const squeezed = found.map((row) => row.replace(/\s+/g, ''));
        return squeezed.some((row) => row.includes('B2C-E0206') && row.includes('main')) &&
          squeezed.some((row) => row.includes('B2C-E0214') && row.includes('grade_of'))
          ? found
          : null;
      },
      {
        timeout: UI_TIMEOUT_MS,
        message: async () =>
          `B2C-E0206 in main and B2C-E0214 on grade_of (Problems shows ${JSON.stringify(await problemRows(driver))})`,
      },
    );
    expect(rows.some((row) => row.replace(/\s+/g, '').includes('isshared,butitsinput'))).toBe(true);

    // (3) Ticked again, both go.
    await foldBottomDock(driver);
    await centerOn(app, student.id);
    await setShared(app, student.id, true);
    await waitForErrors(app, 0);
  }, 240_000);
});
