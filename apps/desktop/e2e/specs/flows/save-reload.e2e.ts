/**
 * Saving and opening again (docs/spec/05-project-format.md §5.2 F5, 04 §4.10): a new project is
 * saved with Ctrl+S (*Save as* the first time, answered by the dialog script), closed, and opened
 * again from the start page. The editor shows the same blocks, and saving the unchanged project
 * again writes the same bytes.
 */
import path from 'node:path';

import { By } from 'selenium-webdriver';
import { describe, expect, it } from 'vitest';

import { NodeIds, print, varGet } from '../../support/bdm';
import { currentDocument, dragBy, foldCodePanel } from '../../support/editor';
import { testIdText } from '../../support/ui';
import { waitFor } from '../../support/wait';
import { startFlow } from './lib/launch';
import {
  block,
  declare,
  fileStamp,
  moduleBlocks,
  newEmptyProject,
  openFromStartPage,
  printValue,
  ref,
  repeat,
  waitForWrite,
} from './lib/project';
import { chooseMenuItem, pressCtrl, UI_TIMEOUT_MS, waitForTestId } from './lib/ui';

/** Waits until the top bar shows no unsaved-changes marker. */
async function waitUntilSaved(driver: Parameters<typeof testIdText>[0]): Promise<void> {
  await waitFor(async () => !(await testIdText(driver, 'project-name')).includes('•'), {
    timeout: UI_TIMEOUT_MS,
    message: 'the project to have no unsaved changes',
  });
}

describe('Save and reload', () => {
  it('opens a saved project with the same blocks, and saves it again byte for byte', async (context) => {
    const flow = startFlow(context);
    const file = path.join(flow.folders.projects, 'Saved project.b2c');
    const app = await flow.launch({ dialogs: { saveAs: [file], open: [file] } });
    const { driver } = app;

    // (1) A project with a few blocks, and main moved on the canvas (positions are saved too).
    const mainId = await newEmptyProject(app);
    await foldCodePanel(app);
    const ids = new NodeIds('saved');
    await app.hook.insertBlocks(mainId, 'BODY', [
      declare(ids, {
        sym: 'sym_e2e_total',
        name: 'total',
        type: 'int',
        value: { expr: [{ num: '3' }] },
      }),
      print(ids, 'Saved and reopened'),
      repeat(ids, ref('sym_e2e_total'), [printValue(ids, block(varGet(ids, 'sym_e2e_total')))]),
    ]);
    await dragBy(app, mainId, 80, 40);
    await waitFor(async () => (await testIdText(driver, 'project-name')).includes('•'), {
      timeout: UI_TIMEOUT_MS,
      message: 'the unsaved-changes marker',
    });

    // (2) Ctrl+S on a project without a file is Save as.
    await pressCtrl(driver, 's');
    const firstBytes = await waitForWrite(file, null);
    await waitUntilSaved(driver);
    expect(await testIdText(driver, 'status-save')).toMatch(/Saved/);
    const saved = await currentDocument(app);

    // (3) Close it, and open it again from the start page.
    await chooseMenuItem(driver, 'Close project');
    await waitForTestId(driver, 'start-page', true);
    const recent = await driver.findElements(By.css('[data-testid="recent-item"]'));
    expect(recent.length).toBeGreaterThanOrEqual(1);
    const reopened = await openFromStartPage(app);
    expect(moduleBlocks(reopened)).toEqual(moduleBlocks(saved));
    await waitUntilSaved(driver);

    // (4) Saving the unchanged project writes the same bytes.
    const before = fileStamp(file);
    await pressCtrl(driver, 's');
    const secondBytes = await waitForWrite(file, before);
    expect(secondBytes.equals(firstBytes)).toBe(true);
  }, 180_000);
});
