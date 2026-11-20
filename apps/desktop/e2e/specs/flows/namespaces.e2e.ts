/**
 * Namespaces and *Textbook style* in the real webview (docs/plans/m3-contracts/w3-modules.md §1.4,
 * §2.12, §5.2 N2), on the fixture `namespaces.b2c` (*when program starts* and a *namespace geo*
 * holding a *define* `area` and a struct `Box`; the clipboard that the test hook inserts with puts
 * no definition into a namespace's mouth):
 *
 * - `area` called from `main` shows `geo::area(3)` in the C++.
 * - A *use namespace* block dropped from the toolbox, with `geo` chosen in its menu, writes
 *   `using namespace geo;`, and the call becomes `area(3)`.
 * - *Textbook style* turned on in the project settings page's *Language* section writes
 *   `using namespace std;`, and the code says `cout`.
 * - The struct renamed `list` (typed into its name field) then shows `B2C-E0215` in Problems;
 *   turning Textbook style off again takes it away.
 *
 * The C++ is read through the test hook, so the C++ panel is folded away to give the canvas room,
 * and the bottom dock too while the block is dropped: at 800 × 560, below the Restricted Mode
 * banner, the canvas is otherwise too short for the flyout and a block side by side. Every click
 * is real pointer input, with the block centred and its field brought into view first; problems
 * are matched without their spaces.
 */
import { By, type WebDriver } from 'selenium-webdriver';
import { describe, expect, it } from 'vitest';

import { allNodes, type BlockNode, NodeIds } from '../../support/bdm';
import {
  centerOn,
  currentDocument,
  fieldOf,
  foldCodePanel,
  showProblems,
  waitForDelivery,
  waitForErrors,
} from '../../support/editor';
import { chooseOption, clickTestId, testId, textContent, typeIntoField } from '../../support/ui';
import { waitFor } from '../../support/wait';
import { dropOnCanvas } from './lib/editor';
import { type FlowApp, startFlow } from './lib/launch';
import { copyFixture, openFromStartPage, printValue } from './lib/project';
import { chooseMenuItem, UI_TIMEOUT_MS, waitForTestId } from './lib/ui';

/** The fixture's blocks: *when program starts* and the namespace `geo`. */
const MAIN = 'b_main';
const GEO_SYM = 's_geo';
/** In `geo`: `define area(w)`, giving `w * 2`, and `struct Box`. */
const AREA_SYM = 's_area';
const BOX = 'b_box';

/** `print area(3)`. */
function printArea(ids: NodeIds): BlockNode {
  return printValue(ids, {
    expr: [{ ref: AREA_SYM }, { op: '(' }, { num: '3' }, { op: ')' }],
  });
}

