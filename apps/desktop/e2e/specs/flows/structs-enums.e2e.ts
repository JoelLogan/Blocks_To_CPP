/**
 * Structs and enums in the real webview (docs/plans/m3-contracts/w2.md §1.5, §5.2, package C1):
 *
 * - **A struct with two fields.** A *struct* block taken from the Types category onto the canvas
 *   gets two fields with two clicks on its ⊕: `int field` and `int field2`.
 * - **Used in *create*.** `create [MyStruct] p = new MyStruct with …`: the *new … with* block
 *   offers ⊕ until it has a part for each field, and its parts are labelled `field:` and
 *   `field2:`.
 * - **A rename follows.** `field2` renamed to `y` in the struct relabels the *new … with* part and
 *   the *… of p* block that reads it, and the C++ panel uses `.y`.
 * - **An enum value's menus.** An enum from the Types category gets `Value` and `Value2` with its ⊕;
 *   beside a variable of the enum and a whole number in `main`, an *enum value*'s value menu lists
 *   exactly the enum's values, and its enum menu the enum, not the variable of its type.
 *
 * Every click is made with real pointer input, every key pressed as a person presses it; what the
 * canvas holds is read from the test hook's document.
 */
import { By, type WebElement } from 'selenium-webdriver';
import { describe, expect, it } from 'vitest';

import { type BlockNode, moduleNodes, NodeIds, type ProjectDocument } from '../../support/bdm';
import { bringIntoView, visibleCanvas } from '../../support/canvas';
import { drag } from '../../support/drag';
import {
  centerOn,
  currentDocument,
  fieldOf,
  foldCodePanel,
  grabPointOf,
} from '../../support/editor';
import { clickMiddle, middleOf, textContent, typeIntoField } from '../../support/ui';
import { sleep, waitFor } from '../../support/wait';
import { closeMenu, dropOnCanvas, menuLabels } from './lib/editor';
import { type FlowApp, startFlow } from './lib/launch';
import { newEmptyProject } from './lib/project';
import { UI_TIMEOUT_MS } from './lib/ui';

/** The *struct* and *enum* toolbox entries. */
const STRUCT = { category: 'Types', type: 'type.struct' } as const;
const ENUM = { category: 'Types', type: 'type.enum' } as const;

/** One row of a struct's `fields` or an enum's `values`. */
interface Row {
  readonly sym: string;
  readonly name: string;
}

/** The top-level blocks of the first module. */
function topBlocks(doc: ProjectDocument): readonly BlockNode[] {
  return doc.modules[0]?.workspace.blocks ?? [];
}

/** A block of the canvas by ID, or `undefined`. */
function nodeOf(doc: ProjectDocument, id: string): BlockNode | undefined {
  return moduleNodes(doc).find((node) => node.id === id);
}

/** The rows of a definition's row extra (`fields`, `values`). */
function rowsOf(node: BlockNode | undefined, key: string): readonly Row[] {
  const rows = node?.extra?.[key];
  return Array.isArray(rows) ? (rows as Row[]) : [];
}

/** The symbol a definition's `NAME` declares. */
function declared(node: BlockNode): Row {
  const name = node.fields?.['NAME'] as Row | undefined;
  if (name === undefined) {
    throw new Error(`block ${node.id} declares nothing`);
  }
  return name;
}

/** Drops a definition from the Types category and waits for it: the new top-level block. */
async function dropDefinition(
  app: FlowApp,
  entry: typeof STRUCT | typeof ENUM,
): Promise<BlockNode> {
  const before = new Set(topBlocks(await currentDocument(app)).map((block) => block.id));
  await dropOnCanvas(app, entry);
  return waitFor(
    async () =>
      topBlocks(await currentDocument(app)).find(
        (block) => block.type === entry.type && !before.has(block.id),
      ) ?? null,
    { timeout: UI_TIMEOUT_MS, message: `${entry.type} from Types on the canvas` },
  );
}

