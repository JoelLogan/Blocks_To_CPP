/**
 * The block editor in the real webview (docs/spec/04-user-interface.md §4.2–4.4, 03 §3.6):
 *
 * - a variable menu lists the variables in scope at the block: the *ask* menu only those it may
 *   change (no `const`), a getter's also the `const` ones, never those declared later or in
 *   another function;
 * - the connection checker refuses a text block in *repeat (10) times*, where a number fits;
 * - clicking a line of the C++ panel selects the block that made it;
 * - a block can be dragged from every toolbox category.
 */
import { By } from 'selenium-webdriver';
import { describe, expect, it } from 'vitest';

import {
  type BlockNode,
  NodeIds,
  onlyTopBlock,
  type ProjectDocument,
  statementsOf,
} from '../../support/bdm';
import { currentDocument, dragFromToolbox, fieldOf, foldCodePanel } from '../../support/editor';
import { clickMiddle } from '../../support/ui';
import { waitFor } from '../../support/wait';
import {
  closeMenu,
  dragValueFromToolbox,
  dropOnCanvas,
  isSelected,
  menuLabels,
  waitForSelected,
} from './lib/editor';
import { startFlow } from './lib/launch';
import {
  copyExample,
  copyFixture,
  mainBody,
  newEmptyProject,
  openFromStartPage,
  repeat,
} from './lib/project';
import { UI_TIMEOUT_MS } from './lib/ui';

/** The block of the guessing game (examples/guessing_game.b2c) that asks for the guess. */
const ASK_BLOCK = 'b005';
/** The line of the generated C++ that block becomes (tests/golden/guessing_game/main.cpp). */
const ASK_LINE = 'guess = b2c::ask<int>("Your guess: ");';

/** The block nested in value input `input` of `node`, or `null` when the input is an expression. */
function nested(node: BlockNode | undefined, input: string): BlockNode | null {
  const value = node?.inputs?.[input];
  return value !== undefined && 'block' in value ? value.block : null;
}

/** The top-level blocks of the first module. */
function topBlocks(doc: ProjectDocument): readonly BlockNode[] {
  return doc.modules[0]?.workspace.blocks ?? [];
}

