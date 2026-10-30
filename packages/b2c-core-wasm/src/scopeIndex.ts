/**
 * The scope index (ADR-0016, docs/plans/m3-contracts/worker.md §4 and w5-performance.md §2.1):
 * what the core's `scope_index()` returns after an update, and the lookups that answer the scope
 * query from it alone, exactly as the core's own query does (`b2c_lang::Analysis::scope_at`,
 * `scope_after`, `parked_scope`, and `visible_at` of `crates/b2c-core-wasm/src/scope.rs`). The
 * editor answers its menus from it while the preview runs in the worker, where the analysis is.
 *
 * A node of the trail is three numbers: its parent (-1 for none, always smaller than the node), its
 * name (an index into `names`, the C++ names that hide one another) and its symbol (an index into
 * the update's `symbols`, -1 for a hidden name or one the program does not list). Positions are a
 * node (-1: no local symbol) and a module index.
 */
import type { SymbolInfo } from '@blocks2cpp/ipc-types';

import type { PreviewResult } from './api';
import { CoreError } from './errors';

/** The version of the index format this package reads. */
export const SCOPE_INDEX_VERSION = 1;

/** `scope_index()`'s JSON, version 1. */
export interface ScopeIndexData {
  readonly v: 1;
  /** The analysed document's content hash: the update's `contentHash`. */
  readonly contentHash: string;
  /** The module IDs, by module index. */
  readonly modules: readonly string[];
  /** The C++ names the trail and the function lists use, each once. */
  readonly names: readonly string[];
  /** Three numbers per node: parent, name, symbol. */
  readonly trail: readonly number[];
  /** Per module: the functions calls can use, as (name, symbol) pairs. */
  readonly functions: readonly (readonly number[])[];
  /** Per module: the node at the end of its first lowered `main` body, or -1. */
  readonly parked: readonly number[];
  /**
   * Per recorded block, sorted by ID: its ID, the node and module at the block (module -1 when
   * nothing was recorded there), then a name, node and module per input with a position of its own.
   */
  readonly blocks: readonly (readonly (string | number)[])[];
  /** Per struct or enum with members: [owner, member, …], as the scope query lists them after it. */
  readonly members: readonly (readonly number[])[];
  /** Per block that creates a variable `symbolsAfter` adds: [block ID, symbol]. */
  readonly created: readonly (readonly [string, number])[];
}

/** A position: a trail node (-1: none) and a module index. */
interface Position {
  readonly node: number;
  readonly module: number;
}

/** A block's positions: at the block (module -1: none), and by input name. */
interface BlockPositions {
  readonly at: Position;
  readonly inputs: ReadonlyMap<string, Position> | null;
}

function refuse(reason: string): never {
  throw new CoreError('protocol', `scopeIndex: ${reason}`);
}

function isIndex(value: unknown, length: number, allowNone = false): value is number {
  return (
    typeof value === 'number' &&
    Number.isInteger(value) &&
    value < length &&
    (value >= 0 || (allowNone && value === -1))
  );
}

function arrayOf(value: unknown, what: string): readonly unknown[] {
  return Array.isArray(value) ? value : refuse(`${what} is not a list`);
}

/** Whether a symbol is a member of a user type (a field or an enumerator), which no name hides. */
function isMember(info: SymbolInfo): boolean {
  return info.kind === 'field' || info.kind === 'enumerator';
}

/** The scope query answered from an index, for the preview it came with. */
export class ScopeIndex {
  readonly #symbols: readonly SymbolInfo[];
  readonly #modules: readonly string[];
  readonly #trail: readonly number[];
  readonly #functions: readonly (readonly number[])[];
  readonly #parked: readonly number[];
  readonly #blocks: ReadonlyMap<string, BlockPositions>;
  readonly #members: ReadonlyMap<number, readonly number[]>;
  readonly #created: ReadonlyMap<string, number>;