/** Waits until block `id` holds `count` rows in its row extra `key`, and returns them. */
function waitForRows(
  app: FlowApp,
  id: string,
  key: string,
  count: number,
): Promise<readonly Row[]> {
  return waitFor(
    async () => {
      const rows = rowsOf(nodeOf(await currentDocument(app), id), key);
      return rows.length === count ? rows : null;
    },
    { timeout: UI_TIMEOUT_MS, message: `block ${id} with ${String(count)} ${key}` },
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
      // A block's own buttons come before those of the blocks in its inputs.
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

/**
 * Clicks the ⊕ of block `id`, with the block centred and the canvas panned until the button is
 * well inside the visible canvas (the toolbox's flyout stays open over the canvas's left part).
 */
async function clickAdd(app: FlowApp, id: string): Promise<void> {
  await centerOn(app, id);
  await bringIntoView(app.driver, async () => middleOf(app.driver, await findAddButton(app, id)));
  await clickMiddle(app.driver, await findAddButton(app, id));
}

/** A block's SVG group on the page, its nested blocks' included, in viewport pixels. */
async function blockBox(
  app: FlowApp,
  id: string,
): Promise<{ left: number; top: number; right: number; bottom: number }> {
  const group = await app.hook.blockElement(id);
  if (group === null) {
    throw new Error(`block ${id} is not on the canvas`);
  }
  // WebKitWebDriver gives no rectangle for an SVG group: the page measures it.
  const box: unknown = await app.driver.executeScript(
    'const r = arguments[0].getBoundingClientRect(); return [r.left, r.top, r.right, r.bottom];',
    group,
  );
  if (!Array.isArray(box) || box.length !== 4 || !box.every((n) => typeof n === 'number')) {
    throw new Error(`block ${id} has no box on the page`);
  }
  const [left, top, right, bottom] = box as [number, number, number, number];
  return { left, top, right, bottom };
}

/** The most drags {@link moveBelow} makes before it gives up. */
const MAX_MOVES = 6;

/**
 * Drags the definition `id` below block `other` while the two overlap, with real pointer input. In
 * a small window (800 × 560) the definition is dropped just below the empty `main`, which then grows
 * over it when the test puts statements in it, and a click on those statements lands on the
 * definition. Each drag starts with the definition's grab point near the top of the visible canvas
 * and goes down as far as the canvas shows.
 */
async function moveBelow(app: FlowApp, id: string, other: string): Promise<void> {
  for (let move = 0; move < MAX_MOVES; move += 1) {
    const block = await blockBox(app, id);
    const below = await blockBox(app, other);
    if (
      block.right <= below.left ||
      below.right <= block.left ||
      block.bottom <= below.top ||
      below.bottom <= block.top
    ) {
      return;
    }
    const rect = await visibleCanvas(app.driver);
    const start = await grabPointOf(app, id);
    await app.hook.scrollCanvasBy(0, Math.round(rect.top + 60 - start.y));
    await sleep(100);
    const grab = await grabPointOf(app, id);
    const room = rect.bottom - 60 - grab.y;
    const needed = below.bottom - block.top + 24;
    await drag(app.driver, grab, { x: grab.x, y: grab.y + Math.max(16, Math.min(room, needed)) });
    await sleep(400);
  }
  throw new Error(`block ${id} still overlaps block ${other}`);
}

/** The text a block's SVG group shows, its nested blocks' included. */
async function blockText(app: FlowApp, id: string): Promise<string> {
  const group = await app.hook.blockElement(id);
  return group === null ? '' : textContent(app.driver, group);
}

describe('Structs and enums', () => {
  it('makes a struct with two fields, uses it in create, and follows a renamed field', async (context) => {
    const flow = startFlow(context);
    const app = await flow.launch();
    const mainId = await newEmptyProject(app);
    await foldCodePanel(app);

    // A struct from the Types category, given two fields with its ⊕.
    const struct = await dropDefinition(app, STRUCT);
    await clickAdd(app, struct.id);
    await waitForRows(app, struct.id, 'fields', 1);
    await clickAdd(app, struct.id);
    const fields = await waitForRows(app, struct.id, 'fields', 2);
    expect(fields.map((row) => row.name)).toEqual(['field', 'field2']);
    const [, second] = fields;
    if (second === undefined) {
      throw new Error('no second field');
    }
    const structSym = declared(struct).sym;

    // create [MyStruct] p = new MyStruct with …, and print (field2 of p).
    const ids = new NodeIds('structs');
    const made: BlockNode = {
      id: ids.next(),
      type: 'type.new',
      v: 1,
      extra: { fieldCount: 0 },
      fields: { TYPE: { ref: structSym } },
    };
    const create: BlockNode = {
      id: ids.next(),
      type: 'var.declare',
      v: 1,
      fields: { CONST: false, NAME: { sym: 's_structs_p', name: 'p' }, TYPE: { ref: structSym } },
      inputs: { VALUE: { block: made } },
    };
    const read: BlockNode = {
      id: ids.next(),
      type: 'type.field',
      v: 1,
      fields: { MEMBER: { ref: second.sym } },
      inputs: { OBJ: { expr: [{ ref: 's_structs_p' }] } },
    };
    const print: BlockNode = {
      id: ids.next(),
      type: 'io.print',
      v: 1,
      extra: { itemCount: 1 },
      inputs: { ITEM0: { block: read } },
    };
    await app.hook.insertBlocks(mainId, 'BODY', [create, print]);
    await moveBelow(app, struct.id, mainId);

    // new … with offers ⊕ until it has a part for each field, each labelled by its field.
    await clickAdd(app, made.id);
    await waitFor(
      async () => nodeOf(await currentDocument(app), made.id)?.extra?.['fieldCount'] === 1,
      { timeout: UI_TIMEOUT_MS, message: 'new … with with one part' },
    );
    await clickAdd(app, made.id);
    await waitFor(
      async () => nodeOf(await currentDocument(app), made.id)?.extra?.['fieldCount'] === 2,
      { timeout: UI_TIMEOUT_MS, message: 'new … with with two parts' },
    );
    await waitFor(
      async () => {
        const text = await blockText(app, made.id);
        return text.includes('field:') && text.includes('field2:') ? true : null;
      },
      {
        timeout: UI_TIMEOUT_MS,
        message: async () =>
          `the parts labelled field: and field2: (${await blockText(app, made.id)})`,
      },
    );

    // Renaming field2 in the struct renames its uses.
    await typeIntoField(app.driver, await fieldOf(app, struct.id, 'B2C_FIELD1_NAME'), 'y');
    await waitFor(
      async () => {
        const madeText = await blockText(app, made.id);
        const readText = await blockText(app, read.id);
        return madeText.includes('y:') && !madeText.includes('field2') && readText.includes('y')
          ? true
          : null;
      },
      {
        timeout: UI_TIMEOUT_MS,
        message: async () =>
          `the renamed field in new … with (${await blockText(app, made.id)}) and in … of p (${await blockText(app, read.id)})`,
      },
    );
    await waitFor(async () => ((await app.hook.code()).includes('p.y') ? true : null), {
      timeout: UI_TIMEOUT_MS,
      message: async () => `p.y in the C++ panel (it has:\n${await app.hook.code()})`,
    });
  }, 180_000);

  it('lists only the enum’s values in an enum value’s menu, and only enums as its enum', async (context) => {
    const flow = startFlow(context);
    const app = await flow.launch();
    const mainId = await newEmptyProject(app);
    await foldCodePanel(app);

    // An enum from the Types category, given Value and Value2 with its ⊕.
    const definition = await dropDefinition(app, ENUM);
    await clickAdd(app, definition.id);
    await waitForRows(app, definition.id, 'values', 1);
    await clickAdd(app, definition.id);
    await waitForRows(app, definition.id, 'values', 2);
    const enumSym = declared(definition).sym;

    // main: a variable of the enum, a whole number, and print (MyEnum ?).
    const ids = new NodeIds('enums');
    const colour: BlockNode = {
      id: ids.next(),
      type: 'var.declare',
      v: 1,
      fields: { CONST: false, NAME: { sym: 's_enums_c', name: 'c' }, TYPE: { ref: enumSym } },
    };
    const count: BlockNode = {
      id: ids.next(),
      type: 'var.declare',
      v: 1,
      fields: { CONST: false, NAME: { sym: 's_enums_n', name: 'n' }, TYPE: 'int' },
      inputs: { VALUE: { expr: [{ num: '0' }] } },
    };
    const value: BlockNode = {
      id: ids.next(),
      type: 'type.enum_value',
      v: 1,
      fields: { ENUM: { ref: enumSym } },
    };
    const print: BlockNode = {
      id: ids.next(),
      type: 'io.print',
      v: 1,
      extra: { itemCount: 1 },
      inputs: { ITEM0: { block: value } },
    };
    await app.hook.insertBlocks(mainId, 'BODY', [colour, count, print]);
    await moveBelow(app, definition.id, mainId);

    // Its value menu lists the enum's values only, not the variables in scope.
    const menuOf = async (field: string, expected: string): Promise<string[]> =>
      waitFor(
        async () => {
          await clickMiddle(app.driver, await fieldOf(app, value.id, field));
          const shown = await menuLabels(app.driver);
          await closeMenu(app.driver);
          // The menu lists them once the analysis has seen the block.
          return shown.includes(expected) ? shown : null;
        },
        { timeout: UI_TIMEOUT_MS, message: `the ${field} menu of the enum value with ${expected}` },
      );
    expect(await menuOf('VALUE', 'Value')).toEqual(['Value', 'Value2']);
    // Its enum menu lists the enum, not the variable `c` of the enum's type.
    expect(await menuOf('ENUM', 'MyEnum')).toEqual(['MyEnum']);
  }, 180_000);
});
