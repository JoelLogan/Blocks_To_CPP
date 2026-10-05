/**
 * The connected dock panels: the C++ tab follows the preview, the hovered and selected blocks and
 * the diagnostics, and selects blocks on a click; Problems lists the live and build diagnostics
 * with block paths, selects and centres blocks, and opens the diagnostics reference.
 */
import type { PreviewResult } from '@blocks2cpp/b2c-core-wasm';
import type { Diagnostic, MappedRange } from '@blocks2cpp/ipc-types';
import { EditorView } from '@codemirror/view';
import { act, fireEvent, render, screen, within } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { providePathCatalog } from '../editor/diagnostics/catalog';
import { BLOCKLY_EXT_PATH_CATALOG } from '../editor/diagnostics/plugin';
import {
  blockById,
  buildBlocks,
  disposeWorkspaces,
  editorHandle,
  guessingGame,
  renderedWorkspace,
} from '../editor/diagnostics/testing';
import { setIpcForTests } from '../lib/ipc';
import { expectNoAxeViolations } from '../test/axe';
import { type SelectBlockOptions, setEditorHandle } from './editor-types';
import { ConnectedCodePanel, ConnectedProblemsPanel, DockPanels } from './panels';
import { resetAppStore, useAppStore } from './store';
import { createFakeIpc, previewFixture, projectFixture } from './testing/fixtures';

const MAIN_LINES = [
  '#include "b2c_support.hpp"',
  '',
  'int main() {',
  '    int guess = 0;',
  '    while (!(guess == secret)) {',
  '        guess = b2c::ask<int>("Your guess: ");',
  '    }',
  '}',
];
const MAIN_CPP = `${MAIN_LINES.join('\n')}\n`;

/** The whole of lines `from` to `to` (1-based) as a range of `block`. */
function lines(from: number, to: number, block: string): MappedRange {
  const last = MAIN_LINES[to - 1] ?? '';
  return {
    start: { line: from, column: 1 },
    end: { line: to, column: new TextEncoder().encode(last).length + 1 },
    module: 'mod_main',
    block,
    part: { kind: 'whole' },
  };
}

/** A preview of part of the guessing game: main (b011), guess (b003), the loop (b010), ask (b005). */
function preview(diagnostics: Diagnostic[] = []): PreviewResult {
  return {
    ...previewFixture(diagnostics),
    files: [
      { path: 'main.cpp', kind: 'source', contents: MAIN_CPP },
      { path: 'ide/b2c_ide_init.cpp', kind: 'source', contents: '// IDE\n' },
    ],
    sourceMap: {
      version: 1,
      files: [
        {
          path: 'main.cpp',
          ranges: [
            lines(3, 8, 'b011'),
            lines(4, 4, 'b003'),
            lines(5, 7, 'b010'),
            lines(6, 6, 'b005'),
          ],
        },
      ],
    },
    buildable: true,
  };
}

function liveError(block: string, code = 'B2C-E0201'): Diagnostic {
  return {
    code,
    severity: 'error',
    message: 'There is no variable called "gues" here.',
    primary: { module: 'mod_main', block, part: { kind: 'whole' } },
    source: 'analyser',
  };
}

function compilerMessage(block: string): Diagnostic {
  return {
    code: 'C:error',
    severity: 'error',
    message: 'The compiler found a problem here.',
    primary: { block, part: { kind: 'whole' } },
    source: 'compiler',
    raw: "main.cpp:4:9: error: invalid conversion from 'int'",
  };
}

const HASH = 'a'.repeat(64);

/** The guessing game's canvas behind an editor handle that records what it selects. */
function connectEditor() {
  const workspace = renderedWorkspace();
  buildBlocks(workspace, guessingGame().modules[0]?.workspace.blocks ?? []);
  const selected: [string, SelectBlockOptions | undefined][] = [];
  setEditorHandle(
    editorHandle(workspace, (id, opts) => {
      selected.push([id, opts]);
    }),
  );
  return { workspace, selected };
}

function codeView(container: HTMLElement): EditorView {
  const element = container.querySelector<HTMLElement>('.cm-editor');
  const view = element === null ? null : EditorView.findFromDOM(element);
  if (view === null) {
    throw new Error('no code view');
  }
  return view;
}

function highlighted(container: HTMLElement): string {
  return [...container.querySelectorAll('.cm-b2c-block-highlight')]
    .map((element) => element.textContent)
    .join('');
}

