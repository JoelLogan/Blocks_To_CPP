/**
 * The accessibility baseline of the panels (docs/spec/04-user-interface.md §4.8) in each of their
 * states: no axe violations, everything that acts reachable with Tab and named, and severities and
 * exit states told by an icon and a word, never by colour alone. The panels' own tests cover the
 * behaviour; these cover the states those tests do not open.
 */
import { act, fireEvent, render, screen } from '@testing-library/react';
import { createRef } from 'react';
import { describe, expect, it, vi } from 'vitest';

import {
  accessibleName,
  describeStop,
  tabStops,
  tabThrough as walkThrough,
} from '../app/layout/sequentialFocus';
import { expectNoAxeViolations } from '../test/axe';
import {
  BuildOutputPanel,
  MAX_BUILD_LINE_CHARS,
  MAX_BUILD_OUTPUT_LINES,
  type BuildOutputLine,
} from './build-output/BuildOutputPanel';
import { CodePanel, type CodePanelProps } from './code/CodePanel';
import {
  ConsolePanel,
  type ConsoleHandle,
  type ConsoleHeader,
  type ConsolePanelProps,
} from './console/ConsolePanel';
import { LinkDialog } from './console/LinkDialog';
import { ProblemsPanel, type ProblemItem, type ProblemsPanelProps } from './problems/ProblemsPanel';
import { MAX_PROBLEM_ROWS } from './problems/problems';
import { SEVERITY_ICON, SEVERITY_WORD } from './shared/severity';
import type { Diagnostic, RunExit } from './types';

/** The tab stops inside `container`, each as role (or tag) and name. */
function stopsIn(container: Element): string[] {
  return tabStops()
    .filter((stop) => container.contains(stop))
    .map(describeStop);
}

/** Tabs through `container` ({@link walkThrough}), letting React handle each focus change. */
function tabThrough(container: Element): string[] {
  let reached: string[] = [];
  act(() => {
    reached = walkThrough(container);
  });
  return reached;
}

// --- Problems -------------------------------------------------------------------------------

function problem(
  key: string,
  severity: Diagnostic['severity'],
  message: string,
  extra: Partial<Diagnostic> & { stale?: boolean; origin?: 'live' | 'build' } = {},
): ProblemItem {
  const { stale = false, origin = 'live', ...diagnostic } = extra;
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
    blockPath: 'main › if',
  };
}

const PROBLEMS: ProblemItem[] = [
  problem('p1', 'error', 'There is no variable called "scret".'),
  problem('p2', 'warning', 'guess is never used.', { code: 'B2C-W0501' }),
  problem('p3', 'info', 'limit could be const.', { code: 'B2C-I0502' }),
  problem('p4', 'error', "expected ';' before '}' token", {
    code: 'B2C-C1001',
    source: 'compiler',
    raw: "main.cpp:5:3: error: expected ';' before '}' token",
    origin: 'build',
    stale: true,
  }),
];

function renderProblems(overrides: Partial<ProblemsPanelProps> = {}) {
  const props: ProblemsPanelProps = {
    items: PROBLEMS,
    onActivate: vi.fn(),
    onShowRaw: vi.fn(),
    onLearnMore: vi.fn(),
    ...overrides,
  };
  return render(<ProblemsPanel {...props} />);
}

describe('the Problems panel', () => {
  it('passes axe when empty, and says what will appear', async () => {
    const { container } = renderProblems({ items: [] });
    expect(screen.getByText(/^No problems\./)).toBeDefined();
    expect(stopsIn(container)).toEqual(['button: Learn more']);
    await expectNoAxeViolations(container);
  });

  it('passes axe with every severity, a stale build diagnostic and its compiler message', async () => {
    const { container } = renderProblems();
    fireEvent.click(screen.getByRole('button', { name: 'Show C++ compiler message' }));
    await expectNoAxeViolations(container);
  });

  it('tells each severity by an icon (hidden from screen readers) and a word', () => {
    const { container } = renderProblems();
    for (const severity of ['error', 'warning', 'info'] as const) {
      const labels = [...container.querySelectorAll(`.b2c-severity-${severity}`)];
      expect(labels.length).toBeGreaterThan(0);
      for (const label of labels) {
        const icon = label.querySelector('[aria-hidden="true"]');
        expect(icon?.textContent).toBe(SEVERITY_ICON[severity]);
        expect(accessibleName(label)).toBe(SEVERITY_WORD[severity]);
      }
    }
  });

  it('makes the grid one tab stop: a single cell is in the Tab order (arrow keys move inside)', () => {
    const { container } = renderProblems();
    const reached = tabThrough(container);
    expect(reached).toHaveLength(2);
    expect(reached[0]).toBe('button: Learn more');
    // The first problem's severity: the grid starts on the first row, the most serious first.
    expect(reached[1]).toBe('gridcell: Error');
  });

  it('passes axe when it shows only the first rows', async () => {
    const many = Array.from({ length: MAX_PROBLEM_ROWS + 1 }, (_, index) =>
      problem(`m${String(index)}`, 'warning', `Problem ${String(index)}`),
    );
    const { container } = renderProblems({ items: many });
    expect(screen.getByText(/Showing the first 1,000 of 1,001 problems/)).toBeDefined();
    await expectNoAxeViolations(container);
  }, 120_000);
});

// --- Code -----------------------------------------------------------------------------------

function renderCode(overrides: Partial<CodePanelProps> = {}) {
  const props: CodePanelProps = {
    files: [
      { path: 'main.cpp', kind: 'source', contents: 'int main() {\n    return 0;\n}\n' },
      { path: 'b2c_support.hpp', kind: 'header', contents: '#pragma once\n' },
    ],
    activePath: null,
    onActivePathChange: vi.fn(),
    sourceMap: null,
    buildable: true,
    diagnostics: [],
    highlightBlockId: null,
    onSelectBlock: vi.fn(),
    ...overrides,
  };
  return render(<CodePanel {...props} />);
}

