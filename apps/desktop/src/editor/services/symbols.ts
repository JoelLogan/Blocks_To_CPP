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
import type { CoreWasm, PreviewResult, StaticType, SymbolInfo } from '@blocks2cpp/b2c-core-wasm';
import {
  B2cSymbolRefField,
  EXPR_SHADOW_TYPE,
  EXPR_VALUE_FIELD,
  type SymbolProvider,
  type TypeOracle,
} from '@blocks2cpp/blockly-ext';
import type * as Blockly from 'blockly/core';

import { symbolsAtBlock, type SymbolSource } from '../toolbox/scope';
import type { SymbolNames } from './names';

const STATIC_TYPES: readonly string[] = [
  'void',
  'bool',
  'char',
  'int',
  'double',
  'string',
  'error',
];

/** Whether a value from the analysis is a static type. */
function isStaticType(value: unknown): value is StaticType {
  return typeof value === 'string' && STATIC_TYPES.includes(value);
}

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
 * and the parameters of the function a call names ({@link calledFunctionParams}).
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
  };
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

/**
 * Static types for the connection checker: the type the latest analysis gave the block, or, for a
 * reference the analysis has not seen yet (a block just dragged out), the type of the symbol it
 * names. `null` ("not known") always connects.
 */
export function createTypeOracle(deps: Pick<SymbolServiceDeps, 'preview'>): TypeOracle {
  return {
    outputTypeOf(block) {
      const preview = deps.preview();
      if (preview === null) {
        return null;
      }
      const types = preview.blockTypes;
      if (Object.hasOwn(types, block.id)) {
        const type: unknown = types[block.id];
        if (isStaticType(type)) {
          return type;
        }
      }
      const sym = referencedSymbol(block);
      const symbol = sym === null ? undefined : symbolsById(preview).get(sym);
      if (symbol === undefined) {
        return null;
      }
      const type = symbol.kind === 'function' ? symbol.returns : symbol.type;
      return isStaticType(type) ? type : null;
    },
  };
}
