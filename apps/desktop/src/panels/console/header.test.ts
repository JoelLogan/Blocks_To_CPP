import { describe, expect, it } from 'vitest';

import type { RunExit } from '../types';
import {
  describeStatus,
  exitTone,
  formatElapsed,
  headerStatus,
  MAX_EXIT_MESSAGE_CHARS,
  skippedMarker,
  type ConsoleHeader,
} from './header';

function exited(exit: Partial<RunExit> & Pick<RunExit, 'status'>): ConsoleHeader {
  return {
    state: 'exited',
    exit: {
      kind: 'exit',
      afterSeq: 3,
      elapsedMs: 1200,
      crash: null,
      sanitizer: null,
      message: describeStatus(exit.status),
      ...exit,
    },
    elapsedMs: 1200,
    notices: [],
  };
}

describe('headerStatus', () => {
  it('names the idle and running states', () => {
    expect(headerStatus({ state: 'idle', exit: null, elapsedMs: 0, notices: [] })).toEqual({
      text: 'Not running',
      tone: 'neutral',
      icon: '',
    });
    expect(headerStatus({ state: 'running', exit: null, elapsedMs: 5, notices: [] }).text).toBe(
      'Running',
    );
  });

  it('shows each kind of exit with the 07 §7.6.4 text and highlights all but success and Stop', () => {
    const cases: [ConsoleHeader, string, string][] = [
      [exited({ status: { type: 'exited', code: 0 } }), 'Finished (exit code 0)', 'ok'],
      [exited({ status: { type: 'exited', code: 3 } }), 'Finished with exit code 3', 'warning'],
      [exited({ status: { type: 'stopped' } }), 'Stopped', 'neutral'],
      [
        exited({
          status: { type: 'signaled', signal: 11 },
          crash: 'memoryAccess',
          message:
            "Crashed: the program tried to use memory it doesn't own (segmentation fault / access violation). (SIGSEGV)",
        }),
        "Crashed: the program tried to use memory it doesn't own (segmentation fault / access violation). (SIGSEGV)",
        'error',
      ],
      [
        exited({
          status: { type: 'exception', ntstatus: 0xc00000fd },
          crash: 'stackOverflow',
          message: 'Crashed: stack overflow, probably infinite recursion (exception 0xC00000FD)',
        }),
        'Crashed: stack overflow, probably infinite recursion (exception 0xC00000FD)',
        'error',
      ],
      [
        exited({
          status: { type: 'signaled', signal: 8 },
          crash: 'divisionByZero',
          message: 'Crashed: integer division by zero (SIGFPE)',
        }),
        'Crashed: integer division by zero (SIGFPE)',
        'error',
      ],
      [
        exited({
          status: { type: 'signaled', signal: 6 },
          crash: 'aborted',
          message: 'Stopped itself: an uncaught error or failed check (SIGABRT)',
        }),
        'Stopped itself: an uncaught error or failed check (SIGABRT)',
        'error',
      ],
      [
        exited({
          status: { type: 'signaled', signal: 6 },
          sanitizer: { tool: 'address', kind: 'heap-buffer-overflow' },
          message: 'Crashed: heap-buffer-overflow (AddressSanitizer)',
        }),
        'Crashed: heap-buffer-overflow (AddressSanitizer)',
        'error',
      ],
    ];
    for (const [header, text, tone] of cases) {
      const status = headerStatus(header);
      expect(status.text).toBe(text);
      expect(status.tone).toBe(tone);
      expect(status.icon).not.toBe('');
    }
  });

  it('falls back to its own wording, and shortens and cleans the message', () => {
    expect(headerStatus(exited({ status: { type: 'exited', code: 0 }, message: ' ' })).text).toBe(
      'Finished (exit code 0)',
    );
    const long = 'x'.repeat(MAX_EXIT_MESSAGE_CHARS + 50);
    expect(headerStatus(exited({ status: { type: 'stopped' }, message: long })).text).toBe(
      `${'x'.repeat(MAX_EXIT_MESSAGE_CHARS)}…`,
    );
    expect(headerStatus(exited({ status: { type: 'stopped' }, message: 'Stopped‮' })).text).toBe(
      'Stopped⟨U+202E⟩',
    );
    expect(headerStatus({ state: 'exited', exit: null, elapsedMs: 0, notices: [] }).text).toBe(
      'Finished',
    );
  });
});

describe('describeStatus and exitTone', () => {
  it('words every status', () => {
    expect(describeStatus({ type: 'exited', code: 0 })).toBe('Finished (exit code 0)');
    expect(describeStatus({ type: 'exited', code: -1 })).toBe('Finished with exit code -1');
    expect(describeStatus({ type: 'stopped' })).toBe('Stopped');
    expect(describeStatus({ type: 'signaled', signal: 9 })).toBe('Crashed (signal 9)');
    expect(describeStatus({ type: 'exception', ntstatus: 0xc0000005 })).toBe(
      'Crashed (exception 0xC0000005)',
    );
    expect(exitTone({ type: 'exception', ntstatus: 1 })).toBe('error');
  });
});

describe('formatElapsed', () => {
  it('shows minutes and seconds, and hours when needed', () => {
    expect(formatElapsed(0)).toBe('0:00');
    expect(formatElapsed(5_999)).toBe('0:05');
    expect(formatElapsed(65_000)).toBe('1:05');
    expect(formatElapsed(3_723_000)).toBe('1:02:03');
    expect(formatElapsed(-5)).toBe('0:00');
    expect(formatElapsed(Number.NaN)).toBe('0:00');
  });
});

describe('skippedMarker', () => {
  it('states the exact count on a line of its own', () => {
    expect(skippedMarker(1_204_331)).toBe(
      '\r\n\u001b[0;7m … 1,204,331 lines skipped \u001b[0m\r\n',
    );
    expect(skippedMarker(1)).toContain('… 1 line skipped');
    expect(skippedMarker(-3)).toBeNull();
    expect(skippedMarker(1.5)).toBeNull();
    expect(skippedMarker(Number.NaN)).toBeNull();
  });

  it('marks dropped output that held no line break (07 §7.6.5: lines can be 0)', () => {
    expect(skippedMarker(0)).toBe('\r\n\u001b[0;7m … output skipped \u001b[0m\r\n');
  });
});
