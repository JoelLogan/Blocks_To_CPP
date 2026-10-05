/**
 * Plain-data helpers over block trees of the Block Document Model (docs/spec/05-project-format.md
 * §5.4–5.5). They work on any block, including blocks whose type the catalog does not know, and
 * mirror the rules of `b2c_model::ids`: a field value `{sym, name}` declares a symbol, so does each
 * row of `extra.params`; a field value `{ref}` and an expression token `{ref}` refer to one.
 *
 * Every walk is iterative: statement lists and stacks may hold thousands of blocks.
 */
import type { BdmBlock, InputValue, JsonValue, TokenJson } from '@blocks2cpp/b2c-core-wasm';

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

/** The declarations of `params` rows (`[{sym, name, …}]`) in an `extra`. */
function paramDecls(extra: Record<string, JsonValue> | undefined): FoundDecl[] {
  if (extra === undefined) {
    return [];
  }
  const rows = own(extra, 'params');
  if (!Array.isArray(rows)) {
    return [];
  }
  const found: FoundDecl[] = [];
  for (const row of rows) {
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
  found.push(...paramDecls(node.extra));
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
 * Renames, in place, block IDs (`blockIds`) and symbol IDs (`syms`) in the trees: block `id`s,
 * declarations (fields and `params` rows), field references and expression-token references.
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
        if ('sym' in value) {
          const renamed = syms.get(value.sym);
          if (renamed !== undefined) {
            setOwn(fields, key, { sym: renamed, name: value.name });
          }
        } else if ('ref' in value) {
          const renamed = syms.get(value.ref);
          if (renamed !== undefined) {
            setOwn(fields, key, { ref: renamed });
          }
        }
      }
    }
    const rows = node.extra === undefined ? undefined : own(node.extra, 'params');
    if (Array.isArray(rows)) {
      rows.forEach((row) => {
        if (isRecord(row) && typeof row['sym'] === 'string') {
          const renamed = syms.get(row['sym']);
          if (renamed !== undefined) {
            row['sym'] = renamed;
          }
        }
      });
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
