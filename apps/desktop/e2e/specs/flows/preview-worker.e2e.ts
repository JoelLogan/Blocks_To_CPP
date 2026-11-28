/**
 * The preview worker under the app's real policy (ADR-0016 §2 "Verify first",
 * docs/plans/m3-contracts/w5-worker.md): on each webview, a dedicated module worker starts from
 * the packaged script under `workers/`, is posted the page's compiled `WebAssembly.Module`, and
 * previews the open project exactly as the main thread does; its probe finds `eval` blocked, and a
 * request to the IPC endpoint that the page makes refused in the worker, so the worker runs under
 * its own policy (`WORKER_CSP`), not the page's.
 * The check (src/e2e/workerCheck.ts) runs beside the editor's own preview.
 *
 * The third test is the editor's own preview, in the worker in every build (there is no switch
 * that keeps it on the main thread, w5-worker.md §4): it moves to the worker after the first
 * update, and after the worker is stopped the next edit previews on a new one.
 *
 * The fourth and fifth are the E2E additions of docs/plans/m3-contracts/w5-performance.md §2.5,
 * with the worker previewing: a variable made with *Make a variable* after the worker started is
 * in a *set* block's variable menu (the M2 exit flow's failure of w5-worker.md §4, when the menus
 * still asked the main thread's core), and each M3-D1 assignment project (tests/assignments/)
 * opens and its preview equals its golden C++ (tests/golden/<name>/main.cpp), one after the other
 * in one app, each opened with ≡ *Open…* once the worker previews.
 *
 * The second test measures, at 1,000 blocks (the benchmarks' document), the worker's start-up, a
 * preview's round trip through `postMessage` against the main thread's call, and the
 * structured-clone cost of the largest messages. It writes them to standard output; only the
 * worker's answer is checked.
 */
import { mkdtempSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';

import { afterAll, describe, expect, it } from 'vitest';

import type { E2eWorkerCheck } from '../../../src/e2e/contract';
import { MAIN_ID, writeBenchProject } from '../../bench/document';
import { openProjectFile } from '../../bench/project';
import { type App, launchApp } from '../../support/app';
import { NodeIds, num, print } from '../../support/bdm';
import { centerOn, fieldOf } from '../../support/editor';
import { clickMiddle } from '../../support/ui';
import { REPOSITORY_ROOT } from '../../support/env';
import { waitFor } from '../../support/wait';
import { closeMenu, makeVariable, menuLabels } from './lib/editor';
import { startFlow } from './lib/launch';
import { copyExample, declare, newEmptyProject, openFromStartPage } from './lib/project';
import { answerDialog, chooseMenuItem, dialogText } from './lib/ui';

/** The M3-D1 assignments (tests/assignments/status.toml), each an example with its golden C++. */
const ASSIGNMENTS = [
  'temperature_table',
  'grade_average',
  'word_count',
  'student_records',
  'bank_menu',
  'palindrome',
  'dice_statistics',
  'guessing_game',
  'matrix_sum',
  'input_validation',
] as const;

/** How long a check may take: a worker's start-up (20 s at most) and its rounds. */
const CHECK_TIMEOUT_MS = 120_000;

/** How long opening the 1,000-block document, rendering it and the first preview may take. */
const OPEN_TIMEOUT_MS = 120_000;

const folders: string[] = [];

afterAll(() => {
  for (const folder of folders) {
    rmSync(folder, { recursive: true, force: true });
  }
});

/** How long an edit may take to reach the C++. */
const EDIT_TIMEOUT_MS = 30_000;

/** Adds a print of `text` to `main` and waits until the C++ shows it. */
async function printAndWait(app: App, mainId: string, ids: NodeIds, text: string): Promise<void> {
  await app.hook.insertBlocks(mainId, 'BODY', [print(ids, text)]);
  await waitFor(async () => (await app.hook.code()).includes(text), {
    timeout: EDIT_TIMEOUT_MS,
    message: `"${text}" in the C++`,
  });
}

/** Waits until the editor's preview runs in the worker. */
async function waitForWorker(app: App): Promise<void> {
  await waitFor(async () => (await app.hook.call('previewThread')) === 'worker', {
    timeout: EDIT_TIMEOUT_MS,
    message: 'the preview worker to start',
  });
}

/** Runs the worker check with `rounds` round trips and returns its result, also on stdout. */
async function checkWorker(app: App, label: string, rounds: number): Promise<E2eWorkerCheck> {
  await app.hook.call('startWorkerCheck', rounds);
  const result = await waitFor(() => app.hook.call('workerCheck'), {
    timeout: CHECK_TIMEOUT_MS,
    interval: 250,
    message: 'the preview worker check to finish',
  });
  process.stdout.write(`preview-worker check (${label}): ${JSON.stringify(result)}\n`);
  return result;
}

/** What every webview must show: the worker previews as the main thread does, under its policy. */
function expectWorkerWorks(result: E2eWorkerCheck): void {
  expect(result.error).toBeNull();
  if (!result.ok) {
    return;
  }
  expect(result.sameAsMain).toBe(true);
  expect(result.contentHash).toMatch(/^[0-9a-f]{64}$/);
  // Some policy applies: neither the page's nor the worker's allows eval.
  expect(result.probe.eval).toBe('blocked');
  // The worker's own policy, not the page's: the page reaches the IPC endpoint and the worker does
  // not. (WebKitGTK refuses the request without a violation event in the worker, so the probe
  // can say `failed` rather than `blocked` there.)
  expect(result.pageRequest).toBe('allowed');
  expect(result.probe.request).not.toBe('allowed');
}

describe('The preview worker', () => {
  it('starts under its own policy and previews a project as the main thread does', async (context) => {
    const flow = startFlow(context);
    const file = copyExample('guessing_game', flow.folders.projects);
    const app = await flow.launch({ dialogs: { open: [file] } });
    await openFromStartPage(app);
    await waitFor(async () => (await app.hook.code()).includes('int main'), {
      timeout: 30_000,
      message: 'the first preview',
    });
    const result = await checkWorker(app, 'guessing game', 3);
    expectWorkerWorks(result);
    // Trusted Types (both webviews have them): the trial reports a worker made from a plain
    // string, and nothing for the app's worker made through the b2c-core-worker policy.
    if (result.ok) {
      expect(result.unwrappedViolations).toBeGreaterThanOrEqual(1);
      expect(result.policyViolations).toBe(0);
    }
  });

  it('is measured at 1,000 blocks', async (context) => {
    const folder = mkdtempSync(path.join(tmpdir(), 'b2c-worker-'));
    folders.push(folder);
    const file = writeBenchProject(folder, { blocks: 1_000, dragHandle: false });
    const app = await launchApp(context, { dialogs: { open: [file] }, recordLongTasks: false });
    await openProjectFile(app, { blockId: MAIN_ID, code: 'value_0', timeout: OPEN_TIMEOUT_MS });
    expectWorkerWorks(await checkWorker(app, '1,000 blocks', 20));
  });

  it('previews the editor’s edits in its worker', async (context) => {
    const flow = startFlow(context);
    const app = await flow.launch();
    const mainId = await newEmptyProject(app);
    const ids = new NodeIds('worker');
    await printAndWait(app, mainId, ids, 'first edit');
    // The first update ran on the main thread and started the worker.
    await waitFor(async () => (await app.hook.call('previewThread')) === 'worker', {
      timeout: EDIT_TIMEOUT_MS,
      message: 'the preview worker to start',
    });
    await printAndWait(app, mainId, ids, 'second edit');
    await app.hook.call('stopPreviewWorker');
    await printAndWait(app, mainId, ids, 'third edit');
    expect(await app.hook.call('previewThread')).toBe('worker');
  });

  it('lists a variable made after the worker started in a set block’s menu', async (context) => {
    const flow = startFlow(context);
    const app = await flow.launch();
    const mainId = await newEmptyProject(app);
    const ids = new NodeIds('workermenu');
    const set = {
      id: ids.next(),
      type: 'var.set',
      v: 1,
      fields: { VAR: { ref: 's_worker_secret' } },
      inputs: { VALUE: num(1) },
    };
    await app.hook.insertBlocks(mainId, 'BODY', [
      declare(ids, { sym: 's_worker_secret', name: 'secret', type: 'int', value: num(0) }),
      set,
    ]);
    await waitFor(async () => (await app.hook.code()).includes('secret = 1;'), {
      timeout: EDIT_TIMEOUT_MS,
      message: 'secret = 1; in the C++',
    });
    await waitForWorker(app);

    // Make a variable, with main selected: guess goes at the top of main, before the set.
    await centerOn(app, mainId);
    await makeVariable(app, 'guess', 'int');
    await waitFor(async () => (await app.hook.code()).includes('int guess'), {
      timeout: EDIT_TIMEOUT_MS,
      message: 'int guess in the C++',
    });
    expect(await app.hook.call('previewThread')).toBe('worker');
    await clickMiddle(app.driver, await fieldOf(app, set.id, 'VAR'));
    expect((await menuLabels(app.driver)).sort()).toEqual(['guess', 'secret']);
    await closeMenu(app.driver);
  });

  it('opens each assignment and previews its golden C++ in the worker', async (context) => {
    const flow = startFlow(context);
    const files = ASSIGNMENTS.map((name) => copyExample(name, flow.folders.projects));
    const [first] = files;
    if (first === undefined) {
      throw new Error('no assignments');
    }
    // The first one is opened twice: from the start page, where its first preview starts the
    // worker, and again last, once the worker previews.
    const app = await flow.launch({ dialogs: { open: [first, ...files.slice(1), first] } });
    await openFromStartPage(app);
    await waitForWorker(app);
    const order = [...ASSIGNMENTS.slice(1), ASSIGNMENTS[0]];
    for (const name of order) {
      expect(await app.hook.call('previewThread')).toBe('worker');
      const golden = readFileSync(
        path.join(REPOSITORY_ROOT, 'tests', 'golden', name, 'main.cpp'),
        'utf8',
      );
      await chooseMenuItem(app.driver, 'Open…');
      await waitFor(
        async () => {
          // The project shown may have become changed (•) as the editor loaded it; its changes
          // are not kept.
          if ((await dialogText(app.driver)).includes('Save changes to')) {
            await answerDialog(app.driver, "Don't save");
          }
          return (await app.hook.code()) === golden ? true : null;
        },
        {
          timeout: EDIT_TIMEOUT_MS,
          message: async () =>
            `the golden C++ of ${name} in the preview (it shows:\n${(await app.hook.code()).slice(0, 400)})`,
        },
      );
      expect(await app.hook.call('previewThread')).toBe('worker');
    }
  }, 300_000);
});
