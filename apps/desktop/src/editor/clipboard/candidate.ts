/**
 * The document as it would be after a paste, built in memory before anything changes on the
 * canvas, so that the loader can check the whole document against the limits of
 * docs/spec/05-project-format.md §5.6 (block count, nesting depth, sizes).
 *
 * The core's `pastePrepare` checks the payload on its own. Pasted deep inside nested blocks, or
 * into a nearly full project, valid blocks can still take the document past a limit, and a
 * document the loader refuses could no longer be previewed or saved. The paste is refused instead.
 *
 * The placement mirrors ./insert.ts: the leading blocks go to the anchor, as if the connection
 * checker allowed it (the deepest they can end up, so the check never passes a paste whose
 * inserted form would fail), and the rest go onto the canvas, each as a top-level block of its own.
 * (./insert.ts chains runs of statements there into one stack instead: the same blocks at the same
 * depth in a shorter text, so a document that passes this check with separate blocks passes it
 * with the stack too.) Only the blocks on the path to the anchor are copied; everything else is
 * shared with the given document, which is never changed.
 */
import type { BdmBlock, BdmDocument, InputValue } from '@blocks2cpp/b2c-core-wasm';

import { clampCoordinate } from '../sync/limits';
import type { PasteAnchor, WorkspacePoint } from './anchor';
import { leadingCount } from './insert';

/** How a block is reached from its parent (or, for `top`, from the module's block list). */
type Step =
  | { readonly kind: 'top'; readonly index: number }
  | { readonly kind: 'input'; readonly name: string }
  | { readonly kind: 'statement'; readonly name: string; readonly index: number }
  | { readonly kind: 'stack'; readonly index: number };

/** A block found in a module, with the way back to the top. */
interface Found {
  readonly node: BdmBlock;
  readonly step: Step;
  readonly parent: Found | null;
}

/** An own property of a record (never one inherited from its prototype). */
function own<T>(record: Readonly<Record<string, T>> | undefined, key: string): T | undefined {
  return record !== undefined && Object.hasOwn(record, key) ? record[key] : undefined;
}

/**
 * A copy of `record` with `key` set as an own data property, never through a setter (input names
 * come from the canvas; a key such as `__proto__` must stay data).
 */
function withEntry<T>(
  record: Readonly<Record<string, T>> | undefined,
  key: string,
  value: T,
): Record<string, T> {
  const copy: Record<string, T> = { ...record };
  Object.defineProperty(copy, key, { value, writable: true, enumerable: true, configurable: true });
  return copy;
}

/** The block with `id` among `roots` and everything nested in them, or `null`. Iterative. */
function locate(roots: readonly BdmBlock[], id: string): Found | null {
  const pending: Found[] = [];
  for (let index = roots.length - 1; index >= 0; index -= 1) {
    const node = roots[index];
    if (node !== undefined) {
      pending.push({ node, step: { kind: 'top', index }, parent: null });
    }
  }
  for (let found = pending.pop(); found !== undefined; found = pending.pop()) {
    const node = found.node;
    if (node.id === id) {
      return found;
    }
    for (const [name, input] of Object.entries(node.inputs ?? {})) {
      if ('block' in input) {
        pending.push({ node: input.block, step: { kind: 'input', name }, parent: found });
      }
    }
    for (const [name, list] of Object.entries(node.statements ?? {})) {
      list.forEach((child, index) => {
        pending.push({ node: child, step: { kind: 'statement', name, index }, parent: found });
      });
    }
    node.stack?.forEach((child, index) => {
      pending.push({ node: child, step: { kind: 'stack', index }, parent: found });
    });
  }
  return null;
}

/** `list` with `nodes` inserted before position `at`. */
function spliced(list: readonly BdmBlock[], at: number, nodes: readonly BdmBlock[]): BdmBlock[] {
  return [...list.slice(0, at), ...nodes, ...list.slice(at)];
}

/** A copy of `parent` with `child` in place of the block that `step` reaches. */
function withChild(parent: BdmBlock, step: Step, child: BdmBlock): BdmBlock {
  switch (step.kind) {
    case 'top':
      return parent;
    case 'input': {
      return {
        ...parent,
        inputs: withEntry<InputValue>(parent.inputs, step.name, { block: child }),
      };
    }
    case 'statement': {
      const list = [...(own(parent.statements, step.name) ?? [])];
      list[step.index] = child;
      return { ...parent, statements: withEntry(parent.statements, step.name, list) };
    }
    case 'stack': {
      const stack = [...(parent.stack ?? [])];
      stack[step.index] = child;
      return { ...parent, stack };
    }
  }
}

