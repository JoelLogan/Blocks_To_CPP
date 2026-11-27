/**
 * Types shared across modules in the real webview (docs/plans/m3-contracts/w3-modules.md §1.3,
 * §5.2 T), on a program built block by block from the Empty template:
 *
 * - ⊕ *Add module* names a module `records`. There a *struct* from the Types category is named
 *   `Student` and given *share with other modules*, and a *define* from the Functions category is
 *   named `grade_of`, given a parameter with its ⊕ whose type is `Student` (chosen in the
 *   parameter row's type picker), and shared too.
 * - In `main`, a *create* from the Variables category, dropped into *when program starts*: its
 *   type field's picker offers `Student` under the heading *Module: records*, after the module's
 *   own types, and `Student` is chosen there; its value is a *new … with* from the Types category,
 *   with `Student` chosen in its own picker.
 * - In `records`, unticking *share with other modules* on the struct gives `B2C-E0206` on its use
 *   in `main` and `B2C-E0214` on `grade_of`, which takes it; ticking it again takes both away.
 *
 * Every click and drag is real pointer input, every key pressed as a person presses it; what the
 * canvas holds is read from the test hook's document. The C++ panel is folded away, and the
 * bottom dock too while the canvas is worked on: at 800 × 560, below the module strip and the
 * Restricted Mode banner, the canvas is otherwise too short for the flyout and a block side by
 * side.
 */
import { By, type WebDriver, type WebElement } from 'selenium-webdriver';
import { describe, expect, it } from 'vitest';

import { allNodes, type BlockNode, type ProjectDocument } from '../../support/bdm';
import { bringIntoView } from '../../support/canvas';
import {
  centerOn,
  currentDocument,
  dragFromToolbox,
  fieldOf,
  foldCodePanel,
  showProblems,
  waitForDelivery,
  waitForErrors,
} from '../../support/editor';
import { clickMiddle, clickTestId, middleOf, textContent, typeIntoField } from '../../support/ui';
import { waitFor } from '../../support/wait';
import { choosePickerType, dragValueFromToolbox, dropOnCanvas } from './lib/editor';
import { type FlowApp, startFlow } from './lib/launch';
import { newEmptyProject } from './lib/project';
import { nameInDialog, UI_TIMEOUT_MS } from './lib/ui';

/** The toolbox entries the flow takes. */
const STRUCT = { category: 'Types', type: 'type.struct' } as const;
const DEFINE = { category: 'Functions', type: 'func.define' } as const;
const CREATE = { category: 'Variables', type: 'var.declare' } as const;
const NEW = { category: 'Types', type: 'type.new' } as const;

/** Every block of every module of `doc`. */
function everyNode(doc: ProjectDocument): BlockNode[] {
  return doc.modules.flatMap((module) => allNodes(module.workspace.blocks));
}

/** The block `id` of any module, or `undefined`. */
function nodeOf(doc: ProjectDocument, id: string): BlockNode | undefined {
  return everyNode(doc).find((node) => node.id === id);
}

