/**
 * Renaming the project and the window's Undo (docs/spec/04-user-interface.md §4.1, 05 §5.3, §5.9;
 * D12): the name in the top bar turns into a field, where Escape cancels and Enter keeps the name;
 * the new name is an unsaved change that the C++ header comment follows; a plain Ctrl+S writes it
 * as `project.name` into the same file, also when pressed with the field still open (it keeps the
 * name first); the start page's recent list shows the new name, and opening the project from there
 * shows it again. A block move is undone with Ctrl+Z pressed
 * outside the canvas (on the Problems tab), and done again with Ctrl+Y.
 */
import { existsSync, readFileSync } from 'node:fs';
import path from 'node:path';

import { By, Key, type WebDriver } from 'selenium-webdriver';
import { describe, expect, it } from 'vitest';

import { onlyTopBlock } from '../../support/bdm';
import { currentDocument, dragBy, showProblems } from '../../support/editor';
import { clickTestId, testId, testIdText, textContent } from '../../support/ui';
import { waitFor } from '../../support/wait';
import { startFlow } from './lib/launch';
import { fileStamp, newEmptyProject, waitForProject, waitForWrite } from './lib/project';
import { chooseMenuItem, pressCtrl, UI_TIMEOUT_MS, waitForTestId } from './lib/ui';

/** The name the Empty template gives a project. */
const TEMPLATE_NAME = 'My Project';

/** The new name. */
const NEW_NAME = 'Space Invaders';

/** The name typed last and saved with Ctrl+S while the field is still open. */
const FINAL_NAME = 'Space Invaders II';

/** Waits until the top bar shows the unsaved-changes marker, or (with `false`) no marker. */
async function waitForMarker(driver: WebDriver, shown: boolean): Promise<void> {
  await waitFor(async () => (await testIdText(driver, 'project-name')).includes('•') === shown, {
    timeout: UI_TIMEOUT_MS,
    message: shown ? 'the unsaved-changes marker' : 'the project to have no unsaved changes',
  });
}

/**
 * Replaces all the text of the focused field with `text`, then presses `last` (Enter or Escape)
 * when given, as a person types: with key actions, which go to the focused element and leave its
 * caret alone (an element's own "send keys" may first move the caret to the end).
 */
async function retype(driver: WebDriver, text: string, last?: string): Promise<void> {
  await driver
    .actions({ async: true })
    .keyDown(Key.CONTROL)
    .sendKeys('a')
    .keyUp(Key.CONTROL)
    .sendKeys(text, ...(last === undefined ? [] : [last]))
    .perform();
}

/** The project name of a saved file. */
function savedName(file: string): unknown {
  const value = JSON.parse(readFileSync(file, 'utf8')) as { project?: { name?: unknown } };
  return value.project?.name;
}

/** Runs in the webview: the C++ panel's first drawn line (the header comment). */
const FIRST_CODE_LINE = `
  const line = document.querySelector('[data-testid="code-panel"] .cm-line');
  return line === null ? '' : (line.textContent ?? '');
`;

