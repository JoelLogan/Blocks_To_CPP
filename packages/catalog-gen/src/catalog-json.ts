// Reading packages/catalog-gen/catalog.json, the validated catalog and toolbox that
// crates/b2c-catalog/tests/export.rs exports in their Rust (serde) shapes.
//
// All catalog validation stays in Rust (b2c-catalog). This module only decodes the JSON into
// typed values: it checks that every key and value has the shape the generator expects, so a
// change of the Rust schema fails here, loudly, instead of producing wrong definitions.

import type {
  CategoryId,
  DynamicCategory,
  FieldKind,
  RowExtraKind,
  Shape,
} from './catalog-types.ts';

/** The largest catalog.json the generator reads, in bytes. */
export const MAX_CATALOG_JSON_BYTES = 4 * 1024 * 1024;

/** catalog.json does not have the expected shape. */
export class CatalogJsonError extends Error {
  /** Where in the JSON, e.g. `blocks[3].field[0].kind`. */
  readonly path: string;

  constructor(path: string, problem: string) {
    super(`catalog.json: ${path === '' ? 'the document' : path} ${problem}`);
    this.name = 'CatalogJsonError';
    this.path = path;
  }
}

/**
 * The export: `{catalogVersion, blocks, toolbox, types}`.
 *
 * The optional keys here and below are the typed signatures of milestone M3
 * (docs/plans/m3-contracts/types.md §4.5), which the export leaves out while they are empty (a
 * block without a signature has none). {@link readCatalogJson} checks their shape like every other
 * key; generating editor code from them is the TypeScript side's (types.md §8.2).
 */
export interface CatalogJson {
  readonly catalogVersion: string;
  readonly blocks: readonly RustBlockDef[];
  readonly toolbox: RustToolbox;
  /** The types of M3 (types.md §1.1), from `b2c_ir::LibType` and the fundamental types. */
  readonly types?: readonly RustTypeEntry[];
}

/** A class of types (`b2c_ir::TypeClass`, types.md §1.3), as catalog files write it. */
export type RustTypeClass =
  | 'any'
  | 'number'
  | 'integer'
  | 'bool'
  | 'text'
  | 'value'
  | 'collection'
  | 'sequence'
  | 'indexable'
  | 'insertable'
  | 'searchable'
  | 'iterable'
  | 'keyed'
  | 'adaptor'
  | 'listable'
  | 'sortable'
  | 'switchable'
  | 'comparable'
  | 'printable'
  | 'struct'
  | 'enum'
  | 'input_file'
  | 'output_file'
  | 'exception';

/**
 * A type as files and the wire write it (types.md §3.1): a name, `{"ref": id}`, or a template
 * object with one key, its qualified C++ name.
 */
export type RustTypeJson =
  string | { readonly [name: string]: RustTypeJson | readonly RustTypeJson[] };

/** One entry of the `types` section: a fundamental type, `std::string` or a library type. */
export interface RustTypeEntry {
  /** Its JSON name: `int`, `std::string`, `std::vector`. */
  readonly name: string;
  /** How many template arguments it takes (0 for a type written as a name). */
  readonly arity: number;
  /** Its friendly label, with `%1` and `%2` for the arguments' labels (`list of %1`). */
  readonly label: string;
  /** The standard header that declares it, if any. */
  readonly header: string | null;
}

/**
 * A type pattern as catalog.json writes it (types.md §4.2, §4.5), tagged by `kind`: `any` and
 * `number` (M2), a fixed type, a type variable, a library template over patterns, a type function,
 * and the forms that take a part's type (`field:`, `symbol`, `param:`, `member:`, `result`).
 */
export type RustPattern =
  | { readonly kind: 'any' }
  | { readonly kind: 'number' }
  | { readonly kind: 'type'; readonly type: RustTypeJson }
  | { readonly kind: 'var'; readonly name: string }
  | { readonly kind: 'lib'; readonly name: string; readonly args: readonly RustPattern[] }
  | { readonly kind: 'fn'; readonly fn: 'elem' | 'index' | 'key'; readonly of: RustPattern }
  | { readonly kind: 'field'; readonly field: string }
  | { readonly kind: 'symbol'; readonly field: string | null }
  | { readonly kind: 'param'; readonly field: string }
  | { readonly kind: 'member'; readonly field: string }
  | { readonly kind: 'result' };

