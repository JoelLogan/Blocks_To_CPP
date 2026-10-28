/**
 * What the type picker offers (docs/spec/03-block-language.md §3.5.1,
 * docs/plans/m3-contracts/w4-editor-tools.md §2.2): pure, no DOM and no Blockly.
 *
 * The first step lists the field's own types (*Common*), the module's structs and enums its class
 * takes (*My types*), and the collections its class takes (*Collections*: list, deque, set, map,
 * queue, stack, priority queue). A collection is a constructor: choosing it opens a step for each
 * of its arguments, which lists the value types an argument may be (the basic types, the module's
 * structs and enums, and the collections again, to {@link MAX_PICKER_DEPTH}). A map's key and the
 * element of a set or a priority queue must be *comparable*.
 *
 * Every type offered is built as a term and checked with the type rules (`inClass`), and kept only
 * when they do not refuse it: a type the rules cannot judge (a struct the facts do not know yet) is
 * offered, and the analysis reports it if it does not fit.
 */
import type { LabelMode } from '../../blocks/labels';
import type { TypeClass } from '../../generated/catalog';
import {
  LIB_TYPES,
  MAX_TYPE_DEPTH,
  MAX_TYPE_NAMES,
  inClass,
  parseTerm,
  termJson,
  termKey,
  typeFromTerm,
  typeLabel,
  type TypeFactsTs,
  type TypeTerm,
} from '../../types/static-type';

/** A struct or an enum the picker may offer. */
export interface PickerUserType {
  /** Its symbol ID. */
  readonly id: string;
  /** Its current name. */
  readonly name: string;
  readonly kind: 'struct' | 'enum';
  /** The module that declares it, when known (W3's types shared across modules). */
  readonly module?: string;
}

/** What the picker works from, for one type field. */
export interface PickerContext {
  /** The class of the types the field takes besides its own, or `null` for its own only. */
  readonly typeClass: TypeClass | null;
  /** The field's own types, as it stores them (names and term keys), in catalog order. */
  readonly fixed: readonly string[];
  /** The module's structs and enums. */
  readonly userTypes: readonly PickerUserType[];
  /** What the type rules know of user types (comparable, printable). */
  readonly facts: TypeFactsTs;
  /** How types read. */
  readonly mode: LabelMode;
}

/** The groups of a step's list. */
export type PickerGroup = 'common' | 'mine' | 'collections' | 'basic';

/** The headings of the groups. */
export const PICKER_GROUP_TITLES: Readonly<Record<PickerGroup, string>> = {
  common: 'Common',
  mine: 'My types',
  collections: 'Collections',
  basic: 'Basic types',
};

/** One thing a step lists: a type, or a collection to build. */
export type PickerEntry =
  | {
      readonly kind: 'type';
      readonly group: PickerGroup;
      /** The term's key, as the field stores it. */
      readonly key: string;
      readonly term: TypeTerm;
      readonly label: string;
    }
  | {
      readonly kind: 'collection';
      readonly group: 'collections';
      /** The library type's C++ name, `std::map`. */
      readonly name: string;
      /** How many arguments it takes. */
      readonly arity: number;
      /** `list of …`, `std::vector<…>`. */
      readonly label: string;
    };

/** How many collections deep the picker builds: `list of list of map from …`. */
export const MAX_PICKER_DEPTH = Math.min(4, MAX_TYPE_DEPTH);

/** The basic types every argument step offers, in order. */
const BASIC: readonly string[] = ['int', 'double', 'bool', 'char', 'std::string'];

/** The collections, in the order of `LIB_TYPES` (the standard library's value templates). */
const COLLECTIONS = LIB_TYPES.filter((info) => info.arity > 0);

/** The arguments that must be comparable: a map's key, a set's and a priority queue's element. */
function needsComparable(name: string, index: number): boolean {
  return (
    (name === 'std::map' && index === 0) ||
    ((name === 'std::set' || name === 'std::priority_queue') && index === 0)
  );
}

/** What a label is worked out from: the facts, the mode and, optionally, the current names. */
export type LabelContext = Pick<PickerContext, 'facts' | 'mode'> & {
  readonly userTypes?: readonly PickerUserType[];
};

/**
 * A type term's label in `mode`. Structs and enums are named by the context's list first (the
 * current names), then by the facts.
 */
export function termLabel(term: TypeTerm, context: LabelContext): string {
  const type = typeFromTerm(term);
  if (type === null) {
    return termKey(term);
  }
  const users = context.userTypes ?? [];
  const facts: TypeFactsTs =
    users.length === 0
      ? context.facts
      : {
          userType(id: string) {
            const user = users.find((candidate) => candidate.id === id);
            let known: ReturnType<TypeFactsTs['userType']>;
            try {
              known = context.facts.userType(id);
            } catch {
              known = null;
            }
            if (user === undefined) {
              return known;
            }
            return known === null
              ? { name: user.name, kind: user.kind, comparable: false, printable: false }
              : { ...known, name: user.name };
          },
        };
  return typeLabel(type, facts, context.mode);
}

