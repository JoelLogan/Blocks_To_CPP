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
 * - which symbols a reference lists (docs/spec/03-block-language.md §3.6): getters list every
 *   variable; set, change, update and ask leave out constants; calls list functions;
 * - text.char takes exactly one character.
 */
export const CATALOG_FIELD_OPTIONS: Readonly<
  Record<string, Readonly<Record<string, CatalogFieldOptions>>>
> = Object.freeze({
  'var.get': { VAR: { kinds: 'variables' } },
  'var.set': { VAR: { kinds: 'assignable' } },
  'var.change': { VAR: { kinds: 'assignable' } },
  'var.update': { VAR: { kinds: 'assignable' } },
  'io.ask': { VAR: { kinds: 'assignable' } },
  'func.call': { FUNC: { kinds: 'functions' } },
  'func.call_stmt': { FUNC: { kinds: 'functions' } },
  'text.char': { VALUE: { singleChar: true } },
});

/** The field choices of one field (none when the catalog's definition is enough). */
export function catalogFieldOptions(type: string, field: string): CatalogFieldOptions {
  return CATALOG_FIELD_OPTIONS[type]?.[field] ?? {};
}
