/**
 * The editor services blockly-ext's fields, mutators and connection checker use (blockly-ext
 * `setEditorServices`): the symbols in scope and their names, and static types, all answered from
 * the latest analysis (docs/spec/06-compiler-pipeline.md §6.5–6.6).
 *
 * The symbols in scope at a block go through the editor's scope resolver
 * (`resolveBlockScope`, ../toolbox/scope.ts), the same one the toolbox lists with. The compiler
 * core answers for every block it has analysed, a parked block (the end of `main`'s body) and a
 * block inside a disabled statement (what that statement sees) included; only a block the latest
 * analysis has not seen yet (one dropped or pasted since) lists what its place gives it instead of
 * nothing (03 §3.6).
 */
import type { CoreWasm, PreviewResult, SymbolInfo } from '@blocks2cpp/b2c-core-wasm';
import {
  B2cSymbolRefField,
  EXPR_SHADOW_TYPE,
  EXPR_VALUE_FIELD,
  NO_FACTS,
  parseType,
  type StaticType,
  type SymbolProvider,
  type TypeFactsTs,
  type TypeOracle,
  type UserTypeTs,
} from '@blocks2cpp/blockly-ext';
import type * as Blockly from 'blockly/core';

import { symbolsAtBlock, type SymbolSource } from '../toolbox/scope';
import type { SymbolNames } from './names';

/** The symbols of a preview by ID, built once per preview. */
const SYMBOLS_BY_ID = new WeakMap<PreviewResult, ReadonlyMap<string, SymbolInfo>>();

function symbolsById(preview: PreviewResult): ReadonlyMap<string, SymbolInfo> {
  let map = SYMBOLS_BY_ID.get(preview);
  if (map === undefined) {
    map = new Map(preview.symbols.map((symbol) => [symbol.id, symbol]));
    SYMBOLS_BY_ID.set(preview, map);
  }
  return map;
}

/** What the symbol services read. */
export interface SymbolServiceDeps {
  /** The running compiler core (its kept analysis answers scope queries), or `null`. */
  readonly core: () => CoreWasm | null;
  /** The latest preview, or `null`. */
  readonly preview: () => PreviewResult | null;
  /** Current and last-known names. */
  readonly names: SymbolNames;
  /** The canvas whose blocks the menus belong to. */
  readonly workspace: Blockly.Workspace;
}

/**
 * The latest analysis as the compiler core gives it: its scope query (`symbolsInScope`, from the
 * analysis the core kept from the last preview, `null` for a block that analysis has not seen) and
 * the preview's symbols, with no fallback of the editor's. Before the project's first preview there
 * are none, and every block sees nothing. Never throws: a trapped core answers again once the
 * preview pipeline has replaced it, and nothing until then.
 */
export function analysisScope(deps: Pick<SymbolServiceDeps, 'core' | 'preview'>): SymbolSource {
  return {
    symbolsAt(blockId, input) {
      const core = deps.core();
      if (core === null || deps.preview() === null) {
        return [];
      }
      try {
        return core.symbolsInScope(blockId, input);
      } catch {
        return [];
      }
    },
    allSymbols() {
      return deps.preview()?.symbols ?? [];
    },
  };
}

/**
 * The symbols in scope at a block, resolved by the editor's scope resolver over the latest
 * analysis (a block of the canvas the analysis has not seen yet gets what its place gives it; any
 * other block, such as one in the toolbox's flyout, gets the scope query's own answer, or nothing),
 * the current name of a symbol (from the document, then the last-known names, then the analysis),
 * the parameters of the function a call names ({@link calledFunctionParams}), the fields of the
 * struct a *new … with* names ({@link namedStructFields}), and the module's structs and enums for
 * the type fields' menus.
 */
export function createSymbolProvider(deps: SymbolServiceDeps): SymbolProvider {
  const scope = analysisScope(deps);
  return {
    symbolsAt(blockId, input) {
      const block = deps.workspace.getBlockById(blockId);
      if (block === null) {
        return scope.symbolsAt(blockId, input) ?? [];
      }
      return symbolsAtBlock(deps.workspace, block, input, scope);
    },
    nameOf(symId) {
      const name = deps.names.nameOf(symId);
      if (name !== null) {
        return name;
      }
      const preview = deps.preview();
      return preview === null ? null : (symbolsById(preview).get(symId)?.name ?? null);
    },
    functionParams: (blockId, funcId) => calledFunctionParams(deps, blockId, funcId),
    structFields: (blockId, structId) => namedStructFields(deps, blockId, structId),
    userTypes: () => userTypesOf(deps),
  };
}

