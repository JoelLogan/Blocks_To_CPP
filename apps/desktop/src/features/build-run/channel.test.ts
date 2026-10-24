/** The checks of build and run channel messages, which are input like any other. */
import { describe, expect, it } from 'vitest';

import { diagnosticFixture } from '../../app/testing/fixtures';
import {
  checkBuildEvent,
  checkDiagnostic,
  checkRunEvent,
  MAX_DIAGNOSTICS_PER_EVENT,
  MAX_RELATED_PER_DIAGNOSTIC,
} from './channel';

const HASH = 'c'.repeat(64);

describe('checkBuildEvent', () => {
  it('accepts the three events of the contract', () => {
    const progress = { kind: 'progress', stage: 'link', done: 1, total: 1 };
    const finished = { kind: 'finished', outcome: 'upToDate', projectHash: HASH, elapsedMs: 9 };
    expect(checkBuildEvent(progress)).toEqual(progress);
    expect(checkBuildEvent(finished)).toEqual(finished);
    expect(checkBuildEvent({ ...finished, projectHash: null })).toEqual({
      ...finished,
      projectHash: null,
    });
    expect(checkBuildEvent({ kind: 'diagnostics', items: [diagnosticFixture()] })).toEqual({
      kind: 'diagnostics',
      items: [diagnosticFixture()],
    });
  });

  it.each([
    ['not an object', 'progress'],
    ['an array', []],
    ['an unknown kind', { kind: 'started' }],
    ['an unknown stage', { kind: 'progress', stage: 'bake', done: 0, total: 1 }],
    ['a negative count', { kind: 'progress', stage: 'compile', done: -1, total: 1 }],
    ['a fractional count', { kind: 'progress', stage: 'compile', done: 0.5, total: 1 }],
    ['items that are not a list', { kind: 'diagnostics', items: {} }],
    ['an unknown outcome', { kind: 'finished', outcome: 'ok', projectHash: null, elapsedMs: 1 }],
    [
      'a hash that is not 64 hex digits',
      { kind: 'finished', outcome: 'built', projectHash: 'C'.repeat(64), elapsedMs: 1 },
    ],
    [
      'an elapsed time that is not a count',
      { kind: 'finished', outcome: 'built', projectHash: null, elapsedMs: '1' },
    ],
  ])('refuses %s', (_name, message) => {
    expect(checkBuildEvent(message)).toBeNull();
  });

  it('drops diagnostics that do not fit and keeps a bounded number', () => {
    const good = diagnosticFixture();
    const event = checkBuildEvent({
      kind: 'diagnostics',
      items: [good, { ...good, severity: 'fatal' }, null, { ...good, primary: {} }],
    });
    expect(event).toEqual({ kind: 'diagnostics', items: [good] });

    const many = checkBuildEvent({
      kind: 'diagnostics',
      items: Array.from({ length: MAX_DIAGNOSTICS_PER_EVENT + 5 }, () => good),
    });
    expect(many?.kind === 'diagnostics' ? many.items.length : 0).toBe(MAX_DIAGNOSTICS_PER_EVENT);
  });
});

