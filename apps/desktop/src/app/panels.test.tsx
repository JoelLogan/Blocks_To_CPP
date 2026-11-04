/**
 * The connected dock panels: the C++ tab follows the preview, the hovered and selected blocks and
 * the diagnostics, and selects blocks on a click; Problems lists the live and build diagnostics
 * with block paths, selects and centres blocks, and opens the diagnostics reference.
 */
import { compactSourceMap, type PreviewResult } from '@blocks2cpp/b2c-core-wasm';
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
import { GENERATOR_BUG_LABEL, labelGeneratorBugs } from '../features/build-run/buildOutput';
import { setIpcForTests } from '../lib/ipc';
import { expectNoAxeViolations } from '../test/axe';
import { type SelectBlockOptions, setEditorHandle } from './editor-types';
import {
  ConnectedBuildOutputPanel,
  ConnectedCodePanel,
  ConnectedProblemsPanel,
  DockPanels,
  moduleSourcePath,
} from './panels';
import { resetAppStore, useAppStore } from './store';
import {
  createFakeIpc,
  documentFixture,
  NO_MODULE_EDITS,
  previewFixture,
  projectFixture,
  toolchainFixture,
} from './testing/fixtures';

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
    sourceMap: compactSourceMap({
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
    }),
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

  it('shows the .cpp of the module on the canvas by its module key, and follows the module strip', () => {
    const doc = documentFixture();
    doc.modules = [
      { id: 'mod_main', name: 'main', workspace: { blocks: [] } },
      { id: 'mod_util', name: 'util', workspace: { blocks: [] } },
    ];
    const { actions } = useAppStore.getState();
    // A preview of the two modules (each edit brings a new one).
    const twoModulePreview = (): PreviewResult => ({
      ...preview(),
      files: [
        // Not in module order: the module key decides (w3-modules.md §2.5).
        { path: 'util.cpp', kind: 'source', module: 'mod_util', contents: '// util\n' },
        { path: 'main.cpp', kind: 'source', module: 'mod_main', contents: MAIN_CPP },
        { path: 'b2c_support.hpp', kind: 'header', contents: '#pragma once\n' },
      ],
    });
    const { container } = render(<ConnectedCodePanel />);
    setState(() => {
      actions.setProject(projectFixture({ document: doc, activeModuleId: 'mod_util' }));
      actions.setAnalysis({ preview: twoModulePreview() });
    });
    const file = screen.getByLabelText<HTMLSelectElement>('File');
    const shown = () => [file.value, codeView(container).state.doc.toString()];
    expect(shown()).toEqual(['util.cpp', '// util\n']);

    setState(() => {
      actions.updateProject({ activeModuleId: 'mod_main' });
    });
    expect(shown()).toEqual(['main.cpp', MAIN_CPP]);

    // The shown module's files first, then the other modules' under their names, then the support
    // header.
    const groups = () =>
      Array.from(file.children, (child) =>
        child instanceof HTMLOptGroupElement
          ? `${child.label}: ${Array.from(child.children, (option) => option.textContent).join(', ')}`
          : child.textContent,
      );
    expect(groups()).toEqual(['main.cpp', 'Module util: util.cpp', 'b2c_support.hpp']);

    // A file picked in the panel is kept for the module it was picked in.
    fireEvent.change(file, { target: { value: 'b2c_support.hpp' } });
    expect(shown()).toEqual(['b2c_support.hpp', '#pragma once\n']);
    setState(() => {
      actions.setAnalysis({ preview: twoModulePreview() });
    });
    expect(file.value).toBe('b2c_support.hpp');
    setState(() => {
      actions.updateProject({ activeModuleId: 'mod_util' });
    });
    expect(shown()).toEqual(['util.cpp', '// util\n']);
    expect(groups()).toEqual(['util.cpp', 'Module main: main.cpp', 'b2c_support.hpp']);
    setState(() => {
      actions.updateProject({ activeModuleId: 'mod_main' });
    });
    expect(file.value).toBe('b2c_support.hpp');
    // A picked file the preview no longer has (a header that went): the module's .cpp.
    setState(() => {
      actions.setAnalysis({
        preview: {
          ...twoModulePreview(),
          files: twoModulePreview().files.filter((f) => f.path !== 'b2c_support.hpp'),
        },
      });
    });
    expect(file.value).toBe('main.cpp');
  });

  it('keeps showing another module’s header whose code was clicked, as that module’s pick', () => {
    const doc = documentFixture();
    doc.modules = [
      { id: 'mod_main', name: 'main', workspace: { blocks: [] } },
      { id: 'mod_utils', name: 'utils', workspace: { blocks: [] } },
    ];
    const header = 'void greet();\n';
    const { actions } = useAppStore.getState();
    const switched: string[] = [];
    setEditorHandle({
      ...NO_MODULE_EDITS,
      // No block of the canvas: the editor is asked to select (and show) it.
      workspace: { getBlockById: () => null } as never,
      loadDocument: () => undefined,
      currentDocument: () => doc,
      updateProjectInfo: () => undefined,
      undo: () => undefined,
      redo: () => undefined,
      // As the session does: a block of another module shows that module first.
      selectBlock: (id) => {
        switched.push(id);
        actions.updateProject({ activeModuleId: 'mod_utils' });
      },
    });
    const { container } = render(<ConnectedCodePanel />);
    setState(() => {
      actions.setProject(projectFixture({ document: doc, activeModuleId: 'mod_main' }));
      actions.setAnalysis({
        preview: {
          ...preview(),
          files: [
            { path: 'main.cpp', kind: 'source', module: 'mod_main', contents: MAIN_CPP },
            { path: 'utils.cpp', kind: 'source', module: 'mod_utils', contents: '// utils\n' },
            { path: 'utils.hpp', kind: 'header', module: 'mod_utils', contents: header },
          ],
          sourceMap: compactSourceMap({
            version: 1,
            files: [
              {
                path: 'utils.hpp',
                module: 'mod_utils',
                ranges: [
                  {
                    start: { line: 1, column: 1 },
                    end: { line: 1, column: 14 },
                    module: 'mod_utils',
                    block: 'blk_greet',
                    part: { kind: 'whole' },
                  },
                ],
              },
            ],
          }),
        },
      });
    });
    const file = screen.getByLabelText<HTMLSelectElement>('File');
    fireEvent.change(file, { target: { value: 'utils.hpp' } });
    act(() => {
      codeView(container).dispatch({ selection: { anchor: 2 }, userEvent: 'select.pointer' });
    });
    expect(switched).toEqual(['blk_greet']);
    expect(useAppStore.getState().project?.activeModuleId).toBe('mod_utils');
    expect(file.value).toBe('utils.hpp');
    setEditorHandle(null);
  });

  it('finds a module’s .cpp by its module key, past the IDE’s own units', () => {
    const files = [
      { path: 'main.cpp', kind: 'source', module: 'mod_main', contents: '' },
      { path: 'ide/b2c_ide_init.cpp', kind: 'source', module: 'mod_util', contents: '' },
      { path: 'util.hpp', kind: 'header', module: 'mod_util', contents: '' },
      { path: 'util.cpp', kind: 'source', module: 'mod_util', contents: '' },
      { path: 'b2c_support.hpp', kind: 'header', contents: '' },
    ] as const;
    expect(moduleSourcePath(files, 'mod_main')).toBe('main.cpp');
    expect(moduleSourcePath(files, 'mod_util')).toBe('util.cpp');
    expect(moduleSourcePath(files, 'mod_gone')).toBeNull();
    expect(moduleSourcePath(files, null)).toBeNull();
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
    expect(texts[0]).toContain('when program starts › repeat until › ask');
    expect(texts[1]).toContain('when program starts › repeat until › if');
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
    expect(rows()[0]?.textContent).toContain('when program starts › control.while › control.if');
    act(() => {
      providePathCatalog(BLOCKLY_EXT_PATH_CATALOG);
    });
    expect(rows()[0]?.textContent).toContain('when program starts › repeat until › if');
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

  it('shows the module of a problem in another module, then reveals its block (w3-modules.md §2.6)', () => {
    const { selected } = connectEditor();
    const doc = guessingGame();
    doc.modules = [...doc.modules, { id: 'mod_utils', name: 'utils', workspace: { blocks: [] } }];
    const { actions } = useAppStore.getState();
    actions.setProject(projectFixture({ document: doc, contentHash: HASH }));
    const elsewhere: Diagnostic = {
      ...liveError('b_call'),
      code: 'B2C-E0206',
      primary: { module: 'mod_utils', block: 'b_call', part: { kind: 'whole' } },
    };
    // A problem about a module itself (B2C-E0221) has no block.
    const moduleOnly: Diagnostic = {
      ...liveError('b_x'),
      code: 'B2C-E0221',
      primary: { module: 'mod_main', part: { kind: 'whole' } },
    };
    actions.setAnalysis({ preview: previewFixture([elsewhere, moduleOnly]) });
    render(<ConnectedProblemsPanel />);
    const activate = (code: string) => {
      const row = rows().find((candidate) => candidate.textContent.includes(code));
      const [cell] = within(row ?? document.body).getAllByRole('gridcell');
      if (cell === undefined) {
        throw new Error('no cell');
      }
      fireEvent.click(cell);
    };
    activate('B2C-E0206');
    expect(useAppStore.getState().project?.activeModuleId).toBe('mod_utils');
    expect(selected).toEqual([['b_call', { center: true }]]);
    activate('B2C-E0221');
    expect(useAppStore.getState().project?.activeModuleId).toBe('mod_main');
    expect(selected).toHaveLength(1);
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

  /** The guessing game's build row `b005`, labelled a generator bug as a build labels it. */
  function generatorBug(): Diagnostic {
    const [bug] = labelGeneratorBugs([compilerMessage('b005')], null).items;
    if (!bug?.message.includes(GENERATOR_BUG_LABEL)) {
      throw new Error('not labelled');
    }
    return bug;
  }

  /** A selected toolchain, and the configuration the build ran in and the one chosen since. */
  function withToolchain(built: 'debug' | 'release', chosen: 'debug' | 'release'): void {
    useAppStore.setState((state) => ({
      toolchains: {
        ...state.toolchains,
        list: [toolchainFixture({ selected: true, usable: true, version: '13.3.0' })],
      },
      build: { ...state.build, diagnosticsConfig: built, config: chosen },
    }));
  }

  it('offers Copy bug report on a generator bug, with the build’s facts', () => {
    openGuessingGame([], [generatorBug(), compilerMessage('b003')]);
    withToolchain('debug', 'release');
    render(<ConnectedProblemsPanel />);
    expect(screen.queryByRole('button', { name: 'Copy bug report…' })).toBeNull();
    for (const button of screen.getAllByRole('button', { name: 'Show C++ compiler message' })) {
      fireEvent.click(button);
    }
    // Only the labelled row offers it.
    expect(screen.getAllByRole('button', { name: 'Copy bug report…' })).toHaveLength(1);
    fireEvent.click(screen.getByRole('button', { name: 'Copy bug report…' }));
    expect(screen.getByRole('dialog', { name: /bug report/iu })).toBeDefined();
    const text = screen.getByTestId('bug-report-text').textContent;
    const standard = guessingGame().project.language.standard;
    expect(text).toContain('13.3.0');
    // The configuration it was built in, not the one chosen since.
    expect(text).toContain(`C++ standard: ${standard}; configuration: debug`);
    fireEvent.click(screen.getByRole('button', { name: 'Close' }));
    expect(screen.queryByRole('dialog', { name: /bug report/iu })).toBeNull();
  });

  it('does not offer Copy bug report on a stale generator bug', () => {
    openGuessingGame([], [generatorBug()], 'c'.repeat(64));
    withToolchain('debug', 'debug');
    render(<ConnectedProblemsPanel />);
    expect(rows()[0]?.textContent).toContain('(from the last build)');
    fireEvent.click(screen.getByRole('button', { name: 'Show C++ compiler message' }));
    expect(screen.queryByRole('button', { name: 'Copy bug report…' })).toBeNull();
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

describe('the Build output tab', () => {
  it('follows each new build from its start, even after scrolling up in the last one', () => {
    const { actions } = useAppStore.getState();
    setState(() => {
      actions.setProject(projectFixture());
      actions.setBuild({ serial: 1, output: [{ kind: 'progress', text: 'Building A…' }] });
    });
    render(<ConnectedBuildOutputPanel />);
    const log = screen.getByRole('log', { name: 'Build output' });
    let height = 500;
    Object.defineProperty(log, 'scrollHeight', { configurable: true, get: () => height });
    Object.defineProperty(log, 'clientHeight', { configurable: true, get: () => 100 });

    // Scrolled up to read: the build's new lines leave the view alone.
    log.scrollTop = 100;
    fireEvent.scroll(log);
    height = 600;
    setState(() => {
      actions.appendBuildOutput([{ kind: 'raw', text: 'main.cpp:3:1: warning: …' }]);
    });
    expect(log.scrollTop).toBe(100);

    // The next build replaces the lines (as the build feature does) and is followed again.
    height = 300;
    setState(() => {
      actions.setBuild({ serial: 2, output: [] });
      actions.appendBuildOutput([{ kind: 'progress', text: 'Building A…' }]);
    });
    expect(log.scrollTop).toBe(300);
    height = 400;
    setState(() => {
      actions.appendBuildOutput([{ kind: 'progress', text: 'Compiling main.cpp (1/1)' }]);
    });
    expect(log.scrollTop).toBe(400);
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
