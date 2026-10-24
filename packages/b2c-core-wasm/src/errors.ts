/** Errors thrown by the compiler core's TypeScript wrapper. */

/**
 * The WebAssembly instance stopped: a trap (`RuntimeError: unreachable` after a Rust panic, an
 * out-of-bounds access, running out of memory) or any other exception from inside a call. The
 * instance's memory may be inconsistent, so it is never used again: every later call on it throws
 * `CoreTrap` too, and `initCore()` starts a fresh instance. A trap is always a bug in Blocks2Cpp,
 * never a problem in the project.
 */
export class CoreTrap extends Error {
  override readonly name = 'CoreTrap';
}

/**
 * Why a call failed without stopping the instance:
 * - `invalidOptions`: the core refused the preview options;
 * - `invalidArguments`: the core refused another argument: a malformed or oversized block ID
 *   list, paste target or seed, or one that names a module or block the document does not have;
 * - `encode`: the core could not encode its result (a bug);
 * - `internal`: the core could not finish for another reason that is a bug (for example no fresh
 *   ID could be found);
 * - `protocol`: the core returned something the wrapper does not understand (a bug, or a wrapper
 *   built for another version of the core);
 * - `init`: the core could not be started (the module is missing, corrupt, or cannot be
 *   compiled under the page's content security policy).
 */
export type CoreErrorKind =
  'invalidOptions' | 'invalidArguments' | 'encode' | 'internal' | 'protocol' | 'init';

/** A call or the start-up failed; see `kind`. The instance itself, if any, is still usable. */
export class CoreError extends Error {
  override readonly name = 'CoreError';
  readonly kind: CoreErrorKind;

  constructor(kind: CoreErrorKind, message: string, options?: ErrorOptions) {
    super(message, options);
    this.kind = kind;
  }
}
