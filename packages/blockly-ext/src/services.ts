/**
 * What the editor's fields need from the rest of the app, behind small interfaces: the symbols in
 * scope (from the WASM analysis), static types, and accessible dialogs. The app installs the real
 * services with {@link setEditorServices}; until then safe defaults answer (no symbols, unknown
 * types, dialogs that cancel), so fields never fail for lack of a service.
 *
 * The same call connects the type-aware connection checker (its type oracle) and the variadic
 * mutators (the symbols that label call arguments), so the editor sets everything up in one place.
 *
 * Every call into a service is guarded: a service that throws is treated like one that has no
 * answer, and the field keeps working.
 */
import type { TypeJson } from '@blocks2cpp/b2c-core-wasm';
import type * as Blockly from 'blockly/core';

import { setCheckerTypeOracle } from './checker/oracle';
import { configureMutators } from './mutators/hooks';
import type { StaticType, TypeFactsTs } from './types';

/** How a parameter is passed (`b2c_ir::PassMode`). */
export type PassMode = 'copy' | 'editable' | 'read_only';

/**
 * The kind of a symbol, as the scope query reports it. Milestone M3 adds user types: `struct`,
 * `field`, `enum` and `enumerator` (docs/plans/m3-contracts/types.md §1.2).
 */
export type SymbolKind =
  | 'variable'
  | 'parameter'
  | 'loopVariable'
  | 'function'
  | 'struct'
  | 'field'
  | 'enum'
  | 'enumerator';

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
  readonly returns?: TypeJson;
  /** Structs: the field symbol IDs, in order. */
  readonly fields?: readonly string[];
  /** Enums: the enumerator symbol IDs, in order. */
  readonly values?: readonly string[];
  /** Fields and enumerators: the symbol ID of the struct or enum they belong to. */
  readonly owner?: string;
  /**
   * The value type (a function's return type), as the wire writes it: its term (`"int"`,
   * `"std::string"`, `{"std::vector": "int"}`) or `"error"` (docs/plans/m3-contracts/types.md
   * §3.1); read it with `parseType`.
   */
  readonly type: TypeJson;
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
  /**
   * Optional: the parameter symbol IDs, in order, of the function `funcId` that a call block
   * `blockId` names, or null when that call has no such function to call. The call-argument
   * mutator labels arguments with it; without it, the mutator looks the function up with
   * {@link symbolsAt}, one scope query per call block whenever the labels are refreshed.
   */
  functionParams?(blockId: string, funcId: string): readonly string[] | null;
  /**
   * Optional: the field symbol IDs, in row order, of the struct `structId` that a *new … with*
   * block `blockId` names, or null when it names no such struct (docs/plans/m3-contracts/w2.md
   * §2.1). The call-argument mutator labels that block's parts with it, as it labels a call's
   * arguments with {@link functionParams}; without it, the struct is looked up with
   * {@link symbolsAt}.
   */
  structFields?(blockId: string, structId: string): readonly string[] | null;
}

/**
 * Static types from the latest analysis, as type objects (`./types`, read from the wire's terms
 * with `parseType`).
 *
 * The four optional methods are typed slots' (ADR-0018, docs/plans/m3-contracts/slots.md §4.3):
 * the connection checker, the reference menus and start values read them (`checker/slots.ts`).
 * The editor's oracle answers all of them; an oracle without them (a test's) answers nothing,
 * which types every reference and expression slot as unknown, so everything connects there.
 */
export interface TypeOracle {
  /** The type of the value a reporter or predicate block gives, or null when unknown. */
  outputTypeOf(block: Blockly.Block): StaticType | null;
  /** The analysis's type of an expression slot (the preview's `slotTypes`), or null. */
  slotTypeOf?(blockId: string, input: string): StaticType | null;
  /** A symbol of the latest analysis, or null. */
  symbol?(symId: string): SymbolInfo | null;
  /**
   * Whether the symbol is visible at the block (and input): the scope index, or for a block the
   * index lacks, its place. Null when not known.
   */
  visibleAt?(symId: string, block: Blockly.Block, input: string | null): boolean | null;
  /** What the type rules need to know about user types, from the latest analysis. */
  facts?(): TypeFactsTs;
}

/**
 * Accessible in-app dialogs (never `window.prompt`; M2 decision "Blockly built-in prompts"). This
 * is the fields' own small interface, not the app shell's `DialogService` (which takes option
 * objects); nothing in this package asks for a dialog yet.
 */
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
  /** Optional: without it, {@link DEFAULT_EDITOR_SERVICES}'s dialogs (which cancel) answer. */
  readonly dialogs?: DialogService;
}

const NO_SYMBOLS: readonly SymbolInfo[] = Object.freeze([]);

/** The services in effect before the app installs its own: nothing known, every dialog cancels. */
export const DEFAULT_EDITOR_SERVICES: Required<EditorServices> = Object.freeze({
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

/**
 * Installs the services the fields use, and connects the rest of the package to them: the
 * connection checker asks `types` for the types of `symbol` and `any` outputs
 * (`setCheckerTypeOracle`), and the call-argument mutators label arguments from `symbols`
 * (`configureMutators({ symbols })`). A later call replaces all of them. The mutators' other hooks
 * (`inputShadow`, `newSymbolId`) are set with `configureMutators` and are left as they are.
 */
export function setEditorServices(services: EditorServices): void {
  current = services;
  setCheckerTypeOracle(services.types);
  configureMutators({ symbols: services.symbols });
}

/** The services in effect. */
export function getEditorServices(): EditorServices {
  return current;
}

/**
 * Goes back to {@link DEFAULT_EDITOR_SERVICES} (tests, and when the editor closes), and
 * disconnects the checker's oracle and the mutators' symbols again.
 */
export function resetEditorServices(): void {
  current = DEFAULT_EDITOR_SERVICES;
  setCheckerTypeOracle(null);
  configureMutators({ symbols: null });
}

/**
 * What the reference fields of a block still in the toolbox's flyout list: groups of symbols, shown
 * one after the other with a separator between groups (docs/spec/04-user-interface.md §4.2: every
 * variable of the module shown, grouped like the Variables category). A block in the flyout is in
 * no place of the program, so the scope query has nothing to say about it.
 */
export type ToolboxSymbolSource = () => readonly (readonly SymbolInfo[])[];

let toolboxSymbols: ToolboxSymbolSource | null = null;

/**
 * Sets what the reference fields of blocks in the toolbox's flyout list (the editor's toolbox sets
 * it while it is attached), or with `null` goes back to none: such a field then lists what
 * {@link SymbolProvider.symbolsAt} answers for its block, as every other field does. It is separate
 * from {@link setEditorServices}: the toolbox knows the module shown and how the Variables
 * category groups its variables, the symbol services do not.
 */
export function setToolboxSymbols(source: ToolboxSymbolSource | null): void {
  toolboxSymbols = source;
}

/**
 * The groups of symbols a reference field of a block in the toolbox's flyout lists, or `null` when
 * the toolbox has set none ({@link setToolboxSymbols}). A source that throws, or answers with
 * something other than lists, gives no groups (an empty list).
 */
export function toolboxSymbolGroups(): readonly (readonly SymbolInfo[])[] | null {
  const source = toolboxSymbols;
  if (source === null) {
    return null;
  }
  try {
    const groups = source();
    return Array.isArray(groups) ? groups.filter((group) => Array.isArray(group)) : [];
  } catch {
    return [];
  }
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
