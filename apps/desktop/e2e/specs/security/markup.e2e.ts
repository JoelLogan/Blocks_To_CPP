/**
 * Nightly E2E security test 5 (docs/spec/08-security.md §8.8 and §8.13; threat T7 of §8.12, XSS
 * in the webview): HTML and script in a project's text are shown literally and run nothing.
 *
 * The fixtures (e2e/fixtures/security/markup*.b2c) put markup that would run script, or at least
 * make an element carrying `data-b2c-xss`, wherever a project holds text: the project's name and
 * description, string literals, a variable's value, block comments (one pinned open), a note, and
 * in a file the loader rejects, the name and an unknown key that its problems quote. The project
 * is opened through the start page, shown, trusted through the scripted native dialog and run;
 * then a variable whose name is markup is added, so that Problems quotes it. At every step the
 * canary global stays unset, no element carries the marker, and the text appears as text: in the
 * load failure, the top bar, the block fields, the comment bubble, the C++, the console and
 * Problems.
 */
import { describe, expect, it } from 'vitest';

import { type App, launchApp } from '../../support/app';
import { declareInt, NodeIds } from '../../support/bdm';
import { showConsole, visibleConsoleText, waitForConsoleState } from '../../support/console';
import { waitForErrors } from '../../support/editor';
import { clickTestId, testId, testIdText, textContent } from '../../support/ui';
import { waitFor } from '../../support/wait';
import { IpcProbe, waitForToolchains } from './lib/ipc';
import { CANARY, injectionProblems, MARKER_ATTRIBUTE, pageState, shownText } from './lib/page';
import { copyInto, fixture, scratchFolder } from './lib/projects';

/** The markup in e2e/fixtures/security/markup.b2c. */
const MARKUP = {
  name: `<img src=x data-b2c-xss onerror="__b2cXss='name'">`,
  pinnedComment: `<svg data-b2c-xss onload="__b2cXss='comment'"></svg>`,
  script: `<script data-b2c-xss>__b2cXss='print'</script>`,
  link: `<a data-b2c-xss href="javascript:__b2cXss='link'">link</a>`,
  variable: `<b data-b2c-xss onmouseover="__b2cXss='variable'">bold</b>`,
} as const;

/**
 * The markup in e2e/fixtures/security/markup-rejected.b2c: an unknown key, which the problems
 * quote. (Its name is markup too; the start page does not show the name of a rejected file.)
 */
const REJECTED = { key: `<svg data-b2c-xss onload="__b2cXss='key'">` } as const;

/** A variable name that is markup (the analyser refuses it; Problems quotes it). */
const MARKUP_NAME = `<i data-b2c-xss onclick="__b2cXss=1">x</i>`;

/** How long a build may take (the compile timeout). */
const BUILD_TIMEOUT_MS = 120_000;

/**
 * Console text in the form the checks compare: as it is on Linux; without white space on
 * Windows, where the pseudoconsole may repaint a line with cursor moves instead of spaces and line
 * breaks (as the exit test's checks allow, e2e/specs/exit).
 */
function consoleForm(text: string): string {
  return process.platform === 'win32' ? text.replace(/\s+/g, '') : text;
}

/** Fails with every injection the page shows (the canary or a marked element), naming `when`. */
async function expectClean(app: App, when: string): Promise<void> {
  expect(injectionProblems(await pageState(app.driver)), when).toEqual([]);
}

/** The value of every `textarea` on the page (Blockly's comment bubbles are textareas). */
async function textareaValues(app: App): Promise<string[]> {
  const values: unknown = await app.driver.executeScript(
    "return Array.from(document.querySelectorAll('textarea'), (area) => area.value);",
  );
  return Array.isArray(values) ? values.filter((value) => typeof value === 'string') : [];
}

/** The shown text of field `field` of the block in `input` of block `blockId`. */
async function fieldText(app: App, blockId: string, field: string, input: string): Promise<string> {
  const element = await waitFor(() => app.hook.fieldElement(blockId, field, input), {
    timeout: 10_000,
    message: `field ${field} in ${input} of block ${blockId}`,
  });
  return shownText(await textContent(app.driver, element));
}

