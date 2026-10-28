/**
 * Reads typed text as a type for the type picker (docs/spec/03-block-language.md §3.5.1): C++
 * (`int`, `string`, `std::string`, `vector<int>`, `std::map<std::string, std::vector<int>>`) or
 * the Friendly labels (`list of int`, `map from string to list of int`, `set of Student`), with the
 * module's struct and enum names. Pure: the text only selects among the catalog's types and the
 * names given, it is never evaluated, and anything else gives no type.
 */
import { MAX_TYPE_DEPTH, type TypeTerm } from '../../types/static-type';
import { withinLimits, type PickerUserType } from './model';

/** The longest text read, in characters: no type of a project is longer. */
export const MAX_TYPED_CHARS = 200;

/** The basic types by the names a person may type. */
const BASIC_NAMES: Readonly<Record<string, string>> = {
  int: 'int',
  double: 'double',
  bool: 'bool',
  char: 'char',
  string: 'std::string',
  'std::string': 'std::string',
};

/** The collections by C++ name (with or without `std::`), and their arities. */
const CPP_COLLECTIONS: Readonly<Record<string, readonly [string, number]>> = {
  vector: ['std::vector', 1],
  deque: ['std::deque', 1],
  set: ['std::set', 1],
  map: ['std::map', 2],
  queue: ['std::queue', 1],
  stack: ['std::stack', 1],
  priority_queue: ['std::priority_queue', 1],
};

/** The Friendly collections that read `<word> of …`. */
const FRIENDLY_OF: Readonly<Record<string, string>> = {
  list: 'std::vector',
  vector: 'std::vector',
  deque: 'std::deque',
  set: 'std::set',
  queue: 'std::queue',
  stack: 'std::stack',
};

/** Splits text into words and the punctuation `<`, `>`, `,`, `(`, `)`. */
function tokens(text: string): string[] | null {
  const out: string[] = [];
  const pattern = /\s*([A-Za-z_][A-Za-z0-9_]*(?:::[A-Za-z_][A-Za-z0-9_]*)*|[<>,()])/uy;
  let index = 0;
  const trimmed = text.trim();
  while (index < trimmed.length) {
    pattern.lastIndex = index;
    const match = pattern.exec(trimmed);
    if (match?.[1] === undefined) {
      return null;
    }
    out.push(match[1]);
    index = pattern.lastIndex;
  }
  return out;
}

/** A reader over the tokens. */
class Reader {
  private index = 0;
  private readonly items: readonly string[];
  private readonly users: ReadonlyMap<string, string>;

  constructor(items: readonly string[], users: ReadonlyMap<string, string>) {
    this.items = items;
    this.users = users;
  }

  done(): boolean {
    return this.index >= this.items.length;
  }

  private peek(offset = 0): string | undefined {
    return this.items[this.index + offset];
  }

  private take(word?: string): string | null {
    const next = this.peek();
    if (next === undefined || (word !== undefined && next.toLowerCase() !== word)) {
      return null;
    }
    this.index += 1;
    return next;
  }

  type(depth: number): TypeTerm | null {
    if (depth > MAX_TYPE_DEPTH) {
      return null;
    }
    if (this.take('(') !== null) {
      const inner = this.type(depth);
      return inner !== null && this.take(')') !== null ? inner : null;
    }
    const word = this.take();
    if (word === null) {
      return null;
    }
    const lower = word.toLowerCase();
    // Friendly: `list of …`, `priority queue of …`, `map from … to …`.
    if (this.peek()?.toLowerCase() === 'of' && Object.hasOwn(FRIENDLY_OF, lower)) {
      this.take('of');
      return this.app(FRIENDLY_OF[lower] ?? '', [this.type(depth + 1)]);
    }
    if (lower === 'priority' && this.peek()?.toLowerCase() === 'queue') {
      this.take('queue');
      return this.take('of') === null
        ? null
        : this.app('std::priority_queue', [this.type(depth + 1)]);
    }
    if (lower === 'map' && this.peek()?.toLowerCase() === 'from') {
      this.take('from');
      const key = this.type(depth + 1);
      if (this.take('to') === null) {
        return null;
      }
      return this.app('std::map', [key, this.type(depth + 1)]);
    }
    // C++: `name<…, …>`.
    const bare = word.startsWith('std::') ? word.slice('std::'.length) : word;
    if (this.peek() === '<') {
      const collection = CPP_COLLECTIONS[bare];
      if (collection === undefined || !Object.hasOwn(CPP_COLLECTIONS, bare)) {
        return null;
      }
      this.take('<');
      const args: (TypeTerm | null)[] = [];
      do {
        args.push(this.type(depth + 1));
      } while (this.take(',') !== null);
      if (this.take('>') === null || args.length !== collection[1]) {
        return null;
      }
      return this.app(collection[0], args);
    }
    return this.name(word);
  }

  private app(name: string, args: readonly (TypeTerm | null)[]): TypeTerm | null {
    const known = args.filter((arg): arg is TypeTerm => arg !== null);
    return known.length === args.length && name !== '' ? { kind: 'app', name, args: known } : null;
  }

  /** A basic type's name (any case) or a struct's or an enum's name (as written). */
  private name(word: string): TypeTerm | null {
    const basic = BASIC_NAMES[word.toLowerCase()];
    if (basic !== undefined && Object.hasOwn(BASIC_NAMES, word.toLowerCase())) {
      return { kind: 'name', name: basic };
    }
    const id = this.users.get(word);
    return id === undefined ? null : { kind: 'ref', id };
  }
}

/** Splits `>>` (the end of two nested templates) into two `>`. */
function splitShifts(text: string): string {
  return text.replace(/>>/gu, '> >');
}

/**
 * The type `text` names, or `null`. A struct or enum is named by `users` (the first of several with
 * one name); the result keeps to a project file's limits on terms.
 */
export function parseTypeText(text: string, users: readonly PickerUserType[]): TypeTerm | null {
  if (text.length > MAX_TYPED_CHARS) {
    return null;
  }
  const items = tokens(splitShifts(text));
  if (items === null || items.length === 0) {
    return null;
  }
  const byName = new Map<string, string>();
  for (const user of users) {
    if (!byName.has(user.name)) {
      byName.set(user.name, user.id);
    }
  }
  const reader = new Reader(items, byName);
  const term = reader.type(0);
  return term !== null && reader.done() && withinLimits(term) ? term : null;
}
