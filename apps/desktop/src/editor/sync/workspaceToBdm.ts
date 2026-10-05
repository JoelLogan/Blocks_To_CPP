/**
 * Blockly → BDM (docs/spec/05-project-format.md §5.4, 02 §2.4.1): reads a module's canvas back into
 * the document that is analysed, saved and sent to the backend.
 *
 * - Statements are written as arrays, never as `next` chains; a loose stack is written as its head
 *   block with `stack` (amendment A1).
 * - Top-level blocks get `x` and `y` as whole numbers within ±10⁷.
 * - An expression shadow is written as `{expr}` (or left out while it is *absent*); a nested block
 *   as `{block}`; a placeholder as the data it keeps.
 * - Everything the editor does not show is kept verbatim from the base document: other modules,
 *   frames, notes, `x-ext` and the project settings. The viewport is not read here: it is captured
 *   only when saving (see ./viewport.ts).
 *
 * Blockly's own serialisation and variable model are never used. Reading is iterative.
 */
import type {
  BdmBlock,
  BdmDocument,
  BdmModule,
  BdmViewport,
  InputValue,
} from '@blocks2cpp/b2c-core-wasm';
import { readExprShadow } from '@blocks2cpp/blockly-ext';
import * as Blockly from 'blockly/core';

import { readComment, readDisabled, readExtra, readFields } from './blockState';
import { blockDefOf, statementDef, valueInputDef } from './catalog';
import { SyncError } from './errors';
import { clampCoordinate } from './limits';
import { originOf } from './origin';
import { placeholderNode } from './placeholders';

const VALUE_INPUT = Blockly.inputs.inputTypes.VALUE;
const STATEMENT_INPUT = Blockly.inputs.inputTypes.STATEMENT;

/** Receives the node read for a block. */
type Sink = (node: BdmBlock) => void;

/** A block still to read, and where its node goes. */
interface ReadTask {
  readonly block: Blockly.Block;
  readonly top: boolean;
  readonly sink: Sink;
}

/**
 * Whether a block is part of the project: a placeholder or a catalog block, and not an insertion
 * marker (a drag preview). Expression shadows are read as their input's value instead.
 */
function isProjectBlock(block: Blockly.Block): boolean {
  if (block.isInsertionMarker() || block.isDeadOrDying()) {
    return false;
  }
  return placeholderNode(block) !== null || blockDefOf(block.type) !== null;
}

/** The project blocks of a `next` chain starting at `first`, in order. */
function chainFrom(first: Blockly.Block | null): Blockly.Block[] {
  const members: Blockly.Block[] = [];
  for (let block = first; block !== null; block = block.getNextBlock()) {
    if (isProjectBlock(block)) {
      members.push(block);
    }
  }
  return members;
}

/** The canvas position of a top-level block, as a project stores it. */
function positionOf(block: Blockly.Block): { x: number; y: number } {
  const xy = block.getRelativeToSurfaceXY();
  return { x: clampCoordinate(xy.x), y: clampCoordinate(xy.y) };
}

/** Queues reading a statement list; returns the array its nodes will be written to. */
function queueList(members: readonly Blockly.Block[], push: (task: ReadTask) => void): BdmBlock[] {
  const list = new Array<BdmBlock>(members.length);
  members.forEach((member, index) => {
    push({
      block: member,
      top: false,
      sink: (node) => {
        list[index] = node;
      },
    });
  });
  return list;
}

/** Reads a placeholder: its kept data with the block's own position, flags and comment. */
function readPlaceholder(
  block: Blockly.Block,
  kept: BdmBlock,
  top: boolean,
  push: (task: ReadTask) => void,
): BdmBlock {
  // Spreading copies own properties as data, never through a setter. The block's own state comes
  // from Blockly (the user may have moved, collapsed, disabled or commented the placeholder).
  const node: BdmBlock = { ...kept, id: block.id };
  const stack = kept.stack;
  delete node.x;
  delete node.y;
  delete node.collapsed;
  delete node.disabled;
  delete node.comment;
  delete node.stack;
  if (top) {
    const { x, y } = positionOf(block);
    node.x = x;
    node.y = y;
  }
  if (block.isCollapsed()) {
    node.collapsed = true;
  }
  if (readDisabled(block)) {
    node.disabled = true;
  }
  const comment = readComment(block);
  if (comment !== undefined) {
    node.comment = comment;
  }
  if (top) {
    const chained = chainFrom(block.getNextBlock());
    if (chained.length > 0) {
      node.stack = queueList(chained, push);
    } else if (stack !== undefined && stack.length > 0) {
      node.stack = stack;
    }
  }
  return node;
}