function setState(update: () => void): void {
  act(update);
}

beforeEach(() => {
  resetAppStore();
  providePathCatalog(null);
});

afterEach(() => {
  setEditorHandle(null);
  setIpcForTests(null);
  providePathCatalog(null);
  disposeWorkspaces();
});

describe('the C++ tab', () => {
  it('shows a note until the preview has code', () => {
    render(<ConnectedCodePanel />);
    expect(screen.getByText('The C++ for your blocks will appear here.')).toBeDefined();
    setState(() => {
      useAppStore.getState().actions.setAnalysis({
        preview: {
          ...preview(),
          files: [{ path: 'ide/b2c_ide_init.cpp', kind: 'source', contents: '' }],
        },
      });
    });
    expect(screen.getByText('The C++ for your blocks will appear here.')).toBeDefined();
    expect(screen.queryByTestId('code-panel')).toBeNull();
  });

  it('highlights the hovered block’s code, or else the selected block’s', () => {
    const { container } = render(<ConnectedCodePanel />);
    setState(() => {
      useAppStore.getState().actions.setAnalysis({ preview: preview() });
    });
    expect(codeView(container).state.doc.toString()).toBe(MAIN_CPP);
    expect(highlighted(container)).toBe('');

    setState(() => {
      useAppStore.getState().actions.setUi({ selection: 'b005' });
    });
    expect(highlighted(container)).toBe('        guess = b2c::ask<int>("Your guess: ");');

    setState(() => {
      useAppStore.getState().actions.setUi({ hoverBlock: 'b003' });
    });
    expect(highlighted(container)).toBe('    int guess = 0;');

    setState(() => {
      useAppStore.getState().actions.setUi({ hoverBlock: null });
    });
    expect(highlighted(container)).toBe('        guess = b2c::ask<int>("Your guess: ");');
  });

  it('marks the live diagnostics, and the build’s only while they match the code', () => {
    const { container } = render(<ConnectedCodePanel />);
    const { actions } = useAppStore.getState();
    setState(() => {
      actions.setProject(projectFixture({ contentHash: HASH }));
      actions.setAnalysis({ preview: preview([liveError('b005')]) });
      actions.setBuild({ diagnostics: [compilerMessage('b003')], diagnosticsHash: HASH });
    });
    expect(container.querySelectorAll('.cm-b2c-gutter-marker')).toHaveLength(2);

    setState(() => {
      actions.updateProject({ contentHash: 'b'.repeat(64) });
    });
    expect(container.querySelectorAll('.cm-b2c-gutter-marker')).toHaveLength(1);
  });

  it('selects the innermost block at a click, or the collapsed block around it', () => {
    const { workspace, selected } = connectEditor();
    const { container } = render(<ConnectedCodePanel />);
    setState(() => {
      useAppStore.getState().actions.setAnalysis({ preview: preview() });
    });
    const view = codeView(container);
    const click = (text: string) => {
      act(() => {
        view.dispatch({
          selection: { anchor: MAIN_CPP.indexOf(text) },
          userEvent: 'select.pointer',
        });
      });
    };

    click('b2c::ask');
    click('while');
    blockById(workspace, 'b010').setCollapsed(true);
    click('b2c::ask');
    expect(selected).toEqual([
      ['b005', undefined],
      ['b010', undefined],
      ['b010', undefined],
    ]);
  });

  it('has no accessibility problems', async () => {
    const { container } = render(<ConnectedCodePanel />);
    setState(() => {
      useAppStore.getState().actions.setAnalysis({ preview: preview([liveError('b005')]) });
      useAppStore.getState().actions.setUi({ selection: 'b005' });
    });
    await expectNoAxeViolations(container);
  });
});

