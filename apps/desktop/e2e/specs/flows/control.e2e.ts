/**
 * *switch on* in the real webview (docs/plans/m3-contracts/w2.md §1.6, §2.9, §5.2, package D):
 *
 * - **The C++ of a switch.** A switch on a whole number with two cases and a default part shows in
 *   the C++ panel with each case in its own `{ }` and a `break;` at its end.
 * - **⊕ adds a case.** A click on the switch's ⊕ adds a third case, with an empty value: Problems
 *   then lists one error, the value the new case needs (`B2C-E0609` from the loader, which finds the
 *   input empty before the analyser does). One Ctrl+Z takes the case away again and the error goes
 *   with it.
 * - **A switch on an enum.** In the fixture `enum-switch.b2c`, a switch on a variable of the enum
 *   `Action` (`Deposit`, `Withdraw`, `Quit`) has cases for `Deposit` and `Withdraw` and no default
 *   part: Problems lists one warning (`B2C-W0508`, `Quit` is missed) and the C++ panel shows the
 *   enum's own labels and `default: { break; }`. ⊕ adds a third case, an enum value repeating
 *   `Withdraw` is one error (`B2C-E0425`), and `Quit` chosen from that value's menu leaves no
 *   problem and no default part.
 *
 * Every click is made with real pointer input, every key pressed as a person presses it; what the
 * canvas holds is read from the test hook's document.
 */
import { By, type WebElement } from 'selenium-webdriver';
import { describe, expect, it } from 'vitest';

import { type BlockNode, moduleNodes, NodeIds } from '../../support/bdm';
import { bringIntoView } from '../../support/canvas';
import {
  centerOn,
  currentDocument,
  fieldOf,
  foldCodePanel,
  problemsSummary,
  showProblems,
  waitForDelivery,
  waitForErrors,
} from '../../support/editor';
import { byTestId, chooseOption, clickMiddle, middleOf, textContent } from '../../support/ui';
import { waitFor } from '../../support/wait';
import { type FlowApp, startFlow } from './lib/launch';
import {
  copyFixture,
  declare,
  newEmptyProject,
  openFromStartPage,
  printValue,
  ref,
} from './lib/project';
import { pressCtrl, UI_TIMEOUT_MS } from './lib/ui';

/** The variable switched on. */
const N = 'sym_e2e_n';

/** Waits until the switch `id` has `count` cases on the canvas, and returns it. */
function waitForCases(app: FlowApp, id: string, count: number): Promise<BlockNode> {
  return waitFor(
    async () => {
      const node = moduleNodes(await currentDocument(app)).find((block) => block.id === id);
      return node?.extra?.['caseCount'] === count ? node : null;
    },
    { timeout: UI_TIMEOUT_MS, message: `the switch with ${String(count)} cases` },
  );
}

/** Waits for the ⊕ of block `id` that adds a case, wherever it is on the canvas. */
function findAddCase(app: FlowApp, id: string): Promise<WebElement> {
  return waitFor(
    async () => {
      const group = await app.hook.blockElement(id);
      if (group === null) {
        return null;
      }
      for (const button of await group.findElements(By.css('[role="button"]'))) {
        const label = (await button.getAttribute('aria-label')) ?? '';
        // The switch's own ⊕ for a case ("Add ‘case’"), not one of the blocks in its parts.
        if (label.startsWith('Add') && label.includes('case') && (await button.isDisplayed())) {
          return button;
        }
      }
      return null;
    },
    { timeout: UI_TIMEOUT_MS, message: `the ⊕ button that adds a case to block ${id}` },
  );
}

/**
 * The ⊕ of switch `id`, with the block centred and the canvas panned until the button is well
 * inside the visible canvas: the toolbox's flyout covers the canvas's left part in a small window
 * (1024 × 768 on the Windows runner, 800 × 560), and a click there would land on the flyout.
 */
async function addCaseButton(app: FlowApp, id: string): Promise<WebElement> {
  await centerOn(app, id);
  await bringIntoView(app.driver, async () => middleOf(app.driver, await findAddCase(app, id)));
  return findAddCase(app, id);
}

/** Waits until the C++ panel's code holds every one of `parts`, and returns it. */
function waitForCode(app: FlowApp, parts: readonly string[], what: string): Promise<string> {
  return waitFor(
    async () => {
      const code = await app.hook.code();
      return parts.every((part) => code.includes(part)) ? code : null;
    },
    {
      timeout: UI_TIMEOUT_MS,
      message: async () => `${what} in the C++ panel (it has:\n${await app.hook.code()})`,
    },
  );
}

/**
 * Shows Problems and waits until its summary is `summary` (`1 warning`, `No problems`, …) and its
 * panel's text holds each of `parts`.
 */
