/**
 * What the editor's fields need from the rest of the app, behind small interfaces: the symbols in
 * scope (from the WASM analysis), static types, and accessible dialogs. The app installs the real
 * services with {@link setEditorServices}; until then safe defaults answer (no symbols, unknown
 * types, dialogs that cancel), so fields never fail for lack of a service.
 *
 * Every call into a service is guarded: a service that throws is treated like one that has no
 * answer, and the field keeps working.
 */
import type { StaticType } from '@blocks2cpp/b2c-core-wasm';
import type * as Blockly from 'blockly/core';

/** How a parameter is passed (`b2c_ir::PassMode`). */
export type PassMode = 'copy' | 'editable' | 'read_only';

/** The kind of a symbol, as the scope query reports it. */
export type SymbolKind = 'variable' | 'parameter' | 'loopVariable' | 'function';

/**
 * One symbol from the scope query (`b2c_ir::SymbolInfo` as JSON, docs/spec/06-compiler-pipeline.md
 * §6.5).
 *
 * This is the subset of the WASM package's `SymbolInfo` the editor reads, so the WASM type can be
 * passed straight in. (`@blocks2cpp/b2c-core-wasm` gains its own `SymbolInfo` in the same milestone
 * wave; this structural type keeps the two packages independent until then.)
 */
export interface SymbolInfo {
  /** The symbol ID that references store. */
  readonly id: string;
  /** The name as the user wrote it in the declaring block. */
  readonly name: string;
  readonly kind: SymbolKind;
  /** Variables: declared `const`. */
  readonly isConst?: boolean;
  /** Parameters: how the argument is passed. */
  readonly mode?: PassMode;
  /** Functions: the parameter symbol IDs, in order. */
  readonly params?: readonly string[];
  /** Functions: the return type. */
  readonly returns?: StaticType;
  /** The value type (a function's return type). */
  readonly type: StaticType;
  /** The ID of the module that declares it. */
  readonly module: string;
  /** The ID of the declaring block. */
  readonly declBlock: string;
}

/** Symbols from the latest analysis. */
export interface SymbolProvider {
  /**
   * The symbols visible at a block, sorted by name. `input` null (or a value-input name) gives what
   * is visible at the block itself; a statement-input name gives what is visible at the start of
   * that list.
   */
  symbolsAt(blockId: string, input: string | null): readonly SymbolInfo[];
  /**
   * The current name of a symbol, or null when no declaration with that ID exists any more. The
   * app keeps last-known names of deleted symbols for the session (docs/spec/03-block-language.md
   * §3.6).
   */
  nameOf(symId: string): string | null;
}

/** Static types from the latest analysis. */
export interface TypeOracle {
  /** The type of the value a reporter or predicate block gives, or null when unknown. */
  outputTypeOf(block: Blockly.Block): StaticType | null;
}

/** Accessible in-app dialogs (never `window.prompt`; M2 decision "Blockly built-in prompts"). */
export interface DialogService {
  /** Asks for text; null when the user cancels. */
  prompt(message: string, defaultValue: string): Promise<string | null>;
  /** Asks a yes/no question; false when the user cancels. */
  confirm(message: string): Promise<boolean>;
  /** Tells the user something. */
  alert(message: string): Promise<void>;
}

/** Everything {@link setEditorServices} installs. */
export interface EditorServices {
  readonly symbols: SymbolProvider;
  readonly types: TypeOracle;
  readonly dialogs: DialogService;
}

const NO_SYMBOLS: readonly SymbolInfo[] = Object.freeze([]);

/** The services in effect before the app installs its own: nothing known, every dialog cancels. */
export const DEFAULT_EDITOR_SERVICES: EditorServices = Object.freeze({
  symbols: Object.freeze({
    symbolsAt: () => NO_SYMBOLS,
    nameOf: () => null,
  }),
  types: Object.freeze({ outputTypeOf: () => null }),
  dialogs: Object.freeze({
    prompt: () => Promise.resolve(null),
    confirm: () => Promise.resolve(false),
    alert: () => Promise.resolve(),
  }),
});

let current: EditorServices = DEFAULT_EDITOR_SERVICES;

/** Installs the services the fields use. A later call replaces them. */
export function setEditorServices(services: EditorServices): void {
  current = services;
}

/** The services in effect. */
export function getEditorServices(): EditorServices {
  return current;
}

/** Goes back to {@link DEFAULT_EDITOR_SERVICES} (tests, and when the editor closes). */
export function resetEditorServices(): void {
  current = DEFAULT_EDITOR_SERVICES;
}

/** The symbols at a block, or none when the provider fails. */
export function symbolsAt(blockId: string, input: string | null): readonly SymbolInfo[] {
  try {
    const symbols = current.symbols.symbolsAt(blockId, input);
    return Array.isArray(symbols) ? symbols : NO_SYMBOLS;
  } catch {
    return NO_SYMBOLS;
  }
}

/** The current name of a symbol, or null when unknown or when the provider fails. */
export function nameOf(symId: string): string | null {
  try {
    const name = current.symbols.nameOf(symId);
    return typeof name === 'string' ? name : null;
  } catch {
    return null;
  }
}

/** The output type of a block, or null when unknown or when the oracle fails. */
export function outputTypeOf(block: Blockly.Block): StaticType | null {
  try {
    return current.types.outputTypeOf(block);
  } catch {
    return null;
  }
}
