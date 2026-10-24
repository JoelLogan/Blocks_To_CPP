/**
 * The open project's file changed by another program (docs/spec/05-project-format.md §5.10,
 * 04 §4.10): the app notices and asks. *Reload* shows the file's new blocks; *Keep mine (save as…)*
 * keeps the editor's blocks and saves them to a new file, leaving the changed file alone. A file
 * that was deleted cannot be reloaded, so only *Keep mine* is offered.
 */
import { readFileSync, rmSync, writeFileSync } from 'node:fs';
import path from 'node:path';

import { By } from 'selenium-webdriver';
import { describe, expect, it } from 'vitest';

import { NodeIds, print } from '../../support/bdm';
import { testIdText } from '../../support/ui';
import { waitFor } from '../../support/wait';
import { type FlowApp, startFlow } from './lib/launch';
import { newEmptyProject, waitForWrite } from './lib/project';
import { answerDialog, DIALOG, pressCtrl, UI_TIMEOUT_MS, waitForDialog } from './lib/ui';

const ORIGINAL = 'B2C-ORIGINAL-TEXT';
const OUTSIDE = 'B2C-CHANGED-OUTSIDE';
const AGAIN = 'B2C-CHANGED-AGAIN';
const MINE = 'B2C-MY-OWN-CHANGE';

/** How long the app may take to notice a change (a 300 ms debounce, at most 2 s, 05 §5.10). */
const NOTICE_TIMEOUT_MS = 15_000;

/** Changes a project file as another program would: `from` becomes `to` in its text. */
function changeOutside(file: string, from: string, to: string): void {
  const text = readFileSync(file, 'utf8');
  expect(text).toContain(from);
  writeFileSync(file, text.replace(from, to));
}

/** Waits until the editor's document has `text`. */
async function waitForBlocksWith(app: FlowApp, text: string): Promise<void> {
  await waitFor(async () => (await app.hook.documentText()).includes(text), {
    timeout: UI_TIMEOUT_MS,
    message: `the editor's blocks to have "${text}"`,
  });
}

/** Whether the top bar shows unsaved changes. */
async function dirty(app: FlowApp): Promise<boolean> {
  return (await testIdText(app.driver, 'project-name')).includes('•');
}

describe('External change', () => {
  it('reloads the changed file, keeps mine with Save as, and offers only Keep mine for a deleted file', async (context) => {
    const flow = startFlow(context);
    const file = path.join(flow.folders.projects, 'Shared.b2c');
    const mine = path.join(flow.folders.projects, 'Mine.b2c');
    const rescued = path.join(flow.folders.projects, 'Rescued.b2c');
    const app = await flow.launch({ dialogs: { saveAs: [file, mine, rescued] } });
    const { driver } = app;

    const mainId = await newEmptyProject(app);
    const ids = new NodeIds('external');
    await app.hook.insertBlocks(mainId, 'BODY', [print(ids, ORIGINAL)]);
    await pressCtrl(driver, 's');
    await waitForWrite(file, null);
    await waitFor(async () => !(await dirty(app)), {
      timeout: UI_TIMEOUT_MS,
      message: 'the project to be saved',
    });

    // (1) Changed outside, no unsaved changes here: Reload shows the file's blocks.
    changeOutside(file, ORIGINAL, OUTSIDE);
    expect(
      await waitForDialog(driver, /was changed outside Blocks2Cpp/, NOTICE_TIMEOUT_MS),
    ).toContain('Shared.b2c');
    await answerDialog(driver, 'Reload');
    await waitForBlocksWith(app, OUTSIDE);
    expect(await app.hook.documentText()).not.toContain(ORIGINAL);
    expect(await dirty(app)).toBe(false);
    // Only a change to Raw C++, libraries, packs or defines restricts it (08 §8.3); this is not one.
    expect(await driver.findElements(By.css('[data-testid="restricted-banner"]'))).toHaveLength(0);

    // (2) Changed outside again while the editor has its own change: Keep mine saves the editor's
    // blocks as a new file and leaves the changed file as it is.
    await app.hook.insertBlocks(mainId, 'BODY', [print(ids, MINE)]);
    await waitFor(() => dirty(app), { timeout: UI_TIMEOUT_MS, message: 'the unsaved change' });
    changeOutside(file, OUTSIDE, AGAIN);
    expect(
      await waitForDialog(driver, /Reloading discards the changes/, NOTICE_TIMEOUT_MS),
    ).toMatch(/was changed outside Blocks2Cpp/);
    await answerDialog(driver, 'Keep mine (save as…)');
    const kept = (await waitForWrite(mine, null)).toString('utf8');
    expect(kept).toContain(MINE);
    expect(kept).toContain(OUTSIDE);
    expect(kept).not.toContain(AGAIN);
    expect(readFileSync(file, 'utf8')).toContain(AGAIN);
    expect(readFileSync(file, 'utf8')).not.toContain(MINE);
    await waitFor(async () => !(await dirty(app)), {
      timeout: UI_TIMEOUT_MS,
      message: 'the project to be saved under the new name',
    });
    await waitForBlocksWith(app, MINE);

    // (3) The new file is deleted: it cannot be reloaded, so only Keep mine is offered.
    rmSync(mine);
    expect(await waitForDialog(driver, /was deleted or moved/, NOTICE_TIMEOUT_MS)).toContain(
      'Mine.b2c',
    );
    const buttons = await driver.findElements(By.css(`${DIALOG} button`));
    const labels = await Promise.all(buttons.map((button) => button.getText()));
    expect(labels).not.toContain('Reload');
    await answerDialog(driver, 'Keep mine (save as…)');
    expect((await waitForWrite(rescued, null)).toString('utf8')).toContain(MINE);
  }, 180_000);
});
