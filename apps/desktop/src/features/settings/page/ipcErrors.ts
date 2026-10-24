/**
 * Reading the errors of the IPC client. Every client method rejects with an `IpcCallError`; the
 * features turn its code into their own sentences (the backend's errors carry no user-facing
 * text, docs/spec/02-architecture.md §2.5).
 */
import { type Diagnostic, IpcCallError, type IpcError } from '@blocks2cpp/ipc-types';

/** The code of a failed call: an `IpcError` code, or `transport` for anything else. */
export type FailureCode = IpcError['code'] | 'transport';

/** The code of `error`, or `transport` when it is not an `IpcCallError` with a typed error. */
export function failureCode(error: unknown): FailureCode {
  return error instanceof IpcCallError ? error.error.code : 'transport';
}

/** The most diagnostics of one error that are shown; the rest are counted. */
export const MAX_ERROR_DIAGNOSTICS = 20;

/**
 * The diagnostics a `toolchainRejected` error carries, or `null` for any other failure. At most
 * {@link MAX_ERROR_DIAGNOSTICS} are returned.
 */
export function rejectionDiagnostics(error: unknown): Diagnostic[] | null {
  if (!(error instanceof IpcCallError) || error.error.code !== 'toolchainRejected') {
    return null;
  }
  const { diagnostics } = error.error;
  return Array.isArray(diagnostics) ? diagnostics.slice(0, MAX_ERROR_DIAGNOSTICS) : [];
}
