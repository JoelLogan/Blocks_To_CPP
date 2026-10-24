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
import { MutatorStateError } from './errors';
import type { CountSpec, FlagSpec, ParamsSpec, VariadicSpec } from './spec';
import { PARAM_MODES, type ParamMode, type ParamRow } from './types';

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

/** Reads the count and flags of a variadic block; absent keys take their catalog defaults. */
export function readVariadicExtra(spec: VariadicSpec, extra: unknown): VariadicState {
  const record = checkKeys(extra, [spec.count.name, ...spec.flags.map((flag) => flag.name)]);
  const count = readCount(record, spec.count);
  const flags = new Map(spec.flags.map((flag) => [flag.name, readFlag(record, flag)]));
  return { count, flags };
}

/** The `extra` of a variadic block: its count, then its flags, in catalog order. */
export function writeVariadicExtra(
  spec: VariadicSpec,
  state: VariadicState,
): Record<string, unknown> {
  const extra: Record<string, unknown> = { [spec.count.name]: state.count };
  for (const flag of spec.flags) {
    extra[flag.name] = state.flags.get(flag.name) ?? flag.default;
  }
  return extra;
}

function badRow(spec: ParamsSpec, index: number, detail: string): MutatorStateError {
  return new MutatorStateError(
    'badParamRow',
    spec.name,
    `Parameter ${String(index + 1)} ${detail}.`,
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

function readRow(spec: ParamsSpec, row: unknown, index: number): ParamRow {
  if (!isPlainObject(row)) {
    throw badRow(spec, index, 'is not an object');
  }
  for (const key of Object.keys(row)) {
    if (key !== 'sym' && key !== 'name' && key !== 'type' && key !== 'mode') {
      throw badRow(spec, index, `has an unknown key "${key}"`);
    }
  }
  const sym = own(row, 'sym');
  const name = own(row, 'name');
  const type = own(row, 'type');
  const mode = own(row, 'mode');
  if (typeof sym !== 'string' || !SYMBOL_ID.test(sym)) {
    throw badRow(spec, index, 'needs a valid symbol ID in "sym"');
  }
  if (typeof name !== 'string' || !isShowableName(name)) {
    throw badRow(spec, index, `needs a name of at most ${String(MAX_PARAM_NAME_CHARS)} characters`);
  }
  if (typeof type !== 'string' || !spec.types.includes(type)) {
    throw badRow(spec, index, 'has a type parameters cannot have');
  }
  if (!isParamMode(mode)) {
    throw badRow(spec, index, 'has an unknown mode');
  }
  return { sym, name, type, mode };
}

/** Reads the parameter rows of a block; an absent `params` is no rows. */
export function readParamsExtra(spec: ParamsSpec, extra: unknown): ParamRow[] {
  const record = checkKeys(extra, [spec.name]);
  const rows = own(record, spec.name);
  if (rows === undefined) {
    return [];
  }
  if (!Array.isArray(rows) || rows.length > MAX_VARIADIC_PARTS) {
    throw new MutatorStateError(
      'badParams',
      spec.name,
      `The setting "${spec.name}" must be a list of at most ${String(MAX_VARIADIC_PARTS)} parameters.`,
    );
  }
  return rows.map((row: unknown, index) => readRow(spec, row, index));
}
