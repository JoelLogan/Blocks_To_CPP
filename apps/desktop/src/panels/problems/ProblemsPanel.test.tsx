import { act, fireEvent, render, screen, within } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';

import { expectNoAxeViolations } from '../../test/axe';
import type { Diagnostic } from '../types';
import { ProblemsPanel, type ProblemItem, type ProblemsPanelProps } from './ProblemsPanel';
import { MAX_PROBLEM_ROWS } from './problems';

function item(
  key: string,
  severity: Diagnostic['severity'],
  message: string,
  extra: Partial<Diagnostic> & {
    stale?: boolean;
    blockPath?: string;
    origin?: 'live' | 'build';
  } = {},
): ProblemItem {
  const { stale = false, blockPath = 'main › if', origin = 'live', ...diagnostic } = extra;
  return {
    key,
    diagnostic: {
      code: 'B2C-E0201',
      severity,
      message,
      primary: { module: 'mod_main', block: key, part: { kind: 'whole' } },
      source: 'analyser',
      ...diagnostic,
    },
    origin,
    stale,
    modulePath: 'main',
    blockPath,
  };
}

const ITEMS: ProblemItem[] = [
  item('b1', 'warning', 'Bravo is never used.', { code: 'B2C-W0501', blockPath: 'main' }),
  item('b2', 'error', 'Alpha is not declared.'),
  item('b3', 'info', 'Charlie could be const.', { code: 'B2C-I0502' }),
  item('b4', 'error', 'expected ; before }', {
    code: 'B2C-C1001',
    source: 'compiler',
    raw: "main.cpp:5:3: error: expected ';' before '}' token",
    origin: 'build',
    stale: true,
    blockPath: 'main › repeat until',
  }),
];

function renderPanel(overrides: Partial<ProblemsPanelProps> = {}) {
  const props: ProblemsPanelProps = {
    items: ITEMS,
    onActivate: vi.fn(),
    onShowRaw: vi.fn(),
    onLearnMore: vi.fn(),
    ...overrides,
  };
  return { ...render(<ProblemsPanel {...props} />), props };
}

/** `list[index]`, failing the test when it is missing. */
function at<T>(list: ArrayLike<T>, index: number): T {
  const value = list[index];
  if (value === undefined) {
    throw new Error(`nothing at index ${String(index)}`);
  }
  return value;
}

/** The element with the keyboard focus. */
function focused(): Element {
  const element = document.activeElement;
  if (element === null) {
    throw new Error('nothing has the focus');
  }
  return element;
}

/** The message column of every row, top to bottom. */
function messages(): string[] {
  return screen.getAllByTestId('problem-row').map((row) => {
    const cells = within(row).getAllByRole('gridcell');
    return (cells[1]?.firstChild?.textContent ?? '').trim();
  });
}