/**
 * The fields (symbol IDs, in row order) of struct `structId` for a *new … with* block `blockId` of
 * the canvas, from the latest preview's symbols by ID, as {@link calledFunctionParams} answers for
 * a call (docs/plans/m3-contracts/w2.md §2.1): `null` when there is no preview, the block is not
 * on the canvas, or `structId` is not a struct of the analysis. A struct is visible everywhere in
 * its module, so the answer does not depend on where the block is.
 */
function namedStructFields(
  deps: Pick<SymbolServiceDeps, 'preview' | 'workspace'>,
  blockId: string,
  structId: string,
): readonly string[] | null {
  const preview = deps.preview();
  if (preview === null || deps.workspace.getBlockById(blockId) === null) {
    return null;
  }
  const struct = symbolsById(preview).get(structId);
  return struct?.kind === 'struct' ? struct.fields : null;
}

/** The structs and enums of the latest preview, which the type fields list (w2.md §2.3). */
function userTypesOf(deps: Pick<SymbolServiceDeps, 'preview'>): readonly SymbolInfo[] {
  return (deps.preview()?.symbols ?? []).filter(
    (symbol) => symbol.kind === 'struct' || symbol.kind === 'enum',
  );
}

/**
 * The parameters (symbol IDs, in order) of function `funcId` for a call block `blockId` of the
 * canvas, from the latest preview's symbols by ID (a table built once per preview, so labelling
 * every call after an analysis costs no scope query): `null` when there is no preview, the call
 * is not on the canvas (one in the toolbox's flyout is labelled by the toolbox), or `funcId` is
 * not a function of the module shown (a call to another module's function is `B2C-E0206`).
 *
 * A call names its function by symbol ID, and every function of a module is visible everywhere in
 * it, so the answer does not depend on where the call is. The one exception the scope query knows
 * is a local variable that hides the function's name: the call still shows the function's
 * parameters, and the analyser reports it (`B2C-E0205`, "rename the variable").
 */
function calledFunctionParams(
  deps: Pick<SymbolServiceDeps, 'preview' | 'workspace'>,
  blockId: string,
  funcId: string,
): readonly string[] | null {
  const preview = deps.preview();
  if (preview === null || deps.workspace.getBlockById(blockId) === null) {
    return null;
  }
  const fn = symbolsById(preview).get(funcId);
  if (fn?.kind !== 'function' || deps.workspace.getBlockById(fn.declBlock) === null) {
    return null;
  }
  return fn.params;
}

/** The symbol a reference reporter names: `var.get`'s VAR, a call's FUNC, a reference shadow. */
function referencedSymbol(block: Blockly.Block): string | null {
  const name =
    block.type === 'var.get'
      ? 'VAR'
      : block.type === 'func.call'
        ? 'FUNC'
        : block.type === EXPR_SHADOW_TYPE.ref
          ? EXPR_VALUE_FIELD
          : null;
  const field = name === null ? null : block.getField(name);
  return field instanceof B2cSymbolRefField ? (field.getRef()?.ref ?? null) : null;
}

/** The facts about user types a preview gives, built once per preview. */
const FACTS = new WeakMap<PreviewResult, TypeFactsTs>();

/**
 * What the type rules need to know about the user types of a preview (`b2c_ir::TypeFacts`): each
 * struct and enum symbol, with the effective *comparable* and *printable* flags the analysis
 * worked out. One object per preview, so a gesture's cache sees a new preview by its facts.
 */
function factsOf(preview: PreviewResult): TypeFactsTs {
  let facts = FACTS.get(preview);
  if (facts === undefined) {
    const users = new Map<string, UserTypeTs>();
    for (const symbol of preview.symbols) {
      if (symbol.kind === 'struct') {
        users.set(symbol.id, {
          name: symbol.name,
          kind: 'struct',
          comparable: symbol.comparable,
          printable: symbol.printable,
        });
      } else if (symbol.kind === 'enum') {
        users.set(symbol.id, {
          name: symbol.name,
          kind: 'enum',
          comparable: true,
          printable: true,
        });
      }
    }
    facts = { userType: (id) => users.get(id) ?? null };
    FACTS.set(preview, facts);
  }
  return facts;
}

