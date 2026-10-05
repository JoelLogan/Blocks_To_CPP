/**
 * Fresh symbol IDs for duplicated blocks (docs/spec/05-project-format.md §5.4–5.5, §5.12).
 *
 * Blockly's *Duplicate*, its own copy and paste, dragging a preset block out of the toolbox, and
 * redo copy a block's field state, including the symbol IDs of the declarations it holds. Two
 * declarations with one ID make the whole document fail to load (B2C-E0115), so when blocks are
 * created (not by loading a project, which runs with events off), every declaration whose symbol
 * ID is already declared elsewhere on the canvas gets a fresh ID, and the references inside the
 * created blocks follow it. A copied placeholder also gets fresh IDs for the blocks nested in its
 * kept data (B2C-E0114).
 *
 * The replacement is remembered per block and old ID, so redoing a duplicate (which recreates the
 * block from the state Blockly recorded before the IDs were replaced) gives the same IDs again.
 * The changes are made with events off: they are not undo steps of their own.
 */
import type { BdmBlock } from '@blocks2cpp/b2c-core-wasm';
import {
  B2cSymbolDeclField,
  B2cSymbolRefField,
  copyTokens,
  type ExprShadowExtra,
  hasB2cMutator,
  isExprShadowExtra,
  isExprShadowType,
  isProjectId,
  newId,
} from '@blocks2cpp/blockly-ext';
import type * as Blockly from 'blockly/core';

import { withoutEvents } from './bdmToWorkspace';
import { renameInTrees, treeBlockIds, treeDecls } from './bdmTree';
import { blockDefOf } from './catalog';
import { MAX_REMAP_MEMO } from './limits';
import { placeholderNode, replacePlaceholderNode } from './placeholders';
import { allBlocks, descendantsOf } from './traverse';

/** The `params` rows of a block's `extra`, or `null`. */
function paramRows(block: Blockly.Block): Record<string, unknown>[] | null {
  if (!hasB2cMutator(block)) {
    return null;
  }
  const rows: unknown = block.b2cGetExtra()['params'];
  if (!Array.isArray(rows)) {
    return null;
  }
  return rows.filter(
    (row): row is Record<string, unknown> => typeof row === 'object' && row !== null,
  );
}

/** The catalog declaration fields of a block (not the parameter rows' name fields). */
function declFields(block: Blockly.Block): B2cSymbolDeclField[] {
  const def = blockDefOf(block.type);
  if (def === null) {
    return [];
  }
  const fields: B2cSymbolDeclField[] = [];
  for (const definition of def.fields) {
    if (definition.kind === 'symbol_decl') {
      const field = block.getField(definition.name);
      if (field instanceof B2cSymbolDeclField) {
        fields.push(field);
      }
    }
  }
  return fields;
}

/** The symbol IDs a block declares: its declaration fields, its parameters, or a placeholder's. */
export function declaredSyms(block: Blockly.Block): string[] {
  const kept = placeholderNode(block);
  if (kept !== null) {
    return treeDecls([kept]).map((decl) => decl.sym);
  }
  const syms: string[] = [];
  for (const field of declFields(block)) {
    const sym = field.getSymbolId();
    if (sym !== null) {
      syms.push(sym);
    }
  }
  for (const row of paramRows(block) ?? []) {
    const sym = row['sym'];
    if (typeof sym === 'string') {
      syms.push(sym);
    }
  }
  return syms;
}

/** Which blocks on the canvas declare which symbol IDs, kept current from Blockly's events. */
export class DeclIndex {
  private readonly blocksBySym = new Map<string, Set<string>>();
  private readonly symsByBlock = new Map<string, readonly string[]>();

  /** Indexes every block of the workspace again (after loading). */
  rebuild(workspace: Blockly.Workspace): void {
    this.blocksBySym.clear();
    this.symsByBlock.clear();
    for (const block of allBlocks(workspace)) {
      this.refresh(block);
    }
  }

  /** Indexes a block again (it was created, or one of its fields or its mutation changed). */
  refresh(block: Blockly.Block): void {
    this.forget(block.id);
    const syms = declaredSyms(block);
    if (syms.length === 0) {
      return;
    }
    this.symsByBlock.set(block.id, syms);
    for (const sym of syms) {
      let holders = this.blocksBySym.get(sym);
      if (holders === undefined) {
        holders = new Set();
        this.blocksBySym.set(sym, holders);
      }
      holders.add(block.id);
    }
  }

  /** Forgets a block (it was deleted). */
  forget(blockId: string): void {
    const syms = this.symsByBlock.get(blockId);
    if (syms === undefined) {
      return;
    }
    this.symsByBlock.delete(blockId);
    for (const sym of syms) {
      const holders = this.blocksBySym.get(sym);
      holders?.delete(blockId);
      if (holders?.size === 0) {
        this.blocksBySym.delete(sym);
      }
    }
  }

  /** Whether a block outside `inside` declares `sym`. */
  declaredOutside(sym: string, inside: ReadonlySet<string>): boolean {
    for (const holder of this.blocksBySym.get(sym) ?? []) {
      if (!inside.has(holder)) {
        return true;
      }
    }
    return false;
  }

  /** Whether any block declares `sym`. */
  isDeclared(sym: string): boolean {
    return this.blocksBySym.has(sym);
  }
}