describe('the Problems tab', () => {
  function openGuessingGame(diagnostics: Diagnostic[], build: Diagnostic[] = [], buildHash = HASH) {
    const { actions } = useAppStore.getState();
    actions.setProject(projectFixture({ document: guessingGame(), contentHash: HASH }));
    actions.setAnalysis({ preview: previewFixture(diagnostics) });
    actions.setBuild({ diagnostics: build, diagnosticsHash: buildHash });
  }

  function rows(): HTMLElement[] {
    const grid = screen.getByRole('grid', { name: 'Problems' });
    return within(grid).getAllByRole('row').slice(1);
  }

  it('shows a note without a project', () => {
    render(<ConnectedProblemsPanel />);
    expect(screen.getByText('Problems in your blocks will be listed here.')).toBeDefined();
  });

  it('lists the live and build diagnostics with module and block path', () => {
    providePathCatalog(BLOCKLY_EXT_PATH_CATALOG);
    openGuessingGame([liveError('b009')], [compilerMessage('b005'), compilerMessage('b_gone')]);
    render(<ConnectedProblemsPanel />);

    const texts = rows().map((row) => row.textContent);
    expect(texts).toHaveLength(2);
    expect(texts[0]).toContain('main › repeat until › ask');
    expect(texts[1]).toContain('main › repeat until › if');
    expect(texts.join('')).not.toContain('from the last build');
  });

  it('marks the last build’s rows once the project has changed', () => {
    openGuessingGame([], [compilerMessage('b005')], 'c'.repeat(64));
    render(<ConnectedProblemsPanel />);
    expect(rows()[0]?.textContent).toContain('(from the last build)');
  });

  it('names blocks with the catalog once the editor provides it', () => {
    openGuessingGame([liveError('b009')]);
    render(<ConnectedProblemsPanel />);
    expect(rows()[0]?.textContent).toContain('main › control.while › control.if');
    act(() => {
      providePathCatalog(BLOCKLY_EXT_PATH_CATALOG);
    });
    expect(rows()[0]?.textContent).toContain('main › repeat until › if');
  });

  it('selects and centres the block of an activated row, or the collapsed block around it', () => {
    const { workspace, selected } = connectEditor();
    openGuessingGame([liveError('b009'), liveError('b006', 'B2C-E0202')]);
    render(<ConnectedProblemsPanel />);

    const activate = (row: HTMLElement | undefined) => {
      const [cell] = within(row ?? document.body).getAllByRole('gridcell');
      if (cell === undefined) {
        throw new Error('no cell');
      }
      fireEvent.click(cell);
    };
    activate(rows()[0]);
    blockById(workspace, 'b009').setCollapsed(true);
    activate(rows()[1]);
    expect(selected.map(([id, opts]) => [id, opts?.center])).toEqual([
      ['b009', true],
      ['b009', true],
    ]);
  });

  it('opens the diagnostics reference, and only logs when that fails', async () => {
    const fake = createFakeIpc();
    fake.openHelpLink.mockResolvedValue({});
    setIpcForTests(fake);
    openGuessingGame([liveError('b009')]);
    render(<ConnectedProblemsPanel />);

    fireEvent.click(screen.getByRole('button', { name: 'Learn more' }));
    expect(fake.openHelpLink).toHaveBeenCalledWith({ linkId: 'diagnosticsReference' });

    const warn = vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    fake.openHelpLink.mockRejectedValue(new Error('no browser'));
    fireEvent.click(screen.getByRole('button', { name: 'Learn more' }));
    await vi.waitFor(() => {
      expect(warn).toHaveBeenCalled();
    });
  });

  it('reveals a compiler message without changing anything else', () => {
    openGuessingGame([], [compilerMessage('b005')]);
    render(<ConnectedProblemsPanel />);
    fireEvent.click(screen.getByRole('button', { name: /compiler message/i }));
    expect(screen.getByText(/invalid conversion/)).toBeDefined();
  });

  it('has no accessibility problems', async () => {
    providePathCatalog(BLOCKLY_EXT_PATH_CATALOG);
    openGuessingGame(
      [liveError('b009'), { ...liveError('b006'), severity: 'warning', code: 'B2C-W0501' }],
      [compilerMessage('b005')],
      'c'.repeat(64),
    );
    const { container } = render(<ConnectedProblemsPanel />);
    await expectNoAxeViolations(container);
  });
});

describe('DockPanels', () => {
  it('connects the code and Problems, and keeps notes in the console and build output', () => {
    const panels = DockPanels();
    render(
      <div>
        {panels.code}
        {panels.problems}
        {panels.console}
        {panels.buildOutput}
      </div>,
    );
    expect(screen.getByText('The C++ for your blocks will appear here.')).toBeDefined();
    expect(screen.getByText('Problems in your blocks will be listed here.')).toBeDefined();
    expect(screen.getByText("Your program's output will appear here.")).toBeDefined();
    expect(screen.getByText("The compiler's messages will appear here.")).toBeDefined();
  });
});
