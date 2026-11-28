/**
 * Ending a program's input and reading it from a file (docs/plans/m3-contracts/w3-export-run.md
 * §1, §3.2, §5), on a program that reads two words with a simple *ask*, prints each (`read: …`,
 * `read: nothing` when a read got nothing), waits a few seconds and prints `done`:
 *
 * - **End input.** While it runs, the console's header has *End input*. A word typed into the
 *   console reaches the program; *End input* then ends its input, so its next read gets nothing at
 *   once (end-of-file): the button says *Input ended* and stays there unusable, the state says
 *   *Running, input ended*, and the program goes on to `done` and finishes with exit code 0.
 * - **An input file.** A file chosen with *Choose an input file…* on the project settings page
 *   (the native dialog answered by the dialog script; the page shows only its name) is the
 *   program's input: it reads the file's one word, then the file's end, and finishes, with no
 *   *End input* in the header, since its input is closed from the start. With *Run programs in a
 *   separate terminal window* ticked too, the input file wins, as the page says: the program runs
 *   in the console again, reading the file. *Type input in the console* (with the window unticked)
 *   gives the console back its input: *End input* shows again.
 *
 * The program comes from the test hook; every click and key is real input. The console is read
 * from the hook's transcript, which drops the control sequences Windows' pseudoconsole repaints
 * lines with, so the lines are matched without their spaces.
 */
import { writeFileSync } from 'node:fs';
import path from 'node:path';

import { By, Key, type WebDriver } from 'selenium-webdriver';
import { describe, expect, it } from 'vitest';

import { type BlockNode, type ExprInput, NodeIds } from '../../support/bdm';
import {
  consoleState,
  showConsole,
  typeIntoConsole,
  waitForConsoleState,
} from '../../support/console';
import { clickTestId, pressKey, testId, testIdText, textContent } from '../../support/ui';
import { waitFor } from '../../support/wait';
import { waitForTranscript } from './lib/console';
import { type FlowApp, startFlow } from './lib/launch';
import { declare, newEmptyProject, repeat } from './lib/project';
import {
  BUILD_TIMEOUT_MS,
  chooseMenuItem,
  UI_TIMEOUT_MS,
  waitForTestId,
  waitForToolchain,
} from './lib/ui';

/** The symbol of the word the program reads. */
const WORD = 'sym_e2e_word';

/** What the word holds before each read: what the program prints when a read got nothing. */
const NOTHING = 'nothing';

/** How long the program waits after its reads, in seconds: time to see its input ended. */
const WAIT_SECONDS = 4;

/** The console's state while the program runs and its input is open (after the state's icon). */
const RUNNING = / Running$/;

/** The input file's name and its one word. */
const INPUT_FILE = 'answers.txt';
const FILE_WORD = 'beta';

/** *print* of `read: ` and the word, then a new line. */
function printWord(ids: NodeIds): BlockNode {
  return {
    id: ids.next(),
    type: 'io.print',
    v: 1,
    extra: { itemCount: 2 },
    fields: { NEWLINE: true, SEP: 'none', STREAM: 'out' },
    inputs: { ITEM0: { expr: [{ str: 'read: ' }] }, ITEM1: { expr: [{ ref: WORD }] } },
  };
}

/** *print* of a text, then a new line. */
function printText(ids: NodeIds, text: string): BlockNode {
  return {
    id: ids.next(),
    type: 'io.print',
    v: 1,
    extra: { itemCount: 1 },
    fields: { NEWLINE: true, SEP: 'none', STREAM: 'out' },
    inputs: { ITEM0: { expr: [{ str: text }] } },
  };
}

/**
 * `word = "nothing"; repeat 2 times { ask (simple) word; print "read: " word; set word to
 * "nothing" }; wait; print "done"`. A simple *ask* is `std::cin >> word;`, which at the end of
 * the input leaves the word as it was.
 */
function program(ids: NodeIds): BlockNode[] {
  const nothing: ExprInput = { expr: [{ str: NOTHING }] };
  return [
    declare(ids, { sym: WORD, name: 'word', type: 'std::string', value: nothing }),
    repeat(ids, { expr: [{ num: '2' }] }, [
      {
        id: ids.next(),
        type: 'io.ask',
        v: 1,
        fields: { MODE: 'simple', VAR: { ref: WORD } },
      },
      printWord(ids),
      {
        id: ids.next(),
        type: 'var.set',
        v: 1,
        fields: { VAR: { ref: WORD } },
        inputs: { VALUE: nothing },
      },
    ]),
    {
      id: ids.next(),
      type: 'control.wait',
      v: 1,
      inputs: { SECONDS: { expr: [{ num: String(WAIT_SECONDS) }] } },
    },
    printText(ids, 'done'),
  ];
}

