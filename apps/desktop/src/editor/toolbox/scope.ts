/**
 * Where the block menus look up symbols, where a block dropped from the toolbox looks them up, and
 * where *Make a variable* inserts (docs/spec/03-block-language.md §3.6, 04 §4.2):
 *
 * - the *scope at a block*, which its own menus list ({@link resolveBlockScope},
 *   {@link symbolsAtBlock}), and the *scope at the connection* a block is attached by, which a
 *   block just dropped sees there before the next analysis has seen it
 *   ({@link resolveConnectionScope}, {@link symbolsAtConnection}); both are resolved so that a
 *   block the latest analysis has not seen yet sees what its place gives it;
 * - the *insertion point* of *Make a variable*: before the selected statement, at the top of the
 *   selected function, or at the top of `main` (created when there is none);
 * - the names a new declaration must not take where it is inserted or dropped.
 *
 * The Variables category does not depend on any of these: it lists every variable of the module
 * (./contents.ts).
 *
 * Symbols come from the latest analysis through a {@link SymbolSource} (the compiler core's scope
 * query, 06 §6.5), never from Blockly's variable model. The core answers for every block it has
 * analysed, also for a parked block (loose, in a loose stack, or nested in one: what is visible at
 * the end of `main`'s body) and for a block inside a disabled statement (what that statement
 * sees). Only a block the latest analysis has not seen (one dropped or pasted since) has no answer;
 * it sees what its place in the program, or on the canvas, gives it. The block structure comes
 * from the workspace, whose block IDs are the project's.
 */
import type { SymbolInfo } from '@blocks2cpp/b2c-core-wasm';
import { B2cSymbolDeclField, isPlaceholder } from '@blocks2cpp/blockly-ext';
import type * as Blockly from 'blockly/core';

import type { DefaultNameKind } from './names';

/** The block that starts the program. */
export const MAIN_TYPE = 'program.main';
/** The block that defines a function. */
export const FUNCTION_TYPE = 'func.define';
/** The block that declares a variable. */
export const DECLARE_TYPE = 'var.declare';
/** The statement input of `main` and of a function. */
export const BODY_INPUT = 'BODY';
/** The name field of a declaration block. */
export const NAME_FIELD = 'NAME';

/** The most statements walked along one list (a project holds at most 100,000 blocks, 05 §5.6). */
const MAX_LIST_LENGTH = 100_000;

/** Symbols from the latest analysis. Implementations never throw (they answer with nothing). */
export interface SymbolSource {
  /**
   * The symbols visible at a block (`input` null) or at the start of one of its statement inputs,
   * sorted by name, then ID; `null` when the latest analysis has no answer for the block (it has
   * not seen it, or it is nested too deeply). An empty list means that nothing is visible there.
   */
  symbolsAt(blockId: string, input: string | null): readonly SymbolInfo[] | null;
  /** Every symbol of the analysed program. */
  allSymbols(): readonly SymbolInfo[];
}

/** A position in the program whose visible symbols are looked up. */
export type ScopePoint =
  /** At a block, or at the start of one of its statement inputs. */
  | { readonly kind: 'at'; readonly blockId: string; readonly input: string | null }
  /** Just after a statement: what is visible at it, plus the variable it declares. */
  | { readonly kind: 'after'; readonly blockId: string };

/** Where *Make a variable* inserts its declaration. */
export type InsertionPoint =
  /** Just before a statement of the program. */
  | { readonly kind: 'before'; readonly block: Blockly.Block }
  /** As the first statement of `main`'s or a function's body. */
  | { readonly kind: 'top'; readonly container: Blockly.Block }
  /** At the top of a new `main` (the module has none). */
  | { readonly kind: 'newMain' };

/** Whether a block is `main` or a function definition: a top-level block with a body. */
export function isContainer(block: Blockly.Block): boolean {
  return block.type === MAIN_TYPE || block.type === FUNCTION_TYPE;
}

