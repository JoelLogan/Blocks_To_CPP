import { describe, expect, it } from 'vitest';

import type { Diagnostic } from '../types';
import {
  DEFAULT_PROBLEM_SORT,
  hasRawText,
  nextSort,
  problemSummary,
  sortProblems,
  type ProblemItem,
} from './problems';

function item(key: string, severity: Diagnostic['severity'], fields: Partial<ProblemItem> = {}) {
  const problem: ProblemItem = {
    key,
    diagnostic: {
      code: 'B2C-E0201',
      severity,
      message: key,
      primary: { part: { kind: 'whole' } },
      source: 'analyser',
    },
    origin: 'live',
    stale: false,
    modulePath: 'main',
    blockPath: 'main',
    ...fields,
  };
  return problem;
}

describe('sortProblems', () => {
  it('puts the most serious first by default and keeps equal items in their order', () => {
    const items = [item('w1', 'warning'), item('e1', 'error'), item('i1', 'info')];
    items.push(item('e2', 'error'));
    expect(sortProblems(items, DEFAULT_PROBLEM_SORT).map((p) => p.key)).toEqual([
      'e1',
      'e2',
      'w1',
      'i1',
    ]);
  });

  it('breaks ties by severity, then module and block path', () => {
    const items = [
      item('x', 'warning', { modulePath: 'b' }),
      item('x', 'error', { modulePath: 'b', key: 'x-error' }),
      item('x', 'error', { modulePath: 'a', key: 'x-a' }),
    ];
    const sorted = sortProblems(items, { column: 'message', direction: 'ascending' });
    expect(sorted.map((p) => p.key)).toEqual(['x-a', 'x-error', 'x']);
  });

  it('compares numbers in text naturally', () => {
    const items = [item('Problem 10', 'error'), item('Problem 9', 'error')];
    const sorted = sortProblems(items, { column: 'message', direction: 'ascending' });
    expect(sorted.map((p) => p.key)).toEqual(['Problem 9', 'Problem 10']);
  });

  it('sorts by code, module and block path', () => {
    const a = item('a', 'error', { modulePath: 'zeta', blockPath: 'main › a' });
    const b = item('b', 'error', { modulePath: 'alpha', blockPath: 'main › b' });
    b.diagnostic = { ...b.diagnostic, code: 'B2C-E0101' };
    expect(sortProblems([a, b], { column: 'code', direction: 'ascending' })[0]).toBe(b);
    expect(sortProblems([a, b], { column: 'module', direction: 'ascending' })[0]).toBe(b);
    expect(sortProblems([a, b], { column: 'block', direction: 'descending' })[0]).toBe(b);
  });
});

describe('nextSort', () => {
  it('flips the direction of the same column and starts other columns sensibly', () => {
    expect(nextSort(DEFAULT_PROBLEM_SORT, 'severity')).toEqual({
      column: 'severity',
      direction: 'ascending',
    });
    expect(nextSort(DEFAULT_PROBLEM_SORT, 'code')).toEqual({
      column: 'code',
      direction: 'ascending',
    });
    expect(nextSort({ column: 'code', direction: 'ascending' }, 'severity')).toEqual({
      column: 'severity',
      direction: 'descending',
    });
  });
});

describe('hasRawText', () => {
  it('is true only for compiler, linker and toolchain diagnostics with text', () => {
    const base = item('a', 'error').diagnostic;
    expect(hasRawText({ ...base, source: 'compiler', raw: 'x' })).toBe(true);
    expect(hasRawText({ ...base, source: 'linker', raw: 'x' })).toBe(true);
    expect(hasRawText({ ...base, source: 'toolchain', raw: 'x' })).toBe(true);
    expect(hasRawText({ ...base, source: 'compiler', raw: '' })).toBe(false);
    expect(hasRawText({ ...base, source: 'compiler' })).toBe(false);
    expect(hasRawText({ ...base, source: 'analyser', raw: 'x' })).toBe(false);
  });
});

describe('problemSummary', () => {
  it('counts by severity', () => {
    expect(problemSummary([])).toBe('No problems');
    expect(problemSummary([item('a', 'error')])).toBe('1 error');
    expect(problemSummary([item('a', 'warning'), item('b', 'warning'), item('c', 'info')])).toBe(
      '2 warnings, 1 info message',
    );
  });
});