/** A part of a block that has a pattern, in catalog order (`b2c_ir::sig::PartName`). */
export type RustPartName = { readonly field: string } | { readonly input: string };

/** `b2c_catalog::BlockDef`. */
export interface RustBlockDef {
  readonly id: string;
  readonly version: number;
  readonly category: CategoryId;
  readonly shape: Shape;
  readonly output: string | null;
  readonly label: { readonly friendly: string; readonly cpp: string };
  readonly lowering: string;
  readonly headers: readonly string[];
  readonly help: string;
  readonly field: readonly RustFieldDef[];
  readonly input: readonly RustInputDef[];
  readonly statement: readonly RustStatementDef[];
  readonly extra: readonly RustExtraDef[];
  /** Each type variable and its class (`where`). */
  readonly typeVars?: Readonly<Record<string, RustTypeClass>>;
  /** The parts that have a pattern, in catalog order: the order they bind in. */
  readonly order?: readonly RustPartName[];
  /** The output's variable when no part binds it: the block takes its type from its place. */
  readonly placeVar?: string | null;
  /** `symbol_ref` fields that list the members of the user type bound to a variable. */
  readonly members?: Readonly<Record<string, string>>;
  /** The headers a dropdown option adds, by field, then option. */
  readonly optionHeaders?: Readonly<Record<string, Readonly<Record<string, readonly string[]>>>>;
}

/** `b2c_catalog::FieldDef`. */
export interface RustFieldDef {
  readonly name: string;
  readonly kind: FieldKind;
  readonly options: readonly (readonly [string, string])[];
  /** The types offered first, as type terms (types.md §4.1). */
  readonly types: readonly RustTypeJson[];
  readonly default: string | boolean | null;
  /** The field's pattern (`type`), on `type`, `symbol_ref` and `symbol_decl` fields. */
  readonly type?: RustPattern;
  /**
   * A repeated field (w2.md §2.1), parts `NAME0`, `NAME1`, …; absent or `null` for a field shown
   * once (the export may leave the key out while it is empty).
   */
  readonly repeat?: RustRepeat | null;
  /**
   * `omit_default`: the editor writes the field into a project only when it differs from its
   * default (w3-modules.md §6 W3M-D11); the export writes the key only when it is true.
   */
  readonly omitDefault?: boolean;
}

/** `b2c_catalog::Repeat`. */
export interface RustRepeat {
  readonly count: string;
  readonly plus: number;
}

/** `b2c_catalog::InputDef`. */
export interface RustInputDef {
  readonly name: string;
  readonly check: RustTypeClass;
  readonly optional: boolean;
  readonly repeat: RustRepeat | null;
  readonly default: readonly RustToken[];
  /** The input's pattern (`type`), for each part of a repeated input. */
  readonly type?: RustPattern;
  /** `assign = true`: the value is assigned, not used to initialise; it never binds. */
  readonly assign?: boolean;
}

/** A token table such as `{"num": "0"}`. */
export interface RustToken {
  readonly kind: TokenKind;
  readonly text: string;
}

/** The token kinds of project files (spec §5.5). */
export type TokenKind = 'num' | 'str' | 'chr' | 'ref' | 'op' | 'kw' | 'text';

/** `b2c_catalog::StatementDef`. */
export interface RustStatementDef {
  readonly name: string;
  readonly repeat: RustRepeat | null;
  readonly when: string | null;
}

/**
 * `b2c_catalog::ExtraDef`. Count defaults are written as text, such as `"1"`. The row extras
 * `params`, `fields` and `values` (w2.md §2.1) are written like `params`.
 */
