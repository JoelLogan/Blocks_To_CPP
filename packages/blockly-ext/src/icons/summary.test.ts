/** What a diagnostics badge says: the winning severity, the tooltip and the accessible name. */
import { describe, expect, it } from 'vitest';

import {
  type BlockDiagnosticItem,
  MAX_TOOLTIP_LINES,
  isSeverity,
  moreSerious,
  sameSummary,
  summariseDiagnostics,
} from './summary';

function item(overrides: Partial<BlockDiagnosticItem> = {}): BlockDiagnosticItem {
  return {
    code: 'B2C-E0201',
    severity: 'error',
    message: 'This uses a variable that does not exist here.',
    ...overrides,
  };
}

describe('summariseDiagnostics', () => {
  it('gives nothing for no diagnostics', () => {
    expect(summariseDiagnostics([])).toBeNull();
  });

  it('lets the most serious severity win, whatever the order', () => {
    const info = item({ severity: 'info', code: 'B2C-I0501', message: 'A hint.' });
    const warning = item({ severity: 'warning', code: 'B2C-W0501', message: 'Careful.' });
    const error = item();
    for (const order of [
      [info, warning, error],
      [error, info, warning],
      [warning, error, info],
    ]) {
      expect(summariseDiagnostics(order)?.severity).toBe('error');
    }
    expect(summariseDiagnostics([info, warning])?.severity).toBe('warning');
    expect(summariseDiagnostics([info])?.severity).toBe('info');
  });

  it('lists every message in the tooltip, the most serious first and otherwise in order', () => {
    const summary = summariseDiagnostics([
      item({ severity: 'warning', code: 'B2C-W0501', message: 'First warning.' }),
      item({ message: 'The error.' }),
      item({ severity: 'warning', code: 'B2C-W0502', message: 'Second warning.' }),
    ]);
    expect(summary?.tooltip.split('\n')).toEqual([
      '✖ Error: The error. (B2C-E0201)',
      '⚠ Warning: First warning. (B2C-W0501)',
      '⚠ Warning: Second warning. (B2C-W0502)',
    ]);
    expect(summary?.count).toBe(3);
    expect(summary?.label).toBe('Error: The error., and 2 more problems');
  });

  it('marks compiler messages from the last build and messages from blocks inside', () => {
    const summary = summariseDiagnostics([
      item({ code: 'C:error', message: 'g++ said no.', stale: true }),
      item({ severity: 'warning', code: 'B2C-W0501', message: 'Inside.', nested: true }),
    ]);
    expect(summary?.tooltip.split('\n')).toEqual([
      '✖ Error: g++ said no. (C:error) – from the last build',
      '⚠ Warning: Inside. (B2C-W0501) – in a block inside this one',
    ]);
    expect(summary?.label).toBe('Error: g++ said no. (from the last build), and 1 more problem');
    expect(summary?.dimmed).toBe(false);
  });

  it('is dimmed only when every diagnostic is from an older build', () => {
    expect(summariseDiagnostics([item({ stale: true })])?.dimmed).toBe(true);
    expect(summariseDiagnostics([item({ stale: true }), item({ stale: false })])?.dimmed).toBe(
      false,
    );
  });

  it('shortens long lists and long messages', () => {
    const many = Array.from({ length: MAX_TOOLTIP_LINES + 5 }, (_, index) =>
      item({ message: `Problem ${String(index)}.` }),
    );
    const lines = summariseDiagnostics(many)?.tooltip.split('\n') ?? [];
    expect(lines).toHaveLength(MAX_TOOLTIP_LINES + 1);
    expect(lines.at(-1)).toBe('and 5 more');

    const long = summariseDiagnostics([item({ message: 'x'.repeat(10_000) })]);
    expect(long?.tooltip.length).toBeLessThan(400);
    expect(long?.tooltip).toContain('…');
  });

  it('shows hidden characters in messages as visible placeholders, on one line', () => {
    const summary = summariseDiagnostics([
      item({ message: 'abc\u202edef\nnext', code: 'B2C-E\u200b0201' }),
    ]);
    expect(summary?.tooltip).toBe('✖ Error: abc⟨U+202E⟩def⟨U+000A⟩next (B2C-E⟨U+200B⟩0201)');
    expect(summary?.label).toBe('Error: abc⟨U+202E⟩def⟨U+000A⟩next');
  });

  it('ignores diagnostics with a severity it does not know', () => {
    const odd = { ...item(), severity: 'fatal' } as unknown as BlockDiagnosticItem;
    expect(summariseDiagnostics([odd])).toBeNull();
    expect(summariseDiagnostics([odd, item({ severity: 'info' })])?.severity).toBe('info');
  });
});

describe('severity helpers', () => {
  it('recognise the three severities only', () => {
    expect(['error', 'warning', 'info'].every(isSeverity)).toBe(true);
    expect([undefined, null, 'Error', '', 3, 'fatal'].some(isSeverity)).toBe(false);
  });

  it('pick the more serious of two', () => {
    expect(moreSerious('info', 'error')).toBe('error');
    expect(moreSerious('error', 'warning')).toBe('error');
    expect(moreSerious('warning', 'info')).toBe('warning');
    expect(moreSerious('info', 'info')).toBe('info');
  });

  it('compare summaries by what they show', () => {
    const a = summariseDiagnostics([item()]);
    const b = summariseDiagnostics([item()]);
    const c = summariseDiagnostics([item({ message: 'Other.' })]);
    if (a === null || b === null || c === null) {
      throw new Error('a summary is missing');
    }
    expect(sameSummary(a, b)).toBe(true);
    expect(sameSummary(a, c)).toBe(false);
  });
});
