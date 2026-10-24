/**
 * The console in the flow tests: waiting for the program's output in the hook's transcript, the
 * "… N lines skipped" markers of the flood protection (docs/spec/07-toolchain-build-run.md
 * §7.6.5), and how long the header takes to reach a state.
 *
 * The transcript keeps the console's last megabyte. While a program floods the console it moves on
 * faster than WebDriver can fetch it, so the flood checks look at it inside the webview
 * ({@link consoleSummary}) and only a small summary crosses WebDriver.
 */
import type { WebDriver } from 'selenium-webdriver';

import { E2E_HOOK_NAME } from '../../../../src/e2e/contract';
import type { App } from '../../../support/app';
import { consoleState } from '../../../support/console';
import { waitFor } from '../../../support/wait';

/** The console transcript, waiting until `ready` accepts it. */
export function waitForTranscript(
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

/** The console's "… N lines skipped" marker (panels/console/header.ts), as plain text. */
const SKIPPED = /… ([\d,]+) lines? skipped|… output skipped/g;

/** The counts of the "… N lines skipped" markers in a transcript (0 for "… output skipped"). */
export function skippedCounts(transcript: string): number[] {
  return Array.from(transcript.matchAll(SKIPPED), (match) =>
    match[1] === undefined ? 0 : Number(match[1].replace(/,/g, '')),
  );
}

/** What {@link consoleSummary} tells about the transcript. */
export interface ConsoleSummary {
  /** The counts of the skipped-lines markers in it, in order. */
  readonly counts: readonly number[];
  /** For each text asked about, whether the transcript has it. */
  readonly found: readonly boolean[];
  /** Its last 200 characters. */
  readonly tail: string;
}

/**
 * Runs in the webview: {@link ConsoleSummary} of the hook's transcript for the texts
 * `arguments[1]`. A plain script (not a function from this file), like the harness's hook calls.
 */
const SUMMARY_IN_PAGE = `
  const hook = window[arguments[0]];
  const needles = arguments[1];
  const text = hook.consoleText();
  const counts = [];
  for (const match of text.matchAll(/… ([\\d,]+) lines? skipped|… output skipped/g)) {
    counts.push(match[1] === undefined ? 0 : Number(match[1].replace(/,/g, '')));
  }
  return {
    counts: counts,
    found: needles.map((needle) => text.includes(needle)),
    tail: text.slice(-200),
  };
`;

/** The skipped-lines markers in the console now, whether it has each of `needles`, and its end. */
export async function consoleSummary(
  driver: WebDriver,
  needles: readonly string[] = [],
): Promise<ConsoleSummary> {
  const result: unknown = await driver.executeScript(SUMMARY_IN_PAGE, E2E_HOOK_NAME, needles);
  if (typeof result !== 'object' || result === null) {
    throw new Error('The console transcript could not be read');
  }
  const { counts, found, tail } = result as { counts?: unknown; found?: unknown; tail?: unknown };
  return {
    counts: Array.isArray(counts)
      ? counts.filter((count): count is number => typeof count === 'number')
      : [],
    found: needles.map((_needle, index) => Array.isArray(found) && found[index] === true),
    tail: typeof tail === 'string' ? tail : '',
  };
}

/** Waits until the console transcript has any of `needles` (checked in the webview). */
export async function waitForConsoleText(
  driver: WebDriver,
  needles: readonly string[],
  timeout: number,
): Promise<void> {
  await waitFor(async () => (await consoleSummary(driver, needles)).found.includes(true), {
    timeout,
    interval: 250,
    message: async () =>
      `any of ${JSON.stringify(needles)} in the console; it ends with: ${(await consoleSummary(driver)).tail}`,
  });
}

/**
 * Waits until the console header's state matches `expected` and returns how long that took, in
 * milliseconds, polling often so the time is close.
 */
export async function timeToState(
  driver: WebDriver,
  expected: RegExp,
  timeout: number,
): Promise<number> {
  const started = Date.now();
  await waitFor(async () => expected.test(await consoleState(driver)), {
    timeout,
    interval: 50,
    message: async () =>
      `the console state ${String(expected)} (it shows "${await consoleState(driver)}")`,
  });
  return Date.now() - started;
}