describe('checkDiagnostic', () => {
  it('keeps only the contract keys and valid related locations', () => {
    const related = { location: { block: 'b1', part: { kind: 'whole' } }, message: 'here' };
    const checked = checkDiagnostic({
      ...diagnosticFixture({ raw: 'main.cpp:1:1: error: x' }),
      extra: '<script>',
      related: [related, { location: {}, message: 'bad' }, 'bad'],
    });
    expect(checked).toEqual({
      ...diagnosticFixture({ raw: 'main.cpp:1:1: error: x' }),
      related: [related],
    });
  });

  it('drops a related list that has nothing valid, and bounds it', () => {
    expect(checkDiagnostic({ ...diagnosticFixture(), related: 'x' })).toEqual(diagnosticFixture());
    const related = { location: { part: { kind: 'field', name: 'VAR' } }, message: 'm' };
    const checked = checkDiagnostic({
      ...diagnosticFixture(),
      related: Array.from({ length: MAX_RELATED_PER_DIAGNOSTIC + 3 }, () => related),
    });
    expect(checked?.related).toHaveLength(MAX_RELATED_PER_DIAGNOSTIC);
  });

  it.each([
    ['a part of an unknown kind', { primary: { part: { kind: 'all' } } }],
    ['a field part without a name', { primary: { part: { kind: 'field' } } }],
    [
      'a token part with a bad range',
      { primary: { part: { kind: 'tokens', input: 'A', start: -1, end: 2 } } },
    ],
    ['a block that is not a string', { primary: { block: 7, part: { kind: 'whole' } } }],
    ['an unknown source', { source: 'linter' }],
    ['raw text that is not a string', { raw: 5 }],
    ['a code that is not a string', { code: null }],
  ])('refuses %s', (_name, change) => {
    expect(checkDiagnostic({ ...diagnosticFixture(), ...change })).toBeNull();
  });

  it('accepts every part kind', () => {
    for (const part of [
      { kind: 'whole' },
      { kind: 'field', name: 'VAR' },
      { kind: 'input', name: 'VALUE' },
      { kind: 'tokens', input: 'COND', start: 0, end: 3 },
    ]) {
      expect(checkDiagnostic({ ...diagnosticFixture(), primary: { part } })).not.toBeNull();
    }
  });
});

describe('checkRunEvent', () => {
  const exit = {
    kind: 'exit',
    afterSeq: 3,
    elapsedMs: 10,
    status: { type: 'exited', code: 0 },
    crash: null,
    sanitizer: null,
    message: 'Finished (exit code 0)',
  };

  it('accepts the three events of the contract', () => {
    const started = {
      kind: 'started',
      containment: 'processGroupOnly',
      mode: 'pipes',
      ideHelpers: false,
    };
    const skipped = { kind: 'skipped', lines: 0, afterSeq: 2 };
    expect(checkRunEvent(started)).toEqual(started);
    expect(checkRunEvent(skipped)).toEqual(skipped);
    expect(checkRunEvent(exit)).toEqual(exit);
    for (const status of [
      { type: 'exited', code: -1 },
      { type: 'signaled', signal: 11 },
      { type: 'exception', ntstatus: 0xc00000fd },
      { type: 'stopped' },
    ]) {
      expect(checkRunEvent({ ...exit, status })).not.toBeNull();
    }
    expect(
      checkRunEvent({
        ...exit,
        crash: 'memoryAccess',
        sanitizer: { tool: 'address', kind: 'heap-buffer-overflow' },
      }),
    ).not.toBeNull();
  });

  it.each([
    ['an unknown kind', { kind: 'output' }],
    [
      'an unknown containment',
      { kind: 'started', containment: 'none', mode: 'pty', ideHelpers: true },
    ],
    ['an unknown mode', { kind: 'started', containment: 'cgroup', mode: 'tty', ideHelpers: true }],
    [
      'ideHelpers that is not a boolean',
      { kind: 'started', containment: 'cgroup', mode: 'pty', ideHelpers: 1 },
    ],
    ['a negative skipped count', { kind: 'skipped', lines: -2, afterSeq: 1 }],
    ['a missing afterSeq', { ...exit, afterSeq: undefined }],
    ['an unknown status', { ...exit, status: { type: 'vanished' } }],
    ['a fractional exit code', { ...exit, status: { type: 'exited', code: 1.5 } }],
    ['an unknown crash', { ...exit, crash: 'meltdown' }],
    ['a sanitizer of an unknown tool', { ...exit, sanitizer: { tool: 'thread', kind: 'race' } }],
    ['a sanitizer kind with markup', { ...exit, sanitizer: { tool: 'address', kind: '<b>' } }],
    ['a message that is not text', { ...exit, message: 0 }],
  ])('refuses %s', (_name, message) => {
    expect(checkRunEvent(message)).toBeNull();
  });
});
