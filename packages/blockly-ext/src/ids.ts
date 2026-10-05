/**
 * IDs for everything the editor creates (docs/spec/05-project-format.md §5.4–5.5, §5.12): a type
 * prefix and 17 random base62 characters (about 101 bits) from the platform's CSPRNG, for example
 * `blk_3fK9q…`. They match the project format's `[A-Za-z0-9_]{1,32}`.
 *
 * IDs are made here, in TypeScript, because the compiler core (Rust and its WASM build) uses no
 * randomness. Blockly's own generator is replaced, so blocks Blockly creates (from the toolbox,
 * duplicate or paste) never get Blockly-style IDs, which contain characters a project file rejects.
 */
import * as Blockly from 'blockly/core';

/** The kinds of ID and their prefixes. */
export type IdKind = 'blk' | 'sym' | 'mod' | 'prj';

/** Random characters after the prefix. */
export const ID_RANDOM_LENGTH = 17;

/** The base62 alphabet. */
const ALPHABET = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789';

/**
 * Bytes at or above this value are rejected, so that `byte % 62` is uniform: 248 is the largest
 * multiple of 62 that fits in a byte.
 */
const REJECT_FROM = 248;

/** What a generated ID of each kind looks like. */
export const GENERATED_ID_PATTERN: Readonly<Record<IdKind, RegExp>> = Object.freeze({
  blk: /^blk_[A-Za-z0-9]{17}$/,
  sym: /^sym_[A-Za-z0-9]{17}$/,
  mod: /^mod_[A-Za-z0-9]{17}$/,
  prj: /^prj_[A-Za-z0-9]{17}$/,
});

/** Any ID a project file accepts (`b2c_ir::ids`): 1–32 characters from A–Z, a–z, 0–9 and `_`. */
export const PROJECT_ID_PATTERN = /^[A-Za-z0-9_]{1,32}$/;

/** Whether `value` is an ID a project file accepts. */
export function isProjectId(value: unknown): value is string {
  return typeof value === 'string' && PROJECT_ID_PATTERN.test(value);
}

/** The ID generator could not do its job: no CSPRNG, or Blockly ignored the replacement. */
export class IdGeneratorError extends Error {
  override readonly name = 'IdGeneratorError';
}

/**
 * A fresh random ID of the given kind: the prefix, `_`, and {@link ID_RANDOM_LENGTH} base62
 * characters from `crypto.getRandomValues`.
 *
 * @throws IdGeneratorError if the platform has no `crypto.getRandomValues` (never in the
 *   supported webviews); a weaker source is never used instead.
 */
export function newId(kind: IdKind): string {
  return `${kind}_${randomBase62(ID_RANDOM_LENGTH)}`;
}

function randomBase62(length: number): string {
  const cryptoApi = globalThis.crypto as Crypto | undefined;
  if (cryptoApi === undefined || typeof cryptoApi.getRandomValues !== 'function') {
    throw new IdGeneratorError('No cryptographic random number generator is available.');
  }
  let out = '';
  // About 3% of bytes are rejected, so 32 bytes almost always give 17 characters in one round.
  const buffer = new Uint8Array(32);
  while (out.length < length) {
    cryptoApi.getRandomValues(buffer);
    for (const byte of buffer) {
      if (byte < REJECT_FROM) {
        out += ALPHABET.charAt(byte % ALPHABET.length);
        if (out.length === length) {
          break;
        }
      }
    }
  }
  return out;
}

/** Blockly's replaceable generator object (see {@link installIdGenerator}). */
interface BlocklyUidHook {
  genUid: () => string;
}

function blocklyUidHook(): BlocklyUidHook {
  // Blockly's exported `genUid` delegates to `idGenerator.TEST_ONLY.genUid` (its internal
  // generator object). That object is the only supported way to change the IDs Blockly gives new
  // blocks, so it is replaced, despite its name, and checked straight after.
  return Blockly.utils.idGenerator.TEST_ONLY;
}

const blockIdGenerator = (): string => newId('blk');

/**
 * Makes Blockly give every new block (and workspace comment and variable) a `blk_` ID from
 * {@link newId}. Call it once before creating a workspace; calling it again does nothing.
 *
 * @throws IdGeneratorError if Blockly does not use the replacement (a changed Blockly version) or
 *   there is no CSPRNG. Failing loudly is deliberate: Blockly-style IDs would make every saved
 *   project fail to load.
 */
export function installIdGenerator(): void {
  const hook = blocklyUidHook();
  if (hook.genUid !== blockIdGenerator) {
    hook.genUid = blockIdGenerator;
  }
  const probe = Blockly.utils.idGenerator.genUid();
  if (!GENERATED_ID_PATTERN.blk.test(probe)) {
    throw new IdGeneratorError(
      'Blockly did not take the Blocks2Cpp ID generator; check the Blockly version.',
    );
  }
}

/** Whether {@link installIdGenerator} is in effect. */
export function isIdGeneratorInstalled(): boolean {
  return blocklyUidHook().genUid === blockIdGenerator;
}
