/**
 * *Copy bug report* (docs/plans/m3-contracts/w4-editor-tools.md §2.7): what the report holds with
 * and without the user's tick, and the dialog.
 */
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';

import { diagnosticFixture, documentFixture, toolchainFixture } from '../../app/testing/fixtures';
import { expectNoAxeViolations } from '../../test/axe';
import { BugReportDialog } from './BugReportDialog';
import {
  bugReport,
  sanitisedMessage,
  topBlockOf,
  withoutPaths,
  type BugReportInput,
} from './report';

const RAW = [
  "/home/ada/projects/Secret Game/.b2c/build/main.cpp: In function 'int main()':",
  "/home/ada/projects/Secret Game/.b2c/build/main.cpp:7:5: error: 'secretScore' was not declared in this scope",
  '    7 |     secretScore += 1;',
  '      |     ^~~~~~~~~~~',
  "C:\\Users\\Ada\\game\\main.cpp:3:1: note: suggested alternative: 'score'",
].join('\n');

function input(): BugReportInput {
  const document = documentFixture('Secret Game');
  const main = document.modules[0];
  if (main !== undefined) {
    main.workspace.blocks = [
      {
        id: 'top',
        type: 'program.main',
        v: 1,
        statements: { BODY: [{ id: 'inner', type: 'control.break', v: 1 }] },
      },
    ];
  }
  return {
    appInfo: { appVersion: '0.3.0', ipcVersion: 1, platform: 'linux', catalogVersion: '1.3.0' },
    toolchain: toolchainFixture({ flavor: 'gcc', version: '13.3.0', target: 'x86_64-linux-gnu' }),
    standard: 'c++20',
    configuration: 'debug',
    diagnostic: diagnosticFixture({
      code: 'C:error',
      source: 'compiler',
      message: 'This looks like a bug in Blocks2Cpp. g++ reported a problem.',
      raw: RAW,
      primary: { module: 'mod_main', block: 'inner', part: { kind: 'whole' } },
    }),
    document,
    files: [
      {
        path: 'main.cpp',
        kind: 'source',
        module: 'mod_main',
        contents: 'int main() { int secretScore = 0; }\n',
      },
    ],
  };
}

describe('the report', () => {
  it('holds no project text, name or path without the tick', () => {
    const text = bugReport(input(), { includeProject: false });
    expect(text).toContain('App: 0.3.0 (catalog 1.3.0, linux)');
    expect(text).toContain('Compiler: gcc 13.3.0 x86_64-linux-gnu');
    expect(text).toContain('C++ standard: c++20; configuration: debug');
    expect(text).toContain('Diagnostic: C:error (compiler, error)');
    expect(text).toContain('was not declared in this scope');
    for (const secret of [
      'secretScore',
      'Secret Game',
      'ada',
      'Ada',
      'score',
      '/home',
      'C:\\',
      '|',
    ]) {
      expect(text, secret).not.toContain(secret);
    }
    expect(text).toContain('The blocks and the generated C++ are not included.');
  });

  it('adds the blocks, the generated C++ and the whole message with the tick, still without paths', () => {
    const text = bugReport(input(), { includeProject: true });
    expect(text).toContain('secretScore += 1;');
    expect(text).toContain('"type": "program.main"');
    expect(text).toContain('Generated C++ (main.cpp):');
    expect(text).toContain('int main() { int secretScore = 0; }');
    expect(text).not.toContain('/home/ada');
    expect(text).not.toContain('C:\\Users');
    expect(text).not.toContain('Secret Game');
    expect(text).not.toContain('Game/');
    expect(text).toContain('<path>/main.cpp:7:5');
  });

  it('replaces paths and quoted names, and drops source lines', () => {
    expect(withoutPaths('see /usr/include/c++/13/vector:12 and D:\\x\\y.cpp')).toBe(
      'see <path>:12 and <path>',
    );
    expect(sanitisedMessage("Game.cpp:1:1: error: ‘x’ and 'y'\n    1 | int x;\n      | ^")).toBe(
      '<file>:1:1: error: ‘…’ and ‘…’',
    );
  });

  it('replaces a path with spaces that g++ prints partway through a line', () => {
    const crashes = [
      'Preprocessed source stored into /home/jo ann/.cache/blocks2cpp/builds/prj_x-1a2b/debug-12345678/tmp/ccQ1w2e3.out file, please attach this to your bugreport.',
      'Preprocessed source stored into C:\\Users\\Alice Smith\\AppData\\Local\\blocks2cpp\\builds\\ab12\\tmp\\ccAb.out file, please attach this to your bugreport.',
    ];
    for (const line of crashes) {
      expect(sanitisedMessage(line)).toBe(
        'Preprocessed source stored into <path> file, please attach this to your bugreport.',
      );
      const text = bugReport(
        { ...input(), diagnostic: { ...input().diagnostic, raw: line } },
        { includeProject: false },
      );
      for (const secret of ['ann', 'Smith', '.cache', 'AppData']) {
        expect(text, secret).not.toContain(secret);
      }
    }
    // Two paths with a word between them stay two.
    expect(withoutPaths('cannot find /usr/lib/x.so and /lib/y')).toBe(
      'cannot find <path> and <path>',
    );
  });

  it('finds the top block of a nested one', () => {
    expect(topBlockOf(input().document ?? documentFixture(), 'inner')?.id).toBe('top');
    expect(topBlockOf(documentFixture(), 'nowhere')).toBeNull();
  });
});

describe('the dialog', () => {
  it('shows the report without the project, adds it when ticked, and copies what it shows', async () => {
    const copy = vi.fn<(text: string) => Promise<boolean>>(() => Promise.resolve(true));
    const onClose = vi.fn();
    render(<BugReportDialog input={input()} onClose={onClose} copy={copy} />);
    const shown = screen.getByTestId('bug-report-text');
    expect(shown.textContent).not.toContain('secretScore');
    await expectNoAxeViolations(screen.getByTestId('bug-report-dialog'));
    fireEvent.click(screen.getByRole('checkbox', { name: /Include my blocks/u }));
    expect(screen.getByTestId('bug-report-text').textContent).toContain('secretScore');
    fireEvent.click(screen.getByRole('button', { name: 'Copy' }));
    await waitFor(() => {
      expect(screen.getByRole('status').textContent).toBe('The report is on the clipboard.');
    });
    expect(copy).toHaveBeenCalledWith(screen.getByTestId('bug-report-text').textContent);
    fireEvent.click(screen.getByRole('button', { name: 'Close' }));
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it('says so when the text could not be copied', async () => {
    render(
      <BugReportDialog input={input()} onClose={vi.fn()} copy={() => Promise.resolve(false)} />,
    );
    fireEvent.click(screen.getByRole('button', { name: 'Copy' }));
    await waitFor(() => {
      expect(screen.getByRole('status').textContent).toMatch(/could not be copied/u);
    });
  });
});
