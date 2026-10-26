/**
 * Webview benchmark: the toolbox's refresh after a variable is declared (docs/spec/04-user-
 * interface.md §4.2; the `declare-variable` item of the M2 polish plan). A new variable changes
 * what the toolbox's *Variables* category shows, so the toolbox shows its flyout again: since
 * package T of M3's W2 (src/editor/toolbox/inflater.ts) only the blocks whose items changed are
 * built, so the refresh should not grow with the number of catalog blocks.
 *
 * Each declaration adds `create int <name> = 0` with a unique name at the end of `main` through
 * the test hook and times it until its C++ is in the editor's state and the page has run no long
 * task for 300 ms (bench/interaction.ts `timedEdit`), which takes in the toolbox's delayed
 * refresh; the block is then deleted again with the keyboard, outside the timing. The app's span
 * `b2c:toolbox-refresh` gives the refresh's time. Two phases of {@link DECLARATIONS} declarations
 * per sample, on the guessing game (examples/guessing_game.b2c, 40 blocks) and on the generated
 * 1,000-block document (bench/document.ts); declare-metrics.ts takes the metrics, all
 * report-only.
 */
import { copyFileSync, mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';

import { afterAll, describe, expect, it, type TestContext } from 'vitest';

import { type App, launchApp } from '../support/app';
import { declareInt, NodeIds } from '../support/bdm';
import { REPOSITORY_ROOT } from '../support/env';
import { requireHookMethods } from './capabilities';
import {
  DECLARATIONS,
  type Declaration,
  declareMetrics,
  TOOLBOX_REFRESH_SPAN,
} from './declare-metrics';
import { MAIN_ID, writeBenchProject } from './document';
import { deleteBlock, SETTLE_MS, timedEdit, waitUntilQuiet } from './interaction';
import { openProjectFile } from './project';
import { benchSamples, writeMetric } from './results';
import { median, rounded } from './stats';
import { GUESSING_MAIN_ID } from './targets';

/** How long opening a document, rendering it and the first preview may take. */
const OPEN_TIMEOUT_MS = 120_000;

/** How long the whole benchmark may take (two documents, two app launches). */
const TEST_TIMEOUT_MS = 1_800_000;

/** Counts the blocks the toolbox's flyout shows (its top-level blocks), in the page. */
const FLYOUT_BLOCKS_SCRIPT = `
  return document.querySelectorAll('.blocklyFlyout .blocklyBlockCanvas > .blocklyDraggable').length;
`;

const folders: string[] = [];

afterAll(() => {
  for (const folder of folders) {
    rmSync(folder, { recursive: true, force: true });
  }
});

/** What one phase measured: its declarations, round by round, and the flyout's block count. */
interface Phase {
  readonly rounds: Declaration[][];
  readonly flyoutBlocks: number;
}

/**
 * Opens `file`, declares {@link DECLARATIONS} variables per sample at the end of `main` (`mainId`),
 * each deleted again, and returns what each declaration measured.
 */
async function phase(
  context: TestContext,
  file: string,
  open: { readonly mainId: string; readonly code: string; readonly prefix: string },
): Promise<Phase> {
  const app: App = await launchApp(context, { dialogs: { open: [file] }, recordLongTasks: false });
  await requireHookMethods(context, app, 'declare');
  await openProjectFile(app, { blockId: open.mainId, code: open.code, timeout: OPEN_TIMEOUT_MS });
  await waitUntilQuiet(app, 'opening the document');
  const flyoutBlocks: unknown = await app.driver.executeScript(FLYOUT_BLOCKS_SCRIPT);
  const ids = new NodeIds(open.prefix);
  const rounds: Declaration[][] = [];
  for (let round = 0; round < benchSamples(); round += 1) {
    const declarations: Declaration[] = [];
    for (let index = 0; index < DECLARATIONS; index += 1) {
      // A unique name, which only this declaration's C++ contains.
      const name = `${open.prefix}${String(round)}x${String(index)}`;
      const timed = await timedEdit(app, declareInt(ids, `s_${name}`, name, 0), {
        parentId: open.mainId,
        input: 'BODY',
        marker: `int ${name} = 0;`,
        settleMs: SETTLE_MS,
      });
      declarations.push({
        refreshMs: timed.spans[TOOLBOX_REFRESH_SPAN] ?? 0,
        longestMs: timed.longestMs,
      });
      await deleteBlock(app, timed.id, `int ${name} = 0;`, true);
    }
    rounds.push(declarations);
  }
  return { rounds, flyoutBlocks: typeof flyoutBlocks === 'number' ? flyoutBlocks : 0 };
}

describe('webview benchmark: the toolbox after a variable is declared', () => {
  it(
    `times the toolbox's refresh after ${String(DECLARATIONS)} declarations per sample`,
    async (context) => {
      const folder = mkdtempSync(path.join(tmpdir(), 'b2c-bench-declare-'));
      folders.push(folder);
      const guessingFile = path.join(folder, 'guessing_game.b2c');
      copyFileSync(path.join(REPOSITORY_ROOT, 'examples', 'guessing_game.b2c'), guessingFile);
      const guessing = await phase(context, guessingFile, {
        mainId: GUESSING_MAIN_ID,
        code: 'Guess a number',
        prefix: 'decl',
      });
      const largeFile = writeBenchProject(folder, { blocks: 1_000, dragHandle: false });
      const large = await phase(context, largeFile, {
        mainId: MAIN_ID,
        code: 'value_0',
        prefix: 'large',
      });

      const metrics = declareMetrics(guessing.rounds, large.rounds);
      writeMetric(metrics.refresh);
      writeMetric(metrics.refresh1000);
      writeMetric(metrics.maxTask);
      const ms = (samples: readonly number[]) => `${String(rounded(median(samples), 1))} ms`;
      process.stdout.write(
        `Toolbox refresh after a declaration (median of the samples): guessing game ` +
          `${ms(metrics.refresh.samples)}, 1,000 blocks ${ms(metrics.refresh1000.samples)}; ` +
          `longest task (guessing game) ${ms(metrics.maxTask.samples)}; ` +
          `the flyout shows ${String(guessing.flyoutBlocks)} blocks\n`,
      );
      expect(metrics.refresh.samples).toHaveLength(benchSamples());
      expect(metrics.refresh1000.samples).toHaveLength(benchSamples());
    },
    TEST_TIMEOUT_MS,
  );
});
