/**
 * Lookups into the generated catalog (src/generated/catalog.ts, from catalog/ via catalog-gen), and
 * the per-field choices the catalog does not express.
 */
import { BLOCK_DEFS, type BlockDefJson } from '../generated/catalog';
import type { CatalogFieldOptions } from '../fields/register';

const BY_ID: ReadonlyMap<string, BlockDefJson> = new Map(BLOCK_DEFS.map((def) => [def.id, def]));

/** The catalog definition of a block type, or undefined for a type the catalog does not have. */
export function catalogBlock(type: string): BlockDefJson | undefined {
  return BY_ID.get(type);
}

/** Whether `type` is a catalog block type. */
export function isCatalogBlockType(type: string): boolean {
  return BY_ID.has(type);
}

/**
 * Field choices by block type and field name:
 * - which symbols a reference lists (docs/spec/03-block-language.md §3.6, `SymbolRefKinds`):
 *   getters list every variable; set and ask only what may be changed there; change only that of
 *   a number, character or text type; calls list functions;
 * - text.char takes one printable ASCII character.
 *
 * The reference fields of milestone M3's wave W2 are listed before the catalog has their blocks
 * (docs/plans/m3-contracts/w2.md §2.13), so the packages that add the blocks never edit this
 * table: a block that changes the symbol lists `assignable` ones, a block that only reads it
 * `variables`, and *enum value*'s `ENUM` lists enum definitions (`enums`). A `members` field
 * (`MEMBER`, `VALUE`) lists the members of a user type instead (w2.md §2.3), whatever its kinds;
 * it has `variables` so that every reference field has its kinds. An entry for a block the catalog
 * does not have is never read.
 */
export const CATALOG_FIELD_OPTIONS: Readonly<
  Record<string, Readonly<Record<string, CatalogFieldOptions>>>
> = Object.freeze({
  'var.get': { VAR: { kinds: 'variables' } },
  'var.set': { VAR: { kinds: 'assignable' } },
  // Numbers and text (*add to the end*, w2.md §1.9); true/false variables are flipped.
  'var.change': { VAR: { kinds: 'changeable' } },
  'io.ask': { VAR: { kinds: 'assignable' } },
  'func.call': { FUNC: { kinds: 'functions' } },
  'func.call_stmt': { FUNC: { kinds: 'functions' } },
  'text.char': { VALUE: { singleChar: true } },
  // W2: collections (w2.md §1.2).
  'coll.add': { LIST: { kinds: 'assignable' } },
  'coll.item': { LIST: { kinds: 'variables' } },
  'coll.set_item': { LIST: { kinds: 'assignable' } },
  'coll.length': { LIST: { kinds: 'variables' } },
  'coll.is_empty': { LIST: { kinds: 'variables' } },
  'coll.for_each_pair': { MAP: { kinds: 'variables' } },
  'coll.contains': { LIST: { kinds: 'variables' } },
  'coll.clear': { LIST: { kinds: 'assignable' } },
  'coll.remove_at': { LIST: { kinds: 'assignable' } },
  'coll.remove_value': { LIST: { kinds: 'assignable' } },
  'coll.next': { LIST: { kinds: 'variables' } },
  'coll.remove_next': { LIST: { kinds: 'assignable' } },
  'coll.sort': { LIST: { kinds: 'assignable' } },
  // W2: structs and enums (w2.md §1.5).
  'type.field': { MEMBER: { kinds: 'variables' } },
  'type.set_field': { VAR: { kinds: 'assignable' }, MEMBER: { kinds: 'variables' } },
  'type.enum_value': { ENUM: { kinds: 'enums' }, VALUE: { kinds: 'variables' } },
  // W2: errors and files (w2.md §1.7, §1.8).
  'error.message': { ERROR: { kinds: 'variables' } },
  'file.read_line': { FILE: { kinds: 'variables' }, LINE: { kinds: 'assignable' } },
  'file.write': { FILE: { kinds: 'variables' } },
  // W2: variables (w2.md §1.9).
  'var.flip': { VAR: { kinds: 'assignable' } },
});

/** The field choices of one field (none when the catalog's definition is enough). */
export function catalogFieldOptions(type: string, field: string): CatalogFieldOptions {
  return CATALOG_FIELD_OPTIONS[type]?.[field] ?? {};
}
