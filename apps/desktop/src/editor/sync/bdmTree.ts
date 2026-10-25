/**
 * Plain-data helpers over block trees of the Block Document Model (docs/spec/05-project-format.md
 * §5.4–5.5). They work on any block, including blocks whose type the catalog does not know, and
 * mirror the rules of `b2c_model::ids`: a field value `{sym, name}` declares a symbol, so does each
 * row of the row extras `extra.params`, `extra.fields` and `extra.values`
 * (docs/plans/m3-contracts/w2.md §2.1); a field value `{ref}` and an expression token `{ref}` refer
 * to one.
 *
 * Every walk is iterative: statement lists and stacks may hold thousands of blocks.
 */
import type {
  BdmBlock,
  InputValue,
  JsonValue,
  TokenJson,
  TypeJson,
} from '@blocks2cpp/b2c-core-wasm';
import { parseTerm, renameTermRefs, termJson } from '@blocks2cpp/blockly-ext';

/** A symbol declaration found in a block. */
export interface FoundDecl {
  readonly sym: string;
  readonly name: string;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

function own(record: Record<string, unknown>, key: string): unknown {
  return Object.hasOwn(record, key) ? record[key] : undefined;
}

/**
 * Sets an own data property, never through a setter: keys come from loaded data, and a key such
 * as `__proto__` must stay data (the loader refuses it, this keeps it harmless regardless).
 */
function setOwn<T>(record: Record<string, T>, key: string, value: T): void {
  Object.defineProperty(record, key, {
    value,
    writable: true,
    enumerable: true,
    configurable: true,
  });
}

/** The blocks nested directly in `node`: in its inputs, its statement lists and its stack. */
export function childNodes(node: BdmBlock): BdmBlock[] {
  const children: BdmBlock[] = [];
  for (const input of Object.values(node.inputs ?? {})) {
    if ('block' in input) {
      children.push(input.block);
    }
  }
  for (const list of Object.values(node.statements ?? {})) {
    children.push(...list);
  }
  if (node.stack !== undefined) {
    children.push(...node.stack);
  }
  return children;
}

/** Calls `visit` for every block of the trees, parents before children. */
export function forEachNode(roots: readonly BdmBlock[], visit: (node: BdmBlock) => void): void {
  const pending: BdmBlock[] = [...roots].reverse();
  for (let node = pending.pop(); node !== undefined; node = pending.pop()) {
    visit(node);
    const children = childNodes(node);
    for (let index = children.length - 1; index >= 0; index -= 1) {
      const child = children[index];
      if (child !== undefined) {
        pending.push(child);
      }
    }
  }
}

/** The declaration in a field value (`{sym, name}`), or `null`. */
export function declInFieldValue(value: unknown): FoundDecl | null {
  if (!isRecord(value)) {
    return null;
  }
  const sym = own(value, 'sym');
  const name = own(value, 'name');
  return typeof sym === 'string' && typeof name === 'string' ? { sym, name } : null;
}

/**
 * The `extra` keys whose rows declare symbols (`b2c_model`'s `ROW_KEYS`, w2.md §2.1): a function's
 * parameters, a struct's fields and an enum's values, each row `{sym, name, …}`.
 */
export const ROW_EXTRA_KEYS: readonly string[] = Object.freeze(['params', 'fields', 'values']);

/** The rows of the row extras of an `extra`, in key order, then row order. */
function extraRows(extra: Record<string, JsonValue> | undefined): unknown[] {
  if (extra === undefined) {
    return [];
  }
  const found: unknown[] = [];
  for (const key of ROW_EXTRA_KEYS) {
    const rows = own(extra, key);
    if (Array.isArray(rows)) {
      for (const row of rows as unknown[]) {
        found.push(row);
      }
    }
  }
  return found;
}

/** The declarations of the row extras' rows (`[{sym, name, …}]`) in an `extra`. */
function rowDecls(extra: Record<string, JsonValue> | undefined): FoundDecl[] {
  const found: FoundDecl[] = [];
  for (const row of extraRows(extra)) {
    const decl = declInFieldValue(row);
    if (decl !== null) {
      found.push(decl);
    }
  }
  return found;
}

/** Every symbol a single block declares (not its nested blocks), in file order. */
export function nodeDecls(node: BdmBlock): FoundDecl[] {
  const found: FoundDecl[] = [];
  for (const value of Object.values(node.fields ?? {})) {
    const decl = declInFieldValue(value);
    if (decl !== null) {
      found.push(decl);
    }
  }
  found.push(...rowDecls(node.extra));
  return found;
}

/** Every symbol the trees declare. */
export function treeDecls(roots: readonly BdmBlock[]): FoundDecl[] {
  const found: FoundDecl[] = [];
  forEachNode(roots, (node) => {
    found.push(...nodeDecls(node));
  });
  return found;
}

/** Every block ID of the trees. */
export function treeBlockIds(roots: readonly BdmBlock[]): string[] {
  const ids: string[] = [];
  forEachNode(roots, (node) => {
    ids.push(node.id);
  });
  return ids;
}

function renameTokens(tokens: TokenJson[], syms: ReadonlyMap<string, string>): void {
  tokens.forEach((token, index) => {
    if ('ref' in token) {
      const renamed = syms.get(token.ref);
      if (renamed !== undefined) {
        tokens[index] = { ref: renamed };
      }
    }
  });
}

function renameInput(input: InputValue, syms: ReadonlyMap<string, string>): void {
  if ('expr' in input) {
    renameTokens(input.expr, syms);
  }
}

/**
 * A type term (a field's value or a parameter row's type) with the user types it names renamed by
 * `syms`, as the core's ID remapping renames them (`terms::rename`); `null` when it is not a term
 * or names none of them.
 */
export function renamedTypeJson(
  value: unknown,
  syms: ReadonlyMap<string, string>,
): TypeJson | null {
  const term = parseTerm(value);
  if (term === null) {
    return null;
  }
  const renamed = renameTermRefs(term, syms);
  return renamed === term ? null : termJson(renamed);
}

/**
 * Renames, in place, block IDs (`blockIds`) and symbol IDs (`syms`) in the trees: block `id`s,
 * declarations (fields and the rows of `params`, `fields` and `values`), field references and
 * expression-token references.
 * IDs that are not in the maps stay as they are.
 */
export function renameInTrees(
  roots: readonly BdmBlock[],
  blockIds: ReadonlyMap<string, string>,
  syms: ReadonlyMap<string, string>,
): void {
  forEachNode(roots, (node) => {
    const id = blockIds.get(node.id);
    if (id !== undefined) {
      node.id = id;
    }
    const fields = node.fields;
    if (fields !== undefined) {
      for (const [key, value] of Object.entries(fields)) {
        if (typeof value !== 'object') {
          continue;
        }
        if ('sym' in value && typeof value.sym === 'string' && typeof value.name === 'string') {
          const renamed = syms.get(value.sym);
          if (renamed !== undefined) {
            setOwn(fields, key, { sym: renamed, name: value.name });
          }
        } else if ('ref' in value && typeof value.ref === 'string') {
          const renamed = syms.get(value.ref);
          if (renamed !== undefined) {
            setOwn(fields, key, { ref: renamed });
          }
        } else {
          // A template type term (format version 2): the user types it names (types.md §2.4).
          const renamed = renamedTypeJson(value, syms);
          if (renamed !== null) {
            setOwn(fields, key, renamed);
          }
        }
      }
    }
    for (const row of extraRows(node.extra)) {
      if (!isRecord(row)) {
        continue;
      }
      if (typeof row['sym'] === 'string') {
        const renamed = syms.get(row['sym']);
        if (renamed !== undefined) {
          row['sym'] = renamed;
        }
      }
      const type = renamedTypeJson(row['type'], syms);
      if (type !== null) {
        row['type'] = type;
      }
    }
    for (const input of Object.values(node.inputs ?? {})) {
      renameInput(input, syms);
    }
  });
}

/**
 * JSON text with object keys sorted, so that two values compare equal exactly when they hold the
 * same data. Used for the small values the sync compares (fields, `extra`), whose depth the
 * loader bounds (05 §5.6).
 */
export function stableJson(value: unknown): string {
  if (Array.isArray(value)) {
    return `[${value.map((item) => stableJson(item)).join(',')}]`;
  }
  if (isRecord(value)) {
    const keys = Object.keys(value).sort();
    return `{${keys.map((key) => `${JSON.stringify(key)}:${stableJson(value[key])}`).join(',')}}`;
  }
  // JSON has no `undefined`; an absent value compares like `null`.
  return value === undefined ? 'null' : JSON.stringify(value);
}