/**
 * The `main` of the canvas, as the analyser chooses it (06 §6.5): the enabled `main` with the
 * smallest block ID, because the analyser lowers the top-level blocks in block ID order and skips
 * the disabled ones (a second enabled `main` is an error, `B2C-E0406`). Its body's end is the
 * parked scope. Without an enabled `main`, the topmost one; `null` when the canvas has none.
 */
export function findMain(workspace: Blockly.Workspace): Blockly.Block | null {
  const mains = workspace.getTopBlocks(true).filter((block) => block.type === MAIN_TYPE);
  let chosen: Blockly.Block | null = null;
  for (const main of mains) {
    if (!isSkipped(main) && (chosen === null || main.id < chosen.id)) {
      chosen = main;
    }
  }
  return chosen ?? mains[0] ?? null;
}

/**
 * The selected block as the scope query knows it: the expression shadows are not part of the
 * project, so their parent stands in for them.
 */
export function projectBlock(block: Blockly.Block | null): Blockly.Block | null {
  let current = block;
  while (current?.isShadow() === true) {
    current = current.getParent();
  }
  return current;
}

/** Whether a block is disabled itself or inside a disabled block (the analyser skips both). */
function isSkipped(block: Blockly.Block): boolean {
  return !block.isEnabled() || block.getInheritedDisabled();
}

/** The last statement of a body that the analyser reaches, or `null` when it has none. */
function lastReachedStatement(container: Blockly.Block, input: string): Blockly.Block | null {
  let last: Blockly.Block | null = null;
  let current = container.getInputTargetBlock(input);
  for (let steps = 0; current !== null && steps < MAX_LIST_LENGTH; steps += 1) {
    if (!isSkipped(current) && !isPlaceholder(current)) {
      last = current;
    }
    current = current.getNextBlock();
  }
  return last;
}

/** The position at the end of a container's body. */
function endOfBody(container: Blockly.Block): ScopePoint {
  const last = lastReachedStatement(container, BODY_INPUT);
  return last === null
    ? { kind: 'at', blockId: container.id, input: BODY_INPUT }
    : { kind: 'after', blockId: last.id };
}

/** Sorts symbols the way the scope query does: by name, then ID. */
export function bySymbolOrder(a: SymbolInfo, b: SymbolInfo): number {
  if (a.name !== b.name) {
    return a.name < b.name ? -1 : 1;
  }
  return a.id < b.id ? -1 : a.id > b.id ? 1 : 0;
}

/**
 * The latest analysis's answer at a position, sorted by name, then ID, or `null` when it has none
 * (it has not seen the block). Just after a statement, that is what is visible at it plus the
 * variable it declares, which hides any other symbol with its name (06 §6.5,
 * `Analysis::scope_after`): a variable is visible from its declaration on, a loop's counter only
 * inside the loop, and a disabled or parked declaration declares nothing.
 */
function answerAt(point: ScopePoint, source: SymbolSource): readonly SymbolInfo[] | null {
  if (point.kind === 'at') {
    return source.symbolsAt(point.blockId, point.input);
  }
  const visible = source.symbolsAt(point.blockId, null);
  if (visible === null) {
    return null;
  }
  const known = new Set(visible.map((symbol) => symbol.id));
  const declared = source
    .allSymbols()
    .filter(
      (symbol) =>
        symbol.declBlock === point.blockId && symbol.kind === 'variable' && !known.has(symbol.id),
    );
  if (declared.length === 0) {
    return visible;
  }
  const hidden = new Set(declared.map((symbol) => symbol.name));
  return [...visible.filter((symbol) => !hidden.has(symbol.name)), ...declared].sort(bySymbolOrder);
}

/** The statement a block belongs to: the block itself, or the statement holding its value input. */
function enclosingStatement(block: Blockly.Block): Blockly.Block {
  let current = block;
  for (let steps = 0; steps < MAX_LIST_LENGTH; steps += 1) {
    const parent = current.getParent();
    if (current.outputConnection === null || parent === null) {
      return current;
    }
    current = parent;
  }
  return current;
}