/** Reads one block's own data and queues the blocks nested in it. */
function readOne(task: ReadTask, push: (task: ReadTask) => void): void {
  const { block, top } = task;
  const kept = placeholderNode(block);
  if (kept !== null) {
    task.sink(readPlaceholder(block, kept, top, push));
    return;
  }
  const def = blockDefOf(block.type);
  if (def === null) {
    return;
  }
  const node: BdmBlock = { id: block.id, type: block.type, v: def.version };
  if (top) {
    const { x, y } = positionOf(block);
    node.x = x;
    node.y = y;
  }
  if (block.isCollapsed()) {
    node.collapsed = true;
  }
  if (readDisabled(block)) {
    node.disabled = true;
  }
  const comment = readComment(block);
  if (comment !== undefined) {
    node.comment = comment;
  }
  const origin = originOf(block);
  const extra = readExtra(block, def, origin);
  if (extra !== undefined) {
    node.extra = extra;
  }
  const fields = readFields(block, def, origin);
  if (fields !== undefined) {
    node.fields = fields;
  }

  const inputs: Record<string, InputValue> = {};
  const statements: Record<string, BdmBlock[]> = {};
  let anyInput = false;
  let anyStatement = false;
  for (const input of block.inputList) {
    const connection = input.connection;
    if (connection === null) {
      continue;
    }
    const name = input.name;
    if (input.type === VALUE_INPUT && valueInputDef(def, name) !== null) {
      const target = connection.targetBlock();
      if (target === null || target.isInsertionMarker()) {
        continue;
      }
      const shadow = target.isShadow() ? readExprShadow(target) : null;
      if (shadow !== null) {
        if (!shadow.absent) {
          inputs[name] = shadow.draft
            ? { expr: shadow.tokens, draft: true }
            : { expr: shadow.tokens };
          anyInput = true;
        }
      } else if (isProjectBlock(target)) {
        anyInput = true;
        push({
          block: target,
          top: false,
          sink: (child) => {
            inputs[name] = { block: child };
          },
        });
      }
    } else if (input.type === STATEMENT_INPUT && statementDef(def, name) !== null) {
      const members = chainFrom(connection.targetBlock());
      if (members.length > 0 || origin?.statements.has(name) === true) {
        statements[name] = queueList(members, push);
        anyStatement = true;
      }
    }
  }
  if (anyInput) {
    node.inputs = inputs;
  }
  if (anyStatement) {
    node.statements = statements;
  }
  if (top) {
    const stacked = chainFrom(block.getNextBlock());
    if (stacked.length > 0) {
      node.stack = queueList(stacked, push);
    }
  }
  task.sink(node);
}

/**
 * Reads blocks and everything nested in them into BDM nodes. `top` says the blocks are on the
 * canvas (they get `x`, `y` and their `next` chain as `stack`). Blocks that are not part of the
 * project (insertion markers, unknown non-placeholder blocks) are left out.
 */
export function readBlockTrees(blocks: readonly Blockly.Block[], top: boolean): BdmBlock[] {
  const roots = blocks.filter(isProjectBlock);
  const out = new Array<BdmBlock | undefined>(roots.length);
  const tasks: ReadTask[] = [];
  const push = (task: ReadTask): void => {
    tasks.push(task);
  };
  for (let index = roots.length - 1; index >= 0; index -= 1) {
    const block = roots[index];
    if (block !== undefined) {
      push({
        block,
        top,
        sink: (node) => {
          out[index] = node;
        },
      });
    }
  }
  for (let task = tasks.pop(); task !== undefined; task = tasks.pop()) {
    readOne(task, push);
  }
  return out.filter((node): node is BdmBlock => node !== undefined);
}

/** The project blocks on the canvas, read into BDM nodes (top-level blocks with `x`, `y`). */
export function readTopBlocks(workspace: Blockly.Workspace): BdmBlock[] {
  const tops = workspace.getTopBlocks(false).filter((block) => !block.isShadow());
  return readBlockTrees(tops, true);
}

/** A copy of `base` whose module at `index` gets `workspace` (other keys are shared). */
function withModuleWorkspace(
  base: BdmDocument,
  index: number,
  change: (module: BdmModule) => BdmModule,
): BdmDocument {
  const modules = base.modules.slice();
  const module = modules[index];
  if (module !== undefined) {
    modules[index] = change(module);
  }
  // Spreading copies own properties as data (an `x-ext` key named `__proto__` stays data).
  return { ...base, modules };
}

function moduleIndex(doc: BdmDocument, moduleId: string): number {
  const index = doc.modules.findIndex((module) => module.id === moduleId);
  if (index < 0) {
    throw new SyncError('unknownModule', 'The document has no module with this ID.');
  }
  return index;
}

/**
 * The document `base` with module `moduleId`'s blocks read from the workspace. Everything else
 * (other modules, frames, notes, the viewport, `x-ext`, the project settings) is kept from `base`.
 *
 * @throws SyncError (`unknownModule`) when `base` has no such module.
 */
export function readModule(
  workspace: Blockly.Workspace,
  base: BdmDocument,
  moduleId: string,
): BdmDocument {
  const index = moduleIndex(base, moduleId);
  const blocks = readTopBlocks(workspace);
  return withModuleWorkspace(base, index, (module) => ({
    ...module,
    workspace: { ...module.workspace, blocks },
  }));
}

/**
 * The document with module `moduleId`'s viewport replaced (`undefined` removes it).
 *
 * @throws SyncError (`unknownModule`) when the document has no such module.
 */
export function withViewport(
  doc: BdmDocument,
  moduleId: string,
  viewport: BdmViewport | undefined,
): BdmDocument {
  const index = moduleIndex(doc, moduleId);
  return withModuleWorkspace(doc, index, (module) => {
    const workspace = { ...module.workspace };
    if (viewport === undefined) {
      delete workspace.viewport;
    } else {
      workspace.viewport = viewport;
    }
    return { ...module, workspace };
  });
}
