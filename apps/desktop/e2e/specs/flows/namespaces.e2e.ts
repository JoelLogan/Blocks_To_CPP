/**
 * Namespaces and *Textbook style* in the real webview (docs/plans/m3-contracts/w3-modules.md §1.4,
 * §2.12, §5.2 N2), on a program built block by block from the Empty template:
 *
 * - A *namespace* from the Program category, named `geo`, takes a *define* from the Functions
 *   category into its mouth, named `area` and given a *print* from Input / Output; its call from
 *   *Your functions*, dragged into *when program starts*, shows `geo::area();` in the C++.
 * - A *use namespace* block dropped from the toolbox, with `geo` chosen in its menu, writes
 *   `using namespace geo;`, and the call becomes `area();`.
 * - *Textbook style* turned on in the project settings page's *Language* section writes
 *   `using namespace std;`, and the code says `cout`.
 * - A *struct* from the Types category, put into `geo`'s mouth below `area` and named `list`
 *   (typed into its name field), then shows `B2C-E0215` in Problems; turning Textbook style off
 *   again takes it away.
 *
 * Every block comes from the toolbox by a real drag (the test hook's insertion puts no definition
 * into a namespace's mouth): onto a connection, or onto an empty part of the canvas. The C++ is
 * read through the test hook, so the C++ panel is folded away to give the canvas room, and the
 * bottom dock too while blocks are dragged: at 800 × 560, below the Restricted Mode banner, the
 * canvas is otherwise too short for the flyout and a block side by side. Every click is real
 * pointer input, with the block centred and its field brought into view first; problems are
 * matched without their spaces.
 */
import { By, type WebDriver } from 'selenium-webdriver';
import { describe, expect, it } from 'vitest';

import { allNodes, type BlockNode, type ProjectDocument } from '../../support/bdm';
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
import { chooseOption, clickTestId, testId, textContent, typeIntoField } from '../../support/ui';
import { waitFor } from '../../support/wait';
import { dropOnCanvas } from './lib/editor';
import { type FlowApp, startFlow } from './lib/launch';
import { newEmptyProject } from './lib/project';
import { chooseMenuItem, UI_TIMEOUT_MS, waitForTestId } from './lib/ui';

/** The toolbox entries the flow takes. */
const NAMESPACE = { category: 'Program', type: 'program.namespace' } as const;
const USE_NAMESPACE = { category: 'Program', type: 'program.use_namespace' } as const;
const DEFINE = { category: 'Functions', type: 'func.define' } as const;
const PRINT = { category: 'Input / Output', type: 'io.print' } as const;
const STRUCT = { category: 'Types', type: 'type.struct' } as const;

/** Every block of the first module. */
function everyNode(doc: ProjectDocument): BlockNode[] {
  return allNodes(doc.modules[0]?.workspace.blocks ?? []);
}

/** The block `id`, or `undefined`. */
function nodeOf(doc: ProjectDocument, id: string): BlockNode | undefined {
  return everyNode(doc).find((node) => node.id === id);
}

/** The symbol and name a block's declaration field `NAME` holds. */
function declared(node: BlockNode | undefined): { sym?: string; name?: string } {
  return (node?.fields?.['NAME'] as { sym?: string; name?: string } | undefined) ?? {};
}

/** Waits for the block of `type` in statement input `input` of block `parent`, and returns it. */
async function waitForChild(
  app: FlowApp,
  parent: string,
  input: string,
  type: string,
): Promise<BlockNode> {
  const found = await waitFor(
    async () =>
      nodeOf(await currentDocument(app), parent)?.statements?.[input]?.find(
        (node) => node.type === type,
      ) ?? null,
    { timeout: UI_TIMEOUT_MS, message: `${type} in ${input} of block ${parent}` },
  );
  await waitForDelivery(app.driver);
  return found;
}

