/**
 * A negative number typed into a block (docs/spec/03-block-language.md §3.4): a countdown whose
 * `change n by (1)` gets `-1` typed into its number. The number is saved as it was typed, as one
 * `num` token; the analyser accepts it, so there are no problems and Run is not held back; the C++
 * panel shows `n += -1;`; and the program counts down when it runs.
 */
import { Key } from 'selenium-webdriver';
import { describe, expect, it } from 'vitest';

import { NodeIds, statementsOf } from '../../support/bdm';
import { waitForConsoleState } from '../../support/console';
import { fieldOf, runHeldBack, waitForErrors } from '../../support/editor';
import { pressKey, typeIntoField } from '../../support/ui';
import { waitFor } from '../../support/wait';
import { waitForTranscript } from './lib/console';
import { startFlow } from './lib/launch';
import {
  changeBy,
  declare,
  mainBody,
  newEmptyProject,
  printValue,
  ref,
  repeat,
} from './lib/project';
import { BUILD_TIMEOUT_MS, UI_TIMEOUT_MS, waitForToolchain } from './lib/ui';

/** The counter's symbol. */
const COUNTER = 'sym_e2e_n';

describe('Countdown', () => {
  it('runs a countdown whose change block has -1 typed into it', async (context) => {
    const flow = startFlow(context);
    const app = await flow.launch();
    await waitForToolchain(app);
    const mainId = await newEmptyProject(app);
    const ids = new NodeIds('countdown');
    // n = 3; repeat 3 times { print n; change n by 1 }; print "Liftoff!"
    const change = changeBy(ids, COUNTER, { expr: [{ num: '1' }] });
    await app.hook.insertBlocks(mainId, 'BODY', [
      declare(ids, { sym: COUNTER, name: 'n', type: 'int', value: { expr: [{ num: '3' }] } }),
      repeat(ids, { expr: [{ num: '3' }] }, [printValue(ids, ref(COUNTER)), change]),
      printValue(ids, { expr: [{ str: 'Liftoff!' }] }),
    ]);

    // (1) Type -1 into the change block's number, as a person does.
    await typeIntoField(app.driver, await fieldOf(app, change.id, 'VALUE', 'BY'), '-1');
    const saved = await waitFor(
      async () => {
        const loop = (await mainBody(app))[1];
        const typed = loop === undefined ? undefined : statementsOf(loop, 'BODY')[1];
        const by = typed?.inputs?.['BY'];
        return by !== undefined && 'expr' in by && by.expr[0]?.['num'] === '-1' ? by : null;
      },
      { timeout: UI_TIMEOUT_MS, message: 'the typed -1 in the project' },
    );
    expect(saved).toEqual({ expr: [{ num: '-1' }] });

    // (2) The analyser accepts it: no problems, Run is not held back, and the C++ subtracts.
    await waitForErrors(app, 0);
    expect(await runHeldBack(app.driver)).toBe(false);
    await waitFor(async () => ((await app.hook.code()).includes('n += -1;') ? true : null), {
      timeout: UI_TIMEOUT_MS,
      message: async () => `n += -1; in the C++ panel (it has:\n${await app.hook.code()})`,
    });

    // (3) F5 runs it, and it counts down (adding 1 would print 3, 4, 5). Windows' pseudoconsole
    // may repaint lines, so the numbers are looked for in order rather than line by line.
    await pressKey(app.driver, Key.F5);
    const countdown = /3\D+2\D+1\D+Liftoff!/;
    await waitForTranscript(app, (text) => countdown.test(text), BUILD_TIMEOUT_MS, 'the countdown');
    await waitForConsoleState(app.driver, /Finished \(exit code 0\)$/, UI_TIMEOUT_MS);
  }, 180_000);
});
