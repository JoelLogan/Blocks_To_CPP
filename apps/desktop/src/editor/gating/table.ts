/**
 * What each gated block, dropdown option, type and toolbox category needs (./gate.ts). Anything
 * not listed is always offered.
 *
 * The rows come from the plans until the catalog carries them itself (the W4 contract,
 * docs/plans/m3-contracts/w4-appearance-expert.md §6.4): then this table is generated from the
 * catalog and keeps only what the catalog does not describe. A row may name a block or an option
 * that does not exist yet; it is listed in {@link AWAITED} with the work that brings it, so that a
 * row left behind by a renamed block shows up in the tests.
 */
import type { Requirement } from './gate';

/** What a requirement is about. */
export type RequirementKind = 'block' | 'option' | 'category' | 'type';

/**
 * The table's key: `block:<id>`, `option:<id>.<FIELD>=<value>`, `category:<id>` or
 * `type:<key>`.
 */
export function requirementKey(kind: RequirementKind, ...parts: string[]): string {
  if (kind === 'option') {
    const [block = '', field = '', value = ''] = parts;
    return `option:${block}.${field}=${value}`;
  }
  return `${kind}:${parts.join('')}`;
}

const ADVANCED: Requirement = Object.freeze({ advanced: true });

/** The rows, as [kind, key parts, requirement]. */
const ROWS: readonly (readonly [RequirementKind, readonly string[], Requirement])[] = [
  // The bitwise compound assignments of *change* (M3-8, w2.md §1.9).
  ...['and', 'or', 'xor', 'shl', 'shr'].map(
    (value) => ['option', ['var.change', 'OP', value], ADVANCED] as const,
  ),
  // *format … with* (03 §3.7.4, w2.md §1.10): std::format, with no older form.
  ['block', ['text.format'], { standard: 'c++20', capability: 'stdFormat' }],
  // The advanced categories of 03 §3.7.
  ['category', ['memory'], ADVANCED],
  ['category', ['concurrency'], ADVANCED],
  ['category', ['generics'], ADVANCED],
  ['category', ['raw'], ADVANCED],
];

/** Every requirement, by {@link requirementKey}. */
export const REQUIREMENTS: ReadonlyMap<string, Requirement> = new Map(
  ROWS.map(([kind, parts, requirement]) => [requirementKey(kind, ...parts), requirement]),
);

/**
 * Rows whose block, option or category is not in the catalog yet, with the work that brings it.
 * Once it lands the row simply applies; the test only checks that every other row names something
 * that exists.
 */
export const AWAITED: ReadonlyMap<string, string> = new Map([
  ['option:var.change.OP=and', 'W2 (C2): the bitwise forms of change'],
  ['option:var.change.OP=or', 'W2 (C2): the bitwise forms of change'],
  ['option:var.change.OP=xor', 'W2 (C2): the bitwise forms of change'],
  ['option:var.change.OP=shl', 'W2 (C2): the bitwise forms of change'],
  ['option:var.change.OP=shr', 'W2 (C2): the bitwise forms of change'],
  ['block:text.format', 'W4: format … with'],
  ['category:memory', 'M4: the Memory category'],
  ['category:concurrency', 'M4: the Concurrency category'],
  ['category:generics', 'M4: the Generics category'],
  ['category:raw', 'M4: Raw C++'],
]);