/** Names the block `id` `name` by typing into its name field, and waits for the document. */
async function rename(app: FlowApp, id: string, name: string): Promise<void> {
  await typeIntoField(app.driver, await fieldOf(app, id, 'NAME'), name);
  await waitFor(
    async () => (declared(nodeOf(await currentDocument(app), id)).name === name ? true : null),
    { timeout: UI_TIMEOUT_MS, message: `block ${id} named ${name}` },
  );
  await waitForDelivery(app.driver);
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
    const app = await flow.launch();
    const { driver } = app;
    const mainId = await newEmptyProject(app);
    await foldCodePanel(app);
    await foldBottomDock(driver);

    // (1) A namespace geo holding a function area that prints, called from main: geo::area().
    await dropOnCanvas(app, NAMESPACE);
    const geo = await waitFor(
      async () =>
        (await currentDocument(app)).modules[0]?.workspace.blocks.find(
          (node) => node.type === NAMESPACE.type,
        ) ?? null,
      { timeout: UI_TIMEOUT_MS, message: 'the namespace block on the canvas' },
    );
    await waitForDelivery(driver);
    await rename(app, geo.id, 'geo');
    const geoSym = declared(geo).sym;
    await dragFromToolbox(app, DEFINE, geo.id, 'DEFINITIONS');
    const area = await waitForChild(app, geo.id, 'DEFINITIONS', DEFINE.type);
    await rename(app, area.id, 'area');
    const areaSym = declared(area).sym;
    if (geoSym === undefined || areaSym === undefined) {
      throw new Error('the namespace or the function declares no symbol');
    }
    await dragFromToolbox(app, PRINT, area.id, 'BODY');
    await waitForChild(app, area.id, 'BODY', PRINT.type);
    await dragFromToolbox(
      app,
      { category: 'Functions', type: 'func.call_stmt', fields: { FUNC: areaSym } },
      mainId,
      'BODY',
    );
    await waitForChild(app, mainId, 'BODY', 'func.call_stmt');
    const qualified = await waitForCode(app, 'geo::area();', (code) =>
      code.includes('geo::area();'),
    );
    expect(qualified).toContain('namespace geo {');
    expect(qualified).toContain('std::cout');
    await waitForErrors(app, 0);

    // (2) use namespace, dropped from the toolbox, with geo chosen: area().
    await foldBottomDock(driver);
    await dropOnCanvas(app, USE_NAMESPACE);
    const use = await waitFor(
      async () =>
        (await currentDocument(app)).modules[0]?.workspace.blocks.find(
          (node) => node.type === USE_NAMESPACE.type,
        ) ?? null,
      { timeout: UI_TIMEOUT_MS, message: 'the use namespace block on the canvas' },
    );
    await waitForDelivery(driver);
    await centerOn(app, use.id);
    await chooseOption(driver, await fieldOf(app, use.id, 'NS'), 'geo');
    await waitFor(
      async () =>
        nodeOf(await currentDocument(app), use.id)?.fields?.['NS'] === geoSym ? true : null,
      { timeout: UI_TIMEOUT_MS, message: 'geo chosen in use namespace' },
    );
    await waitForDelivery(driver);
    const plain = await waitForCode(
      app,
      'using namespace geo; and area();',
      (code) =>
        code.includes('using namespace geo;') &&
        code.includes('area();') &&
        !code.includes('geo::area();'),
    );
    expect(plain).not.toContain('using namespace std;');

    // (3) Textbook style on: using namespace std; and cout.
    await setTextbookStyle(app, true);
    const textbook = await waitForCode(
      app,
      'using namespace std; and cout',
      (code) =>
        code.includes('using namespace std;') &&
        code.includes('cout <<') &&
        !code.includes('std::cout'),
    );
    expect(textbook).toContain('using namespace geo;');
    await waitForErrors(app, 0);

    // (4) A struct put into geo below area and named list: B2C-E0215 in Problems.
    await foldBottomDock(driver);
    await dragFromToolbox(app, STRUCT, area.id, 'next');
    const struct = await waitForChild(app, geo.id, 'DEFINITIONS', STRUCT.type);
    await rename(app, struct.id, 'list');
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
