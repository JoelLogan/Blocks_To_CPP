/**
 * Webview benchmark: selecting blocks at 1,000 blocks (docs/spec/01-overview.md §1.4 N12, 09
 * §9.2: "selecting blocks in 8 scopes at 1,000 blocks"). The generated 1,000-block document
 * (bench/document.ts) is opened, and blocks in 8 scopes (bench/targets.ts `selectionTargets`) are
 * clicked in turn with real pointer input, so every click changes the selection and the variables
 * the toolbox lists. Before each click, outside the timing, the canvas is scrolled through the
 * test hook until the block is well inside the visible canvas (which keeps the selection) and the
 * page is left to settle. The page then times its tasks from the press until it has run no long
 * task for 300 ms (bench/page.ts `TASKS_START_SCRIPT`); the longest task after the press is one
 * sample of `webview.select-1000.max-task` (gated). The C++ panel is folded, as in the drag
 * benchmark, to leave the canvas room on a small window. A first click warms up and is not
 * counted.
 */
import { mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';

import { afterAll, describe, expect, it } from 'vitest';

import { launchApp } from '../support/app';
import { foldCodePanel } from '../support/editor';
import { MAIN_ID, writeBenchProject } from './document';
import { type SpanTimes, spanSummary, timedClick } from './interaction';
import { openProjectFile } from './project';
import { benchSamples, writeMetric } from './results';
import { median, rounded } from './stats';
import { selectionTargets } from './targets';

/** The workspace size. */
const BLOCKS = 1_000;

/** How long opening the document, rendering it and the first preview may take. */
const OPEN_TIMEOUT_MS = 120_000;

/**
 * How long the whole benchmark may take: at 1,000 blocks a click that changes the scope rebuilds
 * the toolbox's flyout, which took seconds on a slow machine (2026-10).
 */
const TEST_TIMEOUT_MS = 1_800_000;

const folders: string[] = [];

afterAll(() => {
  for (const folder of folders) {
    rmSync(folder, { recursive: true, force: true });
  }
});

describe('webview benchmark: selecting blocks at 1,000 blocks', () => {
  it(
    'times the longest task after clicks in 8 scopes',
    async (context) => {
      const samples = benchSamples();
      const folder = mkdtempSync(path.join(tmpdir(), 'b2c-bench-select-'));
      folders.push(folder);
      const file = writeBenchProject(folder, { blocks: BLOCKS, dragHandle: false });
      const app = await launchApp(context, { dialogs: { open: [file] }, recordLongTasks: false });
      await openProjectFile(app, { blockId: MAIN_ID, code: 'value_0', timeout: OPEN_TIMEOUT_MS });
      await foldCodePanel(app);

      const targets = selectionTargets(BLOCKS);
      const target = (index: number): string => {
        const id = targets[index % targets.length];
        if (id === undefined) {
          throw new Error(`No target ${String(index)}`);
        }
        return id;
      };
      // Warm-up: the last target, so the first timed click changes the scope from it.
      await timedClick(app, target(targets.length - 1));
      const longest: number[] = [];
      const respond: number[] = [];
      const spans: SpanTimes[] = [];
      for (let sample = 0; sample < samples; sample += 1) {
        const timed = await timedClick(app, target(sample));
        longest.push(timed.longestMs);
        respond.push(timed.respondMs);
        spans.push(timed.spans);
      }
      writeMetric({
        name: 'webview.select-1000.max-task',
        unit: 'ms',
        better: 'lower',
        gate: true,
        description: `Longest main-thread task after clicking a block at 1,000 blocks, 8 scopes in turn; one click per sample`,
        samples: longest,
      });
      process.stdout.write(
        `Selecting at 1,000 blocks: longest task median ${String(rounded(median(longest), 1))} ms, ` +
          `settled after median ${String(rounded(median(respond), 1))} ms (${String(samples)} clicks)\n` +
          `  spans per click (median): ${spanSummary(spans)}\n`,
      );
      expect(longest).toHaveLength(samples);
    },
    TEST_TIMEOUT_MS,
  );
});