/** Whether a block is part of the program: inside `main` or a function. */
function isInProgram(block: Blockly.Block): boolean {
  return isContainer(block.getRootBlock());
}

/**
 * Whether the analysis knows a symbol declared on this canvas. Every symbol a block of the canvas
 * can see is declared on it (the module's functions, parameters and variables), so without one
 * every answer is empty, and a walk to other positions would only ask again.
 */
function canvasDeclaresSymbols(workspace: Blockly.Workspace, source: SymbolSource): boolean {
  return source.allSymbols().some((symbol) => workspace.getBlockById(symbol.declBlock) !== null);
}

/** One step of the walk to a position around or before a block (see {@link resolve}). */
interface ScopeStep {
  /** Where to ask next. */
  readonly point: ScopePoint;
  /** The block the walk goes on from. */
  readonly block: Blockly.Block;
}

/**
 * The position a block's place gives it, without asking about the block itself, or `null` at the
 * top of its stack:
 *
 * - for a value block, its input in the block that holds it;
 * - for a statement, the position just after the statement before it in the same list;
 * - for the first statement of a list, the start of that list.
 */
function scopeAround(block: Blockly.Block): ScopeStep | null {
  const parent = block.getParent();
  if (parent === null) {
    return null;
  }
  if (block.outputConnection === null) {
    const previous = block.getPreviousBlock();
    if (previous?.getNextBlock() === block) {
      return { point: { kind: 'after', blockId: previous.id }, block: previous };
    }
  }
  const input = parent.getInputWithBlock(block)?.name ?? null;
  return { point: { kind: 'at', blockId: parent.id, input }, block: parent };
}

/** A resolved position and the symbols visible there, as the latest analysis answers. */
interface Resolved {
  readonly point: ScopePoint;
  readonly symbols: readonly SymbolInfo[];
}

/**
 * `point`, resolved for the latest analysis: the point itself when the analysis has an answer
 * there (an empty one included), and otherwise, for a block it has not seen yet (one dropped or
 * pasted since), the nearest position around or before it that it has seen ({@link scopeAround}):
 * what the statement before it, or the list or input around it, sees. At the top of a stack that
 * the analysis has not seen, a loose stack sees the parked scope (the end of `main`'s body, as the
 * core answers for a parked block), and a new `main` or function sees nothing it knows.
 *
 * The walk takes at most {@link MAX_LIST_LENGTH} steps, and none when the analysis knows no symbol
 * of this canvas. Without any answer, `point` stands, and nothing is visible there.
 */
function resolve(workspace: Blockly.Workspace, point: ScopePoint, source: SymbolSource): Resolved {
  const answer = answerAt(point, source);
  if (answer !== null) {
    return { point, symbols: answer };
  }
  const nothing: Resolved = { point, symbols: [] };
  if (!canvasDeclaresSymbols(workspace, source)) {
    return nothing;
  }
  let block = workspace.getBlockById(point.blockId);
  for (let steps = 0; block !== null && steps < MAX_LIST_LENGTH; steps += 1) {
    const step: ScopeStep | null = scopeAround(block);
    if (step === null) {
      return isContainer(block) ? nothing : (programEnd(workspace, source) ?? nothing);
    }
    const around = answerAt(step.point, source);
    if (around !== null) {
      return { point: step.point, symbols: around };
    }
    block = step.block;
  }
  return nothing;
}

/** The first function definition of the canvas that the analyser reads, or `null`. */
function firstDefinition(workspace: Blockly.Workspace): Blockly.Block | null {
  return (
    workspace
      .getTopBlocks(true)
      .find(
        (block) => block.type === FUNCTION_TYPE && !isSkipped(block) && !isPlaceholder(block),
      ) ?? null
  );
}

/**
 * The end of `main`'s body ({@link findMain}), resolved like any other point: the parked scope that
 * a block on the canvas sees (06 §6.5). In a module without `main`, the module's functions only (as
 * the scope query answers at a function definition, and from the program's symbols when it has not
 * seen that definition yet). `null` when the module has neither.
 */