export interface RustExtraDef {
  readonly name: string;
  readonly kind: 'count' | 'flag' | RowExtraKind;
  readonly min: number;
  readonly max: number;
  readonly default: string | boolean | null;
  /** The types a row offers (`params` and `fields`), as type terms. */
  readonly types: readonly RustTypeJson[];
  /** The class of types a row accepts besides its `types` (`type_class`). */
  readonly typeClass?: RustTypeClass;
}

/** `b2c_catalog::Toolbox`. */
export interface RustToolbox {
  readonly category: readonly RustToolboxCategory[];
}

/** `b2c_catalog::ToolboxCategory`. */
export interface RustToolboxCategory {
  readonly id: CategoryId;
  readonly name: string;
  readonly icon: string;
  readonly colour: string;
  readonly dynamic: DynamicCategory | null;
  readonly entry: readonly RustToolboxEntry[];
}

/** `b2c_catalog::ToolboxEntry`. */
export interface RustToolboxEntry {
  readonly block: string;
  readonly label: string | null;
  readonly preset: RustPreset | null;
}

/** `b2c_catalog::Preset`; empty maps are left out of the JSON. */
export interface RustPreset {
  readonly fields: ReadonlyMap<string, string | boolean>;
  readonly extra: ReadonlyMap<string, number | boolean>;
  readonly inputs: ReadonlyMap<string, readonly RustToken[]>;
}

export const CATEGORY_IDS: readonly CategoryId[] = [
  'program',
  'variables',
  'math',
  'logic',
  'text',
  'control',
  'loops',
  'io',
  'functions',
  'collections',
  'types',
  'files',
  'errors',
];
const SHAPES: readonly Shape[] = ['hat', 'definition', 'statement', 'reporter', 'predicate'];
const RUST_TYPE_CLASSES: readonly RustTypeClass[] = [
  'any',
  'number',
  'integer',
  'bool',
  'text',
  'value',
  'collection',
  'sequence',
  'indexable',
  'insertable',
  'searchable',
  'iterable',
  'keyed',
  'adaptor',
  'listable',
  'sortable',
  'switchable',
  'comparable',
  'printable',
  'struct',
  'enum',
  'input_file',
  'output_file',
  'exception',
];
const PATTERN_KINDS = [
  'any',
  'number',
  'type',
  'var',
  'lib',
  'fn',
  'field',
  'symbol',
  'param',
  'member',
  'result',
] as const;
const TYPE_FNS = ['elem', 'index', 'key'] as const;
/** The deepest pattern or type the reader follows (`TypePattern::MAX_DEPTH`, `Type::MAX_DEPTH`). */
const MAX_TYPE_DEPTH = 8;
/** Keys that would reach an object's prototype (spec §5.6). */
const PROTOTYPE_KEYS: readonly string[] = ['__proto__', 'constructor', 'prototype'];
const FIELD_KINDS: readonly FieldKind[] = [
  'dropdown',
  'checkbox',
  'text',
  'number',
  'type',
  'symbol_decl',
  'symbol_ref',
  'header',
];
const EXTRA_KINDS = ['count', 'flag', 'params', 'fields', 'values'] as const;
const TOKEN_KINDS: readonly TokenKind[] = ['num', 'str', 'chr', 'ref', 'op', 'kw', 'text'];
const DYNAMIC_KINDS: readonly DynamicCategory[] = ['variables', 'functions'];

/**
 * Parses and decodes the text of catalog.json.
 *
 * @throws {CatalogJsonError} when the text is too large, is not JSON, or does not have the
 *   shape of the export.
 */
export function readCatalogJson(text: string): CatalogJson {
  if (text.length > MAX_CATALOG_JSON_BYTES) {
    throw new CatalogJsonError('', `is larger than ${String(MAX_CATALOG_JSON_BYTES)} bytes`);
  }
  let value: unknown;
  try {
    value = JSON.parse(text);
  } catch (error) {
    throw new CatalogJsonError('', `is not JSON (${String(error)})`);
  }
  const root = object(value, '', ['catalogVersion', 'blocks', 'toolbox', 'types']);
  return {
    catalogVersion: string(root.catalogVersion, 'catalogVersion'),
    blocks: array(root.blocks, 'blocks', blockDef),
    toolbox: toolbox(root.toolbox, 'toolbox'),
    ...optional(root.types, 'types', (v, p) => ({ types: array(v, p, typeEntry) })),
  };
}

