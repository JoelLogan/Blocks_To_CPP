/**
 * The editor's copy of the type rules (docs/plans/m3-contracts/types.md §1, §3.2), mirroring
 * `b2c_ir::types` and `b2c_lang::typing`: a static type as an object, read from and written to its
 * term (the JSON files and the wire use), its key, conversions, classes, type functions and
 * labels.
 *
 * The shared fixture that `b2c-lang` writes (`./fixture.json`, crates/b2c-lang/tests/fixtures.rs)
 * holds every function here to the Rust rules; `./static-type.test.ts` reads it and compares every
 * case.
 *
 * Everything here is pure, never throws and keeps within the limits of types.md §2.2 (8 levels of
 * templates, 32 names), so a type from outside (the analysis, a file, a paste) can never make it
 * recurse deeply. What it cannot read is `null`: an unknown type, which connects (03 §3.5.3).
 */
import type { TypeJson } from '@blocks2cpp/b2c-core-wasm';

import { isProjectId } from '../ids';

export type { TypeJson };

/** The names of the fundamental types and `std::string`, as files write them. */
export type PrimitiveName = 'void' | 'bool' | 'char' | 'int' | 'double' | 'std::string';

/**
 * A static type (`b2c_ir::Type`): a fundamental type or `std::string`, a standard-library type with
 * its arguments (`name` is its qualified C++ name, such as `std::vector`), a user struct or enum by
 * the symbol ID of its definition, or the type of an expression that already has an error, which
 * fits everything.
 */
export type StaticType =
  | { readonly kind: 'prim'; readonly name: PrimitiveName }
  | { readonly kind: 'lib'; readonly name: string; readonly args: readonly StaticType[] }
  | { readonly kind: 'named'; readonly id: string }
  | { readonly kind: 'error' };

/**
 * How a value of one static type converts to another, by name: `b2c_lang::Conversion::name()`.
 * Only `invalid` is an analyser error.
 */
export type StaticConversion = 'same' | 'widening' | 'narrowing' | 'boolNumber' | 'invalid';

/**
 * A class of types (`b2c_ir::TypeClass`, types.md §1.3), as catalog files write it: what an input's
 * `check` accepts and what a signature's `where` allows a type variable to be.
 */
export type TypeClass =
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

/** Every class, in the order of `b2c_ir::TypeClass::ALL`. */
export const TYPE_CLASSES: readonly TypeClass[] = Object.freeze([
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
]);

/** Whether a value is the name of a class. */
export function isTypeClass(value: unknown): value is TypeClass {
  return typeof value === 'string' && (TYPE_CLASSES as readonly string[]).includes(value);
}

/**
 * A type function of catalog patterns (`b2c_ir::TypeFn`, types.md §1.4): what one item is, what
 * *item of* takes, and what *contains* looks for.
 */
export type TypeFn = 'elem' | 'index' | 'key';

/** A user type as the facts describe it (`b2c_ir::UserType`). */
export interface UserTypeTs {
  readonly name: string;
  readonly kind: 'struct' | 'enum';
  /** Ticked *comparable*, and every field's type is comparable; enums always are. */
  readonly comparable: boolean;
  /** Ticked *printable*, and every field's type is printable; enums always are. */
  readonly printable: boolean;
}

/**
 * What the type rules need to know about user types (`b2c_ir::TypeFacts`), answered from the
 * latest analysis's symbols. `null` when the facts do not know the symbol, or it is not a user
 * type: class membership is then not known, and a value of that type connects.
 */
export interface TypeFactsTs {
  userType(id: string): UserTypeTs | null;
}

/** Facts that know no user type (`b2c_ir::types::NoFacts`). */
export const NO_FACTS: TypeFactsTs = Object.freeze({ userType: () => null });

// --- The standard-library types ---------------------------------------------------------------

/** A standard-library type of M3 (`b2c_ir::LibType`, types.md §1.1). */
export interface LibTypeInfo {
  /** The qualified C++ name, which is also its JSON name: `std::vector`. */
  readonly name: string;
  /** Template arguments: 1, 2 for `std::map`, 0 for the streams and errors. */
  readonly arity: number;
  /** The friendly label, `%1` and `%2` standing for the arguments' labels. */
  readonly label: string;
  /** The standard header that declares it. */
  readonly header: string;
}

