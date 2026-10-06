/**
 * The M2 exit criterion (docs/spec/10-roadmap.md §10.1, 09 §9.2): a first-time user builds and runs
 * the guessing game of 03 §3.13.1 from the Empty template. The test starts the app with a fresh
 * profile, assembles the game (real drags for the program, a variable, a print and the loop; the
 * test hook for the rest), checks the live analysis and the C++, presses F5, plays the game by
 * binary search until *Correct!*, then runs it again and stops it.
 *
 * A second test checks that a program printing more than 8 KiB at once reaches the console: such
 * batches travel through Tauri's channel fetch, which the isolation hook must let through.
 */
import { readFileSync } from 'node:fs';
import path from 'node:path';

import { Key } from 'selenium-webdriver';
import { describe, expect, it } from 'vitest';

import { type App, launchApp } from '../../support/app';
import {
  ask,
  compare,
  declaredSymbol,
  declareInt,
  ifElseIfElse,
  moduleNodes,
  NodeIds,
  onlyTopBlock,
  print,
  randomInt,
  statementsOf,
  varGet,
} from '../../support/bdm';
import {
  consoleState,
  showConsole,
  typeIntoConsole,
  visibleConsoleText,
  waitForConsoleState,
} from '../../support/console';
import {
  currentDocument,
  dragBy,
  dragFromToolbox,
  fieldOf,
  foldCodePanel,
  mainFunction,
  runHeldBack,
  waitForErrors,
} from '../../support/editor';
import { REPOSITORY_ROOT } from '../../support/env';
import {
  answersIn,
  BinarySearch,
  gameTokens,
  goldenShape,
  INTRO,
  MAX_GUESSES,
  PROMPT,
  promptCount,
  withoutEcho,
} from '../../support/guessing';
import { chooseOption, clickTestId, pressKey, testIdText, typeIntoField } from '../../support/ui';
import { waitFor } from '../../support/wait';

/** The suggested waits of the exit flow: start-up, a build (the compile timeout), an answer. */
const TOOLCHAIN_TIMEOUT_MS = 30_000;
const BUILD_TIMEOUT_MS = 120_000;
const ANSWER_TIMEOUT_MS = 10_000;

/** The symbol the inserted `guess` declaration declares (a valid project ID, 05 §5.4). */
const GUESS_SYM = 'sym_e2e_guess';

const GOLDEN = path.join(REPOSITORY_ROOT, 'tests', 'golden', 'guessing_game');

/** Waits until the status bar names a g++ (background discovery found the toolchain). */
async function waitForToolchain(app: App): Promise<string> {
  return waitFor(
    async () => {
      const text = await testIdText(app.driver, 'status-toolchain');
      return /\bg\+\+/.test(text) && !/No g\+\+|Looking for/.test(text) ? text : null;
    },
    {
      timeout: TOOLCHAIN_TIMEOUT_MS,
      message: async () =>
        `the status bar to show g++ (it shows "${await testIdText(app.driver, 'status-toolchain')}")`,
    },
  );
}

/** Creates a project from the Empty template and returns its `program.main` block's ID. */
async function newEmptyProject(app: App): Promise<string> {
  await clickTestId(app.driver, 'template-empty');
  const doc = await waitFor(
    async () => {
      const found = await currentDocument(app);
      return (found.modules[0]?.workspace.blocks.length ?? 0) > 0 ? found : null;
    },
    { timeout: 15_000, message: 'the new project in the editor' },
  );
  return onlyTopBlock(doc, 'program.main').id;
}

/** The blocks of `main`'s body, from the canvas. */
async function mainBody(app: App) {
  return statementsOf(onlyTopBlock(await currentDocument(app), 'program.main'), 'BODY');
}

/** `value`, or a failure naming `what` when it is missing. */
function required<T>(value: T | null | undefined, what: string): T {
  if (value === null || value === undefined) {
    throw new Error(`Expected ${what}, found none`);
  }
  return value;
}

/** The console transcript, waiting until `ready` accepts it. */
function waitForTranscript(
  app: App,
  ready: (text: string) => boolean,
  timeout: number,
  what: string,
): Promise<string> {
  return waitFor(
    async () => {
      const text = await app.hook.consoleText();
      return ready(text) ? text : null;
    },
    {
      timeout,
      interval: 100,
      message: async () =>
        `${what}; the console has:\n${(await app.hook.consoleText()).slice(-2000)}`,
    },
  );
}