function typeEntry(value: unknown, path: string): RustTypeEntry {
  const o = object(value, path, ['name', 'arity', 'label', 'header']);
  return {
    name: string(o.name, `${path}.name`),
    arity: count(o.arity, `${path}.arity`),
    label: string(o.label, `${path}.label`),
    header: nullable(o.header, `${path}.header`, string),
  };
}

function blockDef(value: unknown, path: string): RustBlockDef {
  const keys = [
    'id',
    'version',
    'category',
    'shape',
    'output',
    'label',
    'lowering',
    'headers',
    'help',
    'field',
    'input',
    'statement',
    'extra',
    'typeVars',
    'order',
    'placeVar',
    'members',
    'optionHeaders',
  ] as const;
  const o = object(value, path, keys);
  const label = object(o.label, `${path}.label`, ['friendly', 'cpp']);
  return {
    id: string(o.id, `${path}.id`),
    version: count(o.version, `${path}.version`),
    category: oneOf(o.category, `${path}.category`, CATEGORY_IDS),
    shape: oneOf(o.shape, `${path}.shape`, SHAPES),
    output: nullable(o.output, `${path}.output`, string),
    label: {
      friendly: string(label.friendly, `${path}.label.friendly`),
      cpp: string(label.cpp, `${path}.label.cpp`),
    },
    lowering: string(o.lowering, `${path}.lowering`),
    headers: array(o.headers, `${path}.headers`, string),
    help: string(o.help, `${path}.help`),
    field: array(o.field, `${path}.field`, fieldDef),
    input: array(o.input, `${path}.input`, inputDef),
    statement: array(o.statement, `${path}.statement`, statementDef),
    extra: array(o.extra, `${path}.extra`, extraDef),
    ...optional(o.typeVars, `${path}.typeVars`, (v, p) => ({
      typeVars: record(v, p, (c, q) => oneOf(c, q, RUST_TYPE_CLASSES)),
    })),
    ...optional(o.order, `${path}.order`, (v, p) => ({ order: array(v, p, partName) })),
    ...optional(o.placeVar, `${path}.placeVar`, (v, p) => ({
      placeVar: nullable(v, p, string),
    })),
    ...optional(o.members, `${path}.members`, (v, p) => ({ members: record(v, p, string) })),
    ...optional(o.optionHeaders, `${path}.optionHeaders`, (v, p) => ({
      optionHeaders: record(v, p, (options, q) =>
        record(options, q, (headers, r) => array(headers, r, string)),
      ),
    })),
  };
}

function fieldDef(value: unknown, path: string): RustFieldDef {
  const keys = [
    'name',
    'kind',
    'options',
    'types',
    'default',
    'type',
    'repeat',
    'omitDefault',
  ] as const;
  const o = object(value, path, keys);
  return {
    name: string(o.name, `${path}.name`),
    kind: oneOf(o.kind, `${path}.kind`, FIELD_KINDS),
    options: array(o.options, `${path}.options`, option),
    types: array(o.types, `${path}.types`, (v, p) => typeJson(v, p, 0)),
    default: nullable(o.default, `${path}.default`, stringOrBoolean),
    ...optional(o.type, `${path}.type`, (v, p) => ({ type: pattern(v, p, 0) })),
    ...optional(o.repeat, `${path}.repeat`, (v, p) => ({ repeat: nullable(v, p, repeat) })),
    ...optional(o.omitDefault, `${path}.omitDefault`, (v, p) => ({ omitDefault: boolean(v, p) })),
  };
}

function option(value: unknown, path: string): readonly [string, string] {
  const pair = array(value, path, string);
  const [label, optionValue] = pair;
  if (pair.length !== 2 || label === undefined || optionValue === undefined) {
    throw new CatalogJsonError(path, 'should be a [label, value] pair');
  }
  return [label, optionValue];
}