/** Every library type, in the order of `b2c_ir::LibType::ALL`. */
export const LIB_TYPES: readonly LibTypeInfo[] = Object.freeze(
  [
    ['std::vector', 1, 'list of %1', '<vector>'],
    ['std::deque', 1, 'deque of %1', '<deque>'],
    ['std::set', 1, 'set of %1', '<set>'],
    ['std::map', 2, 'map from %1 to %2', '<map>'],
    ['std::queue', 1, 'queue of %1', '<queue>'],
    ['std::stack', 1, 'stack of %1', '<stack>'],
    ['std::priority_queue', 1, 'priority queue of %1', '<queue>'],
    ['std::ifstream', 0, 'input file', '<fstream>'],
    ['std::ofstream', 0, 'output file', '<fstream>'],
    ['std::exception', 0, 'error', '<exception>'],
    ['std::runtime_error', 0, 'runtime error', '<stdexcept>'],
    ['std::invalid_argument', 0, 'invalid argument error', '<stdexcept>'],
    ['std::out_of_range', 0, 'out-of-range error', '<stdexcept>'],
  ].map(([name, arity, label, header]) =>
    Object.freeze({
      name: name as string,
      arity: arity as number,
      label: label as string,
      header: header as string,
    }),
  ),
);

const LIB_BY_NAME: ReadonlyMap<string, LibTypeInfo> = new Map(
  LIB_TYPES.map((info) => [info.name, info]),
);

/** The library type with this qualified C++ name, or `undefined`. */
export function libType(name: string): LibTypeInfo | undefined {
  return LIB_BY_NAME.get(name);
}

// --- Terms ------------------------------------------------------------------------------------

/** The most library types nested in one type (`Type::MAX_DEPTH`). */
export const MAX_TYPE_DEPTH = 8;
/** The most names and references in one type (`Type::MAX_NAMES`). */
export const MAX_TYPE_NAMES = 32;
/** How `Type::Error`, which has no term, is written (`b2c_ir::types::ERROR_JSON`). */
export const ERROR_JSON = 'error';

/** Keys that would reach an object's prototype (05 §5.6): never a template's name. */
const PROTOTYPE_KEYS: readonly string[] = ['__proto__', 'constructor', 'prototype'];

/**
 * A type term (`b2c_ir::TypeTerm`): a type as files and the wire write it, before its names are
 * looked up. Built fresh by {@link parseTerm}, never shared with the JSON it was read from.
 */
export type TypeTerm =
  | { readonly kind: 'name'; readonly name: string }
  | { readonly kind: 'ref'; readonly id: string }
  | { readonly kind: 'app'; readonly name: string; readonly args: readonly TypeTerm[] };

/**
 * `term` with each user type it names renamed by `ids` (old symbol ID to new; `b2c_model`'s
 * `terms::rename`, which a paste or a duplicate's ID remapping applies). The same term object when
 * nothing in it is renamed.
 */
export function renameTermRefs(term: TypeTerm, ids: ReadonlyMap<string, string>): TypeTerm {
  switch (term.kind) {
    case 'name':
      return term;
    case 'ref': {
      const renamed = ids.get(term.id);
      return renamed === undefined ? term : { kind: 'ref', id: renamed };
    }
    case 'app': {
      const args = term.args.map((arg) => renameTermRefs(arg, ids));
      return args.every((arg, index) => arg === term.args[index])
        ? term
        : { kind: 'app', name: term.name, args };
    }
  }
}

/**
 * The name rule (types.md §2.2): 1 to 64 ASCII characters, identifiers joined by `::` or single
 * spaces.
 */
const TYPE_NAME = /^[A-Za-z_][A-Za-z0-9_]*(?:(?:::| )[A-Za-z_][A-Za-z0-9_]*)*$/;

function isTypeName(name: string): boolean {
  return name.length >= 1 && name.length <= 64 && TYPE_NAME.test(name);
}

/** A plain JSON object (not an array, not `null`). */
function isPlainObject(value: unknown): value is Readonly<Record<string, unknown>> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

/**
 * Reads a term's shape and limits (`TypeTerm`'s `Deserialize`): a name by the name rule, a
 * reference `{"ref": id}`, or a template, an object with exactly one key, a qualified name, whose
 * value is one argument or an array of two or more. At most {@link MAX_TYPE_DEPTH} levels of
 * templates and {@link MAX_TYPE_NAMES} names and references; prototype keys are refused. The names
 * are not looked up: `auto` and unknown names are terms. `null` for anything else.
 */
export function parseTerm(json: unknown): TypeTerm | null {
  const budget = { names: 0 };
  return readTerm(json, 0, budget);
}

