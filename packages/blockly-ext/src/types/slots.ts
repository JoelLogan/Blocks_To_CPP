/**
 * The editor's copy of typed slots' shapes (ADR-0018, docs/plans/m3-contracts/slots.md §4.2),
 * mirroring `b2c_ir::sig::{PartType, BlockParts}` and `b2c_lang::{SlotFit, Binder, BlockTyping}`.
 *
 * Declarations only for now: `typeBlock`, `isFitError` and `startTokens` come with the editor's
 * typed slots of milestone M3's first wave, checked against the slot fixture that `b2c-lang`
 * writes.
 */
import type { StaticConversion, StaticType } from './static-type';

/** What one part of a block is when the block is typed. */
export type PartType =
  /** Nothing chosen or connected, or a disabled block. */
  | { readonly kind: 'empty' }
  /** `auto` in a type field. */
  | { readonly kind: 'auto' }
  /** A value that takes its type from its place: typed after the block. */
  | { readonly kind: 'fromPlace' }
  /** A known type; `error` binds the variable to `error` and fits everything. */
  | { readonly kind: 'known'; readonly type: StaticType }
  /** Not known to the editor (something the analysis has not seen). */
  | { readonly kind: 'unknown' };

/** The parts of the block being typed. A repeated input is asked part by part (`ITEM2`). */
export interface BlockParts {
  field(name: string): PartType;
  input(part: string): PartType;
  /** How many parts the repeated input `base` has now. */
  count(base: string): number;
  /** `param:F`: parameter `index` of the function chosen in field `field`. */
  param(field: string, index: number): PartType;
  /** `member:F`: field `index` of the struct chosen in field `field`. */
  member(field: string, index: number): PartType;
  /** A `members` field: the chosen member's type when it is a member of `owner`. */
  chosenMember(field: string, owner: StaticType): PartType;
  /** `result`: what a `return` gives back where the block is. */
  result(): PartType;
}

/** `b2c_lang::SlotFit`: a conversion that fits, `invalid`, `outsideClass` or `unknown`. */
export type SlotFit = StaticConversion | 'outsideClass' | 'unknown';

/** Which part bound a type variable, for messages. */
export type Binder =
  | { readonly kind: 'field'; readonly name: string }
  | { readonly kind: 'input'; readonly name: string }
  | { readonly kind: 'place' };

/** The result of typing one block (`b2c_lang::BlockTyping`). */
export interface BlockTyping {
  /** Each bound variable, its type and its binder. */
  readonly bound: ReadonlyMap<string, { readonly type: StaticType; readonly binder: Binder }>;
  /** Every value input part the block has: its expected type (null when nothing gives one). */
  readonly inputs: ReadonlyMap<
    string,
    { readonly expects: StaticType | null; readonly fit: SlotFit }
  >;
  /** Every `type` and `symbol_ref` field that has a pattern or `members`. */
  readonly fields: ReadonlyMap<string, SlotFit>;
  /** What each `symbol_decl` field with a pattern declares. */
  readonly declares: ReadonlyMap<string, StaticType>;
  /** The block's value type; null for `any` and `number` outputs, which keep their M2 rules. */
  readonly output: StaticType | null;
  /** A place-typed block: how its place's type fits its output variable. */
  readonly place: SlotFit;
}
