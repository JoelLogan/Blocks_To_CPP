/**
 * Webview benchmark: the live preview and edits at 1,000 blocks (docs/spec/01-overview.md §1.4
 * N4 and N12, 09 §9.2). The generated 1,000-block document (bench/document.ts) is opened. Each
 * edit adds a print with a unique text at the end of `main` through the test hook, and the page
 * clock times it until that text is in the C++ of the editor's state (bench/page.ts
 * `EDIT_SCRIPT`); the print is then deleted again with the keyboard, so every edit is made to the
 * same 1,000 blocks. Two phases of {@link EDITS} edits per sample, whose metrics preview-metrics.ts
 * takes:
 *
 * - with the C++ panel folded, the preview, from the app's User Timing spans of each edit
 *   (src/lib/timing.ts; the preview runs in a Web Worker, so the page's long tasks no longer
 *   find it, docs/plans/m3-contracts/worker.md §9): `webview.preview-1000.p95` (gated), the span
 *   `b2c:preview` from the run's start until its answer is applied;
 *   `webview.preview-1000.update-p95` (gated, N4), `update()` alone where it runs (the span
 *   `b2c:preview:update`); and `webview.preview-1000.main-p95` (reported), the sum of the
 *   main-thread phases. Each sample's p95 is one sample of each;
 * - with the C++ panel shown, as the editor starts, from the edit until the page has finished what
 *   it started (Blockly's re-rendering of the edited stack, the preview, the C++ panel's update;
 *   the page has then run no long task for 300 ms), less the pipeline's 50 ms debounce. After each
 *   deletion the page is left to settle, outside the timing. `webview.edit-1000.p95-all` (gated)
 *   is the p95 of every edit of the run, pooled (decision D19: with 10 samples, 100 edits, so a
 *   few slow edits do not decide it); `webview.edit-1000.p95` (reported; gated before D19) takes
 *   each sample's p95, its slowest edit, as one sample.
 *
 * The pooled metric is taken from the same edits, so the benchmark takes no longer.
 */
import { mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';

import { afterAll, describe, expect, it } from 'vitest';

import { type App, launchApp } from '../support/app';
import { NodeIds, print } from '../support/bdm';
import { foldCodePanel, showCodePanel } from '../support/editor';
import { requireHookMethods } from './capabilities';
import { MAIN_ID, writeBenchProject } from './document';
import {
  deleteBlock,
  editSummary,
  SETTLE_MS,
  type SpanTimes,
  spanSummary,
  timedEdit,
  type TimedEdit,
} from './interaction';
import { EDITS, previewEdit, previewMetrics } from './preview-metrics';
import { openProjectFile } from './project';
import { benchSamples, metricValue, writeMetric } from './results';
import { median, rounded } from './stats';

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

/** What {@link rounds} measured: what was taken from each edit, and every edit with its spans. */
interface Rounds<T> {
  /** What `measure` took from each edit, one list of {@link EDITS} per round (sample). */
  readonly values: T[][];
  readonly spans: SpanTimes[];
  readonly edits: TimedEdit[];
}

/**
 * Makes `samples` rounds of {@link EDITS} edits, each deleted again, and returns what `measure`
 * takes from each edit, per round, with the spans of the edits.
 */
async function rounds<T>(
  app: App,
  phase: { readonly name: string; readonly settleMs: number; readonly samples: number },
  measure: (edit: TimedEdit) => T,
): Promise<Rounds<T>> {
  const ids = new NodeIds(phase.name);
  const result: T[][] = [];
  const spans: SpanTimes[] = [];
  const edits: TimedEdit[] = [];
  for (let round = 0; round < phase.samples; round += 1) {
    const values: T[] = [];
    for (let edit = 0; edit < EDITS; edit += 1) {
      const marker = `bench ${phase.name} ${String(round)}-${String(edit)}`;
      const timed = await timedEdit(app, print(ids, marker), {
        parentId: MAIN_ID,
        input: 'BODY',
        marker,
        settleMs: phase.settleMs,
      });
      values.push(measure(timed));
      spans.push(timed.spans);
      edits.push(timed);
      await deleteBlock(app, timed.id, marker, phase.settleMs > 0);
    }
    result.push(values);
  }
  return { values: result, spans, edits };
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
      await requireHookMethods(context, app, 'preview');
      await openProjectFile(app, { blockId: MAIN_ID, code: 'value_0', timeout: OPEN_TIMEOUT_MS });

      // The preview's spans: folded, the C++ panel's rendering is not part of the run.
      await foldCodePanel(app);
      const preview = await rounds(app, { name: 'preview', settleMs: 0, samples }, (timed) =>
        previewEdit(timed.spans),
      );

      // The whole edit as a person sees it, with the C++ panel shown.
      await showCodePanel(app);
      const edit = await rounds(
        app,
        { name: 'edit', settleMs: SETTLE_MS, samples },
        (timed) => timed.settledMs,
      );

      // D19: gated on the p95 of every edit of the run; each sample's p95 is still reported.
      const metrics = previewMetrics(preview.values, edit.values);
      writeMetric(metrics.preview);
      writeMetric(metrics.update);
      writeMetric(metrics.main);
      writeMetric(metrics.edit);
      writeMetric(metrics.editPerRound);
      const previewSamples = metrics.preview.samples;
      const editSamples = metrics.editPerRound.samples;
      const p95 = (metric: { readonly samples: readonly number[] }) =>
        `${String(rounded(median(metric.samples), 1))} ms`;
      process.stdout.write(
        `At 1,000 blocks: p95 medians of the preview ${p95(metrics.preview)}, its update() ` +
          `${p95(metrics.update)}, its main-thread phases ${p95(metrics.main)}; ` +
          `edit until settled p95 of all ${String(metrics.edit.samples.length)} edits ` +
          `${String(rounded(metricValue(metrics.edit), 1))} ms (p95 per sample, median ` +
          `${String(rounded(median(editSamples), 1))} ms)\n` +
          `  spans per edit, C++ panel folded (median): ${spanSummary(preview.spans)}\n` +
          `  spans per edit, C++ panel shown (median): ${spanSummary(edit.spans)}\n` +
          `  per edit, C++ panel shown (median): ${editSummary(edit.edits)}\n`,
      );
      expect(previewSamples).toHaveLength(samples);
      expect(metrics.update.samples).toHaveLength(samples);
      expect(metrics.main.samples).toHaveLength(samples);
      expect(editSamples).toHaveLength(samples);
      expect(metrics.edit.samples).toHaveLength(samples * EDITS);
    },
    TEST_TIMEOUT_MS,
  );
});