function readTerm(json: unknown, level: number, budget: { names: number }): TypeTerm | null {
  budget.names += 1;
  if (budget.names > MAX_TYPE_NAMES) {
    return null;
  }
  if (typeof json === 'string') {
    return isTypeName(json) ? { kind: 'name', name: json } : null;
  }
  if (!isPlainObject(json)) {
    return null;
  }
  const keys = Object.keys(json);
  const [key] = keys;
  if (keys.length !== 1 || key === undefined || PROTOTYPE_KEYS.includes(key)) {
    return null;
  }
  const value = json[key];
  if (key === 'ref') {
    return isProjectId(value) ? { kind: 'ref', id: value } : null;
  }
  if (!isTypeName(key) || !key.includes('::') || level >= MAX_TYPE_DEPTH) {
    return null;
  }
  let values: readonly unknown[];
  if (Array.isArray(value)) {
    // One argument is never a one-element array, and an empty array is never written.
    if (value.length < 2 || value.length > MAX_TYPE_NAMES) {
      return null;
    }
    values = value;
  } else {
    values = [value];
  }
  const args: TypeTerm[] = [];
  for (const arg of values) {
    const term = readTerm(arg, level + 1, budget);
    if (term === null) {
      return null;
    }
    args.push(term);
  }
  return { kind: 'app', name: key, args };
}

/** A term's JSON, in its one canonical spelling. */
export function termJson(term: TypeTerm): TypeJson {
  switch (term.kind) {
    case 'name':
      return term.name;
    case 'ref':
      return { ref: term.id };
    case 'app': {
      const [only] = term.args;
      return term.args.length === 1 && only !== undefined
        ? { [term.name]: termJson(only) }
        : { [term.name]: term.args.map(termJson) };
    }
  }
}

/**
 * `TypeTerm::key`: a name gives itself (`int`, `std::string`); any other term its canonical
 * compact JSON (`{"std::vector":"int"}`, `{"ref":"sym_x"}`). Equal terms give equal keys, and
 * different terms different keys.
 */
export function termKey(term: TypeTerm): string {
  return term.kind === 'name' ? term.name : JSON.stringify(termJson(term));
}

/**
 * The term a key stands for: a name for a key that is not JSON, else the parsed term. `null` when
 * the key is neither (a name that breaks the name rule, or JSON that is no term).
 */
export function termFromKey(key: string): TypeTerm | null {
  if (!key.startsWith('{')) {
    return parseTerm(key);
  }
  let json: unknown;
  try {
    json = JSON.parse(key);
  } catch {
    return null;
  }
  const term = parseTerm(json);
  // Only a canonical spelling is a key.
  return term !== null && termKey(term) === key ? term : null;
}

// --- Types ------------------------------------------------------------------------------------

const PRIMITIVES: readonly PrimitiveName[] = [
  'void',
  'bool',
  'char',
  'int',
  'double',
  'std::string',
];

const PRIM_CACHE: ReadonlyMap<PrimitiveName, StaticType> = new Map(
  PRIMITIVES.map((name) => [name, Object.freeze({ kind: 'prim', name }) as StaticType]),
);

/** A fundamental type or `std::string`. */
export function prim(name: PrimitiveName): StaticType {
  return PRIM_CACHE.get(name) ?? { kind: 'prim', name };
}

/** `Type::Error`. */
export const ERROR_TYPE: StaticType = Object.freeze({ kind: 'error' });

/** Whether a type may be a template argument (`Type::is_argument`): not `void`, a stream or an error. */
function isArgument(type: StaticType): boolean {
  switch (type.kind) {
    case 'prim':
      return type.name !== 'void';
    case 'lib':
      return (libType(type.name)?.arity ?? 0) > 0;
    case 'named':
    case 'error':
      return true;
  }
}

/**
 * `Type::lib`: a library type applied to its arguments. `error` when an argument is `error`, so an
 * error is reported once; `null` for the wrong number of arguments, an argument that is not a
 * value type, or a result over the limits.
 */
export function libOf(name: string, args: readonly StaticType[]): StaticType | null {
  const info = libType(name);
  if (args.length !== info?.arity) {
    return null;
  }
  if (args.some((arg) => arg.kind === 'error')) {
    return ERROR_TYPE;
  }
  if (!args.every(isArgument)) {
    return null;
  }
  const type: StaticType = { kind: 'lib', name, args: [...args] };
  return depthOf(type) > MAX_TYPE_DEPTH || namesOf(type) > MAX_TYPE_NAMES ? null : type;
}

