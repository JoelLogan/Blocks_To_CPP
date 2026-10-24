/**
 * Text that the recovery and external-change features show, and the backend errors they explain.
 *
 * Project names, file names and loader messages come from outside the app (a project file, a
 * recovery snapshot written by an earlier session). They are always rendered as React text, never
 * as HTML, and these helpers also make invisible and reordering characters visible (`⟨U+202E⟩`,
 * docs/spec/08-security.md §8.4.6) and bound their length, so a crafted name can neither hide text
 * nor flood a dialog. The backend's errors are typed codes without paths or user text
 * (02 §2.5); the words come from here.
 */
import type { Diagnostic } from '@blocks2cpp/b2c-core-wasm';
import { IpcCallError, type IpcError, type TransportError } from '@blocks2cpp/ipc-types';

import { visibleInvisibles } from '../../panels/shared/invisibles';

/** The longest project or file name shown, in UTF-16 code units; longer ones are cut with `…`. */
export const MAX_SHOWN_NAME_CHARS = 120;

/** The longest loader message shown for one problem. */
export const MAX_SHOWN_MESSAGE_CHARS = 600;

/** The most loader problems listed in one message; the rest are counted. */
export const MAX_SHOWN_PROBLEMS = 10;

/** The longest diagnostic code shown (`B2C-E0105` is 9). */
const MAX_SHOWN_CODE_CHARS = 32;

/**
 * `text` for display: hidden characters as visible placeholders, then cut to `max` code units
 * (never inside a surrogate pair), with `…` marking the cut.
 */
export function shownText(text: string, max: number): string {
  const visible = visibleInvisibles(text);
  if (visible.length <= max) {
    return visible;
  }
  let end = Math.max(0, max - 1);
  const last = visible.charCodeAt(end - 1);
  if (last >= 0xd800 && last <= 0xdbff) {
    end -= 1;
  }
  return `${visible.slice(0, end)}…`;
}

/** A project or file name for display (see {@link shownText}); a blank name is *Untitled project*. */
export function shownName(name: string): string {
  const trimmed = name.trim();
  return shownText(trimmed === '' ? 'Untitled project' : trimmed, MAX_SHOWN_NAME_CHARS);
}

/**
 * An RFC 3339 timestamp as a local date and time in the user's format (`5 Oct 2026, 10:42`), or
 * `null` when it does not parse.
 */
export function localDateTime(timestamp: string): string | null {
  const date = new Date(timestamp);
  if (Number.isNaN(date.getTime())) {
    return null;
  }
  return new Intl.DateTimeFormat(undefined, { dateStyle: 'medium', timeStyle: 'short' }).format(
    date,
  );
}

/** The backend's typed error (or the transport failure) behind `error`, if it is an IPC failure. */
export function ipcErrorOf(error: unknown): IpcError | TransportError | null {
  return error instanceof IpcCallError ? error.error : null;
}

/** The error's code for logs: the IPC code, `transport`, or `bug` for anything else. */
export function errorCode(error: unknown): string {
  return ipcErrorOf(error)?.code ?? 'bug';
}

/** The generic sentence for a failure the user cannot fix, with its code for a bug report. */
export function somethingWentWrong(what: string, code: string): string {
  return `Something went wrong in Blocks2Cpp, so ${what}. (Error: ${shownText(code, MAX_SHOWN_CODE_CHARS)})`;
}

/** `bytes` in MiB for messages (`32 MiB`). */
export function mebibytes(bytes: number): string {
  return `${String(Math.round(bytes / (1024 * 1024)))} MiB`;
}

/**
 * The loader's problems (`B2C-E01xx`, docs/reference/diagnostics/loader-and-catalog.md) as lines
 * for a dialog: at most {@link MAX_SHOWN_PROBLEMS}, then how many more there were. A refusal
 * always names a problem; an empty list would be a bug, so it still says something.
 */
export function problemLines(diagnostics: readonly Diagnostic[]): string[] {
  if (diagnostics.length === 0) {
    return ['B2C-E0103: The file is not a valid Blocks2Cpp project.'];
  }
  const lines = diagnostics
    .slice(0, MAX_SHOWN_PROBLEMS)
    .map(
      (diagnostic) =>
        `${shownText(diagnostic.code, MAX_SHOWN_CODE_CHARS)}: ${shownText(diagnostic.message, MAX_SHOWN_MESSAGE_CHARS)}`,
    );
  const omitted = diagnostics.length - lines.length;
  if (omitted > 0) {
    lines.push(`…and ${String(omitted)} more ${omitted === 1 ? 'problem' : 'problems'}.`);
  }
  return lines;
}

/**
 * Why a document from the backend cannot be shown, for the errors every document command shares
 * (`invalidDocument`, `newerFormat`, `payloadTooLarge`); `null` for any other error.
 */
export function documentProblem(error: IpcError | TransportError): string | null {
  switch (error.code) {
    case 'invalidDocument':
      return problemLines(error.diagnostics).join('\n');
    case 'newerFormat': {
      const version = error.needs === null ? '' : shownText(error.needs.trim(), 64);
      return version === ''
        ? 'It was made with a newer version of Blocks2Cpp. Update Blocks2Cpp to open it.'
        : `It was made with a newer version of Blocks2Cpp (needs ≥ ${version}). Update Blocks2Cpp to open it.`;
    }
    case 'payloadTooLarge':
      return `It is larger than ${mebibytes(error.limit)}, far more than any real project.`;
    default:
      return null;
  }
}
