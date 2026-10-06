/**
 * Webview benchmark: the live preview at 1,000 blocks (docs/spec/01-overview.md §1.4 N4, 09 §9.2).
 * The generated 1,000-block document (bench/document.ts) is opened, then {@link ROUNDS} rounds of
 * {@link EDITS} edits are made: each edit adds a print with a unique text at the end of `main`
 * through the test hook, and the page clock times it until that text is in the C++ of the editor's
 * state (bench/page.ts `EDIT_SCRIPT`). The print is then deleted again with the keyboard, so every
 * edit is made to the same 1,000 blocks. Two metrics, each round's p95 being one sample:
 *
 * - `webview.preview-1000.p95` (gated): the preview pipeline's own task, found as the long task
 *   after which the edit is in the C++: reading the canvas, `canonical()` and `preview()` in
 *   WebAssembly, and storing the result (N4's "codegen + analysis", as the editor runs it);
 * - `webview.edit-1000.p95` (reported): the whole time from the edit to its C++, less the
 *   pipeline's 50 ms debounce, which adds Blockly's re-rendering of the edited stack.
 */
import { mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';

import { Key } from 'selenium-webdriver';
import { afterAll, describe, expect, it } from 'vitest';

import { E2E_HOOK_NAME } from '../../src/e2e/contract';
import { type App, launchApp } from '../support/app';
import { NodeIds, print } from '../support/bdm';
import { foldCodePanel } from '../support/editor';
import { pressKey } from '../support/ui';
import { MAIN_ID, writeBenchProject } from './document';
import {
  EDIT_SCRIPT,
  type EditResult,
  GONE_SCRIPT,
  PREVIEW_DEBOUNCE_MS,
  previewTask,
} from './page';
import { openProjectFile } from './project';
import { writeMetric } from './results';
import { median, percentile, rounded } from './stats';

/** Rounds of edits; each round's p95 is one sample (the comparison needs at least 10). */
const ROUNDS = 10;

/** Edits per round. */
const EDITS = 10;

/** How long one edit's preview may take before the benchmark fails. */
const EDIT_TIMEOUT_MS = 30_000;

/** The shortest main-thread task the heartbeat records (bench/page.ts `EDIT_SCRIPT`). */
const LONG_TASK_MS = 4;

/** How long opening the document, rendering it and the first preview may take. */
const OPEN_TIMEOUT_MS = 120_000;

const folders: string[] = [];

afterAll(() => {
  for (const folder of folders) {
    rmSync(folder, { recursive: true, force: true });
  }
});

/** Whether `value` is an {@link EditResult}. */
function isEditResult(value: unknown): value is EditResult {
  return typeof value === 'object' && value !== null && 'kind' in value;
}

/** One timed edit. */
interface TimedEdit {
  /** The inserted block. */
  readonly id: string;
  /** The preview pipeline's task, in milliseconds. */
  readonly previewMs: number;
  /** From the edit to its C++ in the editor's state, less the debounce, in milliseconds. */
  readonly editMs: number;
}

/** Adds a print showing `marker` at the end of `main` and times its preview (page clock). */
async function timedEdit(app: App, ids: NodeIds, marker: string): Promise<TimedEdit> {
  const block = print(ids, marker);
  const result: unknown = await app.driver.executeAsyncScript(
    EDIT_SCRIPT,
    E2E_HOOK_NAME,
    MAIN_ID,
    'BODY',
    [block],
    marker,
    EDIT_TIMEOUT_MS,
    LONG_TASK_MS,
  );
  if (!isEditResult(result)) {
    throw new Error('The edit script returned something unexpected');
  }
  if (result.kind === 'error') {
    throw new Error(`The edit failed: ${result.message}`);
  }
  if (result.kind === 'timeout') {
    throw new Error(`The preview did not show the edit within ${String(EDIT_TIMEOUT_MS / 1000)} s`);
  }
  const task = previewTask(result);
  if (task === null) {
    throw new Error(
      `No task of at least ${String(LONG_TASK_MS)} ms brought the edit's preview: ${JSON.stringify(result)}`,
    );
  }
  return {
    id: block.id,
    previewMs: task.ms,
    editMs: Math.max(0, result.seen - result.inserted - PREVIEW_DEBOUNCE_MS),
  };
}

/** Deletes block `id` with the keyboard (selected, then Delete) and waits until its code is gone. */
async function deleteBlock(app: App, id: string, marker: string): Promise<void> {
  await app.hook.selectBlock(id);
  await pressKey(app.driver, Key.DELETE);
  const gone: unknown = await app.driver.executeAsyncScript(
    GONE_SCRIPT,
    E2E_HOOK_NAME,
    marker,
    EDIT_TIMEOUT_MS,
  );
  if (gone !== 'gone') {
    throw new Error(
      `Deleting block ${id} did not remove "${marker}" from the C++ (${String(gone)})`,
    );
  }
}

describe('webview benchmark: the live preview at 1,000 blocks', () => {
  it(`times ${String(ROUNDS * EDITS)} edits and their previews`, async (context) => {
    const folder = mkdtempSync(path.join(tmpdir(), 'b2c-bench-preview-'));
    folders.push(folder);
    const file = writeBenchProject(folder, { blocks: 1_000, dragHandle: false });
    const app = await launchApp(context, { dialogs: { open: [file] } });
    await openProjectFile(app, { blockId: MAIN_ID, code: 'value_0', timeout: OPEN_TIMEOUT_MS });
    // The C++ panel is not needed; folded, its rendering does not take part in the timing.
    await foldCodePanel(app);

    const ids = new NodeIds('bench');
    const previewSamples: number[] = [];
    const editSamples: number[] = [];
    for (let round = 0; round < ROUNDS; round += 1) {
      const previews: number[] = [];
      const edits: number[] = [];
      for (let edit = 0; edit < EDITS; edit += 1) {
        const marker = `bench edit ${String(round)}-${String(edit)}`;
        const timed = await timedEdit(app, ids, marker);
        previews.push(timed.previewMs);
        edits.push(timed.editMs);
        await deleteBlock(app, timed.id, marker);
      }
      previewSamples.push(percentile(previews, 95));
      editSamples.push(percentile(edits, 95));
    }
    const per = `p95 of ${String(EDITS)} edits per sample`;
    writeMetric({
      name: 'webview.preview-1000.p95',
      unit: 'ms',
      better: 'lower',
      gate: true,
      description: `Preview pipeline task (canvas, canonical, preview, store) at 1,000 blocks; ${per}`,
      samples: previewSamples,
    });
    writeMetric({
      name: 'webview.edit-1000.p95',
      unit: 'ms',
      better: 'lower',
      gate: false,
      description: `Edit to its C++ in the editor's state less the 50 ms debounce, 1,000 blocks; ${per}`,
      samples: editSamples,
    });
    process.stdout.write(
      `At 1,000 blocks: preview task p95 median ${String(rounded(median(previewSamples), 1))} ms, ` +
        `edit to C++ p95 median ${String(rounded(median(editSamples), 1))} ms\n`,
    );
    expect(previewSamples).toHaveLength(ROUNDS);
  });
});