function depthOf(type: StaticType): number {
  if (type.kind !== 'lib' || type.args.length === 0) {
    return 0;
  }
  return 1 + Math.max(...type.args.map(depthOf));
}

function namesOf(type: StaticType): number {
  switch (type.kind) {
    case 'lib':
      return 1 + type.args.reduce((sum, arg) => sum + namesOf(arg), 0);
    case 'error':
      return 0;
    default:
      return 1;
  }
}

/** `Type::from_term`: looks the term's names up; `null` for `auto`, unknown names and bad arguments. */
export function typeFromTerm(term: TypeTerm): StaticType | null {
  switch (term.kind) {
    case 'name': {
      if ((PRIMITIVES as readonly string[]).includes(term.name)) {
        return prim(term.name as PrimitiveName);
      }
      const info = libType(term.name);
      return info?.arity !== 0 ? null : { kind: 'lib', name: info.name, args: [] };
    }
    case 'ref':
      return { kind: 'named', id: term.id };
    case 'app': {
      const info = libType(term.name);
      if (info === undefined || info.arity === 0) {
        return null;
      }
      const args: StaticType[] = [];
      for (const arg of term.args) {
        const type = typeFromTerm(arg);
        if (type === null) {
          return null;
        }
        args.push(type);
      }
      return libOf(info.name, args);
    }
  }
}

/**
 * Reads a type as the wire writes it (types.md §3.1, `term_form::deserialize`): its term, checked
 * for shape, limits and names, or `"error"`. Builds fresh objects and refuses prototype keys.
 * `null` for anything else: `auto`, M2's `"string"`, an unknown name, a bad shape.
 */
export function parseType(json: unknown): StaticType | null {
  if (json === ERROR_JSON) {
    return ERROR_TYPE;
  }
  const term = parseTerm(json);
  return term === null ? null : typeFromTerm(term);
}

/** `Type::to_term`: `null` for `error`, which has no term. */
export function typeToTerm(type: StaticType): TypeTerm | null {
  switch (type.kind) {
    case 'prim':
      return { kind: 'name', name: type.name };
    case 'named':
      return { kind: 'ref', id: type.id };
    case 'error':
      return null;
    case 'lib': {
      if (type.args.length === 0) {
        return { kind: 'name', name: type.name };
      }
      const args: TypeTerm[] = [];
      for (const arg of type.args) {
        const term = typeToTerm(arg);
        if (term === null) {
          return null;
        }
        args.push(term);
      }
      return { kind: 'app', name: type.name, args };
    }
  }
}

/** A type as the wire writes it (`term_form::serialize`): its term, or `"error"`. */
export function toTypeJson(type: StaticType): TypeJson {
  const term = typeToTerm(type);
  return term === null ? ERROR_JSON : termJson(term);
}

/**
 * The type's key (`TypeTerm::key` of its term): `int`, `std::string`, `{"std::vector":"int"}`,
 * `{"ref":"sym_x"}`. `error`, which has no term, gives `"error"`, which no term's key is.
 */
export function typeKey(type: StaticType): string {
  const term = typeToTerm(type);
  return term === null ? ERROR_JSON : termKey(term);
}

/** Structural equality of two types (by key). */
export function sameType(a: StaticType, b: StaticType): boolean {
  if (a.kind !== b.kind) {
    return false;
  }
  if (a.kind === 'prim' && b.kind === 'prim') {
    return a.name === b.name;
  }
  return typeKey(a) === typeKey(b);
}

/** The type with this primitive name? */
function isPrim(type: StaticType, ...names: readonly PrimitiveName[]): boolean {
  return type.kind === 'prim' && names.includes(type.name);
}

// --- Conversions ------------------------------------------------------------------------------

/**
 * `b2c_lang::conversion`: how a value of type `from` converts to a place of type `to`. Numbers
 * are `int`, `double` and `char`. In order:
 *
 * 1. `error` on either side, or the same type on both (by {@link typeKey}): `same`.
 * 2. `bool` to a number, or a number to `bool`: `boolNumber`.
 * 3. `int` or `char` to `double`, `char` to `int`: `widening`.
 * 4. `double` to `int` or `char`, `int` to `char`: `narrowing`.
 * 5. Anything else: `invalid`. A library or user type converts only to the same type.
 */
