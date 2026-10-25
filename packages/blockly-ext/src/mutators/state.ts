/**
 * Reading a block's `extra` (05 §5.4) into mutator state, with explicit limits.
 *
 * `extra` comes from project files, the clipboard or undo history. The loader has checked its JSON
 * shape and size, but the catalog's own rules (ranges, keys) are checked later, by catalog
 * resolution, which reports `B2C-E0611` to `B2C-E0614` without rejecting the file. So a block shows
 * every value it can represent, even one outside the catalog range, and the editor saves it back
 * unchanged with the analyser's error on the block; values it cannot represent raise
 * `MutatorStateError`.
 */
import {
  inClass,
  NO_FACTS,
  parseTerm,
  termFromKey,
  termJson,
  termKey,
  typeFromTerm,
  type TypeJson,
} from '../types/static-type';
import { MutatorStateError } from './errors';
import type { CountSpec, FlagSpec, ParamsSpec, RowsSpec, VariadicSpec } from './spec';
import { PARAM_MODES, type ParamMode, type ParamRow, type RowState } from './types';

/** The most parts one block may have: the project-file limit of 05 §5.6. */
export const MAX_VARIADIC_PARTS = 64;

/** The longest parameter name (in characters), the identifier limit of 05 §5.6. */
export const MAX_PARAM_NAME_CHARS = 64;

/** Symbol IDs: `[A-Za-z0-9_]{1,32}` (05 §5.4). */
const SYMBOL_ID = /^[A-Za-z0-9_]{1,32}$/;

/**
 * Characters never shown in a name: NUL and the other C0 and C1 controls, DEL, and the Unicode
 * bidi controls (08 §8.4, the project-file text rules of 05 §5.6).
 */
// eslint-disable-next-line no-control-regex -- matching control characters is the point.
const FORBIDDEN_IN_NAME = /[\u0000-\u001f\u007f-\u009f؜‎‏‪-‮⁦-⁩]/;

/** A lone UTF-16 surrogate (text that is not well-formed Unicode). */
const LONE_SURROGATE = /[\uD800-\uDBFF](?![\uDC00-\uDFFF])|(?<![\uD800-\uDBFF])[\uDC00-\uDFFF]/;

/** The state of a variadic block: its count and its flags. */
export interface VariadicState {
  readonly count: number;
  readonly flags: ReadonlyMap<string, boolean>;
}

function isPlainObject(value: unknown): value is Record<string, unknown> {
  if (typeof value !== 'object' || value === null || Array.isArray(value)) {
    return false;
  }
  const prototype: unknown = Object.getPrototypeOf(value);
  return prototype === Object.prototype || prototype === null;
}

function own(record: Record<string, unknown>, key: string): unknown {
  return Object.hasOwn(record, key) ? record[key] : undefined;
}

function checkKeys(extra: unknown, allowed: readonly string[]): Record<string, unknown> {
  if (!isPlainObject(extra)) {
    throw new MutatorStateError(
      'notAnObject',
      null,
      'The block settings ("extra") are not an object.',
    );
  }
  for (const key of Object.keys(extra)) {
    if (!allowed.includes(key)) {
      throw new MutatorStateError('unknownKey', key, `This block has no setting "${key}".`);
    }
  }
  return extra;
}

function readCount(extra: Record<string, unknown>, spec: CountSpec): number {
  const value = own(extra, spec.name);
  if (value === undefined) {
    return spec.default;
  }
  if (
    typeof value !== 'number' ||
    !Number.isInteger(value) ||
    value < 0 ||
    value > MAX_VARIADIC_PARTS
  ) {
    throw new MutatorStateError(
      'badCount',
      spec.name,
      `The setting "${spec.name}" must be a whole number from 0 to ${String(MAX_VARIADIC_PARTS)}.`,
    );
  }
  return value;
}

function readFlag(extra: Record<string, unknown>, spec: FlagSpec): boolean {
  const value = own(extra, spec.name);
  if (value === undefined) {
    return spec.default;
  }
  if (typeof value !== 'boolean') {
    throw new MutatorStateError(
      'badFlag',
      spec.name,
      `The setting "${spec.name}" must be true or false.`,
    );
  }
  return value;
}

/**
 * Reads the count and flags of a variadic block; absent keys take their catalog defaults. A block
 * with flag-only branches (no count, w2.md §2.1) has the count 0.
 */
export function readVariadicExtra(spec: VariadicSpec, extra: unknown): VariadicState {
  const countKeys = spec.count === null ? [] : [spec.count.name];
  const record = checkKeys(extra, [...countKeys, ...spec.flags.map((flag) => flag.name)]);
  const count = spec.count === null ? 0 : readCount(record, spec.count);
  const flags = new Map(spec.flags.map((flag) => [flag.name, readFlag(record, flag)]));
  return { count, flags };
}

/** The `extra` of a variadic block: its count (if it has one), then its flags, in catalog order. */
export function writeVariadicExtra(
  spec: VariadicSpec,
  state: VariadicState,
): Record<string, unknown> {
  const extra: Record<string, unknown> =
    spec.count === null ? {} : { [spec.count.name]: state.count };
  for (const flag of spec.flags) {
    extra[flag.name] = state.flags.get(flag.name) ?? flag.default;
  }
  return extra;
}

/** What one row of each row extra is called in messages. */
const ROW_NOUN: Readonly<Record<RowsSpec['rowKind'], string>> = Object.freeze({
  params: 'Parameter',
  fields: 'Field',
  values: 'Value',
});

/** The keys of one row of each row extra (w2.md §2.1), in the order project files write them. */
export const ROW_KEYS: Readonly<Record<RowsSpec['rowKind'], readonly string[]>> = Object.freeze({
  params: ['sym', 'name', 'type', 'mode'],
  fields: ['sym', 'name', 'type'],
  values: ['sym', 'name'],
});

