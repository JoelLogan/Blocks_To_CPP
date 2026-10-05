import { EditorView } from '@codemirror/view';
import { act, fireEvent, render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';

import { expectNoAxeViolations } from '../../test/axe';
import type { Diagnostic, GeneratedFile, MappedRange, Part, SourceMap } from '../types';
import { CodePanel, type CodePanelProps, IDE_INIT_UNIT_PATH, textChange } from './CodePanel';

const MAIN_CPP = [
  '#include <iostream>',
  '',
  'int main() {',
  '    int secret = 42;',
  '    std::cout << "hi\u200bthere" << \'\\n\';',
  '    return 0;',
  '}',
  '',
].join('\n');

const FILES: GeneratedFile[] = [
  { path: 'main.cpp', kind: 'source', contents: MAIN_CPP },
  { path: 'b2c_support.hpp', kind: 'header', contents: '#pragma once\n' },
  { path: IDE_INIT_UNIT_PATH, kind: 'source', contents: '// IDE helpers\n' },
];

function range(sl: number, sc: number, el: number, ec: number, block: string, part?: Part) {
  const mapped: MappedRange = {
    start: { line: sl, column: sc },
    end: { line: el, column: ec },
    module: 'mod_main',
    block,
    part: part ?? { kind: 'whole' },
  };
  return mapped;
}

/** program.main is lines 3–7; the declaration is line 4; the print is line 5. */
const MAP: SourceMap = {
  version: 1,
  files: [
    {
      path: 'main.cpp',
      ranges: [
        range(3, 1, 7, 2, 'b_main'),
        range(4, 5, 4, 21, 'b_decl'),
        range(4, 18, 4, 20, 'b_decl', { kind: 'input', name: 'VALUE' }),
        range(5, 5, 5, 42, 'b_print'),
      ],
    },
  ],
};

function diagnostic(
  block: string | undefined,
  part: Part,
  severity: Diagnostic['severity'] = 'error',
): Diagnostic {
  return {
    code: 'B2C-E0201',
    severity,
    message: 'There is no variable called "scret".',
    primary: { ...(block === undefined ? {} : { block }), module: 'mod_main', part },
    source: 'analyser',
  };
}

function renderPanel(overrides: Partial<CodePanelProps> = {}) {
  const props: CodePanelProps = {
    files: FILES,
    activePath: null,
    onActivePathChange: vi.fn(),
    sourceMap: MAP,
    buildable: true,
    diagnostics: [],
    highlightBlockId: null,
    onSelectBlock: vi.fn(),
    ...overrides,
  };
  const result = render(<CodePanel {...props} />);
  const editorElement = result.container.querySelector<HTMLElement>('.cm-editor');
  if (editorElement === null) {
    throw new Error('no CodeMirror editor');
  }
  const view = EditorView.findFromDOM(editorElement);
  if (view === null) {
    throw new Error('no EditorView');
  }
  return { ...result, props, view };
}

/** The UTF-16 offset of the first occurrence of `text` in main.cpp. */
function offsetOf(text: string): number {
  const offset = MAIN_CPP.indexOf(text);
  expect(offset).toBeGreaterThanOrEqual(0);
  return offset;
}

describe('CodePanel', () => {
  it('shows exactly the text of the active file, the first one by default', () => {
    const { view } = renderPanel();
    expect(view.state.doc.toString()).toBe(MAIN_CPP);
    expect(screen.getByRole('textbox', { name: 'Generated C++, main.cpp' })).toBeDefined();
  });

  it('lists the generated files in the switcher but never the IDE init unit', () => {
    const { props } = renderPanel();
    const options = screen.getAllByRole('option').map((option) => option.textContent);
    expect(options).toEqual(['main.cpp', 'b2c_support.hpp']);
    fireEvent.change(screen.getByLabelText('File'), { target: { value: 'b2c_support.hpp' } });
    expect(props.onActivePathChange).toHaveBeenCalledWith('b2c_support.hpp');
  });

  it('shows the file named by activePath and follows changes to the files', () => {
    const { view, rerender, props } = renderPanel({ activePath: 'b2c_support.hpp' });
    expect(view.state.doc.toString()).toBe('#pragma once\n');
    const changed = MAIN_CPP.replace('42', '7');
    rerender(
      <CodePanel
        {...props}
        activePath="main.cpp"
        files={[{ path: 'main.cpp', kind: 'source', contents: changed }]}
      />,
    );
    expect(view.state.doc.toString()).toBe(changed);
  });

  it('never shows the IDE init unit, even when asked to', () => {
    const { view } = renderPanel({ activePath: IDE_INIT_UNIT_PATH });
    expect(view.state.doc.toString()).toBe(MAIN_CPP);
  });

  it('copies all of the file as plain text', async () => {
    const writeText = vi.spyOn(navigator.clipboard, 'writeText').mockResolvedValue();
    renderPanel();
    await act(async () => {
      fireEvent.click(screen.getByRole('button', { name: 'Copy all' }));
      await Promise.resolve();
    });
    expect(writeText).toHaveBeenCalledWith(MAIN_CPP);
    expect(screen.getByRole('status').textContent).toBe('Copied');
  });

  it('copies the selection, and only when there is one', async () => {
    const writeText = vi.spyOn(navigator.clipboard, 'writeText').mockResolvedValue();
    const { view } = renderPanel();
    const copySelection = screen.getByRole('button', { name: 'Copy selection' });
    expect(copySelection).toHaveProperty('disabled', true);

    const from = offsetOf('int secret');
    act(() => {
      view.dispatch({ selection: { anchor: from, head: from + 'int secret'.length } });
    });
    expect(copySelection).toHaveProperty('disabled', false);
    await act(async () => {
      fireEvent.click(copySelection);
      await Promise.resolve();
    });
    expect(writeText).toHaveBeenCalledWith('int secret');
  });

  it('says when copying failed', async () => {
    vi.spyOn(navigator.clipboard, 'writeText').mockRejectedValue(new Error('denied'));
    renderPanel();
    await act(async () => {
      fireEvent.click(screen.getByRole('button', { name: 'Copy all' }));
      await Promise.resolve();
      await Promise.resolve();
    });
    expect(screen.getByRole('status').textContent).toBe('Could not copy');
  });

  it('shows hidden characters as placeholders without changing the text', () => {
    const { container, view } = renderPanel();
    const placeholder = container.querySelector('.cm-b2c-hidden-char');
    expect(placeholder?.textContent).toBe('⟨U+200B⟩');
    expect(view.state.doc.toString()).toContain('hi\u200bthere');
  });

  it('labels output that cannot be built', () => {
    renderPanel({ buildable: false });
    expect(screen.getByText(/Not buildable: fix the errors first/)).toBeDefined();
  });

  it('has no label when the output can be built, and a hint when there is no code', () => {
    const { rerender, props } = renderPanel();
    expect(screen.queryByText(/Not buildable/)).toBeNull();
    rerender(<CodePanel {...props} files={[]} sourceMap={null} buildable={false} />);
    expect(screen.getByText('The C++ for your blocks will appear here.')).toBeDefined();
    expect(screen.queryByText(/Not buildable/)).toBeNull();
  });

  it('highlights every whole range of the highlighted block', () => {
    const { container, rerender, props } = renderPanel({ highlightBlockId: 'b_decl' });
    const highlighted = container.querySelectorAll('.cm-b2c-block-highlight');
    expect([...highlighted].map((element) => element.textContent).join('')).toBe(
      'int secret = 42;',
    );
    rerender(<CodePanel {...props} highlightBlockId={null} />);
    expect(container.querySelectorAll('.cm-b2c-block-highlight')).toHaveLength(0);
  });

  it('selects the block of the innermost range under a click', () => {
    const { view, props } = renderPanel();
    act(() => {
      view.dispatch({ selection: { anchor: offsetOf('secret') }, userEvent: 'select.pointer' });
    });
    expect(props.onSelectBlock).toHaveBeenLastCalledWith('b_decl');
    act(() => {
      view.dispatch({ selection: { anchor: offsetOf('return') }, userEvent: 'select.pointer' });
    });
    expect(props.onSelectBlock).toHaveBeenLastCalledWith('b_main');
    // Moving the caret with the keyboard does not select blocks; Enter does.
    vi.mocked(props.onSelectBlock).mockClear();
    act(() => {
      view.dispatch({ selection: { anchor: offsetOf('std::cout') }, userEvent: 'select' });
    });
    expect(props.onSelectBlock).not.toHaveBeenCalled();
    fireEvent.keyDown(view.contentDOM, { key: 'Enter', code: 'Enter', keyCode: 13 });
    expect(props.onSelectBlock).toHaveBeenCalledWith('b_print');
  });

  it('does not select anything for a click outside every range', () => {
    const { view, props } = renderPanel();
    act(() => {
      view.dispatch({ selection: { anchor: 0 }, userEvent: 'select.pointer' });
    });
    expect(props.onSelectBlock).not.toHaveBeenCalled();
  });

  it('marks a diagnostic in the gutter and underlines its part', () => {
    const { container } = renderPanel({
      diagnostics: [diagnostic('b_decl', { kind: 'input', name: 'VALUE' })],
    });
    const marker = container.querySelector('.cm-b2c-gutter-marker');
    expect(marker?.textContent).toBe('✖');
    expect(marker?.getAttribute('title')).toBe(
      'Error: There is no variable called "scret". (B2C-E0201)',
    );
    const underline = container.querySelector('.cm-b2c-diag-error');
    expect(underline?.textContent).toBe('42');
    expect(underline?.getAttribute('title')).toContain('There is no variable called');
  });

  it('underlines the whole block when the part has no range, and skips diagnostics without one', () => {
    const { container } = renderPanel({
      diagnostics: [
        diagnostic('b_decl', { kind: 'field', name: 'NAME' }, 'warning'),
        diagnostic(undefined, { kind: 'whole' }),
        diagnostic('b_unknown', { kind: 'whole' }),
      ],
    });
    const underlines = container.querySelectorAll('.cm-b2c-diag');
    expect(underlines).toHaveLength(1);
    expect(underlines[0]?.textContent).toBe('int secret = 42;');
    expect(underlines[0]?.classList.contains('cm-b2c-diag-warning')).toBe(true);
    const markers = container.querySelectorAll('.cm-b2c-gutter-marker');
    expect(markers).toHaveLength(1);
    expect(markers[0]?.textContent).toBe('⚠');
  });

  it('shows the most serious icon when a line has several diagnostics', () => {
    const { container } = renderPanel({
      diagnostics: [
        diagnostic('b_decl', { kind: 'whole' }, 'info'),
        diagnostic('b_decl', { kind: 'input', name: 'VALUE' }, 'error'),
      ],
    });
    const markers = container.querySelectorAll('.cm-b2c-gutter-marker');
    expect(markers).toHaveLength(1);
    expect(markers[0]?.textContent).toBe('✖');
    expect(markers[0]?.getAttribute('title')?.split('\n')).toEqual([
      'Error: There is no variable called "scret". (B2C-E0201)',
      'Info: There is no variable called "scret". (B2C-E0201)',
    ]);
  });

  it('has no accessibility violations', async () => {
    const { container } = renderPanel({
      buildable: false,
      diagnostics: [diagnostic('b_decl', { kind: 'whole' })],
      highlightBlockId: 'b_print',
    });
    await expectNoAxeViolations(container);
  });
});

describe('textChange', () => {
  it('replaces only what differs', () => {
    expect(textChange('abc', 'abc')).toBeUndefined();
    expect(textChange('int x = 1;', 'int x = 22;')).toEqual({ from: 8, to: 9, insert: '22' });
    expect(textChange('', 'new')).toEqual({ from: 0, to: 0, insert: 'new' });
    expect(textChange('old', '')).toEqual({ from: 0, to: 3, insert: '' });
  });

  it('never splits a surrogate pair', () => {
    // 😀 and 😁 share their first code unit; 🙂 and 😂 share their second.
    expect(textChange('a😀b', 'a😁b')).toEqual({ from: 1, to: 3, insert: '😁' });
    const change = textChange('x\ud83d\ude02', 'x\ud83e\ude02');
    expect(change).toEqual({ from: 1, to: 3, insert: '\ud83e\ude02' });
  });
});
