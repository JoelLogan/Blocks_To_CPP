/**
 * The internal expression shadows (docs/spec/03-block-language.md §3.4 "In M2", M2 decision
 * "Expression inputs without an expression editor"). M2 has no slot editor, but projects store
 * values as token lists (`{"expr": [...]}`), so a value input that holds tokens, or nothing, shows
 * one of these shadow blocks. They are never catalog blocks and never saved as `{"block": …}`.
 */
import type { TypeClass, TokenJson } from '../generated/catalog';

/** The shadow block types. */
export const EXPR_SHADOW_TYPE = Object.freeze({
  /** One number token: an editable number. */
  num: 'b2c.expr.num',
  /** One text token: editable text. */
  str: 'b2c.expr.str',
  /** One character token: an editable character. */
  chr: 'b2c.expr.chr',
  /** One keyword token (`true`, `false`, …): a dropdown. */
  kw: 'b2c.expr.kw',
  /** One reference: a dropdown of the variables in scope. */
  ref: 'b2c.expr.ref',
  /** Anything else (several tokens, an operator, a draft): read-only text, tokens kept unchanged. */
  tokens: 'b2c.expr.tokens',
} as const);

/** An expression shadow block type. */
export type ExprShadowType = (typeof EXPR_SHADOW_TYPE)[keyof typeof EXPR_SHADOW_TYPE];

const SHADOW_TYPES: ReadonlySet<string> = new Set(Object.values(EXPR_SHADOW_TYPE));

/** Whether a block type is an expression shadow. */
export function isExprShadowType(type: string): type is ExprShadowType {
  return SHADOW_TYPES.has(type);
}

/** The field that holds a one-token shadow's value. */
export const EXPR_VALUE_FIELD = 'VALUE';

/** What an expression shadow keeps besides its field (its Blockly extra state). */
export interface ExprShadowExtra {
  /** The type class of the input it sits in (its outline: hexagonal for `bool`). */
  readonly check: TypeClass;
  /** The input was absent from the file (the catalog default is shown); see {@link readExprShadow}. */
  readonly absent: boolean;
  /** The tokens are an unfinished draft (`"draft": true`). */
  readonly draft: boolean;
  /** The tokens the shadow was made from. */
  readonly tokens: readonly TokenJson[];
}

/** What {@link readExprShadow} reads back from a shadow. */
export interface ExprShadowValue {
  /** The slot's tokens now: the edited literal, or the tokens kept unchanged. */
  readonly tokens: TokenJson[];
  /** Save with `"draft": true`. */
  readonly draft: boolean;
  /**
   * Save the input as absent (leave it out of the file): the input was absent and still holds the
   * default it showed. The first edit that changes the value makes it explicit.
   */
  readonly absent: boolean;
}

/** A token range of an expression: tokens `start` to `end` (exclusive). */
export interface TokenRange {
  readonly start: number;
  readonly end: number;
}