describe('Block editor', () => {
  it('lists the variables in scope in the ask and getter menus', async (context) => {
    const flow = startFlow(context);
    const fixture = copyFixture('scopes.b2c', flow.folders.projects);
    const app = await flow.launch({ dialogs: { open: [fixture] } });
    await openFromStartPage(app);
    await foldCodePanel(app);

    // `ask … and save answer in score`, inside `for i from 1 to 3`, after `score`, `const limit`
    // and `name`; `later` comes after the loop and `inner` is in another function.
    await clickMiddle(app.driver, await fieldOf(app, 'b_ask', 'VAR'));
    expect((await menuLabels(app.driver)).sort()).toEqual(['i', 'name', 'score']);
    await closeMenu(app.driver);

    // A getter may read a constant too.
    await clickMiddle(app.driver, await fieldOf(app, 'b_get', 'VAR'));
    expect((await menuLabels(app.driver)).sort()).toEqual(['i', 'limit', 'name', 'score']);
    await closeMenu(app.driver);
  }, 180_000);

  it('refuses a text block in repeat (10) times, where a number connects', async (context) => {
    const flow = startFlow(context);
    const app = await flow.launch();
    const mainId = await newEmptyProject(app);
    await foldCodePanel(app);
    const loop = repeat(new NodeIds('check'), { expr: [{ num: '10' }] }, []);
    await app.hook.insertBlocks(mainId, 'BODY', [loop]);

    // A text value dropped on TIMES does not connect: the loop keeps its 10.
    await dragValueFromToolbox(app, { category: 'Text', type: 'text.literal' }, loop.id, 'TIMES');
    const refused = await currentDocument(app);
    const kept = statementsOf(onlyTopBlock(refused, 'program.main'), 'BODY')[0];
    expect(kept?.inputs?.['TIMES']).toEqual({ expr: [{ num: '10' }] });
    expect(topBlocks(refused).some((block) => block.type === 'text.literal')).toBe(true);

    // The same drag with a number connects.
    await dragValueFromToolbox(app, { category: 'Math', type: 'math.number' }, loop.id, 'TIMES');
    const connected = await waitFor(async () => nested((await mainBody(app))[0], 'TIMES'), {
      timeout: UI_TIMEOUT_MS,
      message: 'the number in repeat … times',
    });
    expect(connected.type).toBe('math.number');
  }, 180_000);

  it('selects the block of the C++ line that is clicked', async (context) => {
    const flow = startFlow(context);
    const example = copyExample('guessing_game', flow.folders.projects);
    const app = await flow.launch({ dialogs: { open: [example] } });
    await openFromStartPage(app);
    expect(await isSelected(app, ASK_BLOCK)).toBe(false);

    // CodeMirror draws only the lines near its viewport, so the panel is scrolled on (and back to
    // the top at the end) until the line is drawn; whitespace is compared loosely.
    const normalise = (text: string): string => text.replace(/\s+/g, ' ').trim();
    const lines = () => app.driver.findElements(By.css('[data-testid="code-panel"] .cm-line'));
    const line = await waitFor(
      async () => {
        for (const candidate of await lines()) {
          if (normalise(await candidate.getText()) === ASK_LINE) {
            return candidate;
          }
        }
        await app.driver.executeScript(
          `const scroller = document.querySelector('[data-testid="code-panel"] .cm-scroller');
           if (scroller !== null) {
             const end = scroller.scrollTop + scroller.clientHeight >= scroller.scrollHeight - 1;
             scroller.scrollTop = end ? 0 : scroller.scrollTop + scroller.clientHeight / 2;
           }`,
        );
        return null;
      },
      {
        timeout: UI_TIMEOUT_MS,
        interval: 250,
        message: async () => {
          const shown = await Promise.all((await lines()).map((found) => found.getText()));
          return `the line ${ASK_LINE} in the C++ panel; it shows ${String(shown.length)} lines: ${JSON.stringify(shown.slice(0, 40))}`;
        },
      },
    );
    await app.driver.executeScript('arguments[0].scrollIntoView({ block: "center" });', line);
    await line.click();
    await waitForSelected(app, ASK_BLOCK);
  }, 180_000);

  it('drags a block from every toolbox category', async (context) => {
    const flow = startFlow(context);
    const app = await flow.launch();
    const mainId = await newEmptyProject(app);
    await foldCodePanel(app);
    const body = () => mainBody(app);
    const last = async () => (await body()).at(-1)?.id ?? mainId;

    // Statements, each below the one before (Variables, Input / Output, Control, Loops, Program).
    const statements = [
      { category: 'Variables', type: 'var.declare', fields: { TYPE: 'int' } },
      { category: 'Input / Output', type: 'io.print' },
      { category: 'Control', type: 'control.if' },
      { category: 'Loops', type: 'control.repeat' },
      { category: 'Program', type: 'program.exit' },
    ];
    for (const [index, entry] of statements.entries()) {
      const target = index === 0 ? mainId : await last();
      await dragFromToolbox(app, entry, target, index === 0 ? 'BODY' : 'next');
      await waitFor(async () => (await body()).length === index + 1, {
        timeout: UI_TIMEOUT_MS,
        message: `${entry.type} from ${entry.category} in main`,
      });
    }
    const [, printBlock, ifBlock, repeatBlock] = await body();
    expect((await body()).map((node) => node.type)).toEqual(statements.map((entry) => entry.type));

    // Values into their inputs (Text, Logic, Math).
    const values = [
      { entry: { category: 'Text', type: 'text.literal' }, target: printBlock, input: 'ITEM0' },
      { entry: { category: 'Logic', type: 'logic.boolean' }, target: ifBlock, input: 'COND0' },
      { entry: { category: 'Math', type: 'math.number' }, target: repeatBlock, input: 'TIMES' },
    ];
    for (const { entry, target, input } of values) {
      const targetId = target?.id ?? '';
      await dragValueFromToolbox(app, entry, targetId, input);
      const found = await waitFor(
        async () =>
          nested(
            (await body()).find((node) => node.id === targetId),
            input,
          ),
        { timeout: UI_TIMEOUT_MS, message: `${entry.type} from ${entry.category} in ${input}` },
      );
      expect(found.type).toBe(entry.type);
    }

    // A definition onto the canvas (Functions).
    await dropOnCanvas(app, { category: 'Functions', type: 'func.define' });
    await waitFor(
      async () =>
        topBlocks(await currentDocument(app)).some((block) => block.type === 'func.define'),
      { timeout: UI_TIMEOUT_MS, message: 'func.define from Functions on the canvas' },
    );
  }, 180_000);
});
