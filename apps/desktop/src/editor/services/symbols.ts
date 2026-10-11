/**
 * The editor services blockly-ext's fields, mutators and connection checker use (blockly-ext
 * `setEditorServices`): the symbols in scope and their names, and static types, all answered from
 * the latest analysis (docs/spec/06-compiler-pipeline.md §6.5–6.6).
 *
 * The symbols in scope at a block go through the editor's scope resolver
 * (`resolveBlockScope`, ../toolbox/scope.ts), the same one the toolbox lists with: a block outside
 * the program, or one the analysis has no answer for yet, lists what it would see in the program
 * instead of nothing (03 §3.6).
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

import { resolveBlockScope, symbolsAtPoint, type SymbolSource } from '../toolbox/scope';
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
 * analysis the core kept from the last preview) and the preview's symbols, with no fallback of the
 * editor's. Before the project's first preview there are none. Never throws: a trapped core
 * answers again once the preview pipeline has replaced it, and nothing until then.
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
 * analysis (a block of the canvas outside the program, or one the analysis has no answer for, gets
 * what it would see in the program; any other block, such as one in the toolbox's flyout, gets the
 * scope query's own answer), and the current name of a symbol (from the document, then the
 * last-known names, then the analysis).
 */
export function createSymbolProvider(deps: SymbolServiceDeps): SymbolProvider {
  const scope = analysisScope(deps);
  return {
    symbolsAt(blockId, input) {
      const block = deps.workspace.getBlockById(blockId);
      if (block === null) {
        return scope.symbolsAt(blockId, input);
      }
      return symbolsAtPoint(resolveBlockScope(deps.workspace, block, input, scope), scope);
    },
    nameOf(symId) {
      const name = deps.names.nameOf(symId);
      if (name !== null) {
        return name;
      }
      const preview = deps.preview();
      return preview === null ? null : (symbolsById(preview).get(symId)?.name ?? null);
    },
  };
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
      return symbol.kind === 'function' ? symbol.returns : symbol.type;
    },
  };
}