describe('M2 exit criterion: the guessing game', () => {
  it('is assembled from the Empty template, built with F5, played to "Correct!" and stopped', async (context) => {
    const app = await launchApp(context);
    const { driver, hook } = app;
    const ids = new NodeIds('game');

    // (1) A fresh profile: the toolchain is found in the background, no setup is needed.
    await waitForToolchain(app);

    // (2) New project from the Empty template: one `when program starts` block. The C++ panel is
    // folded away to give the canvas room next to the toolbox's flyout.
    const mainId = await newEmptyProject(app);
    await foldCodePanel(app);

    // (3) Assemble the game. Real drags first: move `main` on the canvas…
    const before = onlyTopBlock(await currentDocument(app), 'program.main');
    await dragBy(app, mainId, 96, 48);
    const moved = onlyTopBlock(await currentDocument(app), 'program.main');
    expect([moved.x, moved.y]).not.toEqual([before.x, before.y]);

    // …then `create int variable` from the toolbox into main's body, renamed to `secret`.
    const variable = { category: 'Variables', type: 'var.declare', fields: { TYPE: 'int' } };
    await dragFromToolbox(app, variable, mainId, 'BODY');
    const secretDecl = required((await mainBody(app))[0], 'the dropped variable');
    expect(secretDecl.type).toBe('var.declare');
    await typeIntoField(driver, await fieldOf(app, secretDecl.id, 'NAME'), 'secret');
    const secret = required(
      declaredSymbol(required((await mainBody(app))[0], 'the variable'), 'NAME'),
      'the declared symbol',
    );
    expect(secret.name).toBe('secret');
    await hook.insertBlocks(secretDecl.id, 'VALUE', [randomInt(ids, 1, 100)]);

    // `int guess = 0;` through the hook.
    const guessDecl = declareInt(ids, GUESS_SYM, 'guess', 0);
    await hook.insertBlocks(mainId, 'BODY', [guessDecl]);

    // A print from the toolbox below it, its text typed into the slot.
    await dragFromToolbox(
      app,
      { category: 'Input / Output', type: 'io.print' },
      guessDecl.id,
      'next',
    );
    const intro = required((await mainBody(app))[2], 'the dropped print');
    expect(intro.type).toBe('io.print');
    await typeIntoField(driver, await fieldOf(app, intro.id, 'VALUE', 'ITEM0'), INTRO);

    // The `repeat until` loop from the toolbox below the print.
    const repeatUntil = { category: 'Loops', type: 'control.while', fields: { MODE: 'until' } };
    await dragFromToolbox(app, repeatUntil, intro.id, 'next');
    const loop = required((await mainBody(app))[3], 'the dropped loop');
    expect(loop.type).toBe('control.while');
    expect(loop.fields?.['MODE']).toBe('until');

    // The rest through the hook: until guess == secret; ask (no variable yet); if / else if / else.
    await hook.insertBlocks(loop.id, 'COND', [
      compare(ids, 'eq', varGet(ids, GUESS_SYM), varGet(ids, secret.sym)),
    ]);
    const askBlock = ask(ids, PROMPT, null);
    await hook.insertBlocks(loop.id, 'BODY', [
      askBlock,
      ifElseIfElse(ids, {
        cond0: compare(ids, 'lt', varGet(ids, GUESS_SYM), varGet(ids, secret.sym)),
        do0: [print(ids, 'Too low!')],
        cond1: compare(ids, 'gt', varGet(ids, GUESS_SYM), varGet(ids, secret.sym)),
        do1: [print(ids, 'Too high!')],
        otherwise: [print(ids, 'Correct!')],
      }),
    ]);

    // (4) An ask without a variable is an error: Problems shows it and Run is held back…
    await waitForErrors(app, 1, { atLeast: true });
    expect(await runHeldBack(driver)).toBe(true);
    // …until `guess` is picked in the ask's variable menu by a real click.
    await chooseOption(driver, await fieldOf(app, askBlock.id, 'VAR'), 'guess');
    await waitForErrors(app, 0);
    expect(await runHeldBack(driver)).toBe(false);

    // The program is complete: no error placeholders, and main() is the golden one.
    const code = await waitFor(
      async () => {
        const text = await hook.code();
        return text.includes('b2c::ask<int>') ? text : null;
      },
      { timeout: 10_000, message: 'the C++ of the complete game' },
    );
    expect(code).not.toContain('/* error */');
    expect(await driver.findElements({ css: '[data-testid="code-not-buildable"]' })).toHaveLength(
      0,
    );
    const golden = readFileSync(path.join(GOLDEN, 'main.cpp'), 'utf8');
    expect(mainFunction(code)).toBe(mainFunction(golden));
    expect(
      moduleNodes(await currentDocument(app))
        .map((node) => node.type)
        .sort(),
    ).toEqual(
      [
        'program.main',
        'var.declare',
        'math.random_int',
        'var.declare',
        'io.print',
        'control.while',
        'math.compare',
        'var.get',
        'var.get',
        'io.ask',
        'control.if',
        'math.compare',
        'var.get',
        'var.get',
        'math.compare',
        'var.get',
        'var.get',
        'io.print',
        'io.print',
        'io.print',
      ].sort(),
    );

    // (5) F5: the program is built, then runs in the console.
    await pressKey(driver, Key.F5);
    await waitForTranscript(
      app,
      (text) => text.includes(INTRO) && promptCount(text) >= 1,
      BUILD_TIMEOUT_MS,
      'the build and the first prompt',
    );
    expect(await consoleState(driver)).toMatch(/Running/);
    await showConsole(driver);

    // (6) Binary search on the answers until "Correct!".
    const search = new BinarySearch();
    for (let turn = 0; turn < MAX_GUESSES && !search.found; turn += 1) {
      const answered = answersIn(await hook.consoleText()).length;
      await typeIntoConsole(driver, `${String(search.next())}${Key.ENTER}`);
      const transcript = await waitForTranscript(
        app,
        (text) => answersIn(text).length > answered,
        ANSWER_TIMEOUT_MS,
        `the answer to ${String(search.guesses.at(-1))}`,
      );
      const answer = answersIn(transcript).at(-1);
      expect(answer).toBeDefined();
      if (answer !== undefined) {
        search.answer(answer);
      }
    }
    expect(search.found, `guesses: ${search.guesses.join(', ')}`).toBe(true);
    await waitForConsoleState(driver, /^✓?\s*Finished \(exit code 0\)$/, ANSWER_TIMEOUT_MS);
    expect(await visibleConsoleText(driver)).toContain('Correct!');

    // The transcript has the golden output's shape (tests/golden/guessing_game/stdout.regex).
    const transcript = await hook.consoleText();
    const shape = goldenShape(readFileSync(path.join(GOLDEN, 'stdout.regex'), 'utf8'));
    if (process.platform === 'win32') {
      // Windows' pseudoconsole may repaint lines with cursor moves instead of line breaks, so only
      // the order of the game's lines is checked there.
      const tokens = gameTokens(transcript);
      expect(tokens.join('\n')).toMatch(
        /^Guess a number from 1 to 100!(\nYour guess: \nToo (low|high)!)*\nYour guess: \nCorrect!$/,
      );
    } else {
      expect(withoutEcho(transcript)).toMatch(shape);
    }

    // (7) Run again, then Stop: the header says "Stopped".
    const prompts = promptCount(transcript);
    await clickTestId(driver, 'console-run-again');
    await waitForTranscript(
      app,
      (text) => promptCount(text) > prompts,
      BUILD_TIMEOUT_MS,
      'the prompt of the second run',
    );
    await waitForConsoleState(driver, /Running/, ANSWER_TIMEOUT_MS);
    await clickTestId(driver, 'toolbar-stop');
    await waitForConsoleState(driver, /^■?\s*Stopped$/, ANSWER_TIMEOUT_MS);
  });

  it('shows a program that prints more than 8 KiB at once in the console (channel fetch)', async (context) => {
    const app = await launchApp(context);
    const ids = new NodeIds('flood');
    await waitForToolchain(app);
    const mainId = await newEmptyProject(app);

    // 10,000 characters in one print: one write, read and sent as a batch of more than 8 KiB.
    const flood = `B2C-FLOOD:${'0123456789'.repeat(1000)}`;
    await app.hook.insertBlocks(mainId, 'BODY', [print(ids, flood), print(ids, 'B2C-FLOOD-END')]);
    await waitForErrors(app, 0);

    await clickTestId(app.driver, 'toolbar-run');
    await waitForConsoleState(app.driver, /Finished \(exit code 0\)$/, BUILD_TIMEOUT_MS);
    const transcript = await waitForTranscript(
      app,
      (text) => text.includes('B2C-FLOOD-END'),
      ANSWER_TIMEOUT_MS,
      'the end of the flood',
    );
    if (process.platform === 'win32') {
      // Windows' pseudoconsole wraps the long line itself (with cursor moves or line breaks), and
      // writes the character at each wrap point again on the next row. The flood never has two
      // equal characters in a row, so squeezing runs keeps every character of it and its order
      // (a lost or reordered character still fails) while forgiving those repeats.
      const squeeze = (text: string): string => text.replace(/\s+/g, '').replace(/(.)\1+/g, '$1');
      expect(squeeze(transcript)).toContain(squeeze(flood));
    } else {
      // On screen the long line wraps; the transcript has it whole, exactly once.
      expect(transcript.split(flood)).toHaveLength(2);
    }
  });
});