/** `roots` with `found.node` replaced by `replacement`: its ancestors are copied, nothing else. */
function replaced(roots: readonly BdmBlock[], found: Found, replacement: BdmBlock): BdmBlock[] {
  let current = replacement;
  let at = found;
  while (at.parent !== null) {
    current = withChild(at.parent.node, at.step, current);
    at = at.parent;
  }
  const copy = [...roots];
  if (at.step.kind === 'top') {
    copy[at.step.index] = current;
  }
  return copy;
}

/** `roots` with `leading` directly after the block `found`, or `null` when it has no list. */
function insertedAfter(
  roots: readonly BdmBlock[],
  found: Found,
  leading: readonly BdmBlock[],
): BdmBlock[] | null {
  const parent = found.parent;
  if (parent === null) {
    // A top-level statement: the blocks go to the start of its loose stack.
    const node = found.node;
    return replaced(roots, found, { ...node, stack: [...leading, ...(node.stack ?? [])] });
  }
  const step = found.step;
  switch (step.kind) {
    case 'statement': {
      const list = own(parent.node.statements, step.name) ?? [];
      const statements = withEntry(
        parent.node.statements,
        step.name,
        spliced(list, step.index + 1, leading),
      );
      return replaced(roots, parent, { ...parent.node, statements });
    }
    case 'stack': {
      const stack = spliced(parent.node.stack ?? [], step.index + 1, leading);
      return replaced(roots, parent, { ...parent.node, stack });
    }
    case 'top':
    case 'input':
      return null;
  }
}

/**
 * `roots` with the leading blocks at the anchor, or `null` when they would stay on the canvas
 * (the anchor block is not in the module, or a value input already holds a block).
 */
function attachedAtAnchor(
  roots: readonly BdmBlock[],
  anchor: PasteAnchor,
  leading: readonly BdmBlock[],
): BdmBlock[] | null {
  if (anchor.kind === 'canvas') {
    return null;
  }
  const found = locate(roots, anchor.block.id);
  if (found === null) {
    return null;
  }
  const node = found.node;
  switch (anchor.kind) {
    case 'after':
      return insertedAfter(roots, found, leading);
    case 'list': {
      const list = own(node.statements, anchor.input) ?? [];
      const statements = withEntry(node.statements, anchor.input, [...leading, ...list]);
      return replaced(roots, found, { ...node, statements });
    }
    case 'value': {
      const first = leading[0];
      const current = own(node.inputs, anchor.input);
      if (first === undefined || (current !== undefined && 'block' in current)) {
        return null;
      }
      const inputs = withEntry<InputValue>(node.inputs, anchor.input, { block: first });
      return replaced(roots, found, { ...node, inputs });
    }
  }
}

/**
 * The document `doc` with `nodes` (prepared by the core for `anchor`) inserted into module
 * `moduleId` the way ./insert.ts inserts them on the canvas: the leading blocks at the anchor, the
 * rest as top-level blocks at `origin`. `doc` itself is left unchanged; a document without that
 * module is returned as it is.
 */
export function documentWithPasted(
  doc: BdmDocument,
  moduleId: string,
  nodes: readonly BdmBlock[],
  anchor: PasteAnchor,
  origin: WorkspacePoint,
): BdmDocument {
  const index = doc.modules.findIndex((module) => module.id === moduleId);
  const module = doc.modules[index];
  if (module === undefined) {
    return doc;
  }
  let roots: BdmBlock[] = [...module.workspace.blocks];
  let loose: readonly BdmBlock[] = nodes;
  const leading = leadingCount(nodes, anchor);
  if (leading > 0) {
    const attached = attachedAtAnchor(roots, anchor, nodes.slice(0, leading));
    if (attached !== null) {
      roots = attached;
      loose = nodes.slice(leading);
    }
  }
  const x = clampCoordinate(origin.x);
  const y = clampCoordinate(origin.y);
  for (const node of loose) {
    roots.push({ ...node, x, y });
  }
  const modules = [...doc.modules];
  modules[index] = { ...module, workspace: { ...module.workspace, blocks: roots } };
  return { ...doc, modules };
}
