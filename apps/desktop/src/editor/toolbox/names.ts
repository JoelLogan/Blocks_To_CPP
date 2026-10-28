/**
 * Default names for the declarations the toolbox offers (docs/spec/03-block-language.md §3.6):
 * the first free name in scope, `value`, `value2`, `value3`, … for a new variable; `i`, `j`, `k`
 * for a counted loop (then `i2`, `j2`, `k2`, `i3`, …); and `myFunction`, `myFunction2`, … for a new
 * function.
 *
 * Milestone M3's wave W2 adds the names of its declarations (docs/plans/m3-contracts/w2.md §2.13):
 * `item` for a *for each* item, `key` and `value` for a map's pair, `line` for a file's line,
 * `file` for an opened file, `e` for a caught error, `MyStruct` and `MyEnum` for a new struct and
 * enum, `value` for a global, `args` for *with arguments*, and the row bases `field` (a struct's
 * fields) and `Value` (an enum's values). {@link DEFAULT_NAME_KINDS} says which declaration field of
 * which block takes which kind; a test checks that every catalog block with a `symbol_decl` field
 * has a kind for it, and that every entry names a catalog declaration field.
 *
 * Every candidate is a valid identifier (03 §3.6: ASCII, starting with a letter, at most 64
 * characters), and the search is bounded: among `taken.size + 1` distinct candidates at least one is
 * free, so it always ends with a free name.
 */

/** What a default name is for. */
export type DefaultNameKind =
  | 'variable'
  | 'loop'
  | 'function'
  | 'item'
  | 'key'
  | 'value'
  | 'line'
  | 'file'
  | 'error'
  | 'global'
  | 'struct'
  | 'enum'
  | 'args'
  | 'field'
  | 'enumValue';

/** The first candidates of each kind, in order. */
const BASES: Readonly<Record<DefaultNameKind, readonly string[]>> = Object.freeze({
  variable: ['value'],
  loop: ['i', 'j', 'k'],
  function: ['myFunction'],
  item: ['item'],
  key: ['key'],
  value: ['value'],
  line: ['line'],
  file: ['file'],
  error: ['e'],
  global: ['value'],
  struct: ['MyStruct'],
  enum: ['MyEnum'],
  args: ['args'],
  field: ['field'],
  enumValue: ['Value'],
});

/**
 * Where a declaration of each kind is visible, which decides the names it must not take where it
 * lands (./scope.ts, `takenNamesAtDrop`):
 *
 * - `list`: after it in its statement list (a variable, an opened file);
 * - `body`: in the block's own statement lists (a loop's counter or item, a caught error);
 * - `function`: among the functions (function names; overloads are W2's package C2's);
 * - `module`: among the module's definitions (structs, enums, globals);
 * - `row`: in its definition's rows (a struct's fields, an enum's values), named by the rows
 *   mutator, not on landing.
 */
export type NameScope = 'list' | 'body' | 'function' | 'module' | 'row';

/** The scope of each kind (see {@link NameScope}). */
export const NAME_SCOPES: Readonly<Record<DefaultNameKind, NameScope>> = Object.freeze({
  variable: 'list',
  file: 'list',
  loop: 'body',
  item: 'body',
  key: 'body',
  value: 'body',
  line: 'body',
  error: 'body',
  args: 'body',
  function: 'function',
  global: 'module',
  struct: 'module',
  enum: 'module',
  field: 'row',
  enumValue: 'row',
});

/**
 * The kind of default name of each declaration field, by block type and field (a repeated field by
 * its base name, `NAME` for *try*'s `NAME0`, `NAME1`, …). W2's blocks were listed before the
 * catalog had them (w2.md §2.13), so the packages that added them never edited this table.
 */
export const DEFAULT_NAME_KINDS: Readonly<
  Record<string, Readonly<Record<string, DefaultNameKind>>>
> = Object.freeze({
  'var.declare': { NAME: 'variable' },
  'control.for_range': { VAR: 'loop' },
  'func.define': { NAME: 'function' },
  // W2 (w2.md §1.2 to §1.9).
  'coll.for_each': { ITEM: 'item' },
  'coll.for_each_pair': { KEY: 'key', VALUE: 'value' },
  'file.for_each_line': { LINE: 'line' },
  'file.open': { FILE: 'file' },
  'error.try': { NAME: 'error' },
  'type.struct': { NAME: 'struct' },
  'type.enum': { NAME: 'enum' },
  'var.global': { NAME: 'global' },
  'program.main': { ARGV: 'args' },
});

/** The kind of default name of each row extra's rows (the rows mutator names new rows). */
export const ROW_NAME_KINDS: Readonly<Record<string, DefaultNameKind>> = Object.freeze({
  fields: 'field',
  values: 'enumValue',
});

/** The kind of default name of a block's declaration field, or `undefined` for none. */
export function defaultNameKind(blockType: string, field: string): DefaultNameKind | undefined {
  const kinds = Object.hasOwn(DEFAULT_NAME_KINDS, blockType)
    ? DEFAULT_NAME_KINDS[blockType]
    : undefined;
  return kinds !== undefined && Object.hasOwn(kinds, field) ? kinds[field] : undefined;
}

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