/** Waits for the compiler, and makes a new project with the program. */
async function withProgram(app: FlowApp): Promise<void> {
  await waitForToolchain(app);
  const mainId = await newEmptyProject(app);
  await app.hook.insertBlocks(mainId, 'BODY', program(new NodeIds('input')));
}

/**
 * The words the program said it read (`read: …`), in order. The console echoes what is typed,
 * and on Windows *End input* too (`^Z`), so the program's lines are picked out of the transcript;
 * a space may go missing where Windows' pseudoconsole repaints a line.
 */
function wordsRead(transcript: string): string[] {
  return Array.from(transcript.matchAll(/read:\s*([A-Za-z]+)/g), (match) => match[1] ?? '');
}

/** The header's *End input* button, or `null` when the header has none. */
async function endInputButton(driver: WebDriver) {
  const found = await driver.findElements(By.css('[data-testid="console-end-input"]'));
  return found[0] ?? null;
}

/** Waits until *End input* says `label`, usable (`aria-disabled` false) or not. */
async function waitForEndInput(driver: WebDriver, label: string, usable: boolean): Promise<void> {
  await waitFor(
    async () => {
      const button = await endInputButton(driver);
      if (button === null) {
        return null;
      }
      const text = (await textContent(driver, button)).trim();
      const disabled = (await button.getAttribute('aria-disabled')) === 'true';
      return text === label && disabled !== usable ? true : null;
    },
    {
      timeout: UI_TIMEOUT_MS,
      message: `End input saying “${label}”, ${usable ? 'usable' : 'not usable'}`,
    },
  );
}

/** The header's notices (*In a separate window*, …). */
async function notices(driver: WebDriver): Promise<string[]> {
  const found = await driver.findElements(By.css('[data-testid="console-notice"] summary'));
  return Promise.all(found.map(async (notice) => (await textContent(driver, notice)).trim()));
}

/** Opens the project settings page (≡ → *Project settings…*). */
async function openProjectSettings(driver: WebDriver): Promise<void> {
  await chooseMenuItem(driver, 'Project settings…');
  await waitForTestId(driver, 'project-settings', true);
}

/** Leaves the project settings page with *Cancel* (the machine's options apply at once). */
async function closeProjectSettings(driver: WebDriver): Promise<void> {
  await clickTestId(driver, 'project-settings-cancel');
  await waitForTestId(driver, 'project-settings', false);
}

/** Waits until the page's input line says `text`. */
async function waitForInputLine(driver: WebDriver, text: string): Promise<void> {
  await waitFor(
    async () =>
      (await testIdText(driver, 'project-settings-input-file')).trim() === text ? true : null,
    {
      timeout: UI_TIMEOUT_MS,
      message: async () =>
        `the input line “${text}” (it says “${await testIdText(driver, 'project-settings-input-file')}”)`,
    },
  );
}

/** Ticks or unticks *Run programs in a separate terminal window* on the project settings page. */
async function setSeparateWindow(driver: WebDriver, on: boolean): Promise<void> {
  const box = await testId(driver, 'project-settings-external', UI_TIMEOUT_MS);
  if ((await box.isSelected()) !== on) {
    await clickTestId(driver, 'project-settings-external');
  }
  await waitFor(
    async () => (await (await testId(driver, 'project-settings-external')).isSelected()) === on,
    { timeout: UI_TIMEOUT_MS, message: `the separate window ${on ? 'ticked' : 'unticked'}` },
  );
}

/** Waits until the run started by F5 has read its input file and finished (run number `run`). */
async function waitForFileRun(app: FlowApp, run: number): Promise<void> {
  await waitForTranscript(
    app,
    (text) =>
      text.replace(/\s+/g, '').split(`read:${FILE_WORD}read:${NOTHING}done`).length - 1 >= run,
    BUILD_TIMEOUT_MS,
    `run ${String(run)}: the file's word, then nothing, then done`,
  );
  await waitForConsoleState(app.driver, /Finished \(exit code 0\)$/, UI_TIMEOUT_MS);
}

