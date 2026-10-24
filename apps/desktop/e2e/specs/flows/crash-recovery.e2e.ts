/**
 * Crash recovery (docs/spec/05-project-format.md §5.10, 04 §4.10): while a project has unsaved
 * changes the app writes a recovery snapshot every 30 s into its own recovery folder; when the app
 * is killed, the next start offers the snapshot on the start page, and *Restore* brings back the
 * same blocks, still with unsaved changes.
 */
import { readdirSync, readFileSync } from 'node:fs';
import path from 'node:path';

import { By } from 'selenium-webdriver';
import { describe, expect, it } from 'vitest';

import { NodeIds, onlyTopBlock, parseDocument, print, statementsOf } from '../../support/bdm';
import { currentDocument, dragBy, foldCodePanel } from '../../support/editor';
import { testIdText } from '../../support/ui';
import { waitFor } from '../../support/wait';
import { startFlow } from './lib/launch';
import {
  declare,
  moduleBlocks,
  newEmptyProject,
  printValue,
  ref,
  waitForProject,
} from './lib/project';
import { UI_TIMEOUT_MS, waitForTestId } from './lib/ui';

/**
 * How long the snapshot with every edit may take: two autosave intervals (features/recovery:
 * `AUTOSAVE_INTERVAL_MS`, 30 s), in case a tick came while the test was still editing.
 */
const SNAPSHOT_TIMEOUT_MS = 70_000;

/** The text that marks this test's blocks. */
const MARK = 'B2C-RECOVER-ME';

/** Every `.b2c` snapshot below the recovery folder (`<recovery>/<instance>/<snapshot>.b2c`). */
function snapshotFiles(recovery: string): string[] {
  const found: string[] = [];
  let instances: string[];
  try {
    instances = readdirSync(recovery);
  } catch {
    return found;
  }
  for (const instance of instances) {
    try {
      for (const name of readdirSync(path.join(recovery, instance))) {
        if (name.endsWith('.b2c')) {
          found.push(path.join(recovery, instance, name));
        }
      }
    } catch {
      // Not a folder (the instance's lock file).
    }
  }
  return found;
}

describe('Crash recovery', () => {
  it('restores the autosaved blocks after the app was killed', async (context) => {
    const flow = startFlow(context);
    const recovery = path.join(flow.folders.profile, 'state', 'recovery');
    const ids = new NodeIds('crash');

    // (1) A new project with unsaved changes: blocks from the hook, and main moved by a real drag.
    const first = await flow.launch();
    const mainId = await newEmptyProject(first);
    await foldCodePanel(first);
    await first.hook.insertBlocks(mainId, 'BODY', [
      declare(ids, {
        sym: 'sym_e2e_count',
        name: 'count',
        type: 'int',
        value: { expr: [{ num: '7' }] },
      }),
      print(ids, MARK),
      printValue(ids, ref('sym_e2e_count')),
    ]);
    await dragBy(first, mainId, 64, 32);
    await waitFor(async () => (await testIdText(first.driver, 'project-name')).includes('•'), {
      timeout: UI_TIMEOUT_MS,
      message: 'the unsaved-changes marker',
    });
    const before = await currentDocument(first);
    expect(statementsOf(onlyTopBlock(before, 'program.main'), 'BODY')).toHaveLength(3);

    // (2) The autosave writes a snapshot with exactly these blocks into the recovery folder.
    await waitFor(
      () => {
        for (const file of snapshotFiles(recovery)) {
          const text = readFileSync(file, 'utf8');
          if (
            text.includes(MARK) &&
            JSON.stringify(moduleBlocks(parseDocument(text))) ===
              JSON.stringify(moduleBlocks(before))
          ) {
            return true;
          }
        }
        return null;
      },
      { timeout: SNAPSHOT_TIMEOUT_MS, interval: 500, message: 'the autosave snapshot' },
    );

    // (3) The app crashes.
    await first.kill();

    // (4) The next start offers the snapshot; Restore brings the same blocks back.
    const second = await flow.launch();
    await waitForTestId(second.driver, 'recovery-offer', true, UI_TIMEOUT_MS);
    const items = await second.driver.findElements(By.css('[data-testid="recovery-item"]'));
    expect(items).toHaveLength(1);
    const restore = await second.driver.findElement(
      By.css('[data-testid="recovery-item"] button[aria-label^="Restore"]'),
    );
    await restore.click();
    const restored = await waitForProject(second);
    expect(moduleBlocks(restored)).toEqual(moduleBlocks(before));
    // Restored work has not been saved to a file: it still has unsaved changes, and is trusted
    // (a never-saved project created in the app, 08 §8.3).
    await waitFor(async () => (await testIdText(second.driver, 'project-name')).includes('•'), {
      timeout: UI_TIMEOUT_MS,
      message: 'the unsaved-changes marker on the restored project',
    });
    expect(
      await second.driver.findElements(By.css('[data-testid="restricted-banner"]')),
    ).toHaveLength(0);
    expect(await second.hook.code()).toContain(MARK);
  }, 180_000);
});
