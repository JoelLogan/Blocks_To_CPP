/**
 * Nightly E2E security test 1 (docs/spec/08-security.md §8.13, threats T1, T2 and T5 of §8.12):
 * every file of the malicious-project suite (tests/security/projects) is rejected, or opens in
 * Restricted Mode, and no compiler process is ever started.
 *
 * Each file is opened through the scripted native open dialog (the app's `e2e-hooks` seam): by
 * calling `project_open_dialog` from the webview for every file, and through the start page's
 * *Open* button for two of them. The suite's README table says what the loader does with each
 * file, so a file must be rejected with exactly its codes, or open. Every file that opens is
 * restricted, and building it (both configurations) is refused with `restricted`. A watch over
 * the process list runs from the end of toolchain discovery to the end of the test, and the
 * build cache must stay empty.
 */
import path from 'node:path';

import type { ProjectOpenDialogResponse } from '@blocks2cpp/ipc-types';
import { Key } from 'selenium-webdriver';
import { describe, expect, it } from 'vitest';

import { launchApp } from '../../support/app';
import { runHeldBack } from '../../support/editor';
import { clickTestId, pressKey, testId, testIdText } from '../../support/ui';
import { sleep } from '../../support/wait';
import { buildFolders, CompilerWatch } from './lib/compilers';
import { describeOutcome, errorCode, IpcProbe, waitForToolchains } from './lib/ipc';
import {
  copyInto,
  profileOf,
  readSuite,
  SECURITY_PROJECTS,
  scratchFolder,
  type SuiteCase,
} from './lib/projects';

/** `project_open_dialog`'s answer for a file that opened. */
type Opened = Extract<ProjectOpenDialogResponse, { status: 'ok' }>;

/** Whether an answer (from the page, so unchecked) is a project that opened. */
function isOpened(value: unknown): value is Opened {
  if (typeof value !== 'object' || value === null) {
    return false;
  }
  const opened = value as Partial<Record<'status' | 'handle' | 'document' | 'trust', unknown>>;
  return (
    opened.status === 'ok' &&
    typeof opened.handle === 'string' &&
    typeof opened.document === 'string' &&
    typeof opened.trust === 'object' &&
    opened.trust !== null
  );
}

/** The diagnostic codes of an `invalidDocument` error, sorted and each once. */
function diagnosticCodes(error: unknown): string[] {
  if (typeof error !== 'object' || error === null) {
    return [];
  }
  const diagnostics = (error as { diagnostics?: unknown }).diagnostics;
  if (!Array.isArray(diagnostics)) {
    return [];
  }
  const codes = diagnostics
    .map((diagnostic: unknown) =>
      typeof diagnostic === 'object' && diagnostic !== null
        ? (diagnostic as { code?: unknown }).code
        : undefined,
    )
    .filter((code): code is string => typeof code === 'string');
  return [...new Set(codes)].sort();
}

/** How a rejected file must be refused: `newerFormat` for B2C-E0108, else exactly its codes. */
function rejectionProblem(entry: SuiteCase, error: unknown, code: string | null): string | null {
  if (entry.loader === 'accepted') {
    return `the loader accepts it, but opening it was refused with ${code ?? 'a non-IPC error'}`;
  }
  if (entry.loader.includes('B2C-E0108')) {
    return code === 'newerFormat' ? null : `expected newerFormat, got ${code ?? 'a non-IPC error'}`;
  }
  if (code !== 'invalidDocument') {
    return `expected invalidDocument, got ${code ?? 'a non-IPC error'}`;
  }
  const expected = [...new Set(entry.loader)].sort();
  const found = diagnosticCodes(error);
  return JSON.stringify(found) === JSON.stringify(expected)
    ? null
    : `expected the codes ${expected.join(', ')}, got ${found.join(', ') || 'none'}`;
}