function inputDef(value: unknown, path: string): RustInputDef {
  const keys = ['name', 'check', 'optional', 'repeat', 'default', 'type', 'assign'] as const;
  const o = object(value, path, keys);
  return {
    name: string(o.name, `${path}.name`),
    check: oneOf(o.check, `${path}.check`, RUST_TYPE_CLASSES),
    optional: boolean(o.optional, `${path}.optional`),
    repeat: nullable(o.repeat, `${path}.repeat`, repeat),
    default: array(o.default, `${path}.default`, token),
    ...optional(o.type, `${path}.type`, (v, p) => ({ type: pattern(v, p, 0) })),
    ...optional(o.assign, `${path}.assign`, (v, p) => ({ assign: boolean(v, p) })),
  };
}

function statementDef(value: unknown, path: string): RustStatementDef {
  const o = object(value, path, ['name', 'repeat', 'when']);
  return {
    name: string(o.name, `${path}.name`),
    repeat: nullable(o.repeat, `${path}.repeat`, repeat),
    when: nullable(o.when, `${path}.when`, string),
  };
}

function repeat(value: unknown, path: string): RustRepeat {
  const o = object(value, path, ['count', 'plus']);
  return { count: string(o.count, `${path}.count`), plus: count(o.plus, `${path}.plus`) };
}

function extraDef(value: unknown, path: string): RustExtraDef {
  const o = object(value, path, ['name', 'kind', 'min', 'max', 'default', 'types', 'typeClass']);
  return {
    name: string(o.name, `${path}.name`),
    kind: oneOf(o.kind, `${path}.kind`, EXTRA_KINDS),
    min: count(o.min, `${path}.min`),
    max: count(o.max, `${path}.max`),
    default: nullable(o.default, `${path}.default`, stringOrBoolean),
    types: array(o.types, `${path}.types`, (v, p) => typeJson(v, p, 0)),
    ...optional(o.typeClass, `${path}.typeClass`, (v, p) => ({
      typeClass: oneOf(v, p, RUST_TYPE_CLASSES),
    })),
  };
}

/** A part in the binding order: `{"field": name}` or `{"input": name}`. */
function partName(value: unknown, path: string): RustPartName {
  const o = object(value, path, ['field', 'input']);
  if (o.field !== undefined && o.input === undefined) {
    return { field: string(o.field, `${path}.field`) };
  }
  if (o.input !== undefined && o.field === undefined) {
    return { input: string(o.input, `${path}.input`) };
  }
  throw new CatalogJsonError(path, 'should be {"field": name} or {"input": name}');
}

/** A type pattern in its tagged form (types.md §4.5), at most {@link MAX_TYPE_DEPTH} levels deep. */
function pattern(value: unknown, path: string, depth: number): RustPattern {
  if (depth > MAX_TYPE_DEPTH) {
    throw new CatalogJsonError(path, `nests more than ${String(MAX_TYPE_DEPTH)} levels`);
  }
  const kind = oneOf(
    object(value, path, ['kind', 'type', 'name', 'args', 'fn', 'of', 'field']).kind,
    `${path}.kind`,
    PATTERN_KINDS,
  );
  switch (kind) {
    case 'any':
    case 'number':
    case 'result':
      object(value, path, ['kind']);
      return { kind };
    case 'type': {
      const o = object(value, path, ['kind', 'type']);
      return { kind, type: typeJson(o.type, `${path}.type`, 0) };
    }
    case 'var': {
      const o = object(value, path, ['kind', 'name']);
      return { kind, name: string(o.name, `${path}.name`) };
    }
    case 'lib': {
      const o = object(value, path, ['kind', 'name', 'args']);
      return {
        kind,
        name: string(o.name, `${path}.name`),
        args: array(o.args, `${path}.args`, (v, p) => pattern(v, p, depth + 1)),
      };
    }
    case 'fn': {
      const o = object(value, path, ['kind', 'fn', 'of']);
      return {
        kind,
        fn: oneOf(o.fn, `${path}.fn`, TYPE_FNS),
        of: pattern(o.of, `${path}.of`, depth + 1),
      };
    }
    case 'symbol': {
      const o = object(value, path, ['kind', 'field']);
      return { kind, field: nullable(o.field, `${path}.field`, string) };
    }
    case 'field':
    case 'param':
    case 'member': {
      const o = object(value, path, ['kind', 'field']);
      return { kind, field: string(o.field, `${path}.field`) };
    }
  }
}

