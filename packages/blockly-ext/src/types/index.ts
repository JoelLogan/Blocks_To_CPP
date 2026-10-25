/**
 * The editor's copy of the type rules (docs/plans/m3-contracts/types.md §3.2) and of typed slots
 * (slots.md §4.2). Not exported from the package entry while the connection checker still uses
 * the string union of `../checker/types`.
 */
export type {
  PrimitiveName,
  StaticConversion,
  StaticType,
  TypeClass,
  TypeFactsTs,
  TypeFn,
  UserTypeTs,
} from './static-type';
export type { Binder, BlockParts, BlockTyping, PartType, SlotFit } from './slots';