describe('Program input', () => {
  it('ends the program’s input with End input', async (context) => {
    const app = await startFlow(context).launch();
    await withProgram(app);
    const { driver } = app;

    // (1) Running, End input usable; a typed word reaches the program.
    await pressKey(driver, Key.F5);
    await waitForConsoleState(driver, RUNNING, BUILD_TIMEOUT_MS);
    await waitForEndInput(driver, 'End input', true);
    await showConsole(driver);
    await typeIntoConsole(driver, `alpha${Key.ENTER}`);
    await waitForTranscript(
      app,
      (text) => wordsRead(text).includes('alpha'),
      UI_TIMEOUT_MS,
      'read: alpha',
    );
    expect(await consoleState(driver)).toMatch(RUNNING);

    // (2) End input: the next read gets nothing at once, and the program goes on.
    await clickTestId(driver, 'console-end-input');
    await waitForEndInput(driver, 'Input ended', false);
    await waitForConsoleState(driver, / Running, input ended$/, UI_TIMEOUT_MS);
    await waitForTranscript(
      app,
      (text) => wordsRead(text).length === 2,
      UI_TIMEOUT_MS,
      `read: ${NOTHING} after End input`,
    );
    const ended = await app.hook.consoleText();
    expect(wordsRead(ended)).toEqual(['alpha', NOTHING]);
    expect(ended).not.toContain('done');

    // (3) It finishes by itself.
    await waitForConsoleState(
      driver,
      /Finished \(exit code 0\)$/,
      WAIT_SECONDS * 1000 + UI_TIMEOUT_MS,
    );
    const finished = await app.hook.consoleText();
    expect(wordsRead(finished)).toEqual(['alpha', NOTHING]);
    expect(finished.slice(finished.lastIndexOf('read:'))).toContain('done');
    expect(await endInputButton(driver)).toBeNull();
  }, 180_000);

  it('reads the input file chosen on the project settings page, also with the separate window ticked', async (context) => {
    const flow = startFlow(context);
    const file = path.join(flow.folders.projects, INPUT_FILE);
    writeFileSync(file, `${FILE_WORD}\n`);
    const app = await flow.launch({ dialogs: { inputFile: [file] } });
    await withProgram(app);
    const { driver } = app;

    // (1) The file chosen on the project settings page: only its name is shown.
    await openProjectSettings(driver);
    await waitForInputLine(driver, 'Input: typed in the console');
    await clickTestId(driver, 'project-settings-choose-input');
    await waitForInputLine(driver, `Input: read from the file “${INPUT_FILE}”`);
    await closeProjectSettings(driver);

    // (2) The program reads the file, then its end, and finishes; no End input while it runs.
    await pressKey(driver, Key.F5);
    await waitForConsoleState(driver, RUNNING, BUILD_TIMEOUT_MS);
    expect(await endInputButton(driver)).toBeNull();
    await waitForFileRun(app, 1);

    // (3) With the separate window ticked too, the input file wins, as the page says: the
    // program runs in the console again.
    await openProjectSettings(driver);
    const machine = await testId(driver, 'project-settings-machine', UI_TIMEOUT_MS);
    expect((await textContent(driver, machine)).replace(/\s+/g, ' ')).toContain(
      'A program that reads an input file still runs in the console.',
    );
    await setSeparateWindow(driver, true);
    await closeProjectSettings(driver);
    await pressKey(driver, Key.F5);
    await waitForFileRun(app, 2);
    expect(await notices(driver)).not.toContain('In a separate window');

    // (4) Back to typing in the console (the window unticked): End input shows again.
    await openProjectSettings(driver);
    await setSeparateWindow(driver, false);
    await clickTestId(driver, 'project-settings-clear-input');
    await waitForInputLine(driver, 'Input: typed in the console');
    await closeProjectSettings(driver);
    await pressKey(driver, Key.F5);
    await waitForConsoleState(driver, RUNNING, BUILD_TIMEOUT_MS);
    await waitForEndInput(driver, 'End input', true);
    await clickTestId(driver, 'console-stop');
    await waitForConsoleState(driver, /Stopped$/, UI_TIMEOUT_MS);
  }, 240_000);
});
