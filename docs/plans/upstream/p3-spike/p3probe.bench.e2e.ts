/**
 * P-3 spike probe (kept as evidence for M3-2, not part of the app's suite; ./README.md says how to
 * run it): with a diagnostic build whose Blockly bundle records timings into `globalThis.__p3`
 * (./instrument_bundle.py), makes the edit and selection benchmarks' interactions and writes what
 * each render pass and gesture cost.
 */
import { appendFileSync, mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';

import { afterAll, describe, it } from 'vitest';

import { type App, launchApp } from '../support/app';
import { NodeIds, print } from '../support/bdm';
import { foldCodePanel } from '../support/editor';
import { MAIN_ID, writeBenchProject } from './document';
import { deleteBlock, SETTLE_MS, timedClick, timedEdit, waitUntilQuiet } from './interaction';
import { openProjectFile } from './project';
import { selectionTargets, targetAt } from './targets';

const OUT = process.env['B2C_P3_OUT'] ?? '/dev/null';
const LABEL = process.env['B2C_P3_LABEL'] ?? 'app';
const folders: string[] = [];

afterAll(() => {
  for (const folder of folders) {
    rmSync(folder, { recursive: true, force: true });
  }
});

async function resetProbe(app: App): Promise<void> {
  await app.driver.executeScript('globalThis.__p3 = undefined;');
}

async function readProbe(app: App): Promise<Record<string, number>> {
  const probe: unknown = await app.driver.executeScript('return globalThis.__p3 ?? null;');
  const rounded: Record<string, number> = {};
  for (const [key, value] of Object.entries((probe ?? {}) as Record<string, number>)) {
    rounded[key] = Math.round(value * 10) / 10;
  }
  return rounded;
}

function write(kind: string, data: Record<string, unknown>): void {
  appendFileSync(OUT, `${JSON.stringify({ label: LABEL, kind, ...data })}\n`);
}

describe('P-3 probe', () => {
  it('records render passes and gestures at 1,000 blocks', async (context) => {
    const folder = mkdtempSync(path.join(tmpdir(), 'b2c-p3-probe-'));
    folders.push(folder);
    const file = writeBenchProject(folder, { blocks: 1_000, dragHandle: false });
    const app = await launchApp(context, { dialogs: { open: [file] }, recordLongTasks: false });
    await openProjectFile(app, { blockId: MAIN_ID, code: 'value_0', timeout: 120_000 });

    // The C++ panel is shown after opening, as in the edit benchmark's second phase.
    const ids = new NodeIds('probe');
    for (let edit = 0; edit < 8; edit += 1) {
      const marker = `probe edit ${String(edit)}`;
      await waitUntilQuiet(app, 'before the edit');
      await resetProbe(app);
      const timed = await timedEdit(app, print(ids, marker), {
        parentId: MAIN_ID,
        input: 'BODY',
        marker,
        settleMs: SETTLE_MS,
      });
      write('edit', { settledMs: timed.settledMs, longestMs: timed.longestMs, ...(await readProbe(app)) });
      await resetProbe(app);
      await deleteBlock(app, timed.id, marker, true);
      write('delete', await readProbe(app));
    }

    await foldCodePanel(app);
    const targets = selectionTargets(1_000);
    await timedClick(app, targetAt(targets, targets.length - 1));
    for (let click = 0; click < 16; click += 1) {
      const target = targetAt(targets, click);
      await waitUntilQuiet(app, 'before the click');
      await resetProbe(app);
      const timed = await timedClick(app, target);
      write('click', {
        target: target.id,
        longestMs: timed.longestMs,
        respondMs: timed.respondMs,
        ...(await readProbe(app)),
      });
    }
  }, 1_800_000);
});
