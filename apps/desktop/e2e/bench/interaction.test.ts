/**
 * The benchmarks' interaction drivers (interaction.ts): what an edit and a click measure, from
 * what the page scripts call back with, that failures fail the benchmark with a reason, and that
 * a click after which the toolbox lists the wrong scope is caught.
 */
import { readdirSync, readFileSync } from 'node:fs';
import path from 'node:path';

import { describe, expect, it } from 'vitest';

import {
  clickMeasurement,
  editMeasurement,
  INTERACTION_TIMEOUT_MS,
  listingMismatch,
  runMeasurement,
  spanSummary,
  spanTimes,
  SUMMARY_SPANS,
} from './interaction';
import { PREVIEW_DEBOUNCE_MS } from './page';

describe('editMeasurement', () => {
  const seen = {
    kind: 'seen',
    begin: 0,
    inserted: 10,
    seen: 900,
    task: { start: 780, ms: 110 },
    settled: 1_260,
    longest: { start: 100, ms: 600 },
    spans: [
      { name: 'b2c:preview', start: 780, ms: 110 },
      { name: 'b2c:code-panel', start: 950, ms: 30 },
    ],
  } as const;

  it('takes the preview task, the times less the debounce, the longest task and the spans', () => {
    expect(editMeasurement('b1', seen)).toEqual({
      id: 'b1',
      previewMs: 110,
      editMs: 900 - 10 - PREVIEW_DEBOUNCE_MS,
      settledMs: 1_260 - 10 - PREVIEW_DEBOUNCE_MS,
      longestMs: 600,
      spans: { 'b2c:preview': 110, 'b2c:code-panel': 30 },
    });
  });

  it('has no preview task when it does not fit, and 0 when nothing was long', () => {
    const measured = editMeasurement('b1', {
      ...seen,
      task: { start: 0, ms: 5 },
      longest: null,
      seen: 20,
      settled: 20,
    });
    expect(measured.previewMs).toBeNull();
    expect(measured.longestMs).toBe(0);
    expect(measured.editMs).toBe(0);
  });

  it('fails with the reason when the edit failed', () => {
    expect(() => editMeasurement('b1', { kind: 'error', message: 'No input BODY' })).toThrow(
      'The edit failed: No input BODY',
    );
    expect(() => editMeasurement('b1', { kind: 'timeout' })).toThrow(
      `within ${String(INTERACTION_TIMEOUT_MS / 1000)} s`,
    );
    expect(() => editMeasurement('b1', 'seen')).toThrow('unexpected');
  });
});

describe('the benchmarks', () => {
  it('turn the long-task recorder off: nothing else may run in the page while they time it', () => {
    const files = readdirSync(import.meta.dirname).filter((name) => name.endsWith('.bench.e2e.ts'));
    expect(files.length).toBeGreaterThanOrEqual(5);
    for (const file of files) {
      const source = readFileSync(path.join(import.meta.dirname, file), 'utf8');
      for (const call of source.matchAll(/launchApp\(([^;]*)\);/g)) {
        expect(call[1], file).toContain('recordLongTasks: false');
      }
    }
  });
});

describe('clickMeasurement', () => {
  it('takes the longest task after the press and the time until the page settled', () => {
    const result = {
      kind: 'settled',
      pressed: 1_000,
      settled: 1_480,
      tasks: [
        { start: 400, ms: 300 },
        { start: 990, ms: 40 },
        { start: 1_100, ms: 380 },
      ],
      spans: [
        { name: 'b2c:toolbox-refresh', start: 400, ms: 290 },
        { name: 'b2c:select', start: 1_001, ms: 12 },
        { name: 'b2c:toolbox-refresh', start: 1_100, ms: 370 },
      ],
    };
    // A span that began before the press is not the click's.
    expect(clickMeasurement('b9', result)).toEqual({
      longestMs: 380,
      respondMs: 480,
      spans: { 'b2c:select': 12, 'b2c:toolbox-refresh': 370 },
    });
    expect(clickMeasurement('b9', { ...result, tasks: [], spans: [] })).toEqual({
      longestMs: 0,
      respondMs: 480,
      spans: {},
    });
  });

  it('fails with the reason when the click was not measured', () => {
    expect(() => clickMeasurement('b9', { kind: 'no-press' })).toThrow(
      'The click on block b9 never reached the page',
    );
    expect(() => clickMeasurement('b9', { kind: 'busy' })).toThrow('still busy');
    expect(() => clickMeasurement('b9', { kind: 'error', message: 'No recorder' })).toThrow(
      'No recorder',
    );
    expect(() => clickMeasurement('b9', null)).toThrow('unexpected');
  });
});

