/**
 * Typed slots in the real webview (ADR-0018, docs/plans/m3-contracts/slots.md §9 "End to end"):
 *
 * - **Create.** A text block taken from the toolbox towards `create [int] n`'s value does not
 *   snap: `n` keeps its `0` and the text lies loose beside it. Taken towards `create [string] s`'s
 *   value, it snaps.
 * - **Start values.** Choosing `double` in `n`'s type menu swaps its start value `0` for `0.0`, and
 *   one Ctrl+Z restores both the type and the `0` (one undo step); Ctrl+Y does both again.
 * - **Call arguments.** In the factorial example, a text block taken towards the argument of
 *   `factorial (i)` does not snap, where a number does; and the keyboard mover (`M`) does not offer
 *   the argument as a place for the text block, where it does for a number block.
 *
 * Every drag is made with real pointer input, every key pressed as a person presses it; what the
 * canvas holds is read from the test hook's document, and the mover's places from its screen-reader
 * announcements (src/editor/keyboard/mover.ts: `… “block” …, k of N.`).
 */
import { By, Key, type WebDriver } from 'selenium-webdriver';
import { describe, expect, it } from 'vitest';

import {
  type BlockNode,
  moduleNodes,
  NodeIds,
  num,
  type ProjectDocument,
  str,
} from '../../support/bdm';
import {
  centerOn,
  currentDocument,
  fieldOf,
  foldCodePanel,
  waitForDelivery,
} from '../../support/editor';
import { clickMiddle, pressKey, textContent } from '../../support/ui';
import { waitFor } from '../../support/wait';
import { dragValueFromToolbox, dropOnCanvas, menuLabels } from './lib/editor';
import { type FlowApp, startFlow } from './lib/launch';
import { copyExample, declare, newEmptyProject, openFromStartPage } from './lib/project';
import { pressCtrl, UI_TIMEOUT_MS } from './lib/ui';

/** The call `factorial (i)` of examples/factorial.b2c, and the print it is in. */
const FACTORIAL_CALL = 'b005';

/** The text toolbox entry, and the number one (Text and Math categories). */
const TEXT = { category: 'Text', type: 'text.literal' } as const;
const NUMBER = { category: 'Math', type: 'math.number' } as const;

/** A block of the canvas by ID, or `undefined`. */
function nodeOf(doc: ProjectDocument, id: string): BlockNode | undefined {
  return moduleNodes(doc).find((node) => node.id === id);
}

/** The top-level blocks of the first module. */
function topBlocks(doc: ProjectDocument): readonly BlockNode[] {
  return doc.modules[0]?.workspace.blocks ?? [];
}

/** What value input `input` of block `id` holds now, from the canvas. */
async function inputOf(app: FlowApp, id: string, input: string): Promise<unknown> {
  return nodeOf(await currentDocument(app), id)?.inputs?.[input];
}

/** Waits until value input `input` of block `id` holds `expected` (compared as JSON). */
function waitForInput(
  app: FlowApp,
  id: string,
  input: string,
  expected: unknown,
  what: string,
): Promise<true> {
  return waitFor(
    async () =>
      JSON.stringify(await inputOf(app, id, input)) === JSON.stringify(expected) ? true : null,
    {
      timeout: UI_TIMEOUT_MS,
      message: async () =>
        `${what} (input ${input} of ${id} holds ${JSON.stringify(await inputOf(app, id, input))})`,
    },
  );
}

/** Waits until the type field of `create` block `id` is `type`. */
function waitForType(app: FlowApp, id: string, type: string): Promise<true> {
  return waitFor(
    async () => (nodeOf(await currentDocument(app), id)?.fields?.['TYPE'] === type ? true : null),
    { timeout: UI_TIMEOUT_MS, message: `the type of ${id} to be ${type}` },
  );
}

/** Clicks the item labelled `label` of Blockly's open dropdown menu. */
async function chooseMenuItem(driver: WebDriver, label: string): Promise<void> {
  expect(await menuLabels(driver)).toContain(label);
  for (const item of await driver.findElements(By.css('.blocklyDropDownDiv .blocklyMenuItem'))) {
    if ((await item.isDisplayed()) && (await textContent(driver, item)) === label) {
      await clickMiddle(driver, item);
      return;
    }
  }
  throw new Error(`The open menu has no item "${label}"`);
}

