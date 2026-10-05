/**
 * BDM → Blockly (docs/spec/05-project-format.md §5.4, 02 §2.4.1): builds a module's canvas from a
 * loaded document, for opening, reloading, recovery and module switching.
 *
 * - The block type is the catalog ID and the Blockly ID is the BDM ID; statement arrays become
 *   `next` chains; `x`, `y`, fields, `extra` (through the mutators' `b2cSetExtra`), the comment,
 *   `collapsed` and `disabled` (Blockly's *manually disabled* reason) map one to one.
 * - A value input holds its nested block, or an expression shadow (blockly-ext's
 *   `exprShadowState`) for its tokens. An input the file leaves out shows the catalog default as a
 *   shadow marked *absent*, so it stays out of the file until the user edits it.
 * - A loose stack (`stack`, amendment A1) becomes the `next` chain under its head block.
 * - A block the catalog does not have, or one the editor cannot show faithfully, becomes a
 *   placeholder that keeps its data verbatim (see ./placeholders.ts). Every catalog block is
 *   checked after it is built: it must read back exactly as the file wrote it.
 *
 * Building is iterative (a work list, no recursion along chains or nesting), runs with Blockly's
 * events off, and records nothing for undo. Blockly's own serialisation and variable model are
 * never used for project data.
 */
import type { BdmBlock, BdmDocument, InputValue, TokenJson } from '@blocks2cpp/b2c-core-wasm';
import {
  type BlockDefJson,
  copyTokens,
  EXPR_SHADOW_TYPE,
  type ExprShadowExtra,
  exprShadowState,
  type InputDefJson,
  isTokenList,
  PLACEHOLDER_TYPE,
  readExprShadow,
  tokensEqual,
  type TypeClass,
} from '@blocks2cpp/blockly-ext';
import * as Blockly from 'blockly/core';

import { readsBackAs, writeFlagsAndComment, writeOwnData } from './blockState';
import { blockDefOf, shapeFits, type Slot, statementDef, valueInputDef } from './catalog';
import { SyncError } from './errors';
import { clampCoordinate } from './limits';
import { rememberOrigin } from './origin';
import { initPlaceholder } from './placeholders';
import { clearWorkspace } from './traverse';

const VALUE_INPUT = Blockly.inputs.inputTypes.VALUE;
const STATEMENT_INPUT = Blockly.inputs.inputTypes.STATEMENT;

/** Where a block built by {@link buildBlockTree} goes. */
export type BuildPlace =
  /** On the canvas, at workspace coordinates. */
  | { readonly kind: 'top'; readonly x: number; readonly y: number }
  /** Into a value input's connection. */
  | { readonly kind: 'value'; readonly connection: Blockly.Connection }
  /** Into a statement connection: a statement input's, or the `next` of a block. */
  | { readonly kind: 'statement'; readonly connection: Blockly.Connection };

/** Work still to do: a nested block, or a statement list to chain. */
type Pending =
  | { readonly kind: 'value'; readonly connection: Blockly.Connection; readonly node: BdmBlock }
  | {
      readonly kind: 'list';
      readonly connection: Blockly.Connection;
      readonly nodes: readonly BdmBlock[];
    };

/** Runs `run` with Blockly's events off, so nothing is recorded or reported. */
export function withoutEvents<T>(run: () => T): T {
  Blockly.Events.disable();
  try {
    return run();
  } finally {
    Blockly.Events.enable();
  }
}

/** Whether `value` is an `{expr}` input. */
function isExprInput(value: InputValue): value is Extract<InputValue, { expr: unknown }> {
  return 'expr' in value;
}

/** The Blockly state of a read-only shadow that keeps `tokens` exactly. */
function tokensShadowState(
  tokens: readonly TokenJson[],
  draft: boolean,
  check: TypeClass,
  absent: boolean,
): Blockly.serialization.blocks.State {
  const extraState: ExprShadowExtra = { check, absent, draft, tokens: copyTokens(tokens) };
  return { type: EXPR_SHADOW_TYPE.tokens, extraState };
}

/** Builds blocks from BDM nodes into one workspace, then initialises them together. */
class TreeBuilder {
  private readonly pending: Pending[] = [];
  /** Every block made with `newBlock`, in creation order (shadows are made by Blockly). */
  private readonly created: Blockly.Block[] = [];
  /** Blocks to collapse once everything is built, so their collapsed text is complete. */
  private readonly toCollapse: Blockly.Block[] = [];
  private readonly roots: Blockly.Block[] = [];
  private readonly workspace: Blockly.Workspace;

  constructor(workspace: Blockly.Workspace) {
    this.workspace = workspace;
  }

  /** Builds `node` (and everything in it) at `place`; returns its block. */
  add(node: BdmBlock, place: BuildPlace): Blockly.Block {
    let block: Blockly.Block;
    if (place.kind === 'top') {
      block = this.build(node, 'top', null);
      block.moveBy(clampCoordinate(place.x), clampCoordinate(place.y));
    } else {
      block = this.build(node, place.kind, place.connection);
    }
    this.roots.push(block);
    this.drain();
    return block;
  }

