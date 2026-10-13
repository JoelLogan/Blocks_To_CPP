/**
 * Opening a project file in the app under test as a person does: *Open* on the start page, which
 * the scripted open dialog (`launchApp(context, { dialogs: { open: [file] } })`) answers with the
 * file. The project opens in Restricted Mode (it is not trusted), which changes nothing the
 * benchmarks and the visual diff look at.
 *
 * A build that cannot read every block of the file shows the ones it cannot read as placeholders
 * (03 §3.11), and works on another program than the file's: opening such a file fails (an A/B
 * run's base older than the head's benchmark documents, README.md "A/B runs"), so its numbers are
 * never compared with a build that read the whole file.
 */
import { E2E_HOOK_NAME } from '../../src/e2e/contract';
import type { App } from '../support/app';
import { byTestId, clickTestId } from '../support/ui';
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

/** The Blockly type of a placeholder block (`PLACEHOLDER_TYPE` of `@blocks2cpp/blockly-ext`). */
export const PLACEHOLDER_BLOCK_TYPE = 'b2c.placeholder';

/**
 * Runs in the webview (`executeScript(PLACEHOLDERS_SCRIPT)`): how many placeholder blocks the
 * editor shows. Blockly gives each block's SVG group its type as a class, in every build.
 */
export const PLACEHOLDERS_SCRIPT = `
  return document.querySelectorAll('g[class~="${PLACEHOLDER_BLOCK_TYPE}"]').length;
`;

/** The project opened in the app is not the file's program: the app could not read every block. */
export class UnreadBlocksError extends Error {
  override readonly name = 'UnreadBlocksError';
}

/**
 * Throws {@link UnreadBlocksError} when the opened project shows `placeholders` placeholder blocks.
 */
export function requireEveryBlockRead(placeholders: number): void {
  if (placeholders > 0) {
    throw new UnreadBlocksError(
      `The opened project shows ${String(placeholders)} placeholder blocks: this build cannot ` +
        'read every block of the file (its block catalog is older than the file needs), so it ' +
        'would measure another program. An A/B run needs a base that reads the head’s ' +
        'benchmark documents.',
    );
  }
}

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

/** How long the window may take to show the start page or, without a usable g++, the setup page. */
const FIRST_PAGE_TIMEOUT_MS = 30_000;

/**
 * Goes to the start page. Without a usable g++ the app opens the toolchain setup page first; the
 * benchmarks build nothing, so they go back from it, as a person can.
 */
async function toStartPage(app: App): Promise<void> {
  const first = await waitFor(
    async () => {
      for (const id of ['start-open', 'toolchain-page'] as const) {
        if ((await app.driver.findElements(byTestId(id))).length > 0) {
          return id;
        }
      }
      return null;
    },
    { timeout: FIRST_PAGE_TIMEOUT_MS, interval: 250, message: 'the start page or the setup page' },
  );
  if (first === 'toolchain-page') {
    process.stderr.write('No usable g++: going back from the setup page to the start page\n');
    await clickTestId(app.driver, 'feature-page-back');
  }
}

/**
 * Clicks *Open* on the start page (the dialog script answers with the file), then waits until
 * `expected.blockId` is on the canvas and the preview's C++ contains `expected.code`.
 *
 * @throws UnreadBlocksError when the app shows a block of the file as a placeholder.
 */
export async function openProjectFile(app: App, expected: OpenedProject): Promise<void> {
  const deadline = Date.now() + expected.timeout;
  const left = () => Math.max(1_000, deadline - Date.now());
  await toStartPage(app);
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
  const placeholders: unknown = await app.driver.executeScript(PLACEHOLDERS_SCRIPT);
  requireEveryBlockRead(typeof placeholders === 'number' ? placeholders : 0);
}