/** The ID of the one top-level block of `type` that is not in `known`. */
function newTopBlock(app: FlowApp, type: string, known: ReadonlySet<string>): Promise<string> {
  return waitFor(
    async () =>
      topBlocks(await currentDocument(app)).find(
        (block) => block.type === type && !known.has(block.id),
      )?.id ?? null,
    { timeout: UI_TIMEOUT_MS, message: `a new ${type} loose on the canvas` },
  );
}

/** Runs in the webview: what the block editor's live region says now (trimmed). */
const ANNOUNCEMENT_SCRIPT = `
  const status = document.querySelector('[data-testid="keyboard-announcer"] [role="status"]');
  return status === null ? '' : (status.textContent || '').trim();
`;

/** The editor's last announcement. */
function announcement(driver: WebDriver): Promise<string> {
  return driver.executeScript<string>(ANNOUNCEMENT_SCRIPT);
}

/** Waits for an announcement that `test` accepts, and returns it. */
function waitForAnnouncement(
  driver: WebDriver,
  test: (text: string) => boolean,
  what: string,
): Promise<string> {
  return waitFor(
    async () => {
      const text = await announcement(driver);
      return test(text) ? text : null;
    },
    {
      timeout: UI_TIMEOUT_MS,
      message: async () => `${what} (the editor says "${await announcement(driver)}")`,
    },
  );
}

/** A place's position in an announcement: `…, 3 of 12.` */
const POSITION = /, (\d+) of (\d+)\.$/;

/**
 * Every place the keyboard mover offers block `blockId` (as it announces them), in its order: the
 * block is selected (which focuses it), picked up with `M`, the places are gone through
 * with the down arrow, and Escape leaves it where it was.
 */
async function keyboardPlaces(app: FlowApp, blockId: string): Promise<string[]> {
  const { driver } = app;
  // Selected (and so focused) through the hook: a click on a number block opens its field.
  await centerOn(app, blockId);
  await pressKey(driver, 'm');
  const first = await waitForAnnouncement(
    driver,
    (text) => text.startsWith('Moving') && POSITION.test(text),
    `the keyboard move of ${blockId} to start`,
  );
  const [, start = '1', total = '0'] = POSITION.exec(first) ?? [];
  const count = Number(total);
  const places = [first.replace(/^.*Escape cancels\. /, '').replace(POSITION, '')];
  // The move starts at the block's own place (loose on the canvas, the last one); the down arrow
  // goes on from there and wraps round to the first.
  for (let step = 1, index = Number(start); step < count; step += 1) {
    index = (index % count) + 1;
    const expected = `, ${String(index)} of ${String(count)}.`;
    await pressKey(driver, Key.ARROW_DOWN);
    const text = await waitForAnnouncement(
      driver,
      (said) => said.endsWith(expected),
      `place ${String(index)} of ${String(count)}`,
    );
    places.push(text.replace(POSITION, ''));
  }
  await pressKey(driver, Key.ESCAPE);
  await waitForAnnouncement(driver, (text) => text.endsWith('stays where it was.'), 'Escape');
  return places;
}