/** The name a block's declaration field `NAME` holds. */
function nameOf(node: BlockNode | undefined): string | undefined {
  return (node?.fields?.['NAME'] as { name?: string } | undefined)?.name;
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

/** Waits until the module whose file is `file` is the one shown. */
async function waitForShown(driver: WebDriver, file: string): Promise<void> {
  await waitFor(
    async () => {
      const button = await moduleButton(driver, file);
      return (await button.getAttribute('aria-pressed')) === 'true' ? true : null;
    },
    { timeout: UI_TIMEOUT_MS, message: `${file} shown` },
  );
  await waitForDelivery(driver);
}

/** Shows the module whose file is `file` with a click on its button, and waits until it is. */
async function showModule(driver: WebDriver, file: string): Promise<void> {
  await clickMiddle(driver, await moduleButton(driver, file));
  await waitForShown(driver, file);
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

/** Waits until the type picker has closed. */
async function waitForPickerClosed(driver: WebDriver): Promise<void> {
  await waitFor(async () => (await driver.findElements(By.css('.b2c-type-picker'))).length === 0, {
    timeout: UI_TIMEOUT_MS,
    message: 'the type picker to close',
  });
}

/**
 * Opens the type picker of field `field` of block `id`, chooses `label` in it, and waits until
 * `chosen` reads the block as having it.
 */
async function chooseType(
  app: FlowApp,
  id: string,
  field: string,
  label: string,
  chosen: (node: BlockNode | undefined) => boolean,
): Promise<void> {
  await clickMiddle(app.driver, await fieldOf(app, id, field));
  await choosePickerType(app.driver, label);
  await waitForPickerClosed(app.driver);
  await waitFor(async () => (chosen(nodeOf(await currentDocument(app), id)) ? true : null), {
    timeout: UI_TIMEOUT_MS,
    message: `${label} chosen in ${field} of block ${id}`,
  });
  await waitForDelivery(app.driver);
}

/** Ticks or unticks *share with other modules* on block `id`, and waits for the document. */
async function setShared(app: FlowApp, id: string, shared: boolean): Promise<void> {
  const box = await fieldOf(app, id, 'SHARED');
  await clickMiddle(app.driver, box);
  await waitFor(
    async () => {
      const node = nodeOf(await currentDocument(app), id);
      return (node?.fields?.['SHARED'] === true) === shared ? true : null;
    },
    {
      timeout: UI_TIMEOUT_MS,
      message: `share with other modules ${shared ? 'ticked' : 'unticked'}`,
    },
  );
  await waitForDelivery(app.driver);
}

/** Names the definition `id` `name` by typing into its name field, and waits for the document. */
async function rename(app: FlowApp, id: string, name: string): Promise<void> {
  await typeIntoField(app.driver, await fieldOf(app, id, 'NAME'), name);
  await waitFor(
    async () => (nameOf(nodeOf(await currentDocument(app), id)) === name ? true : null),
    {
      timeout: UI_TIMEOUT_MS,
      message: `block ${id} named ${name}`,
    },
  );
  await waitForDelivery(app.driver);
}

/**
 * Drops a toolbox entry on an empty part of the canvas of module `index` and waits for its new
 * top-level block there.
 */
async function dropNew(
  app: FlowApp,
  index: number,
  entry: typeof STRUCT | typeof DEFINE,
): Promise<BlockNode> {
  const blocks = async () => (await currentDocument(app)).modules[index]?.workspace.blocks ?? [];
  const before = new Set((await blocks()).map((block) => block.id));
  await dropOnCanvas(app, entry);
  const found = await waitFor(
    async () =>
      (await blocks()).find((block) => block.type === entry.type && !before.has(block.id)) ?? null,
    { timeout: UI_TIMEOUT_MS, message: `${entry.type} from ${entry.category} on the canvas` },
  );
  await waitForDelivery(app.driver);
  return found;
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
    const app = await flow.launch();
    const { driver } = app;
    const mainId = await newEmptyProject(app);
    await foldCodePanel(app);
    await foldBottomDock(driver);

    // (1) Module records: a shared struct Student, and a shared grade_of that takes one.
    await clickTestId(driver, 'module-add');
    await nameInDialog(driver, /Add module/, 'records', 'Add');
    await waitForShown(driver, 'records.cpp');
    const student = await dropNew(app, 1, STRUCT);
    await rename(app, student.id, 'Student');
    const studentSym = (student.fields?.['NAME'] as { sym: string }).sym;
    await setShared(app, student.id, true);

    const gradeOf = await dropNew(app, 1, DEFINE);
    await rename(app, gradeOf.id, 'grade_of');
    await clickAdd(app, gradeOf.id);
    await waitFor(
      async () => {
        const params = nodeOf(await currentDocument(app), gradeOf.id)?.extra?.['params'];
        return Array.isArray(params) && params.length === 1 ? true : null;
      },
      { timeout: UI_TIMEOUT_MS, message: 'grade_of with one parameter' },
    );
    await waitForDelivery(driver);
    const isStudent = (type: unknown) =>
      JSON.stringify(type) === JSON.stringify({ ref: studentSym });
    await chooseType(app, gradeOf.id, 'B2C_PARAM0_TYPE', 'Student', (node) => {
      const params = node?.extra?.['params'];
      return Array.isArray(params) && isStudent((params[0] as { type?: unknown }).type);
    });
    await setShared(app, gradeOf.id, true);
    await waitForErrors(app, 0);
    await foldBottomDock(driver);

    // (2) In main, a create's type picker lists Student under Module: records, after the module's
    // own types; it is chosen.
    await showModule(driver, 'main.cpp');
    await dragFromToolbox(app, CREATE, mainId, 'BODY');
    const create = await waitFor(
      async () =>
        (await currentDocument(app)).modules[0]?.workspace.blocks
          .find((block) => block.id === mainId)
          ?.statements?.['BODY']?.find((block) => block.type === CREATE.type) ?? null,
      { timeout: UI_TIMEOUT_MS, message: 'a create in when program starts' },
    );
    await waitForDelivery(driver);
    await clickMiddle(driver, await fieldOf(app, create.id, 'TYPE'));
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
    expect(records.slice(1)).toEqual(['Student']);
    const headings = groups.map((group) => group[0]);
    expect(headings.indexOf('Module: records')).toBeGreaterThan(headings.indexOf('Common'));
    await choosePickerType(driver, 'Student');
    await waitForPickerClosed(driver);
    await waitFor(
      async () =>
        isStudent(nodeOf(await currentDocument(app), create.id)?.fields?.['TYPE']) ? true : null,
      { timeout: UI_TIMEOUT_MS, message: 'the create of type Student' },
    );
    await waitForDelivery(driver);

    // new … with from the Types category as its value, with Student chosen in it.
    await dragValueFromToolbox(app, NEW, create.id, 'VALUE');
    const made = await waitFor(
      async () => {
        const value = nodeOf(await currentDocument(app), create.id)?.inputs?.['VALUE'];
        return value !== undefined && 'block' in value && value.block.type === NEW.type
          ? value.block
          : null;
      },
      { timeout: UI_TIMEOUT_MS, message: 'new … with as the create’s value' },
    );
    await waitForDelivery(driver);
    await chooseType(app, made.id, 'TYPE', 'Student', (node) => isStudent(node?.fields?.['TYPE']));
    await waitForErrors(app, 0);
    await foldBottomDock(driver);

    // (3) In records, Student's share with other modules unticked: B2C-E0206 where main uses it,
    // B2C-E0214 on grade_of, which takes it.
    await showModule(driver, 'records.cpp');
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

    // (4) Ticked again, both go.
    await foldBottomDock(driver);
    await setShared(app, student.id, true);
    await waitForErrors(app, 0);
  }, 240_000);
});