  private constructor(data: ScopeIndexData, symbols: readonly SymbolInfo[]) {
    this.#symbols = symbols;
    this.#modules = data.modules;
    this.#trail = data.trail;
    this.#functions = data.functions;
    this.#parked = data.parked;
    const blocks = new Map<string, BlockPositions>();
    for (const entry of data.blocks) {
      let inputs: Map<string, Position> | null = null;
      for (let at = 3; at + 2 < entry.length; at += 3) {
        inputs ??= new Map();
        inputs.set(entry[at] as string, {
          node: entry[at + 1] as number,
          module: entry[at + 2] as number,
        });
      }
      blocks.set(entry[0] as string, {
        at: { node: entry[1] as number, module: entry[2] as number },
        inputs,
      });
    }
    this.#blocks = blocks;
    this.#members = new Map(data.members.map((entry) => [entry[0] ?? -1, entry.slice(1)]));
    this.#created = new Map(data.created.map(([block, symbol]) => [block, symbol]));
  }

  /**
   * Reads `scope_index()`'s text for `preview`, the update it came with.
   *
   * @throws CoreError (`protocol`) for an index that is not version 1, belongs to another
   *   document (its content hash is not the preview's), or has a parent that is not smaller than
   *   its node or an index out of range.
   */
  static parse(json: string, preview: PreviewResult): ScopeIndex {
    let value: unknown;
    try {
      value = JSON.parse(json);
    } catch (error) {
      throw new CoreError('protocol', 'scopeIndex: not JSON', { cause: error });
    }
    if (typeof value !== 'object' || value === null || Array.isArray(value)) {
      refuse('not an object');
    }
    const data = value as Partial<Record<keyof ScopeIndexData, unknown>>;
    if (data.v !== SCOPE_INDEX_VERSION) {
      refuse('another version');
    }
    if (preview.contentHash === null || data.contentHash !== preview.contentHash) {
      refuse('another document');
    }
    const symbols = preview.symbols.length;
    const modules = arrayOf(data.modules, 'modules');
    if (!modules.every((id) => typeof id === 'string')) {
      refuse('a module is not an ID');
    }
    const names = arrayOf(data.names, 'names').length;
    const trail = arrayOf(data.trail, 'trail');
    if (trail.length % 3 !== 0) {
      refuse('the trail is cut short');
    }
    const nodes = trail.length / 3;
    for (let node = 0; node < nodes; node += 1) {
      const [parent, name, symbol] = trail.slice(node * 3, node * 3 + 3);
      if (
        !isIndex(parent, node, true) ||
        !isIndex(name, names) ||
        !isIndex(symbol, symbols, true)
      ) {
        refuse(`node ${String(node)} is out of range`);
      }
    }
    const functions = arrayOf(data.functions, 'functions');
    for (const list of functions) {
      const pairs = arrayOf(list, 'a function list');
      if (
        pairs.length % 2 !== 0 ||
        !pairs.every((item, at) => (at % 2 === 0 ? isIndex(item, names) : isIndex(item, symbols)))
      ) {
        refuse('a function list is out of range');
      }
    }
    const parked = arrayOf(data.parked, 'parked');
    if (parked.length !== modules.length || !parked.every((node) => isIndex(node, nodes, true))) {
      refuse('a parked point is out of range');
    }
    const position = (node: unknown, module: unknown, recorded: boolean): boolean =>
      isIndex(node, nodes, true) &&
      (isIndex(module, functions.length) || (!recorded && module === -1 && node === -1));
    for (const entry of arrayOf(data.blocks, 'blocks')) {
      const cells = arrayOf(entry, 'a block');
      if (
        cells.length < 3 ||
        cells.length % 3 !== 0 ||
        typeof cells[0] !== 'string' ||
        !position(cells[1], cells[2], false)
      ) {
        refuse('a block is out of range');
      }
      for (let at = 3; at < cells.length; at += 3) {
        if (typeof cells[at] !== 'string' || !position(cells[at + 1], cells[at + 2], true)) {
          refuse('a block input is out of range');
        }
      }
    }
    for (const entry of arrayOf(data.members, 'members')) {
      const list = arrayOf(entry, 'a member list');
      if (list.length < 2 || !list.every((item) => isIndex(item, symbols))) {
        refuse('a member list is out of range');
      }
    }
    for (const entry of arrayOf(data.created, 'created')) {
      const pair = arrayOf(entry, 'a created pair');
      if (pair.length !== 2 || typeof pair[0] !== 'string' || !isIndex(pair[1], symbols)) {
        refuse('a created pair is out of range');
      }
    }
    return new ScopeIndex(data as ScopeIndexData, preview.symbols);
  }

