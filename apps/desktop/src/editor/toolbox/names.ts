/**
 * Default names for the declarations the toolbox offers (docs/spec/03-block-language.md §3.6):
 * the first free name in scope, `value`, `value2`, `value3`, … for a new variable; `i`, `j`, `k`
 * for a counted loop (then `i2`, `j2`, `k2`, `i3`, …); and `myFunction`, `myFunction2`, … for a new
 * function.
 *
 * Every candidate is a valid identifier (03 §3.6: ASCII, starting with a letter, at most 64
 * characters), and the search is bounded: among `taken.size + 1` distinct candidates at least one is
 * free, so it always ends with a free name.
 */

/** What a default name is for. */
export type DefaultNameKind = 'variable' | 'loop' | 'function';

/** The first candidates of each kind, in order. */
const BASES: Readonly<Record<DefaultNameKind, readonly string[]>> = Object.freeze({
  variable: ['value'],
  loop: ['i', 'j', 'k'],
  function: ['myFunction'],
});

/**
 * The `index`-th candidate of a kind (0-based): the bases in order, then the bases with 2, then
 * with 3, and so on (`value`, `value2`, …; `i`, `j`, `k`, `i2`, …).
 */
export function candidateName(kind: DefaultNameKind, index: number): string {
  const bases = BASES[kind];
  const base = bases[index % bases.length] ?? bases[0] ?? 'value';
  const round = Math.floor(index / bases.length);
  return round === 0 ? base : `${base}${String(round + 1)}`;
}

/**
 * The first candidate of `kind` that is not in `taken`. Names are compared exactly (C++ names are
 * case-sensitive).
 */
export function firstFreeName(kind: DefaultNameKind, taken: ReadonlySet<string>): string {
  // At most taken.size + 1 candidates are needed: they are all different, so one is free.
  for (let index = 0; index <= taken.size; index += 1) {
    const name = candidateName(kind, index);
    if (!taken.has(name)) {
      return name;
    }
  }
  // Unreachable (see above); keeps the return type honest.
  return candidateName(kind, taken.size + 1);
}