/**
 * The blocks that declare names into their own statement lists or bodies, or into the list they
 * are in after themselves, and when they do (`null`: always). W2's are listed before the catalog
 * has them (docs/plans/m3-contracts/w2.md §2.13): *for each* (its item, a map's key and value),
 * the file loop (its line), *open file* (its file, after it), *try* (each catch part's error), and
 * *when program starts* while it has its arguments (`args`, W2's package C2).
 */
const DECLARING: ReadonlyMap<string, ((block: Blockly.Block) => boolean) | null> = new Map([
  ['var.declare', null],
  ['control.for_range', null],
  ['func.define', null],
  ['coll.for_each', null],
  ['coll.for_each_pair', null],
  ['file.for_each_line', null],
  ['file.open', null],
  ['error.try', null],
  ['program.main', (block: Blockly.Block) => isTicked(block.getField('ARGS')?.getValue())],
]);

/** Whether a checkbox field's value is ticked. */
function isTicked(value: unknown): boolean {
  return value === true || value === 'TRUE';
}

/** Whether a block declares names, so that it may hide one (see {@link DECLARING}). */
export function isDeclaringBlock(block: Blockly.Block): boolean {
  const when = DECLARING.get(block.type);
  return when !== undefined && (when === null || when(block));
}

/** The most statements and levels {@link declaredAbove} walks before it asks the core instead. */
const MAX_WALK = 10_000;

/** The names each block of a preview declares (its symbols by declaring block), built once per preview. */
const NAMES_BY_BLOCK = new WeakMap<PreviewResult, ReadonlyMap<string, ReadonlySet<string>>>();

function namesByBlock(preview: PreviewResult): ReadonlyMap<string, ReadonlySet<string>> {
  let map = NAMES_BY_BLOCK.get(preview);
  if (map === undefined) {
    const names = new Map<string, Set<string>>();
    for (const symbol of preview.symbols) {
      let set = names.get(symbol.declBlock);
      if (set === undefined) {
        set = new Set();
        names.set(symbol.declBlock, set);
      }
      set.add(symbol.name);
    }
    map = names;
    NAMES_BY_BLOCK.set(preview, map);
  }
  return map;
}

/**
 * Whether the canvas makes the variable `symbol` visible at `block` for certain, without asking the
 * core (slots.md §4.3, *Speed*: the connection checker asks this for every place it types): its
 * `create` block is enabled, under *when program starts* or a function, and comes before `block`
 * in a statement list that holds `block` (or one of the statements `block` is in), and nothing on
 * the way back from `block` to it may declare the same name (a `create` before, a loop or a
 * function `block` is in, or `block` itself), which could hide it. `false` means "not known this
 * way": the scope query answers then. A value inside the `create`'s own starting value is never
 * found, since the name is hidden there.
 */
function declaredAbove(
  workspace: Blockly.Workspace,
  block: Blockly.Block,
  symbol: SymbolInfo,
  preview: PreviewResult,
): boolean {
  if (symbol.kind !== 'variable') {
    return false;
  }
  const declaration = workspace.getBlockById(symbol.declBlock);
  if (
    declaration?.type !== 'var.declare' ||
    !declaration.isEnabled() ||
    declaration.getInheritedDisabled()
  ) {
    return false;
  }
  const root = declaration.getRootBlock().type;
  if (root !== 'program.main' && root !== 'func.define') {
    return false;
  }
  const names = namesByBlock(preview);
  // A declaring block the preview has not seen may declare anything.
  const mayHide = (other: Blockly.Block): boolean =>
    isDeclaringBlock(other) && (names.get(other.id)?.has(symbol.name) ?? true);
  if (mayHide(block)) {
    // A `create` hides its own name while it is being created, a loop or a function its own.
    return false;
  }
  let current = block;
  for (let steps = 0; steps < MAX_WALK; steps += 1) {
    const previous = current.getPreviousBlock();
    if (previous !== null && previous.getNextBlock() === current) {
      // The statement before `current` in its list.
      if (previous === declaration) {
        return true;
      }
      if (mayHide(previous)) {
        return false;
      }
      current = previous;
      continue;
    }
    // The first statement of a body, or a value in an input: on to the block that holds it.
    const parent = current.getParent();
    if (parent === null || mayHide(parent)) {
      return false;
    }
    current = parent;
  }
  return false;
}