function badRow(spec: RowsSpec, index: number, detail: string): MutatorStateError {
  return new MutatorStateError(
    'badParamRow',
    spec.name,
    `${ROW_NOUN[spec.rowKind]} ${String(index + 1)} ${detail}.`,
  );
}

function isParamMode(value: unknown): value is ParamMode {
  return typeof value === 'string' && (PARAM_MODES as readonly string[]).includes(value);
}

/** Whether a name can be shown in a parameter row (any text within the limits; see above). */
export function isShowableName(name: string): boolean {
  // Counted in code points, as the project format counts identifier length.
  return (
    Array.from(name).length <= MAX_PARAM_NAME_CHARS &&
    !FORBIDDEN_IN_NAME.test(name) &&
    !LONE_SURROGATE.test(name)
  );
}

/** Reads one row of any row extra: the keys its kind has, each checked. */
function readRowState(spec: RowsSpec, row: unknown, index: number): RowState {
  if (!isPlainObject(row)) {
    throw badRow(spec, index, 'is not an object');
  }
  const keys = ROW_KEYS[spec.rowKind];
  for (const key of Object.keys(row)) {
    if (!keys.includes(key)) {
      throw badRow(spec, index, `has an unknown key "${key}"`);
    }
  }
  const sym = own(row, 'sym');
  const name = own(row, 'name');
  if (typeof sym !== 'string' || !SYMBOL_ID.test(sym)) {
    throw badRow(spec, index, 'needs a valid symbol ID in "sym"');
  }
  if (typeof name !== 'string' || !isShowableName(name)) {
    throw badRow(spec, index, `needs a name of at most ${String(MAX_PARAM_NAME_CHARS)} characters`);
  }
  let type: string | null = null;
  if (keys.includes('type')) {
    type = rowTypeKey(spec, own(row, 'type'));
    if (type === null) {
      throw badRow(
        spec,
        index,
        `has a type ${spec.rowKind === 'params' ? 'parameters' : 'fields'} cannot have`,
      );
    }
  }
  let mode: ParamMode | null = null;
  if (keys.includes('mode')) {
    const value = own(row, 'mode');
    if (!isParamMode(value)) {
      throw badRow(spec, index, 'has an unknown mode');
    }
    mode = value;
  }
  return { sym, name, type, mode };
}

function readRow(spec: ParamsSpec, row: unknown, index: number): ParamRow {
  const { sym, name, type, mode } = readRowState(spec, row, index);
  if (type === null || mode === null) {
    // Only a `params` spec reads parameter rows (see readParamsExtra).
    throw badRow(spec, index, 'is not a parameter row');
  }
  return { sym, name, type, mode };
}

/**
 * A row's type as the mutator keeps it, as the catalog accepts it (`row_type_fit`): one of the
 * listed names, or a type term (a template, a user type) whose type is not outside the spec's class
 * (a user type, whose class only the analysis knows, is accepted). A term is kept by its key
 * (`termKey`), as type fields keep one; `null` for a type rows cannot have.
 */
export function rowTypeKey(spec: ParamsSpec, value: unknown): string | null {
  if (typeof value === 'string' && spec.types.includes(value)) {
    return value;
  }
  const term = spec.typeClass === null ? null : parseTerm(value);
  const type = term === null ? null : typeFromTerm(term);
  if (term === null || type === null || spec.typeClass === null) {
    return null;
  }
  const key = termKey(term);
  if (spec.types.includes(key)) {
    return key;
  }
  const isVoid = type.kind === 'prim' && type.name === 'void';
  return !isVoid && inClass(type, spec.typeClass, NO_FACTS) !== false ? key : null;
}

/** A row's type as a project file writes it: a name, or the term of a key ({@link rowTypeKey}). */
export function rowTypeJson(key: string): TypeJson {
  const term = termFromKey(key);
  return term === null ? key : termJson(term);
}

/** The rows of a row extra as an array, checked against the limit; an absent key is no rows. */
function rowList(spec: RowsSpec, extra: unknown): unknown[] {
  const record = checkKeys(extra, [spec.name]);
  const rows = own(record, spec.name);
  if (rows === undefined) {
    return [];
  }
  if (!Array.isArray(rows) || rows.length > MAX_VARIADIC_PARTS) {
    const noun = spec.rowKind === 'params' ? 'parameters' : spec.rowKind;
    throw new MutatorStateError(
      'badParams',
      spec.name,
      `The setting "${spec.name}" must be a list of at most ${String(MAX_VARIADIC_PARTS)} ${noun}.`,
    );
  }
  return rows;
}

/** Reads the parameter rows of a block (a `params` spec); an absent `params` is no rows. */
export function readParamsExtra(spec: ParamsSpec, extra: unknown): ParamRow[] {
  return rowList(spec, extra).map((row: unknown, index) => readRow(spec, row, index));
}

/**
 * Reads the rows of any row extra (w2.md §2.1): `params` rows with their type and mode, `fields`
 * rows with their type, `values` rows with neither (`null`). An absent key is no rows.
 */
export function readRowsExtra(spec: RowsSpec, extra: unknown): RowState[] {
  return rowList(spec, extra).map((row: unknown, index) => readRowState(spec, row, index));
}

/** One row as a project file writes it: only the keys its kind has, types as terms. */
export function writeRow(spec: RowsSpec, row: RowState): Record<string, unknown> {
  const out: Record<string, unknown> = { sym: row.sym, name: row.name };
  if (spec.rowKind !== 'values' && row.type !== null) {
    out['type'] = rowTypeJson(row.type);
  }
  if (spec.rowKind === 'params' && row.mode !== null) {
    out['mode'] = row.mode;
  }
  return out;
}
