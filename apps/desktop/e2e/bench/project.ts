/**
 * Opening a project file in the app under test as a person does: *Open* on the start page, which
 * the scripted open dialog (`launchApp(context, { dialogs: { open: [file] } })`) answers with the
 * file. The project opens in Restricted Mode (it is not trusted), which changes nothing the
 * benchmarks and the visual diff look at.
 */
import { E2E_HOOK_NAME } from '../../src/e2e/contract';
import type { App } from '../support/app';
import { clickTestId } from '../support/ui';
import { waitFor } from '../support/wait';

/**
 * Runs in the webview (`executeScript(CODE_HAS_SCRIPT, hookName, text)`): whether the code the
 * editor shows contains `text`, without sending the code itself back (it is large at 5,000
 * blocks).
 */
export const CODE_HAS_SCRIPT = `
  const [hookName, text] = arguments;
  const hook = window[hookName];
  return typeof hook === 'object' && hook !== null && hook.code().includes(text);
`;

/** What {@link openProjectFile} waits for. */
export interface OpenedProject {
  /** A block of the project that must be on the canvas. */
  readonly blockId: string;
  /** Text the generated C++ must contain once the first preview has run. */
  readonly code: string;
  /** How long opening, rendering and the first preview may take, in milliseconds. */
  readonly timeout: number;
}

/** Whether the code the editor shows contains `text`. */
export async function codeHas(app: App, text: string): Promise<boolean> {
  const found: unknown = await app.driver.executeScript(CODE_HAS_SCRIPT, E2E_HOOK_NAME, text);
  return found === true;
}

/**
 * Clicks *Open* on the start page (the dialog script answers with the file), then waits until
 * `expected.blockId` is on the canvas and the preview's C++ contains `expected.code`.
 */
export async function openProjectFile(app: App, expected: OpenedProject): Promise<void> {
  const deadline = Date.now() + expected.timeout;
  const left = () => Math.max(1_000, deadline - Date.now());
  await clickTestId(app.driver, 'start-open');
  await waitFor(() => app.hook.blockElement(expected.blockId), {
    timeout: left(),
    interval: 250,
    message: `block ${expected.blockId} of the opened project on the canvas`,
  });
  await waitFor(() => codeHas(app, expected.code), {
    timeout: left(),
    interval: 250,
    message: `"${expected.code}" in the C++ of the opened project`,
  });
}