  /** Collapses, initialises and renders what was built. */
  finish(): void {
    for (let index = this.toCollapse.length - 1; index >= 0; index -= 1) {
      this.toCollapse[index]?.setCollapsed(true);
    }
    if (this.workspace.rendered) {
      // As Blockly's own loader does: children first, connections tracked once all are placed.
      for (let index = this.created.length - 1; index >= 0; index -= 1) {
        const block = this.created[index];
        if (block instanceof Blockly.BlockSvg && !block.isDeadOrDying()) {
          block.setConnectionTracking(false);
          block.initSvg();
          void block.queueRender();
        }
      }
      for (const root of this.roots) {
        if (root instanceof Blockly.BlockSvg) {
          setTimeout(() => {
            if (!root.isDeadOrDying()) {
              root.setConnectionTracking(true);
            }
          }, 1);
        }
      }
    } else {
      for (const block of this.created) {
        if (!block.isDeadOrDying()) {
          block.initModel();
        }
      }
    }
  }

  private drain(): void {
    for (let task = this.pending.pop(); task !== undefined; task = this.pending.pop()) {
      if (task.kind === 'value') {
        this.build(task.node, 'value', task.connection);
      } else {
        this.buildList(task.connection, task.nodes);
      }
    }
  }

  /** Chains a statement list under `connection`, one block after the other. */
  private buildList(connection: Blockly.Connection, nodes: readonly BdmBlock[]): void {
    let next: Blockly.Connection | null = connection;
    for (const node of nodes) {
      if (next === null) {
        // Statement blocks and statement placeholders always have a `next` connection.
        console.error('A statement list could not be built completely');
        return;
      }
      next = this.build(node, 'statement', next).nextConnection;
    }
  }

  /** Builds one block, attaches it, and queues the blocks nested in it. */
  private build(node: BdmBlock, slot: Slot, parent: Blockly.Connection | null): Blockly.Block {
    const def = blockDefOf(node.type);
    let block = def === null ? null : this.catalogBlock(node, def, slot);
    if (block !== null && parent !== null && !connect(parent, block, slot)) {
      // Refused by the connection checker (a hat inside a statement list, for example).
      block.dispose(false);
      block = null;
    }
    if (block === null || def === null) {
      const placeholder = this.placeholder(node, slot);
      if (parent !== null && !connect(parent, placeholder, slot)) {
        console.error('A placeholder could not be attached; it stays on the canvas');
      }
      this.keep(placeholder, node);
      return placeholder;
    }
    this.keep(block, node);
    this.expand(block, def, node);
    return block;
  }

  /** Records a block that stays, for collapsing and initialising in {@link finish}. */
  private keep(block: Blockly.Block, node: BdmBlock): void {
    if (node.collapsed === true) {
      this.toCollapse.push(block);
    }
    this.created.push(block);
  }

  /** A catalog block holding exactly `node`'s own data, or `null` when it cannot. */
  private catalogBlock(node: BdmBlock, def: BlockDefJson, slot: Slot): Blockly.Block | null {
    const stackFits = node.stack === undefined || (slot === 'top' && def.shape === 'statement');
    if (node.v !== def.version || !shapeFits(def.shape, slot) || !stackFits) {
      return null;
    }
    let block: Blockly.Block;
    try {
      block = this.workspace.newBlock(node.type, node.id);
    } catch (error: unknown) {
      // A registration problem (for example the mutators were not registered).
      console.error('A catalog block could not be created', error);
      return null;
    }
    rememberOrigin(block, node);
    let fits = writeOwnData(block, def, node) && inputsFit(block, def, node);
    if (fits) {
      writeFlagsAndComment(block, node);
      fits = readsBackAs(block, def, node);
    }
    if (!fits) {
      block.dispose(false);
      return null;
    }
    return block;
  }

  /** A placeholder keeping `node` (and everything nested in it) verbatim. */
  private placeholder(node: BdmBlock, slot: Slot): Blockly.Block {
    const block = this.workspace.newBlock(PLACEHOLDER_TYPE, node.id);
    initPlaceholder(block, node, slot);
    writeFlagsAndComment(block, node);
    return block;
  }

  /** Gives a built catalog block its shadows, and queues its nested blocks and lists. */
  private expand(block: Blockly.Block, def: BlockDefJson, node: BdmBlock): void {
    for (const input of block.inputList) {
      const connection = input.connection;
      if (connection === null) {
        continue;
      }
      if (input.type === VALUE_INPUT) {
        const inputDef = valueInputDef(def, input.name);
        if (inputDef !== null) {
          this.valueInput(connection, inputDef, ownValue(node.inputs, input.name));
        }
      } else if (input.type === STATEMENT_INPUT && statementDef(def, input.name) !== null) {
        const list = ownValue(node.statements, input.name);
        if (list !== undefined && list.length > 0) {
          this.pending.push({ kind: 'list', connection, nodes: list });
        }
      }
    }
    const stack = node.stack;
    if (stack !== undefined && stack.length > 0 && block.nextConnection !== null) {
      this.pending.push({ kind: 'list', connection: block.nextConnection, nodes: stack });
    }
  }