/** A collection's label with its arguments left open: `map from … to …`, `std::map<…, …>`. */
export function collectionLabel(name: string, mode: LabelMode): string {
  const info = COLLECTIONS.find((candidate) => candidate.name === name);
  if (info === undefined) {
    return name;
  }
  if (mode === 'cpp') {
    return `${name}<${Array.from({ length: info.arity }, () => '…').join(', ')}>`;
  }
  return info.label.replace(/%[12]/gu, '…');
}

/**
 * Whether the type rules let `term` be in `cls`: `true` or `null` (not known) offers it, `false`
 * does not. A term that is no type (an unknown name) is offered only as one of the field's own.
 */
function fits(term: TypeTerm, cls: TypeClass, facts: TypeFactsTs): boolean {
  const type = typeFromTerm(term);
  return type !== null && inClass(type, cls, facts) !== false;
}

/** Whether `term` keeps to the limits of a project file's type terms. */
export function withinLimits(term: TypeTerm): boolean {
  return parseTerm(termJson(term)) !== null;
}

/** The module's structs (with `structs`) and enums (with `enums`) as terms, sorted by name. */
function userTerms(
  context: PickerContext,
  structs: boolean,
  enums: boolean,
): { readonly term: TypeTerm; readonly name: string }[] {
  return context.userTypes
    .filter((user) => (user.kind === 'struct' ? structs : enums))
    .map((user) => {
      const term: TypeTerm = { kind: 'ref', id: user.id };
      return { term, name: user.name };
    })
    .sort((a, b) =>
      a.name < b.name ? -1 : a.name > b.name ? 1 : termKey(a.term) < termKey(b.term) ? -1 : 1,
    );
}

/** A type entry for `term`. */
function typeEntry(term: TypeTerm, group: PickerGroup, context: PickerContext): PickerEntry {
  return { kind: 'type', group, key: termKey(term), term, label: termLabel(term, context) };
}

/** A collection entry. */
function collectionEntry(
  info: (typeof COLLECTIONS)[number],
  mode: LabelMode,
): Extract<PickerEntry, { kind: 'collection' }> {
  return {
    kind: 'collection',
    group: 'collections',
    name: info.name,
    arity: info.arity,
    label: collectionLabel(info.name, mode),
  };
}

/** A term of collection `name` with every argument `int`, to ask the type rules about its class. */
function sample(name: string, arity: number): TypeTerm {
  return {
    kind: 'app',
    name,
    args: Array.from({ length: arity }, () => ({ kind: 'name', name: 'int' })),
  };
}

/**
 * The first step's entries for a field: its own types, then the module's structs and enums its
 * class takes, then the collections its class takes. A field without a class lists its own types
 * only.
 */
export function rootEntries(context: PickerContext): PickerEntry[] {
  const entries: PickerEntry[] = [];
  const seen = new Set<string>();
  for (const key of context.fixed) {
    const term = parseTermKey(key);
    if (term !== null && !seen.has(key)) {
      seen.add(key);
      entries.push(typeEntry(term, 'common', context));
    }
  }
  const cls = context.typeClass;
  if (cls === null) {
    return entries;
  }
  const structs = cls === 'value' || cls === 'struct' || cls === 'any';
  const enums = cls === 'value' || cls === 'enum' || cls === 'any';
  for (const { term } of userTerms(context, structs, enums)) {
    const key = termKey(term);
    if (!seen.has(key)) {
      seen.add(key);
      entries.push(typeEntry(term, 'mine', context));
    }
  }
  if (cls === 'struct' || cls === 'enum') {
    return entries;
  }
  for (const info of COLLECTIONS) {
    if (fits(sample(info.name, info.arity), cls, context.facts)) {
      entries.push(collectionEntry(info, context.mode));
    }
  }
  return entries;
}

/**
 * The entries of the step that chooses argument `index` of collection `name`, at `depth` (1 for an
 * argument of a collection chosen in the first step): the basic types, the module's structs and
 * enums, and, while the depth allows, the collections. A comparable argument lists only what is
 * comparable (a struct the facts do not know is offered).
 */
export function argumentEntries(
  context: PickerContext,
  name: string,
  index: number,
  depth: number,
): PickerEntry[] {
  const comparable = needsComparable(name, index);
  const keep = (term: TypeTerm): boolean =>
    fits(term, 'value', context.facts) && (!comparable || fits(term, 'comparable', context.facts));
  const entries: PickerEntry[] = [];
  for (const basic of BASIC) {
    const term: TypeTerm = { kind: 'name', name: basic };
    if (keep(term)) {
      entries.push(typeEntry(term, 'basic', context));
    }
  }
  for (const { term } of userTerms(context, true, true)) {
    if (keep(term)) {
      entries.push(typeEntry(term, 'mine', context));
    }
  }
  if (depth < MAX_PICKER_DEPTH) {
    for (const info of COLLECTIONS) {
      if (keep(sample(info.name, info.arity))) {
        entries.push(collectionEntry(info, context.mode));
      }
    }
  }
  return entries;
}