describe('the C++ panel', () => {
  it('passes axe before there is any C++, with nothing to Tab to', async () => {
    const { container } = renderCode({ files: [] });
    expect(screen.getByText('The C++ for your blocks will appear here.')).toBeDefined();
    expect(stopsIn(container)).toEqual([]);
    await expectNoAxeViolations(container);
  });

  it('passes axe with code that is not buildable, said in words and not by colour', async () => {
    const { container } = renderCode({ buildable: false });
    expect(screen.getByTestId('code-not-buildable').textContent).toContain(
      'Not buildable: fix the errors first',
    );
    await expectNoAxeViolations(container);
  });

  it('reaches the file list, Copy all and the code with Tab', () => {
    const { container } = renderCode();
    const reached = tabThrough(container);
    expect(reached.slice(0, 2)).toEqual(['select: File', 'button: Copy all']);
    // Copy selection is disabled until there is a selection; the code itself is the last stop.
    expect(reached).not.toContain('button: Copy selection');
    expect(reached.at(-1)).toMatch(/^textbox: /);
  });
});

// --- Console --------------------------------------------------------------------------------

function exited(status: RunExit['status'], message: string): ConsoleHeader {
  return {
    state: 'exited',
    exit: {
      kind: 'exit',
      afterSeq: 1,
      elapsedMs: 900,
      status,
      crash: null,
      sanitizer: null,
      message,
    },
    elapsedMs: 900,
    notices: [],
  };
}

const IDLE: ConsoleHeader = { state: 'idle', exit: null, elapsedMs: 0, notices: [] };
const RUNNING_WITH_NOTICES: ConsoleHeader = {
  state: 'running',
  exit: null,
  elapsedMs: 4_200,
  notices: ['ideHelpers', 'processGroupOnly'],
};
const EXIT_0 = exited({ type: 'exited', code: 0 }, 'Finished (exit code 0)');
const EXIT_3 = exited({ type: 'exited', code: 3 }, 'Finished (exit code 3)');

const HEADERS: readonly (readonly [string, ConsoleHeader])[] = [
  ['idle', IDLE],
  ['running with notices', RUNNING_WITH_NOTICES],
  ['finished with exit code 0', EXIT_0],
  ['finished with exit code 3', EXIT_3],
];

function renderConsole(header: ConsoleHeader) {
  const props: ConsolePanelProps = {
    header,
    scrollbackLines: 10_000,
    onStop: vi.fn(),
    onRunAgain: vi.fn(),
    onClear: vi.fn(),
  };
  const ref = createRef<ConsoleHandle>();
  return render(<ConsolePanel {...props} ref={ref} />);
}

describe('the Console panel', () => {
  for (const [state, header] of HEADERS) {
    it(`passes axe ${state}`, async () => {
      const { container } = renderConsole(header);
      await expectNoAxeViolations(container);
    });
  }

  it('opens each notice’s explanation from the keyboard (a details element, not a tooltip)', async () => {
    const { container } = renderConsole(RUNNING_WITH_NOTICES);
    const notices = screen.getAllByTestId('console-notice');
    expect(notices).toHaveLength(2);
    const reached = tabThrough(container);
    expect(reached).toEqual(
      expect.arrayContaining(['summary: Running with IDE helpers', 'summary: Process group only']),
    );
    for (const notice of notices) {
      expect(notice).toBeInstanceOf(HTMLDetailsElement);
      notice.setAttribute('open', '');
    }
    await expectNoAxeViolations(container);
  });

  it('says a non-zero exit in words and an icon, not only in colour', () => {
    const { container } = renderConsole(EXIT_3);
    expect(container.textContent).toContain('Finished (exit code 3)');
    const icons = [...container.querySelectorAll('[aria-hidden="true"]')].map((icon) =>
      icon.textContent.trim(),
    );
    expect(icons).toContain('⚠');
  });
});

describe('the link dialog', () => {
  it('passes axe and names its controls', async () => {
    render(<LinkDialog url="https://example.com/a?b=c" onClose={vi.fn()} />);
    const dialog = screen.getByRole('dialog', { name: 'Link from your program' });
    expect(screen.getByRole('button', { name: 'Copy link' })).toBeDefined();
    expect(screen.getByRole('button', { name: 'Close' })).toBeDefined();
    await expectNoAxeViolations(dialog);
  });
});

// --- Build output ---------------------------------------------------------------------------

describe('the Build output panel', () => {
  it('passes axe when empty, with nothing to Tab to', async () => {
    const { container } = render(<BuildOutputPanel lines={[]} />);
    expect(stopsIn(container)).toEqual([]);
    await expectNoAxeViolations(container);
  });

  it('is one named, scrollable tab stop when it has lines, also when cut short', async () => {
    const lines: BuildOutputLine[] = Array.from({ length: MAX_BUILD_OUTPUT_LINES + 2 }, (_, i) => ({
      kind: i % 2 === 0 ? 'progress' : 'raw',
      text: i === 1 ? 'x'.repeat(MAX_BUILD_LINE_CHARS + 10) : `line ${String(i)}`,
    }));
    const { container } = render(<BuildOutputPanel lines={lines} />);
    expect(screen.getByText(/2 earlier lines not shown/)).toBeDefined();
    expect(tabThrough(container)).toEqual([`log: Build output`]);
    await expectNoAxeViolations(container);
  }, 120_000);
});
