/**
 * *for each line in file* built and run in the real app (docs/plans/m3-contracts/w2.md §1.8, §2.4,
 * §5.2, package D; the risk of next steps §3.5): a program runs in its project folder (07 §7.6.2)
 * and names its file with a path relative to it (decision M3-D3).
 *
 * - A new project is saved with Ctrl+S (*Save as*, answered by the dialog script) into a folder,
 *   and the fixture `lines.txt` is copied next to it.
 * - F5 builds and runs it: it prints each line of the file with its number, the empty line too,
 *   and finishes with exit code 0.
 * - With the file gone, F5 runs it again: its *if it can't be opened* part writes to the error
 *   stream and stops the program with exit code 1.
 * - Neither run adds, removes or changes a file in the project folder but the project's own.
 */
import { copyFileSync, readdirSync, rmSync } from 'node:fs';
import path from 'node:path';

import { Key } from 'selenium-webdriver';
import { describe, expect, it } from 'vitest';

import { type BlockInput, type BlockNode, type ExprInput, NodeIds } from '../../support/bdm';
import { waitForConsoleState } from '../../support/console';
import { foldCodePanel } from '../../support/editor';
import { pressKey } from '../../support/ui';
import { waitForTranscript } from './lib/console';
import { startFlow } from './lib/launch';
import {
  changeBy,
  declare,
  FIXTURES,
  newEmptyProject,
  printValue,
  ref,
  waitForWrite,
} from './lib/project';
import { BUILD_TIMEOUT_MS, pressCtrl, UI_TIMEOUT_MS, waitForToolchain } from './lib/ui';

/** The fixture the program reads, by the name it uses. */
const DATA = 'lines.txt';

/** The symbols of the line and the counter. */
const LINE = 'sym_e2e_line';
const COUNT = 'sym_e2e_count';

/** *print* of these tokens and values on one line. */
function printItems(ids: NodeIds, items: readonly (ExprInput | BlockInput)[]): BlockNode {
  const inputs = Object.fromEntries(items.map((item, index) => [`ITEM${String(index)}`, item]));
  return {
    id: ids.next(),
    type: 'io.print',
    v: 1,
    extra: { itemCount: items.length },
    fields: { NEWLINE: true, SEP: 'none', STREAM: 'out' },
    inputs,
  };
}

describe('Files', () => {
  it('reads a file from the project folder, and runs the part for a missing file', async (context) => {
    const flow = startFlow(context);
    const folder = flow.folders.projects;
    const file = path.join(folder, 'Lines.b2c');
    const app = await flow.launch({ dialogs: { saveAs: [file] } });
    await waitForToolchain(app);
    const mainId = await newEmptyProject(app);
    await foldCodePanel(app);
    const ids = new NodeIds('files');
    // count = 0; for each line in file "lines.txt" { change count by 1; print count ": " line }
    // if it can't be opened { print "Cannot open lines.txt" to the error stream; stop with 1 }
    const failed: BlockNode[] = [
      {
        id: ids.next(),
        type: 'io.print',
        v: 1,
        extra: { itemCount: 1 },
        fields: { NEWLINE: true, SEP: 'none', STREAM: 'err' },
        inputs: { ITEM0: { expr: [{ str: `Cannot open ${DATA}` }] } },
      },
      {
        id: ids.next(),
        type: 'program.exit',
        v: 1,
        inputs: { CODE: { expr: [{ num: '1' }] } },
      },
    ];
    await app.hook.insertBlocks(mainId, 'BODY', [
      declare(ids, { sym: COUNT, name: 'count', type: 'int', value: { expr: [{ num: '0' }] } }),
      {
        id: ids.next(),
        type: 'file.for_each_line',
        v: 1,
        extra: { hasFailed: true },
        fields: { LINE: { sym: LINE, name: 'line' } },
        inputs: { PATH: { expr: [{ str: DATA }] } },
        statements: {
          BODY: [
            changeBy(ids, COUNT, { expr: [{ num: '1' }] }),
            printItems(ids, [ref(COUNT), { expr: [{ str: ': ' }] }, ref(LINE)]),
          ],
          FAILED: failed,
        },
      },
      printValue(ids, { expr: [{ str: 'done' }] }),
    ]);

    // (1) Saved into the folder, with the file it reads beside it.
    await pressCtrl(app.driver, 's');
    await waitForWrite(file, null);
    copyFileSync(path.join(FIXTURES, DATA), path.join(folder, DATA));
    const before = readdirSync(folder).sort();

    // (2) F5 reads it line by line from the project folder.
    await pressKey(app.driver, Key.F5);
    await waitForTranscript(
      app,
      (text) => /1: first line\D+2: \D+3: third line\D+done/.test(text),
      BUILD_TIMEOUT_MS,
      'the three lines of the file and done',
    );
    await waitForConsoleState(app.driver, /Finished \(exit code 0\)$/, UI_TIMEOUT_MS);
    expect(readdirSync(folder).sort()).toEqual(before);

    // (3) Without the file, the part for a file that can't be opened runs and stops it with 1.
    rmSync(path.join(folder, DATA));
    await pressKey(app.driver, Key.F5);
    await waitForConsoleState(app.driver, /Finished with exit code 1$/, BUILD_TIMEOUT_MS);
    const transcript = await app.hook.consoleText();
    expect(transcript).toContain(`Cannot open ${DATA}`);
    expect(readdirSync(folder).sort()).toEqual(before.filter((name) => name !== DATA));
  }, 300_000);
});
