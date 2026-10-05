/** The display and error text the recovery and external-change features share. */
import { describe, expect, it } from 'vitest';

import {
  documentProblem,
  errorCode,
  localDateTime,
  MAX_SHOWN_NAME_CHARS,
  MAX_SHOWN_PROBLEMS,
  mebibytes,
  problemLines,
  shownName,
  shownText,
  somethingWentWrong,
} from './text';
import { ipcFailure } from './testing';

describe('display text', () => {
  it('shows hidden characters and cuts long text without splitting a surrogate pair', () => {
    expect(shownText('a‮b', 100)).toBe('a⟨U+202E⟩b');
    expect(shownText('abcdef', 4)).toBe('abc…');
    // The cut would fall between the halves of 😀: the whole pair goes.
    expect(shownText('ab😀cd', 4)).toBe('ab…');
    expect(shownName('   ')).toBe('Untitled project');
    expect(shownName('  Game ')).toBe('Game');
    expect(shownName('x'.repeat(500))).toHaveLength(MAX_SHOWN_NAME_CHARS);
  });

  it('formats timestamps, refusing ones that do not parse', () => {
    expect(localDateTime('2026-10-05T10:42:00Z')).toMatch(/2026/);
    expect(localDateTime('yesterday')).toBeNull();
  });

  it('words sizes and bugs', () => {
    expect(mebibytes(33554432)).toBe('32 MiB');
    expect(somethingWentWrong('nothing happened', 'internal')).toBe(
      'Something went wrong in Blocks2Cpp, so nothing happened. (Error: internal)',
    );
    expect(errorCode(ipcFailure({ code: 'busy' }))).toBe('busy');
    expect(errorCode(new Error('x'))).toBe('bug');
  });
});

describe('loader problems', () => {
  const diagnostic = (code: string) => ({
    code,
    severity: 'error' as const,
    message: `problem ${code}`,
    primary: { part: { kind: 'whole' as const } },
    source: 'loader' as const,
  });

  it('lists at most ten problems, counts the rest, and never says nothing', () => {
    const many = Array.from({ length: 13 }, (_, index) => diagnostic(`B2C-E01${String(index)}`));
    const lines = problemLines(many);
    expect(lines).toHaveLength(MAX_SHOWN_PROBLEMS + 1);
    expect(lines.at(-1)).toBe('…and 3 more problems.');
    expect(problemLines(many.slice(0, 11)).at(-1)).toBe('…and 1 more problem.');
    expect(problemLines([])).toEqual(['B2C-E0103: The file is not a valid Blocks2Cpp project.']);
  });

  it('explains the document errors every document command shares, and only those', () => {
    expect(
      documentProblem({ code: 'invalidDocument', diagnostics: [diagnostic('B2C-E0105')] }),
    ).toBe('B2C-E0105: problem B2C-E0105');
    expect(documentProblem({ code: 'newerFormat', needs: ' 2.0.0 ' })).toContain('needs ≥ 2.0.0');
    expect(documentProblem({ code: 'newerFormat', needs: '  ' })).not.toContain('needs');
    expect(documentProblem({ code: 'payloadTooLarge', limit: 33554432 })).toContain('32 MiB');
    expect(documentProblem({ code: 'io', kind: 'other' })).toBeNull();
    expect(documentProblem({ code: 'transport', message: 'gone' })).toBeNull();
  });
});