function programEnd(workspace: Blockly.Workspace, source: SymbolSource): Resolved | null {
  const main = findMain(workspace);
  if (main !== null) {
    return resolve(workspace, endOfBody(main), source);
  }
  const definition = firstDefinition(workspace);
  if (definition === null) {
    return null;
  }
  const point: ScopePoint = { kind: 'at', blockId: definition.id, input: null };
  return { point, symbols: answerAt(point, source) ?? canvasFunctions(workspace, source) };
}

/** The functions defined on this canvas that the latest analysis knows, sorted like the query. */
function canvasFunctions(workspace: Blockly.Workspace, source: SymbolSource): SymbolInfo[] {
  return functionSymbols(source)
    .filter((symbol) => workspace.getBlockById(symbol.declBlock) !== null)
    .sort(bySymbolOrder);
}

/**
 * The position whose answer a block's own menus list (03 §3.6): what the block sees at itself, or
 * (`input`) at the start of one of its statement lists or inside one of its value inputs. The core
 * answers there for every block it has analysed (parked blocks and blocks inside disabled
 * statements included); a block it has not seen yet sees what its place gives it: what the
 * statement before it, or the list or input around it, sees, or, at the top of a stack on the
 * canvas, the parked scope.
 */
export function resolveBlockScope(
  workspace: Blockly.Workspace,
  block: Blockly.Block,
  input: string | null,
  source: SymbolSource,
): ScopePoint | null {
  return resolve(workspace, { kind: 'at', blockId: block.id, input }, source).point;
}

/**
 * The symbols a block's own menus offer: what is visible at its {@link resolveBlockScope}
 * position, asking the scope query once when the latest analysis has seen the block.
 */
export function symbolsAtBlock(
  workspace: Blockly.Workspace,
  block: Blockly.Block,
  input: string | null,
  source: SymbolSource,
): readonly SymbolInfo[] {
  return resolve(workspace, { kind: 'at', blockId: block.id, input }, source).symbols;
}

/**
 * Where a block sees what it sees through the connection it is attached by, resolved for the
 * latest analysis, **without asking about the block itself** (which that analysis may know at
 * another place, or not at all): just after the statement before it, at the start of the
 * statement list or inside the value input that holds it, or, for the top block of a stack on the
 * canvas, the parked scope (the end of `main`'s body). `null` for a `main` or a function, which
 * are attached to nothing.
 */
export function resolveConnectionScope(
  workspace: Blockly.Workspace,
  block: Blockly.Block,
  source: SymbolSource,
): ScopePoint | null {
  return connectionScope(workspace, block, source)?.point ?? null;
}

/**
 * What a block sees at its {@link resolveConnectionScope} position: what is visible at a block
 * just dropped or pasted into place, before the next analysis has seen it there (a dropped *set*
 * or *change* from the toolbox is re-targeted with it, and a dropped declaration named, 04 §4.2).
 * Nothing for a `main` or a function.
 */
export function symbolsAtConnection(
  workspace: Blockly.Workspace,
  block: Blockly.Block,
  source: SymbolSource,
): readonly SymbolInfo[] {
  return connectionScope(workspace, block, source)?.symbols ?? [];
}

/** {@link resolveConnectionScope} with its answer. */
function connectionScope(
  workspace: Blockly.Workspace,
  block: Blockly.Block,
  source: SymbolSource,
): Resolved | null {
  const around = scopeAround(block);
  if (around === null) {
    return isContainer(block) ? null : programEnd(workspace, source);
  }
  return resolve(workspace, around.point, source);
}

/**
 * Where *Make a variable* inserts: before the selected statement (the statement holding a selected
 * value block), at the top of a selected `main` or function, and otherwise at the top of `main`,
 * or of a new `main` when the canvas has none.
 */
export function insertionPoint(
  workspace: Blockly.Workspace,
  selected: Blockly.Block | null,
): InsertionPoint {
  const block = projectBlock(selected);
  if (block !== null) {
    const statement = enclosingStatement(block);
    if (isContainer(statement)) {
      return { kind: 'top', container: statement };
    }
    if (statement.previousConnection !== null && isInProgram(statement)) {
      return { kind: 'before', block: statement };
    }
  }
  const main = findMain(workspace);
  return main === null ? { kind: 'newMain' } : { kind: 'top', container: main };
}