describe('E2E security 5: markup in a project is shown as text', () => {
  it('runs no script and shows the text literally in every view', async (context) => {
    const folder = scratchFolder(context);
    const rejected = copyInto(folder, fixture('markup-rejected.b2c'));
    const accepted = copyInto(folder, fixture('markup.b2c'));
    const app = await launchApp(context, {
      dialogs: { open: [rejected, accepted], trust: ['trustProject'] },
    });
    const { driver, hook } = app;
    await waitForToolchains(new IpcProbe(driver));

    // The check sees what it looks for: an element the test itself marks, and the canary set.
    await driver.executeScript(
      "const mark = document.createElement('b'); mark.setAttribute(arguments[0], ''); mark.id = 'b2c-e2e-control'; document.body.appendChild(mark); window[arguments[1]] = 'control';",
      MARKER_ATTRIBUTE,
      CANARY,
    );
    expect(injectionProblems(await pageState(driver))).toHaveLength(2);
    await driver.executeScript(
      "document.getElementById('b2c-e2e-control').remove(); delete window[arguments[0]];",
      CANARY,
    );
    await expectClean(app, 'the start page');

    // A rejected file: the start page quotes its problems (the unknown key), as text.
    await clickTestId(driver, 'start-open');
    await testId(driver, 'project-load-failure');
    const failure = shownText(await testIdText(driver, 'project-load-failure'));
    expect(failure).toContain('B2C-E0110');
    expect(failure).toContain(REJECTED.key.slice(0, 24));
    await expectClean(app, 'the load failure');

    // The accepted file, in Restricted Mode: the name, the fields, the pinned comment, the C++.
    await clickTestId(driver, 'start-open');
    await testId(driver, 'restricted-banner', 20_000);
    await waitFor(() => hook.blockElement('b_show'), { timeout: 15_000, message: 'the blocks' });
    expect(shownText(await testIdText(driver, 'project-name'))).toContain(MARKUP.name);
    expect(await fieldText(app, 'b_script', 'VALUE', 'ITEM0')).toContain('<script data-b2c-xss>');
    expect(await fieldText(app, 'b_link', 'VALUE', 'ITEM0')).toContain('<a data-b2c-xss href=');
    expect(await textareaValues(app)).toContain(MARKUP.pinnedComment);
    const code = await waitFor(
      async () => {
        const text = await hook.code();
        return text.includes('data-b2c-xss') ? text : null;
      },
      { timeout: 15_000, message: 'the C++ of the project' },
    );
    expect(code).toContain(`"${MARKUP.script}"`);
    expect(code).toContain(`"${MARKUP.variable.replace(/"/g, '\\"')}"`);
    expect(code).toContain(MARKUP.pinnedComment);
    await waitForErrors(app, 0);
    await expectClean(app, 'the project in Restricted Mode');

    // Trusted (scripted: Trust this project) and run: the output is text in the terminal.
    await clickTestId(driver, 'restricted-banner-trust');
    await waitFor(
      async () =>
        (await driver.findElements({ css: '[data-testid="restricted-banner"]' })).length === 0,
      { timeout: 15_000, message: 'the project to be trusted' },
    );
    await clickTestId(driver, 'toolbar-run');
    await waitForConsoleState(driver, /Finished \(exit code 0\)$/, BUILD_TIMEOUT_MS);
    const transcript = await waitFor(
      async () => {
        const text = consoleForm(await hook.consoleText());
        return text.includes(consoleForm(MARKUP.variable)) ? text : null;
      },
      { timeout: 10_000, message: 'the program output in the console' },
    );
    for (const line of [MARKUP.script, MARKUP.link, MARKUP.variable]) {
      expect(transcript).toContain(consoleForm(line));
    }
    await showConsole(driver);
    expect(consoleForm(await visibleConsoleText(driver))).toContain(consoleForm(MARKUP.script));
    await expectClean(app, 'the program output');

    // A variable named with markup: the analyser refuses the name, and Problems quotes it.
    const main = 'b_main';
    await hook.insertBlocks(main, 'BODY', [
      declareInt(new NodeIds('xss'), 'sym_e2e_xss', MARKUP_NAME, 0),
    ]);
    await waitForErrors(app, 1, { atLeast: true });
    const problems = await waitFor(
      async () => {
        const rows = await driver.findElements({ css: '[data-testid="problem-row"]' });
        const texts = await Promise.all(rows.map((row) => textContent(driver, row)));
        return texts.find((text) => text.includes('data-b2c-xss')) ?? null;
      },
      { timeout: 15_000, message: 'a problem quoting the markup name' },
    );
    expect(shownText(problems)).toContain('<i data-b2c-xss');
    await expectClean(app, 'Problems');
  });
});