describe('Typed slots', () => {
  it('refuses text in create [int] where create [string] takes it, and swaps the start value with the type in one undo step', async (context) => {
    const flow = startFlow(context);
    const app = await flow.launch();
    const mainId = await newEmptyProject(app);
    await foldCodePanel(app);
    const ids = new NodeIds('typed');
    const n = declare(ids, { sym: 's_typed_n', name: 'n', type: 'int', value: num(0) });
    const s = declare(ids, { sym: 's_typed_s', name: 's', type: 'std::string', value: str('') });
    await app.hook.insertBlocks(mainId, 'BODY', [n, s]);
    // Blockly draws the inserted blocks in the next animation frame: measure them after it.
    await waitForDelivery(app.driver);

    // Text towards create [int]'s value: refused, so n keeps its 0 and the text lies loose.
    await dragValueFromToolbox(app, TEXT, n.id, 'VALUE');
    await waitFor(
      async () =>
        topBlocks(await currentDocument(app)).some((block) => block.type === 'text.literal'),
      { timeout: UI_TIMEOUT_MS, message: 'the refused text loose on the canvas' },
    );
    expect(await inputOf(app, n.id, 'VALUE')).toEqual(num(0));

    // Text towards create [string]'s value: it snaps.
    await dragValueFromToolbox(app, TEXT, s.id, 'VALUE');
    const taken = await waitFor(
      async () => {
        const value = await inputOf(app, s.id, 'VALUE');
        return typeof value === 'object' && value !== null && 'block' in value
          ? (value.block as BlockNode)
          : null;
      },
      { timeout: UI_TIMEOUT_MS, message: 'the text in create [string]' },
    );
    expect(taken.type).toBe('text.literal');

    // double chosen in n's type menu: the start value follows, and one Ctrl+Z restores both.
    await clickMiddle(app.driver, await fieldOf(app, n.id, 'TYPE'));
    await chooseMenuItem(app.driver, 'double');
    await waitForType(app, n.id, 'double');
    await waitForInput(app, n.id, 'VALUE', { expr: [{ num: '0.0' }] }, 'the start value 0.0');
    await waitForDelivery(app.driver);
    await pressCtrl(app.driver, 'z');
    await waitForType(app, n.id, 'int');
    await waitForInput(app, n.id, 'VALUE', num(0), 'Ctrl+Z to restore the start value 0');
    await waitForDelivery(app.driver);
    await pressCtrl(app.driver, 'y');
    await waitForType(app, n.id, 'double');
    await waitForInput(app, n.id, 'VALUE', { expr: [{ num: '0.0' }] }, 'Ctrl+Y to swap it again');
  }, 180_000);

  it('refuses text in a whole-number argument, by drag and by keyboard, where a number goes', async (context) => {
    const flow = startFlow(context);
    const example = copyExample('factorial', flow.folders.projects);
    const app = await flow.launch({ dialogs: { open: [example] } });
    await openFromStartPage(app);
    await foldCodePanel(app);
    const argument = await inputOf(app, FACTORIAL_CALL, 'ARG0');
    expect(argument).toEqual({ expr: [{ ref: 's_i' }] });

    // The keyboard mover offers the argument to a number block, not to a text block.
    const loose = new Set<string>();
    await dropOnCanvas(app, TEXT);
    const text = await newTopBlock(app, 'text.literal', loose);
    loose.add(text);
    await dropOnCanvas(app, NUMBER);
    const number = await newTopBlock(app, 'math.number', loose);
    // The call is named by its label (`factorial …`); the define block, named by its function alone,
    // takes a number too, in its parameter row's default part (W2's package C2).
    const intoCall = (place: string) => place.startsWith('into “factorial …”');
    const numberPlaces = await keyboardPlaces(app, number);
    expect(numberPlaces.filter(intoCall), numberPlaces.join(' | ')).toHaveLength(1);
    expect(numberPlaces, numberPlaces.join(' | ')).toContain('into “factorial”');
    const textPlaces = await keyboardPlaces(app, text);
    expect(textPlaces.filter(intoCall), textPlaces.join(' | ')).toEqual([]);
    // Neither move changed the call.
    expect(await inputOf(app, FACTORIAL_CALL, 'ARG0')).toEqual(argument);

    // A text block dragged towards the argument does not snap; a number does.
    await dragValueFromToolbox(app, TEXT, FACTORIAL_CALL, 'ARG0');
    expect(await inputOf(app, FACTORIAL_CALL, 'ARG0')).toEqual(argument);
    await dragValueFromToolbox(app, NUMBER, FACTORIAL_CALL, 'ARG0');
    const connected = await waitFor(
      async () => {
        const value = await inputOf(app, FACTORIAL_CALL, 'ARG0');
        return typeof value === 'object' && value !== null && 'block' in value
          ? (value.block as BlockNode)
          : null;
      },
      { timeout: UI_TIMEOUT_MS, message: 'the number in the argument of factorial' },
    );
    expect(connected.type).toBe('math.number');
  }, 180_000);
});
