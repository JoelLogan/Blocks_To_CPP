/**
 * What the mutators need from the rest of the editor, set once by the editor (the seam to the
 * editor services and expression shadows of blockly-ext core).
 *
 * * `symbols` gives the parameter names that label a call's arguments. It is structurally a subset
 *   of the editor's `SymbolProvider`, so the editor can pass that object unchanged; its optional
 *   `functionParams` lets the labels skip the scope query.
 * * `inputShadow` gives the shadow a new value input shows (the catalog default as an expression
 *   shadow, 03 §3.4). Without it, new inputs start empty.
 * * `newSymbolId` makes the symbol ID of a new parameter. Without it, IDs are `sym_` and 17 random
 *   base62 characters from the platform's cryptographic generator.
 * * `declName` gives the name a new part of a repeated declaration field starts with (a second
 *   catch part's `e`, docs/plans/m3-contracts/w2.md §2.1); the editor answers from its default
 *   names. Without it, or when it has no name, the part is named after its field in lower case.
 */
import type * as Blockly from 'blockly/core';

import type { InputDefJson } from '../generated/catalog';

/** The part of a symbol record the mutators read (a subset of the editor's `SymbolInfo`). */
export interface MutatorSymbolInfo {
  readonly id: string;
  readonly kind: string;
  /** For a function: its parameters' symbol IDs, in order. */
  readonly params?: readonly string[];
  /** For a function: how many of its last parameters have a default value (w2.md §2.8). */
  readonly optional?: number;
  /** For a struct: its fields' symbol IDs, in row order (w2.md §2.2). */
  readonly fields?: readonly string[];
}

/** Symbols as the latest analysis sees them. */
export interface MutatorSymbols {
  /** The symbols visible at a block (at the block itself when `input` is `null`). */
  symbolsAt(blockId: string, input: string | null): readonly MutatorSymbolInfo[];
  /** The current name of a symbol, or `null` when it is unknown. */
  nameOf(symId: string): string | null;
  /**
   * The parameters (their symbol IDs, in order) of the function `funcId` that a call at block
   * `blockId` names, or `null` when that call has no such function to call. Optional: without it,
   * a call looks its function up among {@link symbolsAt} at itself, one scope query per call
   * block every time the labels are refreshed. An editor that answers from a table it builds once
   * per analysis gives it, so refreshing the labels of every call costs no scope query.
   */
  functionParams?(blockId: string, funcId: string): readonly string[] | null;
  /**
   * How many of the last parameters of that function have a default value, so that a call may
   * leave their arguments out (w2.md §2.8). Optional, with {@link functionParams}.
   */
  functionOptional?(blockId: string, funcId: string): number;
  /**
   * The fields (their symbol IDs, in row order) of the struct `structId` that a *new … with* block
   * `blockId` names, or `null` when it names no such struct (w2.md §2.1, *Call arguments named by
   * a struct*). Optional, as {@link functionParams} is: without it, the struct is looked up among
   * {@link symbolsAt} at the block.
   */
  structFields?(blockId: string, structId: string): readonly string[] | null;
}

/** The shadow state for a new value input, or `null` for none. */
export type InputShadowFactory = (
  block: Blockly.Block,
  input: InputDefJson,
  name: string,
) => Blockly.serialization.blocks.State | null;

/**
 * The name a new part of a repeated declaration field starts with (`field` is the catalog field's
 * base name, `NAME`), or `null` for the mutators' own choice.
 */
export type DeclNameFactory = (block: Blockly.Block, field: string) => string | null;

/** The editor hooks; every one is optional. */
export interface MutatorHooks {
  symbols?: MutatorSymbols | null;
  inputShadow?: InputShadowFactory | null;
  newSymbolId?: (() => string) | null;
  declName?: DeclNameFactory | null;
}

interface ResolvedHooks {
  symbols: MutatorSymbols | null;
  inputShadow: InputShadowFactory | null;
  newSymbolId: () => string;
  declName: DeclNameFactory | null;
}

const BASE62 = '0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz';

/** The number of random base62 characters in a generated symbol ID (about 101 bits). */
const ID_CHARS = 17;

/**
 * `sym_` and 17 base62 characters from `crypto.getRandomValues`, without modulo bias. Matches the
 * ID rule of 05 §5.4 (`[A-Za-z0-9_]{1,32}`).
 */
export function randomSymbolId(): string {
  const out: string[] = [];
  const bytes = new Uint8Array(32);
  while (out.length < ID_CHARS) {
    globalThis.crypto.getRandomValues(bytes);
    for (const byte of bytes) {
      // 248 = 4 × 62: bytes from 248 to 255 would favour the first eight characters.
      if (byte < 248 && out.length < ID_CHARS) {
        out.push(BASE62.charAt(byte % 62));
      }
    }
  }
  return `sym_${out.join('')}`;
}

const DEFAULTS: ResolvedHooks = {
  symbols: null,
  inputShadow: null,
  newSymbolId: randomSymbolId,
  declName: null,
};

let hooks: ResolvedHooks = { ...DEFAULTS };

/**
 * Sets the editor hooks. A key that is absent keeps its current value; `null` goes back to the
 * default.
 */
export function configureMutators(next: MutatorHooks): void {
  hooks = {
    symbols: next.symbols === undefined ? hooks.symbols : next.symbols,
    inputShadow: next.inputShadow === undefined ? hooks.inputShadow : next.inputShadow,
    newSymbolId:
      next.newSymbolId === undefined ? hooks.newSymbolId : (next.newSymbolId ?? randomSymbolId),
    declName: next.declName === undefined ? hooks.declName : next.declName,
  };
}

/** The hooks in use. */
export function mutatorHooks(): Readonly<ResolvedHooks> {
  return hooks;
}