/**
 * A type term (types.md §3.1): a name, or an object with one key whose value is a term or an
 * array of two or more. Only the shape is checked here; the names were checked in Rust.
 */
function typeJson(value: unknown, path: string, depth: number): RustTypeJson {
  if (typeof value === 'string') {
    return value;
  }
  if (depth >= MAX_TYPE_DEPTH) {
    throw new CatalogJsonError(path, `nests more than ${String(MAX_TYPE_DEPTH)} levels`);
  }
  if (typeof value !== 'object' || value === null || Array.isArray(value)) {
    throw new CatalogJsonError(path, 'should be a type: a name or an object with one key');
  }
  const entries: [string, unknown][] = Object.entries(value);
  const [entry] = entries;
  if (entries.length !== 1 || entry === undefined || PROTOTYPE_KEYS.includes(entry[0])) {
    throw new CatalogJsonError(path, 'should be a type: a name or an object with one key');
  }
  const [name, argument] = entry;
  const inner = `${path}.${name}`;
  if (Array.isArray(argument)) {
    if (argument.length < 2) {
      throw new CatalogJsonError(inner, 'should list two or more arguments');
    }
    return {
      [name]: argument.map((arg: unknown, i) => typeJson(arg, `${inner}[${String(i)}]`, depth + 1)),
    };
  }
  if (name === 'ref') {
    return { ref: string(argument, inner) };
  }
  return { [name]: typeJson(argument, inner, depth + 1) };
}

function token(value: unknown, path: string): RustToken {
  const o = object(value, path, TOKEN_KINDS);
  const entries = Object.entries(o);
  const [entry] = entries;
  if (entries.length !== 1 || entry === undefined) {
    throw new CatalogJsonError(
      path,
      'should be a token with exactly one key, such as {"num": "0"}',
    );
  }
  const [kind, text] = entry;
  return { kind: oneOf(kind, path, TOKEN_KINDS), text: string(text, `${path}.${kind}`) };
}

function toolbox(value: unknown, path: string): RustToolbox {
  const o = object(value, path, ['category']);
  return { category: array(o.category, `${path}.category`, toolboxCategory) };
}

function toolboxCategory(value: unknown, path: string): RustToolboxCategory {
  const o = object(value, path, ['id', 'name', 'icon', 'colour', 'dynamic', 'entry']);
  return {
    id: oneOf(o.id, `${path}.id`, CATEGORY_IDS),
    name: string(o.name, `${path}.name`),
    icon: string(o.icon, `${path}.icon`),
    colour: string(o.colour, `${path}.colour`),
    dynamic: nullable(o.dynamic, `${path}.dynamic`, (v, p) => oneOf(v, p, DYNAMIC_KINDS)),
    entry: array(o.entry, `${path}.entry`, toolboxEntry),
  };
}

function toolboxEntry(value: unknown, path: string): RustToolboxEntry {
  const o = object(value, path, ['block', 'label', 'preset']);
  return {
    block: string(o.block, `${path}.block`),
    label: nullable(o.label, `${path}.label`, string),
    preset: nullable(o.preset, `${path}.preset`, preset),
  };
}

function preset(value: unknown, path: string): RustPreset {
  const o = object(value, path, ['fields', 'extra', 'inputs']);
  return {
    fields: map(o.fields, `${path}.fields`, stringOrBoolean),
    extra: map(o.extra, `${path}.extra`, (v, p) => (typeof v === 'boolean' ? v : count(v, p))),
    inputs: map(o.inputs, `${path}.inputs`, (v, p) => array(v, p, token)),
  };
}

