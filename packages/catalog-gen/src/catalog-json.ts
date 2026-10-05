// Reading packages/catalog-gen/catalog.json, the validated catalog and toolbox that
// crates/b2c-catalog/tests/export.rs exports in their Rust (serde) shapes.
//
// All catalog validation stays in Rust (b2c-catalog). This module only decodes the JSON into
// typed values: it checks that every key and value has the shape the generator expects, so a
// change of the Rust schema fails here, loudly, instead of producing wrong definitions.

import type { CategoryId, DynamicCategory, FieldKind, Shape, TypeClass } from './catalog-types.ts';

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

/** The export: `{catalogVersion, blocks, toolbox}`. */
export interface CatalogJson {
  readonly catalogVersion: string;
  readonly blocks: readonly RustBlockDef[];
  readonly toolbox: RustToolbox;
}

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
}

/** `b2c_catalog::FieldDef`. */
export interface RustFieldDef {
  readonly name: string;
  readonly kind: FieldKind;
  readonly options: readonly (readonly [string, string])[];
  readonly types: readonly string[];
  readonly default: string | boolean | null;
}

/** `b2c_catalog::Repeat`. */
export interface RustRepeat {
  readonly count: string;
  readonly plus: number;
}

/** `b2c_catalog::InputDef`. */
export interface RustInputDef {
  readonly name: string;
  readonly check: TypeClass;
  readonly optional: boolean;
  readonly repeat: RustRepeat | null;
  readonly default: readonly RustToken[];
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

/** `b2c_catalog::ExtraDef`. Count defaults are written as text, such as `"1"`. */
export interface RustExtraDef {
  readonly name: string;
  readonly kind: 'count' | 'flag' | 'params';
  readonly min: number;
  readonly max: number;
  readonly default: string | boolean | null;
  readonly types: readonly string[];
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
];
const SHAPES: readonly Shape[] = ['hat', 'definition', 'statement', 'reporter', 'predicate'];
const TYPE_CLASSES: readonly TypeClass[] = ['any', 'number', 'integer', 'bool', 'text'];
const FIELD_KINDS: readonly FieldKind[] = [
  'dropdown',
  'checkbox',
  'text',
  'number',
  'type',
  'symbol_decl',
  'symbol_ref',
];
const EXTRA_KINDS = ['count', 'flag', 'params'] as const;
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
  const root = object(value, '', ['catalogVersion', 'blocks', 'toolbox']);
  return {
    catalogVersion: string(root.catalogVersion, 'catalogVersion'),
    blocks: array(root.blocks, 'blocks', blockDef),
    toolbox: toolbox(root.toolbox, 'toolbox'),
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
  };
}

function fieldDef(value: unknown, path: string): RustFieldDef {
  const o = object(value, path, ['name', 'kind', 'options', 'types', 'default']);
  return {
    name: string(o.name, `${path}.name`),
    kind: oneOf(o.kind, `${path}.kind`, FIELD_KINDS),
    options: array(o.options, `${path}.options`, option),
    types: array(o.types, `${path}.types`, string),
    default: nullable(o.default, `${path}.default`, stringOrBoolean),
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
  const o = object(value, path, ['name', 'check', 'optional', 'repeat', 'default']);
  return {
    name: string(o.name, `${path}.name`),
    check: oneOf(o.check, `${path}.check`, TYPE_CLASSES),
    optional: boolean(o.optional, `${path}.optional`),
    repeat: nullable(o.repeat, `${path}.repeat`, repeat),
    default: array(o.default, `${path}.default`, token),
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
  const o = object(value, path, ['name', 'kind', 'min', 'max', 'default', 'types']);
  return {
    name: string(o.name, `${path}.name`),
    kind: oneOf(o.kind, `${path}.kind`, EXTRA_KINDS),
    min: count(o.min, `${path}.min`),
    max: count(o.max, `${path}.max`),
    default: nullable(o.default, `${path}.default`, stringOrBoolean),
    types: array(o.types, `${path}.types`, string),
  };
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
