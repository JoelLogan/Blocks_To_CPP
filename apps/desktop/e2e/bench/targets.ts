/**
 * The blocks the selection benchmarks click, in turn (docs/spec/09-quality-and-delivery.md §9.2:
 * "selecting blocks in 8 scopes at 1,000 blocks"; on the guessing game, the longest task after a
 * selection). Each click must change the selection, as clicking around a program does:
 * consecutive blocks are in different scopes (they see different variables), and so are the last
 * and the first, because the benchmark goes round the list. The toolbox does not depend on the
 * selection (04 §4.2), so a click does not rebuild its flyout.
 */
import { UNIT_BLOCKS } from './document';

/** A block a selection benchmark clicks. */
export interface ClickTarget {
  /** The block. */
  readonly id: string;
}

/**
 * Target `index` of `targets`, going round the list: a benchmark clicks `index` 0, 1, 2, … from
 * its offset (results.ts `benchOffset`).
 *
 * @throws Error when `targets` is empty.
 */
export function targetAt(targets: readonly ClickTarget[], index: number): ClickTarget {
  const target = targets[index % targets.length];
  if (target === undefined) {
    throw new Error(`No target ${String(index)}`);
  }
  return target;
}

/** The blocks of a unit of the generated document (bench/document.ts), in document order. */
const UNIT_PARTS = ['declare', 'loop', 'if', 'change', 'update', 'print', 'sum', 'get'] as const;

/** A block of a unit. */
export type UnitPart = (typeof UNIT_PARTS)[number];

/** How many scopes the selection benchmark clicks in. */
export const SCOPES = 8;

/**
 * The part of a unit each of the {@link SCOPES} targets is, in the order they are clicked: inside
 * the loop's `if` (`change`, `update`, the `if` itself see the loop's counter too) and in `main`'s
 * body (the others), alternating.
 */
const TARGET_PARTS: readonly UnitPart[] = [
  'change',
  'print',
  'update',
  'loop',
  'if',
  'get',
  'declare',
  'sum',
];

/** The ID of a part of unit `unit` of a document without a drag handle. */
export function unitBlockId(unit: number, part: UnitPart): string {
  return `b${String(1 + unit * UNIT_BLOCKS + UNIT_PARTS.indexOf(part))}`;
}

/**
 * The {@link SCOPES} blocks the 1,000-block selection benchmark clicks in turn, in a generated
 * document of `blocks` blocks without a drag handle: one in each of 8 units spread from the first
 * to the last, so each sees a different set of the units' variables, and kinds alternating between
 * the loop's body and `main`'s.
 *
 * @throws Error when the document has fewer than {@link SCOPES} units.
 */
export function selectionTargets(blocks: number): ClickTarget[] {
  const units = Math.floor((blocks - 1) / UNIT_BLOCKS);
  if (units < SCOPES) {
    throw new Error(
      `A document of ${String(blocks)} blocks has fewer than ${String(SCOPES)} units`,
    );
  }
  return TARGET_PARTS.map((part, index) => ({
    id: unitBlockId(Math.round((index * (units - 1)) / (SCOPES - 1)), part),
  }));
}

/**
 * The blocks of the guessing game (examples/guessing_game.b2c) its selection benchmark clicks in
 * turn: `random 1 to 100` (nothing declared yet), the *Too low!* print (in the loop: `secret` and
 * `guess`), `create secret` (`secret`), the *Correct!* print (`secret` and `guess`).
 */
export const GUESSING_TARGETS: readonly ClickTarget[] = [
  { id: 'b001' },
  { id: 'b006' },
  { id: 'b002' },
  { id: 'b008' },
];

/** `program.main` of the guessing game, where its edit benchmark adds a print. */
export const GUESSING_MAIN_ID = 'b011';