describe('ProblemsPanel', () => {
  it('shows severity as icon and word, message, module, block path and code', () => {
    renderPanel();
    const grid = screen.getByRole('grid', { name: 'Problems' });
    const headers = within(grid)
      .getAllByRole('columnheader')
      .map((header) => header.textContent);
    expect(headers).toEqual(['Severity ▼', 'Message', 'Module', 'Block', 'Code', 'Details']);
    const first = at(screen.getAllByTestId('problem-row'), 0);
    const cells = within(first).getAllByRole('gridcell');
    expect(cells.map((cell) => cell.textContent)).toEqual([
      '✖ Error',
      'Alpha is not declared.',
      'main',
      'main › if',
      'B2C-E0201',
      '',
    ]);
    expect(screen.getByText('2 errors, 1 warning, 1 info message')).toBeDefined();
  });

  it('sorts by severity first and by any column on request', () => {
    renderPanel();
    expect(messages()).toEqual([
      'Alpha is not declared.',
      'expected ; before }',
      'Bravo is never used.',
      'Charlie could be const.',
    ]);

    const messageHeader = screen.getByRole('columnheader', { name: /Message/ });
    fireEvent.click(within(messageHeader).getByRole('button'));
    expect(messageHeader.getAttribute('aria-sort')).toBe('ascending');
    expect(messages()).toEqual([
      'Alpha is not declared.',
      'Bravo is never used.',
      'Charlie could be const.',
      'expected ; before }',
    ]);

    fireEvent.click(within(messageHeader).getByRole('button'));
    expect(messageHeader.getAttribute('aria-sort')).toBe('descending');
    expect(messages()[0]).toBe('expected ; before }');

    const severityHeader = screen.getByRole('columnheader', { name: /Severity/ });
    expect(severityHeader.getAttribute('aria-sort')).toBeNull();
    fireEvent.click(within(severityHeader).getByRole('button'));
    expect(severityHeader.getAttribute('aria-sort')).toBe('descending');
    fireEvent.click(within(severityHeader).getByRole('button'));
    expect(messages()[0]).toBe('Charlie could be const.');
  });

  it('activates a row on click and on Enter', () => {
    const { props } = renderPanel();
    fireEvent.click(screen.getByText('Bravo is never used.'));
    expect(props.onActivate).toHaveBeenCalledWith(ITEMS[0]);

    const cell = at(
      within(at(screen.getAllByTestId('problem-row'), 1)).getAllByRole('gridcell'),
      3,
    );
    fireEvent.keyDown(cell, { key: 'Enter' });
    expect(props.onActivate).toHaveBeenLastCalledWith(ITEMS[3]);
  });

  it('has one Tab stop and moves between cells with the grid keys', () => {
    renderPanel();
    const rows = screen.getAllByTestId('problem-row');
    const cellsOf = (index: number) => within(at(rows, index)).getAllByRole('gridcell');
    const focusable = screen.getByRole('grid').querySelectorAll('[tabindex="0"]');
    expect(focusable).toHaveLength(1);
    expect(focusable[0]).toBe(cellsOf(0)[0]);

    const start = at(cellsOf(0), 0);
    act(() => {
      start.focus();
    });
    fireEvent.keyDown(start, { key: 'ArrowRight' });
    expect(document.activeElement).toBe(cellsOf(0)[1]);
    fireEvent.keyDown(focused(), { key: 'ArrowDown' });
    expect(document.activeElement).toBe(cellsOf(1)[1]);
    fireEvent.keyDown(focused(), { key: 'End' });
    // The last column of a compiler row holds the toggle button, which takes the focus.
    expect(document.activeElement).toBe(
      within(at(rows, 1)).getByRole('button', { name: 'Show C++ compiler message' }),
    );
    fireEvent.keyDown(focused(), { key: 'Home' });
    expect(document.activeElement).toBe(cellsOf(1)[0]);
    fireEvent.keyDown(focused(), { key: 'End', ctrlKey: true });
    expect(document.activeElement).toBe(cellsOf(3)[5]);
    fireEvent.keyDown(focused(), { key: 'PageUp' });
    expect(document.activeElement?.textContent).toBe('Details');
    fireEvent.keyDown(focused(), { key: 'ArrowLeft' });
    expect(document.activeElement?.textContent).toBe('Code');
    fireEvent.keyDown(focused(), { key: 'PageDown' });
    expect(document.activeElement).toBe(cellsOf(3)[4]);
    fireEvent.keyDown(focused(), { key: 'Home', ctrlKey: true });
    expect(document.activeElement?.textContent).toBe('Severity ▼');
  });

  it('reveals the compiler message of compiler rows only', () => {
    const { props } = renderPanel();
    const toggles = screen.getAllByRole('button', { name: 'Show C++ compiler message' });
    expect(toggles).toHaveLength(1);
    const toggle = at(toggles, 0);
    expect(toggle.getAttribute('aria-expanded')).toBe('false');

    fireEvent.click(toggle);
    expect(props.onShowRaw).toHaveBeenCalledWith(ITEMS[3]);
    expect(props.onActivate).not.toHaveBeenCalled();
    expect(toggle.getAttribute('aria-expanded')).toBe('true');
    expect(toggle.textContent).toBe('Hide C++ compiler message');
    const raw = document.getElementById(toggle.getAttribute('aria-controls') ?? '');
    expect(raw?.textContent).toBe("main.cpp:5:3: error: expected ';' before '}' token");

    fireEvent.click(toggle);
    expect(toggle.getAttribute('aria-expanded')).toBe('false');
    expect(screen.queryByText(/main\.cpp:5:3/)).toBeNull();
    expect(props.onShowRaw).toHaveBeenCalledTimes(1);
  });

  it('marks diagnostics from an outdated build', () => {
    renderPanel();
    const stale = screen.getByText('(from the last build)');
    expect(stale.closest('[role="row"]')?.className).toContain('b2c-problems-stale');
  });

  it('opens the diagnostics reference', () => {
    const { props } = renderPanel();
    fireEvent.click(screen.getByRole('button', { name: 'Learn more' }));
    expect(props.onLearnMore).toHaveBeenCalledTimes(1);
  });

  it('shows hidden characters in messages as placeholders', () => {
    renderPanel({ items: [item('b9', 'error', 'Name "a‮b" is odd.')] });
    expect(screen.getByText('Name "a⟨U+202E⟩b" is odd.')).toBeDefined();
  });

  it('says when there are no problems', () => {
    renderPanel({ items: [] });
    expect(screen.getByText('No problems')).toBeDefined();
    expect(screen.queryByRole('grid')).toBeNull();
  });

  it('shows at most MAX_PROBLEM_ROWS rows and says how many there are', () => {
    const many = Array.from({ length: MAX_PROBLEM_ROWS + 5 }, (_, index) =>
      item(`b${String(index)}`, 'warning', `Problem ${String(index)}`),
    );
    renderPanel({ items: many });
    expect(screen.getAllByTestId('problem-row')).toHaveLength(MAX_PROBLEM_ROWS);
    expect(screen.getByText(/Showing the first 1,000 of 1,005 problems/)).toBeDefined();
    expect(screen.getByRole('grid').getAttribute('aria-rowcount')).toBe(String(1006));
  });

  it('has no accessibility violations, with a compiler message shown', async () => {
    const { container } = renderPanel();
    fireEvent.click(screen.getByRole('button', { name: 'Show C++ compiler message' }));
    await expectNoAxeViolations(container);
  });
});