// ---------------------------------------------------------------------------------------------
// Shape checks
// ---------------------------------------------------------------------------------------------

/** A JSON object with only the allowed keys (absent keys read as `undefined`). */
function object<const K extends string>(
  value: unknown,
  path: string,
  allowed: readonly K[],
): Readonly<Partial<Record<K, unknown>>> {
  if (typeof value !== 'object' || value === null || Array.isArray(value)) {
    throw new CatalogJsonError(path, 'should be an object');
  }
  for (const key of Object.keys(value)) {
    if (!allowed.some((name) => name === key)) {
      throw new CatalogJsonError(path, `has the unknown key ${JSON.stringify(key)}`);
    }
  }
  // Every key is one of K, and the values are still unknown.
  return value as Readonly<Partial<Record<K, unknown>>>;
}

/**
 * An optional key: `{}` when it is absent, else what `item` makes of it (an object with the key, so
 * that absent keys stay absent under `exactOptionalPropertyTypes`).
 */
function optional<T extends object>(
  value: unknown,
  path: string,
  item: (value: unknown, path: string) => T,
): Partial<T> {
  return value === undefined ? {} : item(value, path);
}

/** A JSON object of values, as a fresh record in key order (prototype keys are refused). */
function record<T>(
  value: unknown,
  path: string,
  item: (value: unknown, path: string) => T,
): Readonly<Record<string, T>> {
  if (typeof value !== 'object' || value === null || Array.isArray(value)) {
    throw new CatalogJsonError(path, 'should be an object');
  }
  return Object.fromEntries(
    Object.entries(value).map(([key, entry]) => {
      if (PROTOTYPE_KEYS.includes(key)) {
        throw new CatalogJsonError(path, `has the reserved key ${JSON.stringify(key)}`);
      }
      return [key, item(entry, `${path}.${key}`)] as const;
    }),
  );
}

/** An optional object of values, as a map in key order. */
function map<T>(
  value: unknown,
  path: string,
  item: (value: unknown, path: string) => T,
): ReadonlyMap<string, T> {
  if (value === undefined) {
    return new Map();
  }
  if (typeof value !== 'object' || value === null || Array.isArray(value)) {
    throw new CatalogJsonError(path, 'should be an object');
  }
  return new Map(
    Object.entries(value).map(([key, entry]) => [key, item(entry, `${path}.${key}`)] as const),
  );
}

function array<T>(
  value: unknown,
  path: string,
  item: (value: unknown, path: string) => T,
): readonly T[] {
  if (!Array.isArray(value)) {
    throw new CatalogJsonError(path, 'should be an array');
  }
  return value.map((entry: unknown, index) => item(entry, `${path}[${String(index)}]`));
}

function string(value: unknown, path: string): string {
  if (typeof value !== 'string') {
    throw new CatalogJsonError(path, 'should be a string');
  }
  return value;
}

function boolean(value: unknown, path: string): boolean {
  if (typeof value !== 'boolean') {
    throw new CatalogJsonError(path, 'should be true or false');
  }
  return value;
}

function stringOrBoolean(value: unknown, path: string): string | boolean {
  if (typeof value !== 'string' && typeof value !== 'boolean') {
    throw new CatalogJsonError(path, 'should be a string or true or false');
  }
  return value;
}

/** A whole number from 0. */
function count(value: unknown, path: string): number {
  if (typeof value !== 'number' || !Number.isSafeInteger(value) || value < 0) {
    throw new CatalogJsonError(path, 'should be a whole number from 0');
  }
  return value;
}

function nullable<T>(
  value: unknown,
  path: string,
  item: (value: unknown, path: string) => T,
): T | null {
  return value === null ? null : item(value, path);
}

function oneOf<T extends string>(value: unknown, path: string, allowed: readonly T[]): T {
  const found = allowed.find((candidate) => candidate === value);
  if (found === undefined) {
    throw new CatalogJsonError(path, `should be one of ${allowed.join(', ')}`);
  }
  return found;
}