/**
 * Whether a typed type may be argument `index` of collection `name`: a value type, comparable where
 * the argument must be, within the limits. (No depth limit: typing reaches any type a file may
 * hold.)
 */
export function acceptsArgument(
  context: PickerContext,
  name: string,
  index: number,
  term: TypeTerm,
): boolean {
  if (!withinLimits(term) || (term.kind === 'name' && term.name === 'auto')) {
    return false;
  }
  return (
    fits(term, 'value', context.facts) &&
    (!needsComparable(name, index) || fits(term, 'comparable', context.facts))
  );
}

/** The term a field value stands for (`int`, `{"std::vector":"int"}`), or `null`. */
export function parseTermKey(key: string): TypeTerm | null {
  if (!key.startsWith('{')) {
    return parseTerm(key);
  }
  try {
    return parseTerm(JSON.parse(key));
  } catch {
    return null;
  }
}

/** A collection being built: its name and the arguments chosen so far. */
export interface OpenCollection {
  readonly name: string;
  readonly arity: number;
  readonly args: readonly TypeTerm[];
}

/**
 * Adds `arg` to the innermost open collection of `path` and closes every collection that is then
 * complete. Returns the new path and, when the outermost collection is complete, its term.
 */
export function addArgument(
  path: readonly OpenCollection[],
  arg: TypeTerm,
): { readonly path: OpenCollection[]; readonly done: TypeTerm | null } {
  const stack = path.map((open) => ({ ...open, args: [...open.args] }));
  let value: TypeTerm | null = arg;
  while (value !== null) {
    const top = stack.at(-1);
    if (top === undefined) {
      return { path: [], done: value };
    }
    top.args.push(value);
    if (top.args.length < top.arity) {
      value = null;
    } else {
      stack.pop();
      value = { kind: 'app', name: top.name, args: top.args };
    }
  }
  return { path: stack, done: null };
}

/**
 * The breadcrumb of an open path: the collections and arguments chosen so far, then `…` for the
 * argument being chosen (`map from string to …`, `std::map<std::string, …>`).
 */
export function pathLabel(path: readonly OpenCollection[], context: LabelContext): string {
  // Fill each open argument with a placeholder name, then label the whole as a term.
  const HOLE = 'B2C_PICKER_HOLE';
  let inner: TypeTerm | null = null;
  for (let level = path.length - 1; level >= 0; level -= 1) {
    const open = path[level];
    if (open === undefined) {
      continue;
    }
    const args: TypeTerm[] = [...open.args];
    args.push(inner ?? { kind: 'name', name: HOLE });
    while (args.length < open.arity) {
      args.push({ kind: 'name', name: HOLE });
    }
    inner = { kind: 'app', name: open.name, args };
  }
  if (inner === null) {
    return '';
  }
  return labelWithHoles(inner, context, HOLE);
}

/** A term's label in which the name `hole` reads `…`. */
function labelWithHoles(term: TypeTerm, context: LabelContext, hole: string): string {
  switch (term.kind) {
    case 'name':
      return term.name === hole ? '…' : termLabel(term, context);
    case 'ref':
      return termLabel(term, context);
    case 'app': {
      const args = term.args.map((arg) => labelWithHoles(arg, context, hole));
      if (context.mode === 'cpp') {
        return `${term.name}<${args.join(', ')}>`;
      }
      const info = COLLECTIONS.find((candidate) => candidate.name === term.name);
      const template = info?.label ?? term.name;
      return template.replace(/%([12])/gu, (_, digit: string) => {
        const index = Number(digit) - 1;
        const text = args[index] ?? '…';
        const nestedMap =
          index === 0 &&
          term.name === 'std::map' &&
          term.args[0]?.kind === 'app' &&
          term.args[0].name === 'std::map';
        return nestedMap ? `(${text})` : text;
      });
    }
  }
}

/**
 * Whether a typed or built type may be chosen for the field: within the file's limits, and one of
 * its own types or of its class.
 */
export function acceptsTerm(context: PickerContext, term: TypeTerm): boolean {
  if (!withinLimits(term)) {
    return false;
  }
  if (context.fixed.includes(termKey(term))) {
    return true;
  }
  const cls = context.typeClass;
  // `auto` is a field's own type where it is one (*create*'s), never one of a class.
  if (cls === null || (term.kind === 'name' && term.name === 'auto')) {
    return false;
  }
  return fits(term, cls, context.facts) && countNames(term) <= MAX_TYPE_NAMES;
}

/** How many names and references a term has. */
function countNames(term: TypeTerm): number {
  return term.kind === 'app' ? 1 + term.args.reduce((sum, arg) => sum + countNames(arg), 0) : 1;
}
