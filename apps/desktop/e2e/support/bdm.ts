/**
 * Project documents in the tests: reading the open project (the hook's canonical text) and building
 * the block nodes the tests insert (docs/spec/05-project-format.md §5.4). The builders cover the
 * blocks of the guessing game (03 §3.13.1); e2e/support/bdm.test.ts checks them against the
 * catalog (packages/catalog-gen/catalog.json).
 */

/** An expression input: tokens (05 §5.5). */
export interface ExprInput {
  readonly expr: readonly Record<string, string>[];
}

/** A nested block in an input. */
export interface BlockInput {
  readonly block: BlockNode;
}

/** A block node as the project file writes it (the parts the tests use). */
export interface BlockNode {
  readonly id: string;
  readonly type: string;
  readonly v: number;
  readonly x?: number;
  readonly y?: number;
  readonly extra?: Readonly<Record<string, unknown>>;
  readonly fields?: Readonly<Record<string, unknown>>;
  readonly inputs?: Readonly<Record<string, ExprInput | BlockInput>>;
  readonly statements?: Readonly<Record<string, readonly BlockNode[]>>;
  readonly stack?: readonly BlockNode[];
}

/** A project document (the parts the tests read). */
export interface ProjectDocument {
  readonly modules: readonly {
    readonly id: string;
    readonly name: string;
    readonly workspace: { readonly blocks: readonly BlockNode[] };
  }[];
}

/** Parses the hook's document text, checking the little the tests rely on. */
export function parseDocument(text: string): ProjectDocument {
  const value: unknown = JSON.parse(text);
  if (
    typeof value !== 'object' ||
    value === null ||
    !Array.isArray((value as { modules?: unknown }).modules)
  ) {
    throw new Error('The document has no modules');
  }
  return value as ProjectDocument;
}

/** Every block of the trees, parents before children (iterative). */
export function allNodes(roots: readonly BlockNode[]): BlockNode[] {
  const found: BlockNode[] = [];
  const pending: BlockNode[] = [...roots].reverse();
  for (let node = pending.pop(); node !== undefined; node = pending.pop()) {
    found.push(node);
    const children: BlockNode[] = [];
    for (const input of Object.values(node.inputs ?? {})) {
      if ('block' in input) {
        children.push(input.block);
      }
    }
    for (const list of Object.values(node.statements ?? {})) {
      children.push(...list);
    }
    children.push(...(node.stack ?? []));
    pending.push(...children.reverse());
  }
  return found;
}

/** The blocks of the first module, at every depth. */
export function moduleNodes(doc: ProjectDocument): BlockNode[] {
  return allNodes(doc.modules[0]?.workspace.blocks ?? []);
}

/** The one top-level block of `type` in the first module; throws when there is not exactly one. */
export function onlyTopBlock(doc: ProjectDocument, type: string): BlockNode {
  const found = (doc.modules[0]?.workspace.blocks ?? []).filter((block) => block.type === type);
  if (found.length !== 1 || found[0] === undefined) {
    throw new Error(`Expected one top-level ${type} block, found ${String(found.length)}`);
  }
  return found[0];
}

/** The statement list `input` of `block` (empty when it has none). */
export function statementsOf(block: BlockNode, input: string): readonly BlockNode[] {
  return block.statements?.[input] ?? [];
}

/** The symbol a `{sym, name}` field declares, or `null`. */
export function declaredSymbol(
  block: BlockNode,
  field: string,
): { sym: string; name: string } | null {
  const value = block.fields?.[field];
  if (typeof value === 'object' && value !== null) {
    const { sym, name } = value as { sym?: unknown; name?: unknown };
    if (typeof sym === 'string' && typeof name === 'string') {
      return { sym, name };
    }
  }
  return null;
}

/** Fresh block IDs for one test: `e2e_<prefix>_<n>` (valid project IDs, 05 §5.4). */
export class NodeIds {
  #next = 1;
  readonly #prefix: string;

  constructor(prefix: string) {
    if (!/^[a-z][a-z0-9]{0,11}$/.test(prefix)) {
      throw new Error('An ID prefix is 1 to 12 lower-case letters and digits');
    }
    this.#prefix = prefix;
  }

  next(): string {
    const id = `e2e_${this.#prefix}_${String(this.#next)}`;
    this.#next += 1;
    return id;
  }
}

/** A number literal input. */
export function num(value: number): ExprInput {
  return { expr: [{ num: String(value) }] };
}

/** A text literal input. */
export function str(text: string): ExprInput {
  return { expr: [{ str: text }] };
}

/** `var.get`: the value of a variable. */
export function varGet(ids: NodeIds, sym: string): BlockNode {
  return { id: ids.next(), type: 'var.get', v: 1, fields: { VAR: { ref: sym } } };
}

/** The comparison operators of `math.compare`. */
export type CompareOp = 'lt' | 'le' | 'gt' | 'ge' | 'eq' | 'ne';

/** `math.compare`: `a OP b`. */
export function compare(ids: NodeIds, op: CompareOp, a: BlockNode, b: BlockNode): BlockNode {
  return {
    id: ids.next(),
    type: 'math.compare',
    v: 1,
    fields: { OP: op },
    inputs: { A: { block: a }, B: { block: b } },
  };
}

/** `math.random_int`: a random whole number from `low` to `high`. */
export function randomInt(ids: NodeIds, low: number, high: number): BlockNode {
  return {
    id: ids.next(),
    type: 'math.random_int',
    v: 1,
    inputs: { LOW: num(low), HIGH: num(high) },
  };
}

/** `var.declare`: `int name = value;`, declaring `sym`. */
export function declareInt(ids: NodeIds, sym: string, name: string, value: number): BlockNode {
  return {
    id: ids.next(),
    type: 'var.declare',
    v: 1,
    fields: { CONST: false, NAME: { sym, name }, TYPE: 'int' },
    inputs: { VALUE: num(value) },
  };
}

/** `io.print`: one text item, then a new line. */
export function print(ids: NodeIds, text: string): BlockNode {
  return {
    id: ids.next(),
    type: 'io.print',
    v: 1,
    extra: { itemCount: 1 },
    fields: { NEWLINE: true, SEP: 'none', STREAM: 'out' },
    inputs: { ITEM0: str(text) },
  };
}

/** `io.ask` in *keep asking* mode with `prompt`, saving into `sym` (or no variable yet). */
export function ask(ids: NodeIds, prompt: string, sym: string | null): BlockNode {
  return {
    id: ids.next(),
    type: 'io.ask',
    v: 1,
    fields: sym === null ? { MODE: 'keep_asking' } : { MODE: 'keep_asking', VAR: { ref: sym } },
    inputs: { PROMPT: str(prompt) },
  };
}

/** `control.if` with one *else if* and an *else*. */
export function ifElseIfElse(
  ids: NodeIds,
  parts: {
    readonly cond0: BlockNode;
    readonly do0: readonly BlockNode[];
    readonly cond1: BlockNode;
    readonly do1: readonly BlockNode[];
    readonly otherwise: readonly BlockNode[];
  },
): BlockNode {
  return {
    id: ids.next(),
    type: 'control.if',
    v: 1,
    extra: { elseIfCount: 1, hasElse: true },
    inputs: { COND0: { block: parts.cond0 }, COND1: { block: parts.cond1 } },
    statements: { DO0: parts.do0, DO1: parts.do1, ELSE: parts.otherwise },
  };
}
