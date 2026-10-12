/**
 * The benchmarks' interaction drivers (interaction.ts): what an edit and a click measure, from
 * what the page scripts call back with, and that failures fail the benchmark with a reason.
 */
import { readdirSync, readFileSync } from 'node:fs';
import path from 'node:path';

import { describe, expect, it } from 'vitest';

import { clickMeasurement, editMeasurement, INTERACTION_TIMEOUT_MS } from './interaction';
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
  } as const;

  it('takes the preview task, the times less the debounce and the longest task', () => {
    expect(editMeasurement('b1', seen)).toEqual({
      id: 'b1',
      previewMs: 110,
      editMs: 900 - 10 - PREVIEW_DEBOUNCE_MS,
      settledMs: 1_260 - 10 - PREVIEW_DEBOUNCE_MS,
      longestMs: 600,
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
    };
    expect(clickMeasurement('b9', result)).toEqual({ longestMs: 380, respondMs: 480 });
    expect(clickMeasurement('b9', { ...result, tasks: [] })).toEqual({
      longestMs: 0,
      respondMs: 480,
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
