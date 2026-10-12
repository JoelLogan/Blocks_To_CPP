/**
 * Webview benchmark: the live preview and edits at 1,000 blocks (docs/spec/01-overview.md §1.4
 * N4 and N12, 09 §9.2). The generated 1,000-block document (bench/document.ts) is opened. Each
 * edit adds a print with a unique text at the end of `main` through the test hook, and the page
 * clock times it until that text is in the C++ of the editor's state (bench/page.ts
 * `EDIT_SCRIPT`); the print is then deleted again with the keyboard, so every edit is made to the
 * same 1,000 blocks. Two phases of {@link EDITS} edits per sample, each sample's p95 being one
 * sample of its metric:
 *
 * - with the C++ panel folded, `webview.preview-1000.p95` (gated): the preview pipeline's own
 *   task, found as the long task after which the edit is in the C++: reading the canvas,
 *   `canonical()` and `preview()` in WebAssembly, and storing the result (N4's update, as the
 *   editor runs it);
 * - with the C++ panel shown, as the editor starts, `webview.edit-1000.p95` (gated): from the
 *   edit until the page has finished what it started (Blockly's re-rendering of the edited stack,
 *   the preview, the C++ panel's update; the page has then run no long task for 300 ms), less the
 *   pipeline's 50 ms debounce. After each deletion the page is left to settle, outside the timing.
 */
import { mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';

import { afterAll, describe, expect, it } from 'vitest';

import { type App, launchApp } from '../support/app';
import { NodeIds, print } from '../support/bdm';
import { foldCodePanel, showCodePanel } from '../support/editor';
import { MAIN_ID, writeBenchProject } from './document';
import { deleteBlock, LONG_TASK_MS, SETTLE_MS, timedEdit, type TimedEdit } from './interaction';
import { openProjectFile } from './project';
import { benchSamples, writeMetric } from './results';
import { median, percentile, rounded } from './stats';

/** Edits per sample. */
const EDITS = 10;

/** How long opening the document, rendering it and the first preview may take. */
const OPEN_TIMEOUT_MS = 120_000;

/** How long the whole benchmark may take (two phases of edits at 1,000 blocks). */
const TEST_TIMEOUT_MS = 1_800_000;

const folders: string[] = [];

afterAll(() => {
  for (const folder of folders) {
    rmSync(folder, { recursive: true, force: true });
  }
});

/**
 * Makes `samples` rounds of {@link EDITS} edits, each deleted again, and returns each round's p95
 * of what `measure` takes from an edit.
 */
async function rounds(
  app: App,
  phase: { readonly name: string; readonly settleMs: number; readonly samples: number },
  measure: (edit: TimedEdit) => number,
): Promise<number[]> {
  const ids = new NodeIds(phase.name);
  const result: number[] = [];
  for (let round = 0; round < phase.samples; round += 1) {
    const values: number[] = [];
    for (let edit = 0; edit < EDITS; edit += 1) {
      const marker = `bench ${phase.name} ${String(round)}-${String(edit)}`;
      const timed = await timedEdit(app, print(ids, marker), {
        parentId: MAIN_ID,
        input: 'BODY',
        marker,
        settleMs: phase.settleMs,
      });
      values.push(measure(timed));
      await deleteBlock(app, timed.id, marker, phase.settleMs > 0);
    }
    result.push(percentile(values, 95));
  }
  return result;
}

describe('webview benchmark: the live preview and edits at 1,000 blocks', () => {
  it(
    `times ${String(EDITS)} edits per sample, with the C++ panel folded and shown`,
    async (context) => {
      const samples = benchSamples();
      const folder = mkdtempSync(path.join(tmpdir(), 'b2c-bench-preview-'));
      folders.push(folder);
      const file = writeBenchProject(folder, { blocks: 1_000, dragHandle: false });
      const app = await launchApp(context, { dialogs: { open: [file] }, recordLongTasks: false });
      await openProjectFile(app, { blockId: MAIN_ID, code: 'value_0', timeout: OPEN_TIMEOUT_MS });

      // The preview pipeline's own task: folded, the C++ panel's rendering is not part of it.
      await foldCodePanel(app);
      const previewSamples = await rounds(
        app,
        { name: 'preview', settleMs: 0, samples },
        (timed) => {
          if (timed.previewMs === null) {
            throw new Error(
              `No task of at least ${String(LONG_TASK_MS)} ms brought the edit's preview`,
            );
          }
          return timed.previewMs;
        },
      );

      // The whole edit as a person sees it, with the C++ panel shown.
      await showCodePanel(app);
      const editSamples = await rounds(
        app,
        { name: 'edit', settleMs: SETTLE_MS, samples },
        (timed) => timed.settledMs,
      );

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
        gate: true,
        description: `Edit until the page settles (rendering, preview, C++ panel shown) less the 50 ms debounce, 1,000 blocks; ${per}`,
        samples: editSamples,
      });
      process.stdout.write(
        `At 1,000 blocks: preview task p95 median ${String(rounded(median(previewSamples), 1))} ms, ` +
          `edit until settled p95 median ${String(rounded(median(editSamples), 1))} ms\n`,
      );
      expect(previewSamples).toHaveLength(samples);
      expect(editSamples).toHaveLength(samples);
    },
    TEST_TIMEOUT_MS,
  );
});
