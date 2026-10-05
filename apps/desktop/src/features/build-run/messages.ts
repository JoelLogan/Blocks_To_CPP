/**
 * The user-facing text for failed build and run commands (docs/spec/02-architecture.md §2.5.5:
 * the text comes from the frontend, never from the backend's error). The log gets only the
 * command and the error code.
 */
import { type IpcError, IpcCallError, type TransportError } from '@blocks2cpp/ipc-types';

/** The error code of a failed IPC call, `transport` for a failure below the command. */
export type FailureCode = IpcError['code'] | TransportError['code'];

/** The code of `error`, or `transport` when it is not an IPC call error. */
export function failureCode(error: unknown): FailureCode {
  return error instanceof IpcCallError ? error.error.code : 'transport';
}

/** What a dialog about a failure says. */
export interface FailureMessage {
  readonly title: string;
  readonly message: string;
}

/** The text for a build (`build_start`) or a program start (`run_start`) that failed. */
export function failureMessage(code: FailureCode, action: 'build' | 'run'): FailureMessage {
  const title = action === 'build' ? 'The build could not start' : 'The program could not start';
  switch (code) {
    case 'restricted':
      return {
        title: 'Restricted Mode',
        message:
          action === 'build'
            ? 'This project is in Restricted Mode. Trust it to build it.'
            : 'This project is in Restricted Mode. Trust it to run it.',
      };
    case 'tooManySessions':
      return {
        title,
        message: 'Too many programs are running. Stop one of them, then try again.',
      };
    case 'unknownHandle':
      return { title, message: 'The project is no longer open.' };
    case 'payloadTooLarge':
      return { title, message: 'The project is too large to build.' };
    case 'invalidDocument':
    case 'newerFormat':
      return { title, message: 'The project could not be read for the build.' };
    case 'projectErrors':
      return { title, message: 'The blocks have errors. Fix them, then try again.' };
    case 'staleBuild':
    case 'buildNotSuccessful':
    case 'unknownBuild':
      return { title, message: 'The program is out of date. Build the project, then try again.' };
    case 'io':
      return {
        title,
        message:
          action === 'build'
            ? 'A file the build needs could not be read or written.'
            : 'The program file could not be started. Build the project again, then try again.',
      };
    default:
      return { title, message: 'Something went wrong. The details are in the log file.' };
  }
}