describe('spanTimes', () => {
  it('adds up the time of each span name from a point on, and ignores what is not a span', () => {
    const spans = [
      { name: 'b2c:preview', start: 10, ms: 5 },
      { name: 'b2c:preview', start: 40, ms: 7.5 },
      { name: 'b2c:select', start: 50, ms: 1 },
      { name: 'b2c:early', start: 9, ms: 100 },
      { name: 'other', start: 60, ms: 3 },
      { name: 'b2c:bad', start: 60, ms: -1 },
      { name: 'b2c:nan', start: 60, ms: Number.NaN },
      { name: 7, start: 60, ms: 3 },
      null,
    ];
    expect(spanTimes(spans, 10)).toEqual({ 'b2c:preview': 12.5, 'b2c:select': 1 });
    expect(spanTimes('spans', 0)).toEqual({});
    expect(spanTimes(undefined, 0)).toEqual({});
  });
});

describe('runMeasurement', () => {
  const result = {
    kind: 'settled',
    pressed: 1_000,
    settled: 2_650,
    until: 2_600,
    tasks: [
      { start: 1_200, ms: 30 },
      { start: 2_590, ms: 60 },
    ],
    spans: [{ name: 'b2c:select', start: 900, ms: 1 }],
  };

  it('takes the time until the console said Finished, and the longest task until it settled', () => {
    expect(runMeasurement(result)).toEqual({ longestMs: 60, runMs: 1_600, spans: {} });
  });

  it('fails when the console’s state was not waited for, or the run was not measured', () => {
    expect(() => runMeasurement({ ...result, until: null })).toThrow('without waiting');
    expect(() => runMeasurement({ kind: 'no-press' })).toThrow('never reached the page');
    expect(() => runMeasurement({ kind: 'busy' })).toThrow('had not finished');
    expect(() => runMeasurement({ kind: 'error', message: 'Not a regular expression' })).toThrow(
      'Not a regular expression',
    );
    expect(() => runMeasurement('settled')).toThrow('unexpected');
  });
});

describe('spanSummary', () => {
  it('names the spans with the longest medians, a span missing from an interaction counting 0', () => {
    const summary = spanSummary([
      { 'b2c:toolbox-refresh': 900, 'b2c:select': 2 },
      { 'b2c:toolbox-refresh': 1_100, 'b2c:select': 3, 'b2c:code-panel': 40 },
      { 'b2c:select': 2.25 },
    ]);
    expect(summary).toBe('b2c:toolbox-refresh 900 ms, b2c:select 2.3 ms, b2c:code-panel 0 ms');
  });

  it('names at most a few spans, and says when there were none', () => {
    const many = Object.fromEntries(
      Array.from({ length: SUMMARY_SPANS + 2 }, (_, index) => [`b2c:s${String(index)}`, index]),
    );
    expect(spanSummary([many]).split(', ')).toHaveLength(SUMMARY_SPANS);
    expect(spanSummary([{}, {}])).toMatch(/^no User Timing spans/);
    expect(spanSummary([])).toMatch(/^no User Timing spans/);
  });
});

describe('listingMismatch', () => {
  const target = { id: 'b6', lists: ['s_secret', 's_guess'], omits: ['s_other'] };

  it('accepts the target’s listing, whatever else the toolbox shows', () => {
    expect(listingMismatch(target, new Set(['s_secret', 's_guess']))).toBeNull();
    expect(listingMismatch({ id: 'b1', lists: [], omits: [] }, new Set())).toBeNull();
  });

  it('names the getters missing and those shown that should not be', () => {
    expect(listingMismatch(target, new Set(['s_secret', 's_other']))).toBe(
      'After the click on block b6 the toolbox shows no getter of s_guess and a getter of ' +
        's_other: it was not rebuilt for the new selection while the click was timed',
    );
    expect(listingMismatch(target, new Set())).toMatch(/no getter of s_secret, s_guess:/);
    expect(listingMismatch(target, new Set(['s_secret', 's_guess', 's_other']))).toMatch(
      /shows a getter of s_other:/,
    );
  });
});