/**
 * `source` with each scope query's answer kept until the preview or the core changes: the core
 * answers from the analysis of the last update, so a query's answer is the same until the next one.
 * The connection checker asks whether a symbol is visible at a block for each candidate place it
 * types (slots.md §4.3, *Speed*), and each query serialises every visible symbol.
 */
function rememberedScope(
  source: SymbolSource,
  deps: Pick<SymbolServiceDeps, 'core' | 'preview'>,
): SymbolSource {
  let core: CoreWasm | null = null;
  let preview: PreviewResult | null = null;
  const answers = new Map<string, readonly SymbolInfo[] | null>();
  return {
    symbolsAt(blockId, input) {
      const nowCore = deps.core();
      const nowPreview = deps.preview();
      if (nowCore !== core || nowPreview !== preview) {
        core = nowCore;
        preview = nowPreview;
        answers.clear();
      }
      const key = `${blockId}\u0000${input ?? ''}`;
      if (!answers.has(key)) {
        answers.set(key, source.symbolsAt(blockId, input));
      }
      return answers.get(key) ?? null;
    },
    allSymbols: () => source.allSymbols(),
  };
}

/** What the type oracle reads: the latest preview, and for scope questions the core and canvas. */
export type TypeOracleDeps = Pick<SymbolServiceDeps, 'preview'> &
  Partial<Pick<SymbolServiceDeps, 'core' | 'workspace'>>;

/**
 * Static types for the connection checker and typed slots (ADR-0018, slots.md §4.3): the type the
 * latest analysis gave the block, or, for a reference the analysis has not seen yet (a block just
 * dragged out), the type of the symbol it names, read from the wire's term (`parseType`,
 * docs/plans/m3-contracts/types.md §3.1); an expression slot's type (`slotTypes`); a symbol;
 * whether a symbol is visible at a block of the canvas (the editor's scope resolver over the
 * scope query, as the menus list them); and the facts about user types. `null` ("not known",
 * also for anything that does not read as a type) always connects.
 */
export function createTypeOracle(deps: TypeOracleDeps): TypeOracle {
  const scope = rememberedScope(
    analysisScope({ core: deps.core ?? (() => null), preview: deps.preview }),
    {
      core: deps.core ?? (() => null),
      preview: deps.preview,
    },
  );
  return {
    outputTypeOf(block) {
      const preview = deps.preview();
      if (preview === null) {
        return null;
      }
      const types = preview.blockTypes;
      if (Object.hasOwn(types, block.id)) {
        const type = parseType(types[block.id]);
        if (type !== null) {
          return type;
        }
      }
      const sym = referencedSymbol(block);
      const symbol = sym === null ? undefined : symbolsById(preview).get(sym);
      if (symbol === undefined) {
        return null;
      }
      return parseType(symbol.kind === 'function' ? symbol.returns : symbol.type);
    },
    slotTypeOf(blockId, input): StaticType | null {
      const slots = deps.preview()?.slotTypes;
      if (slots === undefined || !Object.hasOwn(slots, blockId)) {
        return null;
      }
      const inputs = slots[blockId];
      return inputs !== undefined && Object.hasOwn(inputs, input) ? parseType(inputs[input]) : null;
    },
    symbol(symId) {
      const preview = deps.preview();
      return preview === null ? null : (symbolsById(preview).get(symId) ?? null);
    },
    visibleAt(symId, block, input) {
      const workspace = deps.workspace;
      const preview = deps.preview();
      if (workspace === undefined || block.workspace !== workspace || preview === null) {
        // A block in the toolbox's flyout is in no place of the program.
        return null;
      }
      const symbol = symbolsById(preview).get(symId);
      if (symbol !== undefined && declaredAbove(workspace, block, symbol, preview)) {
        return true;
      }
      return symbolsAtBlock(workspace, block, input, scope).some((symbol) => symbol.id === symId);
    },
    facts() {
      const preview = deps.preview();
      return preview === null ? NO_FACTS : factsOf(preview);
    },
  };
}