async function waitForSummary(
  app: FlowApp,
  summary: string,
  parts: readonly string[] = [],
): Promise<void> {
  await showProblems(app.driver);
  const panelText = async (): Promise<string> =>
    textContent(app.driver, await app.driver.findElement(byTestId('problems-panel')));
  await waitFor(
    async () => {
      if ((await problemsSummary(app.driver)) !== summary) {
        return null;
      }
      const text = await panelText();
      return parts.every((part) => text.includes(part)) ? true : null;
    },
    {
      timeout: UI_TIMEOUT_MS,
      message: async () =>
        `"${summary}" and ${JSON.stringify(parts)} in Problems (it says "${await panelText()}")`,
    },
  );
}

describe('Control', () => {
  it('shows a switch in C++, and adds a case with ⊕ that Ctrl+Z takes away', async (context) => {
    const flow = startFlow(context);
    const app = await flow.launch();
    const mainId = await newEmptyProject(app);
    await foldCodePanel(app);
    const ids = new NodeIds('control');
    const switchBlock: BlockNode = {
      id: ids.next(),
      type: 'control.switch',
      v: 1,
      extra: { caseCount: 2, hasDefault: true },
      inputs: { VALUE: ref(N), CASE0: { expr: [{ num: '1' }] }, CASE1: { expr: [{ num: '2' }] } },
      statements: {
        DO0: [printValue(ids, { expr: [{ str: 'one' }] })],
        DEFAULT: [printValue(ids, { expr: [{ str: 'other' }] })],
      },
    };
    await app.hook.insertBlocks(mainId, 'BODY', [
      declare(ids, { sym: N, name: 'n', type: 'int', value: { expr: [{ num: '2' }] } }),
      switchBlock,
    ]);
    await waitForCases(app, switchBlock.id, 2);

    // (1) Each case is a scope of its own that ends with a break; the default part needs none.
    await waitForErrors(app, 0);
    const code = await waitForCode(
      app,
      ['switch (n) {', 'case 1: {', 'case 2: {', 'default: {', 'break;'],
      'the switch',
    );
    expect(code).toMatch(/case 2: \{\s+break;\s+\}/);

    // (2) ⊕ adds a third case, whose value is empty until one is put there.
    await clickMiddle(app.driver, await addCaseButton(app, switchBlock.id));
    const grown = await waitForCases(app, switchBlock.id, 3);
    expect(grown.extra?.['hasDefault']).toBe(true);
    expect(grown.inputs?.['CASE2']).toBeUndefined();
    const problems = await waitForErrors(app, 1);
    expect(problems).toMatch(/1 error/);

    // (3) One Ctrl+Z takes the case away again, and its error with it.
    await waitForDelivery(app.driver);
    await pressCtrl(app.driver, 'z');
    const undone = await waitForCases(app, switchBlock.id, 2);
    expect(undone.inputs?.['CASE1']).toEqual({ expr: [{ num: '2' }] });
    await waitForErrors(app, 0);
  }, 180_000);

  it('switches on an enum with its own values, and warns of a missed one', async (context) => {
    const flow = startFlow(context);
    const fixture = copyFixture('enum-switch.b2c', flow.folders.projects);
    const app = await flow.launch({ dialogs: { open: [fixture] } });
    await openFromStartPage(app);
    await foldCodePanel(app);
    const switchId = 'b_switch';

    // (1) Quit has no case and there is no default part: a warning, and the generator's
    // `default: { break; }`, which keeps g++'s -Wswitch quiet.
    await waitForSummary(app, '1 warning', ['no case for `Quit`']);
    const missed = await waitForCode(
      app,
      ['switch (act) {', 'case Action::Deposit: {', 'case Action::Withdraw: {', 'default: {'],
      'the switch on an enum that misses Quit',
    );
    expect(missed).toMatch(/default: \{\s+break;\s+\}/);
    expect(missed).not.toContain('case Action::Quit');

    // (2) ⊕ adds a third case, whose empty value is an error.
    await clickMiddle(app.driver, await addCaseButton(app, switchId));
    await waitForCases(app, switchId, 3);
    await waitForSummary(app, '1 error, 1 warning');

    // (3) A second Withdraw is one case too many.
    const third: BlockNode = {
      id: 'b_third',
      type: 'type.enum_value',
      v: 1,
      fields: { ENUM: { ref: 's_Action' }, VALUE: { ref: 's_Action_Withdraw' } },
    };
    await app.hook.insertBlocks(switchId, 'CASE2', [third]);
    await waitForDelivery(app.driver);
    await waitForSummary(app, '1 error, 1 warning', [
      'the same value (Withdraw)',
      'no case for `Quit`',
    ]);

    // (4) Quit, chosen from the value's menu, covers every value: no problem, no default part.
    await chooseOption(app.driver, await fieldOf(app, third.id, 'VALUE'), 'Quit');
    await waitForSummary(app, 'No problems');
    const covered = await waitForCode(app, ['case Action::Quit: {'], 'the case for Quit');
    expect(covered).not.toContain('default:');
    const node = moduleNodes(await currentDocument(app)).find((block) => block.id === third.id);
    expect(node?.fields?.['VALUE']).toEqual({ ref: 's_Action_Quit' });
  }, 180_000);
});