/** The first statement of the list a statement is in. */
function firstOfList(statement: Blockly.Block): Blockly.Block {
  let current = statement;
  for (let steps = 0; steps < MAX_LIST_LENGTH; steps += 1) {
    const previous = current.getPreviousBlock();
    if (previous?.getNextBlock() !== current) {
      return current;
    }
    current = previous;
  }
  return current;
}

/** The name a declaration block declares, or `null`. */
export function declaredName(block: Blockly.Block): string | null {
  const field = block.getField(NAME_FIELD);
  if (field instanceof B2cSymbolDeclField) {
    return field.getDecl()?.name ?? null;
  }
  return null;
}

/**
 * The names the `var.declare` blocks of a statement list declare, from `first` on, leaving out
 * `except` (a declaration whose own name is being chosen).
 */
function namesDeclaredFrom(
  first: Blockly.Block | null,
  except: Blockly.Block | null = null,
): Set<string> {
  const names = new Set<string>();
  let current = first;
  for (let steps = 0; current !== null && steps < MAX_LIST_LENGTH; steps += 1) {
    if (current.type === DECLARE_TYPE && current !== except) {
      const name = declaredName(current);
      if (name !== null) {
        names.add(name);
      }
    }
    current = current.getNextBlock();
  }
  return names;
}

/** Adds every symbol's name to a set. */
function addNames(names: Set<string>, symbols: readonly SymbolInfo[]): Set<string> {
  for (const symbol of symbols) {
    names.add(symbol.name);
  }
  return names;
}

/** The functions of the program (visible everywhere). */
function functionSymbols(source: SymbolSource): SymbolInfo[] {
  return source.allSymbols().filter((symbol) => symbol.kind === 'function');
}

/**
 * The names a new variable must not take at an insertion point: everything visible there, and the
 * variables declared in the same statement list (a second declaration in one scope is an error).
 */
export function takenNamesAtInsertion(point: InsertionPoint, source: SymbolSource): Set<string> {
  switch (point.kind) {
    case 'before':
      return addNames(
        namesDeclaredFrom(firstOfList(point.block)),
        source.symbolsAt(point.block.id, null) ?? [],
      );
    case 'top':
      return addNames(
        namesDeclaredFrom(point.container.getInputTargetBlock(BODY_INPUT)),
        source.symbolsAt(point.container.id, BODY_INPUT) ?? [],
      );
    case 'newMain':
      return addNames(new Set(), functionSymbols(source));
  }
}

/**
 * The names a declaration of `kind` on `block`, just dropped where it is, should not take there
 * (its own name aside):
 *
 * - a variable: what is visible at the block's connection ({@link symbolsAtConnection}) and the
 *   variables the other `create` blocks of its statement list declare (a second declaration in
 *   one scope is an error);
 * - a loop counter: what is visible at the block's connection;
 * - a function: every other function, analysed or on the canvas (function names are unique,
 *   `B2C-E0211`).
 */
export function takenNamesAtDrop(
  workspace: Blockly.Workspace,
  block: Blockly.Block,
  kind: DefaultNameKind,
  source: SymbolSource,
): Set<string> {
  if (kind === 'function') {
    const names = addNames(
      new Set(),
      functionSymbols(source).filter((symbol) => symbol.declBlock !== block.id),
    );
    for (const top of workspace.getTopBlocks(false)) {
      const name = top !== block && top.type === FUNCTION_TYPE ? declaredName(top) : null;
      if (name !== null) {
        names.add(name);
      }
    }
    return names;
  }
  const names = addNames(new Set(), symbolsAtConnection(workspace, block, source));
  if (kind === 'variable') {
    for (const name of namesDeclaredFrom(firstOfList(block), block)) {
      names.add(name);
    }
  }
  return names;
}