/** Renames what one created block declares or refers to. */
function renameInBlock(
  block: Blockly.Block,
  syms: ReadonlyMap<string, string>,
  nestedIds: (placeholder: Blockly.Block, node: BdmBlock) => ReadonlyMap<string, string>,
): void {
  const kept = placeholderNode(block);
  if (kept !== null) {
    const blockIds = new Map(nestedIds(block, kept));
    if (kept.id !== block.id) {
      blockIds.set(kept.id, block.id);
    }
    renameInTrees([kept], blockIds, syms);
    replacePlaceholderNode(block, kept);
    return;
  }
  for (const field of declFields(block)) {
    const decl = field.getDecl();
    const renamed = decl === null ? undefined : syms.get(decl.sym);
    if (decl !== null && renamed !== undefined) {
      field.setDecl({ sym: renamed, name: decl.name });
    }
  }
  for (const input of block.inputList) {
    for (const field of input.fieldRow) {
      if (field instanceof B2cSymbolRefField) {
        const ref = field.getRef();
        const renamed = ref === null ? undefined : syms.get(ref.ref);
        if (renamed !== undefined) {
          field.setRef({ ref: renamed });
        }
      }
    }
  }
  const rows = paramRows(block);
  if (rows !== null && hasB2cMutator(block) && rows.some((row) => syms.has(String(row['sym'])))) {
    const params = rows.map((row) => {
      const renamed = syms.get(String(row['sym']));
      return renamed === undefined ? row : { ...row, sym: renamed };
    });
    try {
      block.b2cSetExtra({ ...block.b2cGetExtra(), params });
    } catch (error: unknown) {
      console.error('The parameters of a copied function could not be renamed', error);
    }
  }
  if (block.isShadow() && isExprShadowType(block.type)) {
    renameShadowTokens(block, syms);
  }
}

/** Blockly's extra-state members of a block. */
interface ExtraStateMembers {
  saveExtraState?: () => unknown;
  loadExtraState?: (state: unknown) => void;
}

/** Renames references in the tokens an expression shadow keeps. */
function renameShadowTokens(block: Blockly.Block, syms: ReadonlyMap<string, string>): void {
  const members = block as unknown as ExtraStateMembers;
  const state = members.saveExtraState?.();
  if (!isExprShadowExtra(state)) {
    return;
  }
  const tokens = copyTokens(state.tokens);
  let changed = false;
  for (let index = 0; index < tokens.length; index += 1) {
    const token = tokens[index];
    const renamed = token !== undefined && 'ref' in token ? syms.get(token.ref) : undefined;
    if (renamed !== undefined) {
      tokens[index] = { ref: renamed };
      changed = true;
    }
  }
  if (changed) {
    const next: ExprShadowExtra = { ...state, tokens };
    members.loadExtraState?.(next);
  }
}

/** Gives created blocks fresh IDs where they would clash with blocks already on the canvas. */
export class DuplicateGuard {
  private readonly memo = new Map<string, string>();
  private readonly workspace: Blockly.Workspace;
  private readonly index: DeclIndex;
  private readonly onRenamed: (syms: ReadonlyMap<string, string>) => void;

  constructor(
    workspace: Blockly.Workspace,
    index: DeclIndex,
    onRenamed: (syms: ReadonlyMap<string, string>) => void = () => undefined,
  ) {
    this.workspace = workspace;
    this.index = index;
    this.onRenamed = onRenamed;
  }

  /** Forgets remembered replacements (a new document was loaded). */
  reset(): void {
    this.memo.clear();
  }

  /**
   * Handles blocks Blockly created: `rootId` and everything in it. Returns the symbol IDs that were
   * replaced (old → new).
   */
  onCreated(rootId: string): ReadonlyMap<string, string> {
    const root = this.workspace.getBlockById(rootId);
    if (root === null || root.isDeadOrDying()) {
      return new Map();
    }
    const blocks = descendantsOf(root);
    const inside = new Set(blocks.map((block) => block.id));
    const syms = new Map<string, string>();
    for (const block of blocks) {
      for (const sym of declaredSyms(block)) {
        if (!syms.has(sym) && this.index.declaredOutside(sym, inside)) {
          syms.set(
            sym,
            this.fresh('sym', block.id, sym, (id) => !this.index.isDeclared(id)),
          );
        }
      }
    }
    const nestedIds = (placeholder: Blockly.Block, node: BdmBlock): ReadonlyMap<string, string> => {
      const ids = new Map<string, string>();
      if (node.id === placeholder.id) {
        // Not a copy (for example a deleted placeholder brought back by undo).
        return ids;
      }
      for (const id of treeBlockIds([node]).slice(1)) {
        ids.set(
          id,
          this.fresh(
            'blk',
            placeholder.id,
            id,
            (candidate) => this.workspace.getBlockById(candidate) === null,
          ),
        );
      }
      return ids;
    };
    const copiedPlaceholders = blocks.some((block) => {
      const kept = placeholderNode(block);
      return kept !== null && kept.id !== block.id;
    });
    if (syms.size > 0 || copiedPlaceholders) {
      withoutEvents(() => {
        for (const block of blocks) {
          renameInBlock(block, syms, nestedIds);
        }
      });
    }
    for (const block of blocks) {
      this.index.refresh(block);
    }
    if (syms.size > 0) {
      this.onRenamed(syms);
    }
    return syms;
  }

  /** A fresh ID for `old` in `owner`: the remembered one when it is still free, or a new one. */
  private fresh(
    kind: 'sym' | 'blk',
    owner: string,
    old: string,
    isFree: (id: string) => boolean,
  ): string {
    const key = `${kind}|${owner}|${old}`;
    const remembered = this.memo.get(key);
    if (remembered !== undefined && isFree(remembered)) {
      return remembered;
    }
    let id = newId(kind);
    // About 101 random bits: a clash is practically impossible, but checking costs nothing.
    while (!isFree(id) || !isProjectId(id)) {
      id = newId(kind);
    }
    if (this.memo.size >= MAX_REMAP_MEMO) {
      this.memo.clear();
    }
    this.memo.set(key, id);
    return id;
  }
}
