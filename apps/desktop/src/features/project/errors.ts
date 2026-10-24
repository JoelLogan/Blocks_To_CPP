/**
 * What the project feature tells the user when a backend command fails (docs/spec/04-user-interface.md
 * §4.10). The backend's errors are typed codes without paths or user text (02 §2.5); the words
 * come from here. A file that does not load shows the loader's own `B2C-E01xx` problems
 * (docs/reference/diagnostics/loader-and-catalog.md), and nothing opens.
 */
import type { Diagnostic } from '@blocks2cpp/b2c-core-wasm';
import { IpcCallError, type IpcError, type TransportError } from '@blocks2cpp/ipc-types';

import { MAX_SHOWN_MESSAGE_CHARS, shownText } from './text';

/** The most loader problems listed for one file; the rest are counted. */
export const MAX_SHOWN_PROBLEMS = 20;

/** The loader's code for a project made with a newer version (05 §5.7). */
export const NEWER_FORMAT_CODE = 'B2C-E0108';

/** The loader's code for a project file over the size limit (05 §5.6). */
export const TOO_LARGE_CODE = 'B2C-E0101';

/** The longest version text shown from a `newerFormat` error. */
const MAX_VERSION_CHARS = 64;

/** One problem of a file that did not load. */
export interface LoadProblem {
  /** The diagnostic code, for example `B2C-E0105`. */
  readonly code: string;
  /** The loader's message, made safe for display. */
  readonly message: string;
}

/** The problems of a file that did not load, as the user sees them. */
export interface LoadProblems {
  /** At most {@link MAX_SHOWN_PROBLEMS}, in the loader's order. */
  readonly problems: readonly LoadProblem[];
  /** How many more problems there were. */
  readonly omitted: number;
}

/** What a failed open means for the user. */
export type OpenFailure =
  /** The file is not a project this version can open: show its problems. */
  | ({ readonly kind: 'load' } & LoadProblems)
  /** The file is gone (a recent-list entry whose file was moved or deleted). */
  | { readonly kind: 'notFound' }
  /** The recent-list entry no longer exists in the backend's list. */
  | { readonly kind: 'unknownRecent' }
  /** Anything else: a sentence to show. */
  | { readonly kind: 'message'; readonly message: string };

/** The backend's typed error (or the transport failure) behind `error`, if it is an IPC failure. */
export function ipcErrorOf(error: unknown): IpcError | TransportError | null {
  return error instanceof IpcCallError ? error.error : null;
}

/** The error's code for logs: the IPC code, `transport`, or `bug` for anything else. */
export function errorCode(error: unknown): string {
  return ipcErrorOf(error)?.code ?? 'bug';
}

/** E0108's message (05 §5.7): *made with a newer version of Blocks2Cpp (needs ≥ X)*. */
export function newerFormatMessage(needs: string | null): string {
  const version = needs === null ? '' : shownText(needs.trim(), MAX_VERSION_CHARS);
  return version === ''
    ? 'This project was made with a newer version of Blocks2Cpp. Update Blocks2Cpp to open it.'
    : `This project was made with a newer version of Blocks2Cpp (needs ≥ ${version}). Update Blocks2Cpp to open it.`;
}

/** The loader's diagnostics as problems to show, at most {@link MAX_SHOWN_PROBLEMS}. */
export function loadProblems(diagnostics: readonly Diagnostic[]): LoadProblems {
  const problems = diagnostics.slice(0, MAX_SHOWN_PROBLEMS).map((diagnostic) => ({
    code: shownText(diagnostic.code, 32),
    message: shownText(diagnostic.message, MAX_SHOWN_MESSAGE_CHARS),
  }));
  if (problems.length === 0) {
    // A refusal always names a problem; an empty list would be a backend bug. Say something.
    return {
      problems: [{ code: 'B2C-E0103', message: 'The file is not a valid Blocks2Cpp project.' }],
      omitted: 0,
    };
  }
  return { problems, omitted: Math.max(0, diagnostics.length - problems.length) };
}

/** `bytes` in MiB for messages (`32 MiB`). */
function mebibytes(bytes: number): string {
  return `${String(Math.round(bytes / (1024 * 1024)))} MiB`;
}

/** The generic sentence for a failure the user cannot fix, with its code for a bug report. */
function somethingWentWrong(what: string, code: string): string {
  return `Something went wrong in Blocks2Cpp, so ${what}. (Error: ${code})`;
}

/** What a failed `project_new`, `project_open_dialog` or `project_open_recent` means. */
export function describeOpenError(error: unknown): OpenFailure {
  const ipcError = ipcErrorOf(error);
  if (ipcError === null) {
    return { kind: 'message', message: somethingWentWrong('the project was not opened', 'bug') };
  }
  switch (ipcError.code) {
    case 'invalidDocument':
      return { kind: 'load', ...loadProblems(ipcError.diagnostics) };
    case 'newerFormat':
      return {
        kind: 'load',
        problems: [{ code: NEWER_FORMAT_CODE, message: newerFormatMessage(ipcError.needs) }],
        omitted: 0,
      };
    case 'payloadTooLarge':
      return {
        kind: 'load',
        problems: [
          {
            code: TOO_LARGE_CODE,
            message: `The project file is larger than ${mebibytes(ipcError.limit)}, far more than any real project, so it was not opened.`,
          },
        ],
        omitted: 0,
      };
    case 'notFound':
      return { kind: 'notFound' };
    case 'unknownRecent':
      return { kind: 'unknownRecent' };
    case 'io':
      switch (ipcError.kind) {
        case 'notFound':
          return { kind: 'notFound' };
        case 'permissionDenied':
          return {
            kind: 'message',
            message: 'Blocks2Cpp is not allowed to read this file, so it was not opened.',
          };
        default:
          return { kind: 'message', message: 'The file could not be read, so it was not opened.' };
      }
    case 'busy':
      return { kind: 'message', message: 'Another dialog is already open. Close it first.' };
    case 'tooManyHandles':
      return {
        kind: 'message',
        message: 'Too many projects are open in Blocks2Cpp. Close one and try again.',
      };
    default:
      return {
        kind: 'message',
        message: somethingWentWrong('the project was not opened', ipcError.code),
      };
  }
}

/**
 * What a failed `project_save` or `project_save_as_dialog` means. `noPath` and `changedOnDisk`
 * are not errors for the user; the caller handles them before asking for a message.
 */
export function describeSaveError(error: unknown): string {
  const ipcError = ipcErrorOf(error);
  if (ipcError === null) {
    return somethingWentWrong('the project was not saved', 'bug');
  }
  switch (ipcError.code) {
    case 'payloadTooLarge':
      return `The project was not saved: a project file can be at most ${mebibytes(ipcError.limit)}.`;
    case 'invalidDocument':
    case 'newerFormat':
      return `The project was not saved, because Blocks2Cpp made a file it cannot read back. This is a bug in Blocks2Cpp. (Error: ${ipcError.code})`;
    case 'io':
      switch (ipcError.kind) {
        case 'permissionDenied':
          return 'Blocks2Cpp is not allowed to write this file. Use Save as… to save the project somewhere else.';
        default:
          return 'The file could not be written. Check that the disk is not full, or use Save as… to save the project somewhere else.';
      }
    case 'busy':
      return 'Another dialog is already open. Close it first.';
    case 'unknownHandle':
      return somethingWentWrong('the project is no longer open and was not saved', 'unknownHandle');
    default:
      return somethingWentWrong('the project was not saved', ipcError.code);
  }
}

/** What a failed `app_quit` means. */
export function describeQuitError(error: unknown): string {
  return somethingWentWrong('the app could not quit', errorCode(error));
}
