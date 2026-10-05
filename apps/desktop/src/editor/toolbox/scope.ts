/**
 * Where the toolbox looks and inserts (docs/spec/03-block-language.md §3.6, 04 §4.2):
 *
 * - the *listing point*, whose symbols the Variables category lists: the selected block, or, with
 *   nothing selected (or a function or `main` selected), the end of that body (`main`'s by default);
 * - the *insertion point* of *Make a variable*: before the selected statement, at the top of the
 *   selected function, or at the top of `main` (created when there is none);
 * - the names a new declaration must not take there.
 *
 * Symbols come from the latest analysis through a {@link SymbolSource} (the compiler core's scope
 * query, 06 §6.5), never from Blockly's variable model. The block structure comes from the
 * workspace, whose block IDs are the project's.
 */
import type { SymbolInfo } from '@blocks2cpp/b2c-core-wasm';
import { B2cSymbolDeclField, isPlaceholder } from '@blocks2cpp/blockly-ext';
import type * as Blockly from 'blockly/core';

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
   * sorted by name, then ID.
   */
  symbolsAt(blockId: string, input: string | null): readonly SymbolInfo[];
  /** Every symbol of the analysed program. */
  allSymbols(): readonly SymbolInfo[];
}

/** Where the symbols the Variables category lists are looked up. */
export type ListingPoint =
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

/** The first `main` on the canvas (a project has one; extra ones are analyser errors). */
export function findMain(workspace: Blockly.Workspace): Blockly.Block | null {
  return workspace.getTopBlocks(true).find((block) => block.type === MAIN_TYPE) ?? null;
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

/** The listing point at the end of a container's body. */
function endOfBody(container: Blockly.Block): ListingPoint {
  const last = lastReachedStatement(container, BODY_INPUT);
  return last === null
    ? { kind: 'at', blockId: container.id, input: BODY_INPUT }
    : { kind: 'after', blockId: last.id };
}

/**
 * The listing point for a selection: the selected block itself, or the end of the body of a
 * selected `main` or function, or (nothing selected) the end of `main`'s body. `null` when nothing
 * is selected and there is no `main`.
 */
export function listingPoint(
  workspace: Blockly.Workspace,
  selected: Blockly.Block | null,
): ListingPoint | null {
  const block = projectBlock(selected);
  if (block === null) {
    const main = findMain(workspace);
    return main === null ? null : endOfBody(main);
  }
  if (isContainer(block)) {
    return endOfBody(block);
  }
  return { kind: 'at', blockId: block.id, input: null };
}

/** Sorts symbols the way the scope query does: by name, then ID. */
function bySymbolOrder(a: SymbolInfo, b: SymbolInfo): number {
  if (a.name !== b.name) {
    return a.name < b.name ? -1 : 1;
  }
  return a.id < b.id ? -1 : a.id > b.id ? 1 : 0;
}

/** The symbols visible at a listing point, sorted by name, then ID. */
export function symbolsAtPoint(
  point: ListingPoint | null,
  source: SymbolSource,
): readonly SymbolInfo[] {
  if (point === null) {
    return [];
  }
  if (point.kind === 'at') {
    return source.symbolsAt(point.blockId, point.input);
  }
  const visible = source.symbolsAt(point.blockId, null);
  const known = new Set(visible.map((symbol) => symbol.id));
  // A variable is visible from its declaration on, so after the last statement its own variable
  // is visible too. (A loop's counter is visible only inside the loop.)
  const declared = source
    .allSymbols()
    .filter(
      (symbol) =>
        symbol.declBlock === point.blockId && symbol.kind === 'variable' && !known.has(symbol.id),
    );
  return declared.length === 0 ? visible : [...visible, ...declared].sort(bySymbolOrder);
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

/** The names the `var.declare` blocks of a statement list declare, from `first` on. */
function namesDeclaredFrom(first: Blockly.Block | null): Set<string> {
  const names = new Set<string>();
  let current = first;
  for (let steps = 0; current !== null && steps < MAX_LIST_LENGTH; steps += 1) {
    if (current.type === DECLARE_TYPE) {
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
        source.symbolsAt(point.block.id, null),
      );
    case 'top':
      return addNames(
        namesDeclaredFrom(point.container.getInputTargetBlock(BODY_INPUT)),
        source.symbolsAt(point.container.id, BODY_INPUT),
      );
    case 'newMain':
      return addNames(new Set(), functionSymbols(source));
  }
}

/**
 * The names a new variable from the toolbox should not take at a listing point: everything visible
 * there, and the variables declared in the same statement list.
 */
export function takenVariableNames(
  workspace: Blockly.Workspace,
  point: ListingPoint | null,
  source: SymbolSource,
): Set<string> {
  const names = addNames(new Set(), symbolsAtPoint(point, source));
  if (point === null) {
    return names;
  }
  const block = workspace.getBlockById(point.blockId);
  if (block === null) {
    return names;
  }
  const first =
    point.kind === 'at' && point.input !== null
      ? block.getInputTargetBlock(point.input)
      : firstOfList(enclosingStatement(block));
  for (const name of namesDeclaredFrom(first)) {
    names.add(name);
  }
  return names;
}

/** The names a new loop counter should not take at a listing point: everything visible there. */
export function takenLoopNames(point: ListingPoint | null, source: SymbolSource): Set<string> {
  return addNames(new Set(), symbolsAtPoint(point, source));
}

/** The names a new function should not take: every function, analysed or on the canvas. */
export function takenFunctionNames(
  workspace: Blockly.Workspace,
  source: SymbolSource,
): Set<string> {
  const names = addNames(new Set(), functionSymbols(source));
  for (const block of workspace.getTopBlocks(false)) {
    if (block.type === FUNCTION_TYPE) {
      const name = declaredName(block);
      if (name !== null) {
        names.add(name);
      }
    }
  }
  return names;
}
