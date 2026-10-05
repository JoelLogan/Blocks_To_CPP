/**
 * The console header's texts (docs/spec/04-user-interface.md §4.5, 07 §7.6.4): the run state, the
 * exit text with a non-zero exit highlighted, the elapsed time and the notices.
 */
import { visibleInvisibles } from '../shared/invisibles';
import type { ExitStatus, RunExit } from '../types';

/** Where the program is. */
export type ConsoleState = 'idle' | 'running' | 'exited';

/** Notices next to the state: IDE helpers linked in, and no cgroup on Linux (08 §8.14). */
export type ConsoleNotice = 'ideHelpers' | 'processGroupOnly';

/** What the header shows. */
export interface ConsoleHeader {
  state: ConsoleState;
  /** How the last run ended; only used in the `exited` state. */
  exit: RunExit | null;
  /** How long the current or last run took. */
  elapsedMs: number;
  notices: readonly ConsoleNotice[];
}

/** How an exit is highlighted; the icon and the text always say the same as the colour. */
export type ExitTone = 'neutral' | 'ok' | 'warning' | 'error';

/** The state line of the header. */
export interface HeaderStatus {
  readonly text: string;
  readonly tone: ExitTone;
  /** A symbol before the text, hidden from screen readers (the text says it all). */
  readonly icon: string;
}

/** The most characters of the backend's exit message shown. */
export const MAX_EXIT_MESSAGE_CHARS = 300;

/** The text of each notice. */
export const NOTICE_TEXT: Readonly<Record<ConsoleNotice, string>> = {
  ideHelpers: 'Running with IDE helpers',
  processGroupOnly: 'Process group only',
};

/** A longer explanation of each notice, for its tooltip. */
export const NOTICE_DETAIL: Readonly<Record<ConsoleNotice, string>> = {
  ideHelpers:
    'A small helper is linked into IDE runs (it sets up the console); the C++ you see is unchanged.',
  processGroupOnly:
    'This Linux system cannot put the program in a cgroup, so it is contained by its process group only.',
};

/** The state line for a header. */
export function headerStatus(header: ConsoleHeader): HeaderStatus {
  switch (header.state) {
    case 'idle':
      return { text: 'Not running', tone: 'neutral', icon: '' };
    case 'running':
      return { text: 'Running', tone: 'neutral', icon: '▶' };
    case 'exited':
      return header.exit === null
        ? { text: 'Finished', tone: 'neutral', icon: '' }
        : exitStatus(header.exit);
  }
}

function exitStatus(exit: RunExit): HeaderStatus {
  const tone = exitTone(exit.status);
  const icon = { neutral: '■', ok: '✓', warning: '⚠', error: '✖' }[tone];
  const message = exit.message.trim();
  const text =
    message === ''
      ? describeStatus(exit.status)
      : visibleInvisibles(
          message.length > MAX_EXIT_MESSAGE_CHARS
            ? `${message.slice(0, MAX_EXIT_MESSAGE_CHARS)}…`
            : message,
        );
  return { text, tone, icon };
}

/** Exit code 0 is good, another exit code is highlighted, a crash is an error, a stop is neutral. */
export function exitTone(status: ExitStatus): ExitTone {
  switch (status.type) {
    case 'exited':
      return status.code === 0 ? 'ok' : 'warning';
    case 'stopped':
      return 'neutral';
    case 'signaled':
    case 'exception':
      return 'error';
  }
}

/**
 * The 07 §7.6.4 wording for a status, used only if the backend's message is missing (it always
 * sends one; the full crash texts come from `b2c_process::ExitStatus::describe`).
 */
export function describeStatus(status: ExitStatus): string {
  switch (status.type) {
    case 'exited':
      return status.code === 0
        ? 'Finished (exit code 0)'
        : `Finished with exit code ${String(status.code)}`;
    case 'stopped':
      return 'Stopped';
    case 'signaled':
      return `Crashed (signal ${String(status.signal)})`;
    case 'exception':
      return `Crashed (exception 0x${(status.ntstatus >>> 0).toString(16).toUpperCase().padStart(8, '0')})`;
  }
}

/** Elapsed time as `m:ss`, or `h:mm:ss` from an hour on. */
export function formatElapsed(ms: number): string {
  const total = Number.isFinite(ms) && ms > 0 ? Math.floor(ms / 1000) : 0;
  const seconds = total % 60;
  const minutes = Math.floor(total / 60) % 60;
  const hours = Math.floor(total / 3600);
  const ss = String(seconds).padStart(2, '0');
  return hours > 0
    ? `${String(hours)}:${String(minutes).padStart(2, '0')}:${ss}`
    : `${String(minutes)}:${ss}`;
}

/**
 * The marker written into the terminal where output was dropped (07 §7.6.5), in inverse video on
 * a line of its own: `… 1,204,331 lines skipped`. `null` for a count that is not a positive integer.
 */
export function skippedMarker(lines: number): string | null {
  if (!Number.isSafeInteger(lines) || lines <= 0) {
    return null;
  }
  const count = lines.toLocaleString('en-US');
  return `\r\n\u001b[0;7m … ${count} ${lines === 1 ? 'line' : 'lines'} skipped \u001b[0m\r\n`;
}
