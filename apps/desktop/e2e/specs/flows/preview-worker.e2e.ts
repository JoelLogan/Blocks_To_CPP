/**
 * The preview worker under the app's real policy (ADR-0016 §2 "Verify first",
 * docs/plans/m3-contracts/w5-worker.md): on each webview, a dedicated module worker starts from
 * the packaged script under `workers/`, is posted the page's compiled `WebAssembly.Module`, and
 * previews the open project exactly as the main thread does; its probe finds `eval` blocked, and a
 * request to the IPC endpoint that the page makes refused in the worker, so the worker runs under
 * its own policy (`WORKER_CSP`), not the page's.
 * The check (src/e2e/workerCheck.ts) runs beside the editor's own preview, which stays on the
 * main thread until the worker preview is switched on (w5-worker.md §4).
 *
 * The third test is the editor's own preview: in a build made with `B2C_PREVIEW_WORKER=1` it moves
 * to the worker after the first update, and after the worker is stopped the next edit previews on
 * a new one; in any other build (every build in CI until the flag is the default) it stays on the
 * main thread.
 *
 * The second test measures, at 1,000 blocks (the benchmarks' document), the worker's start-up, a
 * preview's round trip through `postMessage` against the main thread's call, and the
 * structured-clone cost of the largest messages. It writes them to standard output; only the
 * worker's answer is checked.
 */
import { mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';

import { afterAll, describe, expect, it } from 'vitest';

import type { E2eWorkerCheck } from '../../../src/e2e/contract';
import { MAIN_ID, writeBenchProject } from '../../bench/document';
import { openProjectFile } from '../../bench/project';
import { type App, launchApp } from '../../support/app';
import { NodeIds, print } from '../../support/bdm';
import { waitFor } from '../../support/wait';
import { startFlow } from './lib/launch';
import { copyExample, newEmptyProject, openFromStartPage } from './lib/project';

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
    expectWorkerWorks(await checkWorker(app, 'guessing game', 3));
  });

  it('is measured at 1,000 blocks', async (context) => {
    const folder = mkdtempSync(path.join(tmpdir(), 'b2c-worker-'));
    folders.push(folder);
    const file = writeBenchProject(folder, { blocks: 1_000, dragHandle: false });
    const app = await launchApp(context, { dialogs: { open: [file] }, recordLongTasks: false });
    await openProjectFile(app, { blockId: MAIN_ID, code: 'value_0', timeout: OPEN_TIMEOUT_MS });
    expectWorkerWorks(await checkWorker(app, '1,000 blocks', 20));
  });

  it('previews the editor’s edits in its worker when the build has it on', async (context) => {
    const flow = startFlow(context);
    const app = await flow.launch();
    const mainId = await newEmptyProject(app);
    const ids = new NodeIds('worker');
    await printAndWait(app, mainId, ids, 'first edit');
    if ((await app.hook.call('previewThread')) === 'off') {
      // A build without the flag: the editor previews on the main thread, as in M2.
      await printAndWait(app, mainId, ids, 'second edit');
      expect(await app.hook.call('previewThread')).toBe('off');
      return;
    }
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
});