describe('Renaming the project', () => {
  it('renames, saves to the same file, and reopens from the recent list with the new name', async (context) => {
    const flow = startFlow(context);
    const file = path.join(flow.folders.projects, 'Arcade.b2c');
    const app = await flow.launch({ dialogs: { saveAs: [file] } });
    const { driver } = app;

    // (1) A new project, saved once: Save as chooses the file's name. A new template project has
    // no unsaved changes, so the marker says nothing about the save; the status bar's "Saved"
    // shows that the Save as has answered, before (4) saves again right after an edit.
    await newEmptyProject(app);
    await pressCtrl(driver, 's');
    await waitForWrite(file, null);
    await waitFor(async () => (await testIdText(driver, 'status-save')).startsWith('Saved'), {
      timeout: UI_TIMEOUT_MS,
      message: 'the status bar to say the project is saved',
    });
    await waitForMarker(driver, false);
    expect(savedName(file)).toBe(TEMPLATE_NAME);

    // (2) Clicking the name opens the field with the name selected; Escape cancels.
    await clickTestId(driver, 'project-name-button');
    const cancelled = await testId(driver, 'project-name-input');
    expect(await cancelled.getAttribute('value')).toBe(TEMPLATE_NAME);
    await retype(driver, 'Not this', Key.ESCAPE);
    await waitForTestId(driver, 'project-name-input', false);
    expect(await testIdText(driver, 'project-name-text')).toBe(TEMPLATE_NAME);
    expect(await testIdText(driver, 'project-name')).not.toContain('•');

    // (3) ≡ Rename project… opens it too; Enter keeps the new name, an unsaved change that the
    // C++ header comment follows.
    await chooseMenuItem(driver, 'Rename project…');
    await testId(driver, 'project-name-input');
    await retype(driver, NEW_NAME, Key.ENTER);
    await waitForTestId(driver, 'project-name-input', false);
    expect(await testIdText(driver, 'project-name-text')).toBe(NEW_NAME);
    await waitForMarker(driver, true);
    await waitFor(
      async () => {
        const line: unknown = await driver.executeScript(FIRST_CODE_LINE);
        return typeof line === 'string' && line.includes(`from project "${NEW_NAME}"`);
      },
      { timeout: UI_TIMEOUT_MS, message: 'the header comment to name the new project' },
    );

    // (4) A plain Ctrl+S writes the name into the same file.
    const before = fileStamp(file);
    await pressCtrl(driver, 's');
    await waitForWrite(file, before);
    await waitForMarker(driver, false);
    expect(savedName(file)).toBe(NEW_NAME);
    expect(existsSync(path.join(flow.folders.projects, `${NEW_NAME}.b2c`))).toBe(false);

    // (4b) Ctrl+S with the field still open keeps the name first: the file gets the name shown,
    // and nothing is left unsaved.
    await clickTestId(driver, 'project-name-button');
    await testId(driver, 'project-name-input');
    const typed = fileStamp(file);
    await retype(driver, FINAL_NAME);
    await pressCtrl(driver, 's');
    await waitForWrite(file, typed);
    await waitForTestId(driver, 'project-name-input', false);
    await waitForMarker(driver, false);
    expect(savedName(file)).toBe(FINAL_NAME);
    expect(await testIdText(driver, 'project-name-text')).toBe(FINAL_NAME);

    // (5) The recent list names the project by its new name; it opens from there.
    await chooseMenuItem(driver, 'Close project');
    await waitForTestId(driver, 'start-page', true);
    const entry = await waitFor(
      async () => {
        for (const item of await driver.findElements(By.css('[data-testid="recent-item"]'))) {
          const text = await textContent(driver, item);
          if (text.includes(FINAL_NAME) && text.includes('Arcade.b2c')) {
            return item;
          }
        }
        return null;
      },
      { timeout: UI_TIMEOUT_MS, message: `a recent project named "${FINAL_NAME}"` },
    );
    expect(await textContent(driver, entry)).not.toContain(TEMPLATE_NAME);
    await entry.findElement(By.css('.recent-open')).click();
    await waitForProject(app);
    expect(await testIdText(driver, 'project-name-text')).toBe(FINAL_NAME);
    expect(await testIdText(driver, 'project-name')).not.toContain('•');
  }, 180_000);

  it('undoes a block move with Ctrl+Z outside the canvas, and redoes it with Ctrl+Y', async (context) => {
    const flow = startFlow(context);
    const app = await flow.launch();
    const { driver } = app;
    const mainId = await newEmptyProject(app);
    const mainX = async (): Promise<number | undefined> =>
      onlyTopBlock(await currentDocument(app), 'program.main').x;
    const start = await mainX();

    await dragBy(app, mainId, 120, 0);
    await waitForMarker(driver, true);
    const moved = await mainX();
    expect(moved).not.toBe(start);

    // The focus on the Problems tab: Blockly does not see the key there, the window does.
    await showProblems(driver);
    await pressCtrl(driver, 'z');
    await waitFor(async () => (await mainX()) === start, {
      timeout: UI_TIMEOUT_MS,
      message: 'the move to be undone',
    });
    await waitForMarker(driver, false);

    await pressCtrl(driver, 'y');
    await waitFor(async () => (await mainX()) === moved, {
      timeout: UI_TIMEOUT_MS,
      message: 'the move to be redone',
    });
    await waitForMarker(driver, true);
  }, 180_000);
});
