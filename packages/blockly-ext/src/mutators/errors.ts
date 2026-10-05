/**
 * Typed errors of the mutators. Nothing here is shown to the user as is; the editor maps the
 * `problem` to its own text.
 */

/** Why an `extra` value cannot be shown on a block. */
export type MutatorStateProblem =
  /** `extra` is not a JSON object. */
  | 'notAnObject'
  /** A key the block's catalog schema does not have. */
  | 'unknownKey'
  /** A count that is not a whole number from 0 to `MAX_VARIADIC_PARTS`. */
  | 'badCount'
  /** A flag that is not `true` or `false`. */
  | 'badFlag'
  /** `params` that is not an array of at most `MAX_VARIADIC_PARTS` rows. */
  | 'badParams'
  /** A parameter row with a missing, unknown or invalid key. */
  | 'badParamRow';

/**
 * An `extra` value a block cannot represent (05 §5.4). The block is left unchanged; the editor can
 * keep the project data verbatim (for example in a placeholder block) and report it.
 */
export class MutatorStateError extends Error {
  override readonly name = 'MutatorStateError';
  /** What is wrong. */
  readonly problem: MutatorStateProblem;
  /** The `extra` key concerned, when there is one. */
  readonly key: string | null;

  constructor(problem: MutatorStateProblem, key: string | null, message: string) {
    super(message);
    this.problem = problem;
    this.key = key;
  }
}

/**
 * A mutator applied to a block whose catalog definition does not fit it: a programming error in
 * block registration, raised when such a block is created.
 */
export class MutatorConfigError extends Error {
  override readonly name = 'MutatorConfigError';
  /** The block type. */
  readonly blockType: string;

  constructor(blockType: string, message: string) {
    super(message);
    this.blockType = blockType;
  }
}