describe('E2E security 1: the malicious-project suite', () => {
  it('opens every file only in Restricted Mode, or not at all, and never starts a compiler', async (context) => {
    const suite = readSuite();
    const folder = scratchFolder(context);
    const copies = suite.map((entry) => copyInto(folder, path.join(SECURITY_PROJECTS, entry.file)));
    const app = await launchApp(context, { dialogs: { open: copies } });
    const probe = new IpcProbe(app.driver);
    const profile = profileOf(app);
    // Discovery probes the real g++ at start-up; only then does a compiler mean a build.
    await waitForToolchains(probe);
    const watch = CompilerWatch.start(profile);

    const problems: string[] = [];
    let opened = 0;
    for (const entry of suite) {
      const outcome = await probe.call({ cmd: 'project_open_dialog' });
      const fail = (problem: string) => {
        problems.push(`${entry.file}: ${problem}`);
      };
      if (outcome.kind === 'refused') {
        const problem = rejectionProblem(entry, outcome.error, errorCode(outcome));
        if (problem !== null) {
          fail(problem);
        }
        continue;
      }
      if (outcome.kind !== 'answered' || !isOpened(outcome.value)) {
        fail(
          `expected the file to open or be rejected, but the call was ${describeOutcome(outcome)}`,
        );
        continue;
      }
      opened += 1;
      const project = outcome.value;
      if (entry.loader !== 'accepted') {
        fail(`it opened, but the loader rejects it with ${entry.loader.join(', ')}`);
      }
      if (project.trust.state !== 'restricted' || project.trust.restrictedReason !== 'noRecord') {
        fail(`it opened with trust ${JSON.stringify(project.trust)}, not restricted (noRecord)`);
      }
      // Building it, as compromised script would: refused before anything runs.
      problems.push(
        ...(
          await probe.check(
            (['debug', 'release'] as const).map((config) => ({
              name: config,
              cmd: 'build_start',
              args: {
                request: { handle: project.handle, document: project.document, config },
                onEvent: { $channel: `build-${entry.file}-${config}` },
              },
              expected: { code: 'restricted' },
            })),
          )
        ).map((problem) => `${entry.file}: ${problem}`),
      );
      const closed = await probe.call({
        cmd: 'project_close',
        args: { request: { handle: project.handle } },
      });
      if (closed.kind !== 'answered') {
        fail(`project_close was ${describeOutcome(closed)}`);
      }
    }

    // Every file was offered: the dialog script is used up, and the next open is cancelled.
    expect(await probe.answer('project_open_dialog')).toEqual({ status: 'cancelled' });
    const sightings = await watch.stop();
    expect(watch.samples).toBeGreaterThan(0);
    expect(problems).toEqual([]);
    expect(opened).toBe(suite.filter((entry) => entry.loader === 'accepted').length);
    expect(sightings, 'compiler processes started by the app').toEqual([]);
    expect(buildFolders(profile), 'build folders in the cache').toEqual([]);
  });

  it('shows a rejected file as not opened and an accepted one in Restricted Mode, and F5 builds nothing', async (context) => {
    const folder = scratchFolder(context);
    const rejected = copyInto(folder, path.join(SECURITY_PROJECTS, 'proto-key.b2c'));
    const accepted = copyInto(folder, path.join(SECURITY_PROJECTS, 'injection-string-literal.b2c'));
    const app = await launchApp(context, { dialogs: { open: [rejected, accepted] } });
    const { driver } = app;
    const probe = new IpcProbe(driver);
    const profile = profileOf(app);
    await waitForToolchains(probe);
    const watch = CompilerWatch.start(profile);

    // The rejected file: the start page says nothing was opened, with the loader's code.
    await clickTestId(driver, 'start-open');
    await testId(driver, 'project-load-failure');
    expect(await testIdText(driver, 'project-load-failure')).toContain('B2C-E0127');
    // Still the start page: nothing was opened.
    await testId(driver, 'start-page');

    // The accepted one opens in Restricted Mode: the banner, the status bar, Run held back.
    await clickTestId(driver, 'start-open');
    await testId(driver, 'restricted-banner', 20_000);
    await testId(driver, 'status-restricted');
    expect(await runHeldBack(driver)).toBe(true);

    // F5 and a click on Run (held back, still clickable) start nothing.
    await pressKey(driver, Key.F5);
    await clickTestId(driver, 'toolbar-run');
    // A build would have started at once; give it time to show.
    await sleep(3_000);
    expect(await testIdText(driver, 'console-state')).not.toMatch(/Building|Running|Finished/);
    expect(await driver.findElements({ css: '[data-testid="restricted-banner"]' })).toHaveLength(1);
    const sightings = await watch.stop();
    expect(sightings, 'compiler processes started by the app').toEqual([]);
    expect(buildFolders(profile), 'build folders in the cache').toEqual([]);
  });
});
