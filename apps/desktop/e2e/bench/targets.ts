/**
 * The blocks the selection benchmarks click, in turn (docs/spec/09-quality-and-delivery.md §9.2:
 * "selecting blocks in 8 scopes at 1,000 blocks"; on the guessing game, the longest task after a
 * selection). Each click must change the selection and the scope the toolbox lists, as clicking
 * around a program does: consecutive blocks see different variables, and so do the last and the
 * first, because the benchmark goes round the list. Each target also names getters the toolbox
 * lists, and getters it does not list, once the block is selected; they tell its listing from the
 * one before, so a click after which the toolbox was not rebuilt fails the benchmark instead of
 * giving a fast sample (interaction.ts `timedClick`).
 */
import { UNIT_BLOCKS } from './document';

/** A block a selection benchmark clicks, and what the toolbox lists once it is selected. */
export interface ClickTarget {
  /** The block. */
  readonly id: string;
  /** Symbols the Variables category lists a getter (`var.get`) of. */
  readonly lists: readonly string[];
  /** Symbols it lists no getter of. */
  readonly omits: readonly string[];
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

/** The parts of a unit inside its loop, which see the loop's counter. */
const INNER_PARTS: readonly UnitPart[] = ['if', 'change', 'update'];

/** The ID of a part of unit `unit` of a document without a drag handle. */
export function unitBlockId(unit: number, part: UnitPart): string {
  return `b${String(1 + unit * UNIT_BLOCKS + UNIT_PARTS.indexOf(part))}`;
}

/** The symbol of unit `unit`'s variable (`value_<unit>`) in a generated document. */
export function valueSymbol(unit: number): string {
  return `s_value_${String(unit)}`;
}

/** The symbol of unit `unit`'s loop counter (`i_<unit>`) in a generated document. */
export function counterSymbol(unit: number): string {
  return `s_i_${String(unit)}`;
}

/**
 * The {@link SCOPES} blocks the 1,000-block selection benchmark clicks in turn, in a generated
 * document of `blocks` blocks without a drag handle: one in each of 8 units spread from the first
 * to the last, so each sees a different set of the units' variables, and kinds alternating between
 * the loop's body and `main`'s. A block in unit `u` sees `value_u` and not `value_(u+1)`, and its
 * loop's counter `i_u` only inside the loop.
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
  return TARGET_PARTS.map((part, index) => {
    const unit = Math.round((index * (units - 1)) / (SCOPES - 1));
    const inner = INNER_PARTS.includes(part);
    const later = unit + 1 < units ? [valueSymbol(unit + 1)] : [];
    return {
      id: unitBlockId(unit, part),
      lists: inner ? [valueSymbol(unit), counterSymbol(unit)] : [valueSymbol(unit)],
      omits: inner ? later : [...later, counterSymbol(unit)],
    };
  });
}

/** The symbols of the guessing game's variables. */
const SECRET = 's_secret';
const GUESS = 's_guess';

/**
 * The blocks of the guessing game (examples/guessing_game.b2c) its selection benchmark clicks in
 * turn: `random 1 to 100` (nothing declared yet), the *Too low!* print (in the loop: `secret` and
 * `guess`), `create secret` (`secret`), the *Correct!* print (`secret` and `guess`).
 */
export const GUESSING_TARGETS: readonly ClickTarget[] = [
  { id: 'b001', lists: [], omits: [SECRET, GUESS] },
  { id: 'b006', lists: [SECRET, GUESS], omits: [] },
  { id: 'b002', lists: [SECRET], omits: [GUESS] },
  { id: 'b008', lists: [SECRET, GUESS], omits: [] },
];

/** `program.main` of the guessing game, where its edit benchmark adds a print. */
export const GUESSING_MAIN_ID = 'b011';