  /** `Analysis::scope_at`: what is visible at a block, or `null` when the index has no answer. */
  symbolsAt(blockId: string, input: string | null): SymbolInfo[] | null {
    const block = this.#blocks.get(blockId);
    if (block === undefined) {
      return null;
    }
    const position = (input === null ? undefined : block.inputs?.get(input)) ?? block.at;
    return position.module < 0 ? null : this.#visible(position.node, position.module);
  }

  /**
   * `Analysis::scope_after`: what is visible directly after a block, with the variable it creates
   * (which hides what has its name, but not a type's members); `null` without an answer.
   */
  symbolsAfter(blockId: string): SymbolInfo[] | null {
    const visible = this.symbolsAt(blockId, null);
    const created = this.#created.get(blockId);
    const own = created === undefined ? undefined : this.#symbols[created];
    if (visible === null || own === undefined) {
      return visible;
    }
    const after = visible.filter((info) => info.name !== own.name || isMember(info));
    after.push(own);
    return after.sort(byNameThenId);
  }

  /** `Analysis::parked_scope`: what a block on the module's canvas sees; empty for no such module. */
  parkedScope(moduleId: string): SymbolInfo[] {
    const module = this.#modules.indexOf(moduleId);
    return module < 0 ? [] : this.#visible(this.#parked[module] ?? -1, module);
  }

  /**
   * `visible_at` of `crates/b2c-core-wasm/src/scope.rs`: what a paste binds against. Without a
   * block, the parked scope; with an input, what is visible there; otherwise after the block. A
   * block without an answer sees the module's functions and user types, with their members.
   */
  visibleAt(moduleId: string, blockId: string | null, input: string | null): SymbolInfo[] {
    if (blockId === null) {
      return this.parkedScope(moduleId);
    }
    const answer = input === null ? this.symbolsAfter(blockId) : this.symbolsAt(blockId, input);
    return (
      answer ??
      this.#symbols.filter(
        (info) =>
          info.module === moduleId &&
          (info.kind === 'function' ||
            info.kind === 'struct' ||
            info.kind === 'enum' ||
            isMember(info)),
      )
    );
  }

  /** The symbols visible at a position, each once, in the update's order (by name, then ID). */
  #visible(start: number, module: number): SymbolInfo[] {
    const names = new Set<number>();
    const found = new Set<number>();
    const add = (symbol: number): void => {
      found.add(symbol);
      for (const member of this.#members.get(symbol) ?? []) {
        found.add(member);
      }
    };
    let node = start;
    while (node >= 0) {
      const at = node * 3;
      const parent = this.#trail[at];
      const name = this.#trail[at + 1];
      const symbol = this.#trail[at + 2];
      if (parent === undefined || name === undefined || symbol === undefined) {
        break;
      }
      if (!names.has(name)) {
        names.add(name);
        if (symbol >= 0) {
          add(symbol);
        }
      }
      // Parents always come first; the check keeps a lookup from looping.
      node = parent < node ? parent : -1;
    }
    const functions = this.#functions[module] ?? [];
    for (let at = 0; at + 1 < functions.length; at += 2) {
      const name = functions[at] ?? -1;
      const symbol = functions[at + 1] ?? -1;
      if (!names.has(name) && symbol >= 0) {
        add(symbol);
      }
    }
    return [...found]
      .sort((a, b) => a - b)
      .flatMap((symbol) => {
        const info = this.#symbols[symbol];
        return info === undefined ? [] : [info];
      });
  }
}

function byNameThenId(a: SymbolInfo, b: SymbolInfo): number {
  return compareBytes(a.name, b.name) || compareBytes(a.id, b.id);
}

/** Rust's byte order of two strings (UTF-8), which the core sorts by. */
function compareBytes(a: string, b: string): number {
  if (a === b) {
    return 0;
  }
  const encoder = new TextEncoder();
  const x = encoder.encode(a);
  const y = encoder.encode(b);
  const length = Math.min(x.length, y.length);
  for (let at = 0; at < length; at += 1) {
    const difference = (x[at] ?? 0) - (y[at] ?? 0);
    if (difference !== 0) {
      return difference;
    }
  }
  return x.length - y.length;
}
