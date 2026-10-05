/**
 * The editor services blockly-ext's fields, mutators and connection checker use (blockly-ext
 * `setEditorServices`): the symbols in scope and their names, and static types, all answered from
 * the latest analysis (docs/spec/06-compiler-pipeline.md §6.5–6.6).
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
}

/**
 * The symbols in scope at a block (the compiler core's `symbolsInScope`, from the last preview's
 * analysis) and the current name of a symbol (from the document, then the last-known names, then
 * the analysis).
 */
export function createSymbolProvider(deps: SymbolServiceDeps): SymbolProvider {
  return {
    symbolsAt(blockId, input) {
      const core = deps.core();
      return core === null ? [] : core.symbolsInScope(blockId, input);
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