export function staticConversion(from: StaticType, to: StaticType): StaticConversion {
  if (from.kind === 'error' || to.kind === 'error' || sameType(from, to)) {
    return 'same';
  }
  if (
    (isPrim(from, 'bool') && isPrim(to, 'int', 'double', 'char')) ||
    (isPrim(from, 'int', 'double', 'char') && isPrim(to, 'bool'))
  ) {
    return 'boolNumber';
  }
  if (
    (isPrim(from, 'int', 'char') && isPrim(to, 'double')) ||
    (isPrim(from, 'char') && isPrim(to, 'int'))
  ) {
    return 'widening';
  }
  if (
    (isPrim(from, 'double') && isPrim(to, 'int', 'char')) ||
    (isPrim(from, 'int') && isPrim(to, 'char'))
  ) {
    return 'narrowing';
  }
  return 'invalid';
}

/**
 * `b2c_lang::assignment`: {@link staticConversion} where a value is assigned (`assign = true`, as in
 * `set`): also `char` to `std::string`, as `widening`.
 */
export function staticAssignment(from: StaticType, to: StaticType): StaticConversion {
  return isPrim(from, 'char') && isPrim(to, 'std::string')
    ? 'widening'
    : staticConversion(from, to);
}

// --- Classes ----------------------------------------------------------------------------------

/** `false` as soon as one answer is `false`, `true` when all are `true`, `null` otherwise. */
function allOf(answers: readonly (boolean | null)[]): boolean | null {
  let all: boolean | null = true;
  for (const answer of answers) {
    if (answer === false) {
      return false;
    }
    if (answer === null) {
      all = null;
    }
  }
  return all;
}

function userIsIn(user: UserTypeTs, cls: TypeClass): boolean {
  const isEnum = user.kind === 'enum';
  switch (cls) {
    case 'any':
    case 'value':
      return true;
    case 'comparable':
      return isEnum || user.comparable;
    case 'printable':
      return isEnum || user.printable;
    case 'switchable':
    case 'enum':
      return isEnum;
    case 'struct':
      return !isEnum;
    default:
      return false;
  }
}

/** The facts' answer, or `null` when they do not know the symbol or fail. */
function userTypeOf(facts: TypeFactsTs, id: string): UserTypeTs | null {
  try {
    const user = facts.userType(id);
    return user === null || typeof user !== 'object' ? null : user;
  } catch {
    return null;
  }
}

/**
 * Class membership (`Type::is_in`, types.md §1.3), as `where` uses it: `true` for `error`, which is
 * in every class, `false` for `void`; `null` when not known: a user type the facts do not know, or
 * a library type whose answer depends on one (`sortable`, `comparable` for a container).
 */
export function inClass(type: StaticType, cls: TypeClass, facts: TypeFactsTs): boolean | null {
  if (type.kind === 'error') {
    return true;
  }
  if (type.kind === 'named') {
    const user = userTypeOf(facts, type.id);
    return user === null ? null : userIsIn(user, cls);
  }
  if (isPrim(type, 'void')) {
    return false;
  }
  const lib = type.kind === 'lib' ? type : null;
  const libIn = (...names: readonly string[]): boolean => lib !== null && names.includes(lib.name);
  const isValueLib = lib !== null && (libType(lib.name)?.arity ?? 0) > 0;
  switch (cls) {
    case 'any':
      return true;
    case 'number':
      return isPrim(type, 'int', 'double', 'char');
    case 'integer':
    case 'switchable':
      return isPrim(type, 'int', 'char');
    case 'bool':
      return isPrim(type, 'bool');
    case 'text':
      return isPrim(type, 'std::string', 'char');
    case 'value':
      return lib === null || isValueLib;
    case 'collection':
      return isValueLib;
    case 'sequence':
      return libIn('std::vector', 'std::deque');
    case 'indexable':
      return libIn('std::vector', 'std::deque', 'std::map');
    case 'insertable':
      return libIn(
        'std::vector',
        'std::deque',
        'std::set',
        'std::queue',
        'std::stack',
        'std::priority_queue',
      );
    case 'searchable':
      return libIn('std::vector', 'std::deque', 'std::set', 'std::map');
    case 'keyed':
      return libIn('std::set', 'std::map');
    case 'adaptor':
      return libIn('std::queue', 'std::stack', 'std::priority_queue');
    case 'listable':
      return libIn('std::vector', 'std::deque', 'std::set');
    case 'iterable':
      return isPrim(type, 'std::string') || libIn('std::vector', 'std::deque', 'std::set');
    case 'printable':
      return lib === null;
    case 'struct':
    case 'enum':
      return false;
    case 'input_file':
      return libIn('std::ifstream');
    case 'output_file':
      return libIn('std::ofstream');
    case 'exception':
      return libIn(
        'std::exception',
        'std::runtime_error',
        'std::invalid_argument',
        'std::out_of_range',
      );
    case 'sortable': {
      if (lib === null || !libIn('std::vector', 'std::deque')) {
        return false;
      }
      const [first] = lib.args;
      return first === undefined ? null : inClass(first, 'comparable', facts);
    }
    case 'comparable':
      if (lib === null) {
        return true;
      }
      if (libIn('std::vector', 'std::deque', 'std::set', 'std::map', 'std::queue', 'std::stack')) {
        return allOf(lib.args.map((arg) => inClass(arg, 'comparable', facts)));
      }
      return false;
  }
}

