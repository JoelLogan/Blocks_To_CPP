/**
 * A block left loose on the canvas is parked (docs/adr/0012-parked-blocks.md, 03 §3.3, 04 §4.4):
 * a *print* dragged from the toolbox onto an empty part of the canvas is listed in Problems as one
 * warning (`B2C-W0505`), not an error, so ▶ Run stays enabled; the C++ panel's code (read from the
 * test hook) has only the attached *print*, and F5 runs the program without the parked block. An
 * *ask* from the toolbox, whose variable is not chosen yet, is parked the same way: only warnings,
 * and Run is still not held back.
 */
import { By, Key } from 'selenium-webdriver';
import { describe, expect, it } from 'vitest';

import { type BlockNode, NodeIds, type ProjectDocument } from '../../support/bdm';
import { waitForConsoleState } from '../../support/console';
import {
  currentDocument,
  foldCodePanel,
  problemsSummary,
  runHeldBack,
  showProblems,
} from '../../support/editor';
import { pressKey, textContent } from '../../support/ui';
import { waitFor } from '../../support/wait';
import { waitForTranscript } from './lib/console';
import { dropOnCanvas } from './lib/editor';
import { type FlowApp, startFlow } from './lib/launch';
import { mainBody, newEmptyProject, printValue } from './lib/project';
import { BUILD_TIMEOUT_MS, UI_TIMEOUT_MS, waitForToolchain } from './lib/ui';

/** What the attached *print* writes. */
const ATTACHED = 'Attached and running';

/** What a *print* from the toolbox writes (its preset, `io.print`'s default item). */
const TOOLBOX_PRINT = 'Hello, world!';

/** Waits for a *print* directly on the canvas (a top-level block), and returns it. */
function loosePrint(app: FlowApp): Promise<BlockNode> {
  const find = (doc: ProjectDocument) =>
    (doc.modules[0]?.workspace.blocks ?? []).find((block) => block.type === 'io.print') ?? null;
  return waitFor(async () => find(await currentDocument(app)), {
    timeout: UI_TIMEOUT_MS,
    message: 'a print directly on the canvas',
  });
}

describe('Parked blocks', () => {
  it('lets Run go ahead with a print left loose on the canvas', async (context) => {
    const flow = startFlow(context);
    const app = await flow.launch();
    await waitForToolchain(app);
    const mainId = await newEmptyProject(app);
    // The C++ is read from the test hook, so the panel is folded away to give the canvas room.
    await foldCodePanel(app);
    await app.hook.insertBlocks(mainId, 'BODY', [
      printValue(new NodeIds('parked'), { expr: [{ str: ATTACHED }] }),
    ]);

    // (1) A print dragged from the toolbox onto an empty part of the canvas stays loose: parked.
    await dropOnCanvas(app, { category: 'Input / Output', type: 'io.print' });
    await loosePrint(app);
    expect(await mainBody(app)).toHaveLength(1);

    // (2) Problems lists it as one warning, with the parked block's message, and no error.
    await showProblems(app.driver);
    await waitFor(async () => (await problemsSummary(app.driver)) === '1 warning', {
      timeout: UI_TIMEOUT_MS,
      message: async () =>
        `"1 warning" in Problems (it says "${await problemsSummary(app.driver)}")`,
    });
    const rows = await app.driver.findElements(By.css('[data-testid="problem-row"]'));
    expect(rows).toHaveLength(1);
    const [row] = rows;
    if (row === undefined) {
      throw new Error('Problems has no row');
    }
    expect(await textContent(app.driver, row)).toContain(
      "This block isn't attached to 'when program starts' or a function",
    );

    // (3) Run is not held back, and the C++ has only the attached print.
    expect(await runHeldBack(app.driver)).toBe(false);
    const code = await waitFor(
      async () => {
        const text = await app.hook.code();
        return text.includes(ATTACHED) ? text : null;
      },
      { timeout: UI_TIMEOUT_MS, message: `"${ATTACHED}" in the C++ panel` },
    );
    expect(code).not.toContain(TOOLBOX_PRINT);

    // (4) F5 builds and runs it: the attached print runs, the parked one does not.
    await pressKey(app.driver, Key.F5);
    const transcript = await waitForTranscript(
      app,
      (text) => text.includes(ATTACHED),
      BUILD_TIMEOUT_MS,
      'the attached print',
    );
    await waitForConsoleState(app.driver, /Finished \(exit code 0\)$/, UI_TIMEOUT_MS);
    expect(transcript).not.toContain(TOOLBOX_PRINT);

    // (5) An ask from the toolbox has no variable chosen yet (the editor saves it without one). Left
    // loose, it is parked too: one warning for each loose stack, no error, and Run is not held back.
    await dropOnCanvas(app, { category: 'Input / Output', type: 'io.ask' });
    const stacks = await waitFor(
      async () => {
        const blocks = (await currentDocument(app)).modules[0]?.workspace.blocks ?? [];
        const loose = blocks.filter((block) => block.type !== 'program.main');
        const ask = loose
          .flatMap((block) => [block, ...(block.stack ?? [])])
          .find((block) => block.type === 'io.ask');
        if (ask === undefined) {
          return null;
        }
        expect(ask.fields?.['VAR']).toBeUndefined();
        return loose.length;
      },
      { timeout: UI_TIMEOUT_MS, message: 'an ask left loose on the canvas' },
    );
    const warnings = stacks === 1 ? '1 warning' : `${String(stacks)} warnings`;
    await showProblems(app.driver);
    await waitFor(async () => (await problemsSummary(app.driver)) === warnings, {
      timeout: UI_TIMEOUT_MS,
      message: async () =>
        `"${warnings}" in Problems (it says "${await problemsSummary(app.driver)}")`,
    });
    expect(await runHeldBack(app.driver)).toBe(false);
  }, 180_000);
});
