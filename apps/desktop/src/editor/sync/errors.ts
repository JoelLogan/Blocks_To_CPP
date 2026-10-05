/** Errors of the document sync. */

/**
 * Why the sync could not do what was asked:
 * - `unknownModule`: the document has no module with the given ID;
 * - `noDocument`: no project is open in the editor.
 *
 * Problems inside a document are never errors: blocks the editor cannot show are kept as
 * placeholders, and the analyser reports what is wrong with them.
 */
export type SyncErrorKind = 'unknownModule' | 'noDocument';

/** The document sync was asked for something it cannot do; see {@link SyncError.kind}. */
export class SyncError extends Error {
  override readonly name = 'SyncError';
  readonly kind: SyncErrorKind;

  constructor(kind: SyncErrorKind, message: string) {
    super(message);
    this.kind = kind;
  }
}