  /** Fills a value input: the default shadow (absent), an expression shadow, or a nested block. */
  private valueInput(
    connection: Blockly.Connection,
    inputDef: InputDefJson,
    value: InputValue | undefined,
  ): void {
    if (value !== undefined && isExprInput(value)) {
      setShadow(connection, value.expr, value.draft === true, inputDef.check, false);
      return;
    }
    if (inputDef.default.length > 0) {
      setShadow(connection, inputDef.default, false, inputDef.check, true);
    } else {
      connection.setShadowState(null);
    }
    if (value !== undefined) {
      this.pending.push({ kind: 'value', connection, node: value.block });
    }
  }
}

/** An own property of a record from a loaded document (never an inherited one). */
function ownValue<T>(record: Readonly<Record<string, T>> | undefined, key: string): T | undefined {
  return record !== undefined && Object.hasOwn(record, key) ? record[key] : undefined;
}

/**
 * Whether every input and statement list `node` names exists on the block, with the right kind,
 * and every expression is one a shadow can keep.
 */
function inputsFit(block: Blockly.Block, def: BlockDefJson, node: BdmBlock): boolean {
  for (const [name, value] of Object.entries(node.inputs ?? {})) {
    const input = block.getInput(name);
    if (input?.type !== VALUE_INPUT || valueInputDef(def, name) === null) {
      return false;
    }
    if (isExprInput(value) && !isTokenList(value.expr)) {
      return false;
    }
  }
  for (const name of Object.keys(node.statements ?? {})) {
    const input = block.getInput(name);
    if (input?.type !== STATEMENT_INPUT || statementDef(def, name) === null) {
      return false;
    }
  }
  return true;
}

/** Connects a new block's output (value) or previous (statement) connection to `parent`. */
function connect(parent: Blockly.Connection, block: Blockly.Block, slot: Slot): boolean {
  const child = slot === 'value' ? block.outputConnection : block.previousConnection;
  if (child === null || slot === 'top') {
    return false;
  }
  try {
    return parent.connect(child);
  } catch (error: unknown) {
    console.warn('Blockly refused a connection while loading', error);
    return false;
  }
}

/**
 * Gives a value input the expression shadow for `tokens`, and makes sure it reads back exactly; a
 * value the editable shadow would change is shown read-only instead.
 */
function setShadow(
  connection: Blockly.Connection,
  tokens: readonly TokenJson[],
  draft: boolean,
  check: TypeClass,
  absent: boolean,
): void {
  connection.setShadowState(exprShadowState(tokens, draft, check, absent));
  const shadow = connection.targetBlock();
  const read = shadow === null ? null : readExprShadow(shadow);
  if (read?.absent !== absent || read.draft !== draft || !tokensEqual(read.tokens, tokens)) {
    connection.setShadowState(tokensShadowState(tokens, draft, check, absent));
  }
}

/**
 * Replaces the workspace's content with module `moduleId` of `doc` (which must come from the
 * compiler core's loader). Blockly's events are off while it builds, so nothing is recorded for
 * undo; clearing the undo history is the caller's choice. The block types must be registered
 * first (`registerEditorBlocks()`).
 *
 * @throws SyncError (`unknownModule`) when the document has no such module.
 */
export function loadModule(workspace: Blockly.Workspace, doc: BdmDocument, moduleId: string): void {
  const module = doc.modules.find((candidate) => candidate.id === moduleId);
  if (module === undefined) {
    throw new SyncError('unknownModule', 'The document has no module with this ID.');
  }
  withoutEvents(() => {
    const svg = workspace instanceof Blockly.WorkspaceSvg ? workspace : null;
    svg?.setResizesEnabled(false);
    try {
      clearWorkspace(workspace);
      const builder = new TreeBuilder(workspace);
      for (const node of module.workspace.blocks) {
        builder.add(node, { kind: 'top', x: node.x ?? 0, y: node.y ?? 0 });
      }
      builder.finish();
    } finally {
      svg?.setResizesEnabled(true);
    }
  });
}

/**
 * Builds one block tree (for example a pasted one) at `place`, with Blockly's events off. The
 * caller fires a `BlockCreate` event for the returned block when the insertion should be undoable.
 */
export function buildBlockTree(
  workspace: Blockly.Workspace,
  node: BdmBlock,
  place: BuildPlace,
): Blockly.Block {
  return withoutEvents(() => {
    const builder = new TreeBuilder(workspace);
    const block = builder.add(node, place);
    builder.finish();
    return block;
  });
}