/**
 * `b2c_lang::check_accepts`: whether an input whose `check` is `cls` accepts a value of `type`:
 * `bool`, `integer` and `number` by conversion (to `bool`, `int` and `double`), `any` every type but
 * `void`, the other classes by membership. `null` when not known.
 */
export function checkAccepts(cls: TypeClass, type: StaticType, facts: TypeFactsTs): boolean | null {
  switch (cls) {
    case 'bool':
      return staticConversion(type, prim('bool')) !== 'invalid';
    case 'integer':
      return staticConversion(type, prim('int')) !== 'invalid';
    case 'number':
      return staticConversion(type, prim('double')) !== 'invalid';
    case 'any':
      return !isPrim(type, 'void');
    default:
      return inClass(type, cls, facts);
  }
}

// --- Type functions ---------------------------------------------------------------------------

/** `TypeFn::apply`: `error` for `error`, `null` where the function does not apply. */
export function applyTypeFn(fn: TypeFn, type: StaticType): StaticType | null {
  if (type.kind === 'error') {
    return ERROR_TYPE;
  }
  if (isPrim(type, 'std::string')) {
    switch (fn) {
      case 'elem':
        return prim('char');
      case 'index':
        return prim('int');
      case 'key':
        return null;
    }
  }
  if (type.kind !== 'lib') {
    return null;
  }
  const [first, second] = type.args;
  const name = type.name;
  const elemOf = [
    'std::vector',
    'std::deque',
    'std::set',
    'std::queue',
    'std::stack',
    'std::priority_queue',
  ];
  switch (fn) {
    case 'elem':
      if (elemOf.includes(name)) {
        return first ?? null;
      }
      return name === 'std::map' ? (second ?? null) : null;
    case 'key':
      return ['std::vector', 'std::deque', 'std::set', 'std::map'].includes(name)
        ? (first ?? null)
        : null;
    case 'index':
      if (name === 'std::map') {
        return first ?? null;
      }
      return name === 'std::vector' || name === 'std::deque' ? prim('int') : null;
  }
}

// --- Labels -----------------------------------------------------------------------------------

/**
 * `Type::label` (`friendly`) and `Type::cpp` (`cpp`): `list of Student`, `map from string to list of
 * int` (a map's first argument in parentheses when it is itself a map); `std::vector<Student>`, the
 * standard names in full and user types by their name.
 */
export function typeLabel(type: StaticType, facts: TypeFactsTs, mode: 'friendly' | 'cpp'): string {
  const friendly = mode === 'friendly';
  switch (type.kind) {
    case 'error':
      return friendly ? 'unknown' : '<error>';
    case 'prim':
      return friendly && type.name === 'std::string' ? 'string' : type.name;
    case 'named': {
      const user = userTypeOf(facts, type.id);
      if (user !== null) {
        return user.name;
      }
      return friendly ? 'unknown type' : '<unknown type>';
    }
    case 'lib': {
      if (!friendly) {
        return type.args.length === 0
          ? type.name
          : `${type.name}<${type.args.map((arg) => typeLabel(arg, facts, mode)).join(', ')}>`;
      }
      const label = libType(type.name)?.label ?? type.name;
      return label.replace(/%([12])/g, (_, digit: string) => {
        const index = Number(digit) - 1;
        const arg = type.args[index];
        if (arg === undefined) {
          return '';
        }
        const text = typeLabel(arg, facts, mode);
        const nestedMap =
          index === 0 && type.name === 'std::map' && arg.kind === 'lib' && arg.name === 'std::map';
        return nestedMap ? `(${text})` : text;
      });
    }
  }
}