/** Waits until the C++ satisfies `test`, describing it with `what`. */
async function waitForCode(
  app: FlowApp,
  what: string,
  test: (code: string) => boolean,
): Promise<string> {
  return waitFor(
    async () => {
      const code = await app.hook.code();
      return test(code) ? code : null;
    },
    { timeout: UI_TIMEOUT_MS, message: async () => `${what} in\n${await app.hook.code()}` },
  );
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

/** The Problems panel's rows' texts, without their spaces. */
async function problemRows(driver: WebDriver): Promise<string[]> {
  const rows = await driver.findElements(By.css('[data-testid="problem-row"]'));
  return Promise.all(rows.map(async (row) => (await textContent(driver, row)).replace(/\s+/g, '')));
}

/** Whether the project text has *Textbook style* on (`options.usingNamespaceStd`). */
function textbookStyleOf(text: string): boolean {
  const document = JSON.parse(text) as { project?: { options?: { usingNamespaceStd?: unknown } } };
  return document.project?.options?.usingNamespaceStd === true;
}

/**
 * Sets *Textbook style* on the project settings page (≡ → *Project settings…*) and saves, which
 * is one edit of the project; waits for the page to close.
 */
async function setTextbookStyle(app: FlowApp, on: boolean): Promise<void> {
  const { driver } = app;
  await chooseMenuItem(driver, 'Project settings…');
  await waitForTestId(driver, 'project-settings', true);
  const box = await testId(driver, 'project-settings-textbook', UI_TIMEOUT_MS);
  if ((await box.isSelected()) !== on) {
    await clickTestId(driver, 'project-settings-textbook');
  }
  await waitFor(
    async () => (await (await testId(driver, 'project-settings-textbook')).isSelected()) === on,
    { timeout: UI_TIMEOUT_MS, message: `the Textbook style switch ${on ? 'on' : 'off'}` },
  );
  await clickTestId(driver, 'project-settings-save');
  await waitForTestId(driver, 'project-settings', false);
  await waitFor(async () => textbookStyleOf(await app.hook.documentText()) === on, {
    timeout: UI_TIMEOUT_MS,
    message: `Textbook style ${on ? 'on' : 'off'} in the project`,
  });
}

describe('Namespaces', () => {
  it('qualifies a namespace’s function, drops the qualification after use namespace, and follows Textbook style', async (context) => {
    const flow = startFlow(context);
    const fixture = copyFixture('namespaces.b2c', flow.folders.projects);
    const app = await flow.launch({ dialogs: { open: [fixture] } });
    const { driver } = app;
    await openFromStartPage(app);
    await foldCodePanel(app);
    await waitForErrors(app, 0);
    const ids = new NodeIds('ns');

    // (1) The namespace's function called from main: geo::area(3).
    await app.hook.insertBlocks(MAIN, 'BODY', [printArea(ids)]);
    await waitForDelivery(driver);
    const qualified = await waitForCode(app, 'geo::area(3)', (code) =>
      code.includes('geo::area(3)'),
    );
    expect(qualified).toContain('namespace geo {');
    expect(qualified).toContain('std::cout');
    await waitForErrors(app, 0);

    // (2) use namespace, dropped from the toolbox, with geo chosen: area(3).
    await foldBottomDock(driver);
    await dropOnCanvas(app, { category: 'Program', type: 'program.use_namespace' });
    const use = await waitFor(
      async () =>
        (await currentDocument(app)).modules[0]?.workspace.blocks.find(
          (node) => node.type === 'program.use_namespace',
        ) ?? null,
      { timeout: UI_TIMEOUT_MS, message: 'the use namespace block on the canvas' },
    );
    await waitForDelivery(driver);
    await centerOn(app, use.id);
    await chooseOption(driver, await fieldOf(app, use.id, 'NS'), 'geo');
    await waitFor(
      async () =>
        (await currentDocument(app)).modules[0]?.workspace.blocks.find((node) => node.id === use.id)
          ?.fields?.['NS'] === GEO_SYM
          ? true
          : null,
      { timeout: UI_TIMEOUT_MS, message: 'geo chosen in use namespace' },
    );
    await waitForDelivery(driver);
    const plain = await waitForCode(
      app,
      'using namespace geo; and area(3)',
      (code) =>
        code.includes('using namespace geo;') &&
        code.includes('area(3)') &&
        !code.includes('geo::area(3)'),
    );
    expect(plain).not.toContain('using namespace std;');

    // (3) Textbook style on: using namespace std; and cout.
    await setTextbookStyle(app, true);
    const textbook = await waitForCode(
      app,
      'using namespace std; and cout',
      (code) =>
        code.includes('using namespace std;') &&
        code.includes('cout << area(3)') &&
        !code.includes('std::cout'),
    );
    expect(textbook).toContain('using namespace geo;');
    await waitForErrors(app, 0);

    // (4) The struct in geo renamed list: B2C-E0215 in Problems.
    await foldBottomDock(driver);
    await centerOn(app, BOX);
    await typeIntoField(driver, await fieldOf(app, BOX, 'NAME'), 'list');
    await waitFor(
      async () =>
        allNodes((await currentDocument(app)).modules[0]?.workspace.blocks ?? []).some(
          (node) =>
            node.id === BOX &&
            (node.fields?.['NAME'] as { name?: string } | undefined)?.name === 'list',
        ),
      { timeout: UI_TIMEOUT_MS, message: 'the struct renamed list' },
    );
    await waitForDelivery(driver);
    await showProblems(driver);
    await waitFor(
      async () =>
        (await problemRows(driver)).some(
          (row) => row.includes('B2C-E0215') && row.includes('list'),
        ),
      {
        timeout: UI_TIMEOUT_MS,
        message: async () =>
          `B2C-E0215 on list (Problems shows ${JSON.stringify(await problemRows(driver))})`,
      },
    );

    // (5) Textbook style off: the problem goes, and so does using namespace std;.
    await setTextbookStyle(app, false);
    await waitForErrors(app, 0);
    expect((await problemRows(driver)).some((row) => row.includes('B2C-E0215'))).toBe(false);
    await waitForCode(
      app,
      'std::cout without using namespace std;',
      (code) => code.includes('std::cout') && !code.includes('using namespace std;'),
    );
  }, 240_000);
});
