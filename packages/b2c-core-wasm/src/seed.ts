/**
 * Fresh randomness for `CoreWasm.pastePrepare()`. The compiler core uses no randomness itself
 * (it is pure, 06 §6.13): the IDs of pasted blocks are derived in Rust from a 256-bit seed that
 * the editor takes from the platform's cryptographic random source (spec gap decision
 * "Clipboard transport and API").
 */

/** The number of random bytes in a seed (256 bits). */
export const SEED_BYTES = 32;

/** Something with `crypto.getRandomValues` (the global `crypto` by default). */
export interface RandomSource {
  getRandomValues<T extends ArrayBufferView>(array: T): T;
}

/**
 * 256 fresh random bits as 64 lower-case hex digits, for `CoreWasm.pastePrepare()`. Use a new seed
 * for every paste or duplicate.
 *
 * @throws Error when no cryptographic random source is available (never fall back to
 *   `Math.random`).
 */
export function randomSeedHex(source?: RandomSource): string {
  // Checked at run time: an old or stripped-down engine may have no `crypto`.
  const random: Partial<RandomSource> | undefined =
    source ?? (globalThis as { crypto?: Partial<RandomSource> }).crypto;
  if (random?.getRandomValues === undefined) {
    throw new Error('no cryptographic random source (crypto.getRandomValues) is available');
  }
  const bytes = random.getRandomValues(new Uint8Array(SEED_BYTES));
  return Array.from(bytes, (byte) => byte.toString(16).padStart(2, '0')).join('');
}
