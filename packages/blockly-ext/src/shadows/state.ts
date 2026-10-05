/**
 * Making, reading and marking expression shadows: the functions the document sync and the
 * diagnostics layer use (see ./types.ts for what the shadows are).
 */
import * as Blockly from 'blockly/core';

import { B2C_CSS_CLASS } from '../blocks/css';
import { B2cSymbolRefField } from '../fields/symbol-ref';
import type { TokenJson, TypeClass } from '../generated/catalog';
import { renderTokens, isTokenRange, type ExprShadowBlock } from './register';
import {
  MAX_EXPR_TOKENS,
  copyTokens,
  isTokenJson,
  makeToken,
  tokenParts,
  tokensEqual,
} from './tokens';
import {
  EXPR_SHADOW_TYPE,
  EXPR_VALUE_FIELD,
  isExprShadowType,
  type ExprShadowExtra,
  type ExprShadowValue,
  type TokenRange,
} from './types';

/** Tokens that cannot be shown as a shadow: too many, or not tokens a project file accepts. */
export class ExprShadowError extends Error {
  override readonly name = 'ExprShadowError';
}

/**
 * The Blockly state of the shadow for an expression input, to use as the input's `shadow` in
 * Blockly's JSON (`{inputs: {NAME: {shadow: exprShadowState(…)}}}`):
 *
 * - one literal token (`num`, `str`, `chr`, `kw`) gives an editable literal;
 * - one `ref` gives a dropdown of the variables in scope at the input;
 * - anything else, and every draft, gives a read-only shadow that keeps the tokens unchanged.
 *
 * `check` is the input's type class: `bool` inputs get a hexagonal shadow, all others a round one.
 * `absent` says the input is absent from the file and the shadow shows the catalog default; it
 * stays absent until the user changes the value (see {@link readExprShadow}).
 *
 * @throws ExprShadowError when `tokens` is longer than 512 or holds something that is not a token a
 *   project file accepts. Tokens from a loaded document never do.
 */
export function exprShadowState(
  tokens: readonly TokenJson[],
  draft: boolean,
  check: TypeClass,
  absent: boolean,
): Blockly.serialization.blocks.State {
  if (tokens.length > MAX_EXPR_TOKENS) {
    throw new ExprShadowError(
      `An expression slot holds at most ${String(MAX_EXPR_TOKENS)} tokens.`,
    );
  }
  if (!tokens.every(isTokenJson)) {
    throw new ExprShadowError('An expression slot holds a value that is not a token.');
  }
  const extraState: ExprShadowExtra = { check, absent, draft, tokens: copyTokens(tokens) };
  const only = tokens.length === 1 && !draft ? tokens[0] : undefined;
  if (only !== undefined) {
    const { kind, text } = tokenParts(only);
    switch (kind) {
      case 'num':
      case 'str':
      case 'chr':
      case 'kw':
        return { type: EXPR_SHADOW_TYPE[kind], fields: { [EXPR_VALUE_FIELD]: text }, extraState };
      case 'ref':
        return {
          type: EXPR_SHADOW_TYPE.ref,
          fields: { [EXPR_VALUE_FIELD]: { ref: text } },
          extraState,
        };
      case 'op':
      case 'text':
        break;
    }
  }
  return { type: EXPR_SHADOW_TYPE.tokens, extraState };
}

function shadowOf(block: Blockly.Block): ExprShadowBlock | null {
  if (!isExprShadowType(block.type)) {
    return null;
  }
  const candidate = block as unknown as Partial<ExprShadowBlock>;
  return candidate.b2cExpr === undefined ? null : (block as unknown as ExprShadowBlock);
}

/** The tokens a one-token shadow holds now, or null when its field has no value. */
function currentTokens(block: ExprShadowBlock): TokenJson[] | null {
  const field = block.getField(EXPR_VALUE_FIELD);
  if (field === null) {
    return null;
  }
  if (field instanceof B2cSymbolRefField) {
    const ref = field.getValue();
    return ref === null ? null : [{ ref }];
  }
  const value: unknown = field.getValue();
  if (typeof value !== 'string') {
    return null;
  }
  switch (block.type) {
    case EXPR_SHADOW_TYPE.num:
      return [makeToken('num', value)];
    case EXPR_SHADOW_TYPE.str:
      return [makeToken('str', value)];
    case EXPR_SHADOW_TYPE.chr:
      return [makeToken('chr', value)];
    case EXPR_SHADOW_TYPE.kw:
      return [makeToken('kw', value)];
    default:
      return null;
  }
}

/**
 * Reads an expression shadow back for saving: its tokens now (an edited literal is a single-token
 * expression), whether they are a draft, and whether the input is still absent. An absent input
 * stays absent while its value equals the default it was created with; the first change makes it
 * explicit. Null when the block is not an expression shadow.
 */
export function readExprShadow(block: Blockly.Block): ExprShadowValue | null {
  const shadow = shadowOf(block);
  if (shadow === null) {
    return null;
  }
  const extra = shadow.b2cExpr;
  if (block.type === EXPR_SHADOW_TYPE.tokens) {
    return { tokens: copyTokens(extra.tokens), draft: extra.draft, absent: extra.absent };
  }
  const tokens = currentTokens(shadow) ?? copyTokens(extra.tokens);
  return {
    tokens,
    draft: false,
    absent: extra.absent && tokensEqual(tokens, extra.tokens),
  };
}

/**
 * Marks the tokens a diagnostic points at (`range`, as token indexes `start` to `end`), or clears
 * the mark (null). A read-only shadow underlines just those tokens; a one-token shadow is marked as
 * a whole. A range outside the expression clears the mark. Does nothing for other blocks.
 */
export function setTokenHighlight(block: Blockly.Block, range: TokenRange | null): void {
  const shadow = shadowOf(block);
  if (shadow === null) {
    return;
  }
  const length = block.type === EXPR_SHADOW_TYPE.tokens ? shadow.b2cExpr.tokens.length : 1;
  const valid =
    range !== null && isTokenRange(range, length) ? { start: range.start, end: range.end } : null;
  shadow.b2cHighlight = valid;
  if (block.type === EXPR_SHADOW_TYPE.tokens) {
    renderTokens(shadow);
  } else if (block instanceof Blockly.BlockSvg) {
    if (valid === null) {
      block.removeClass(B2C_CSS_CLASS.tokenHighlight);
    } else {
      block.addClass(B2C_CSS_CLASS.tokenHighlight);
    }
  }
}

/** The token range marked on a shadow, or null. */
export function getTokenHighlight(block: Blockly.Block): TokenRange | null {
  return shadowOf(block)?.b2cHighlight ?? null;
}

/**
 * Shows the current symbol names everywhere in a workspace: every reference field and every
 * read-only expression. Call it after each analysis, so a rename shows on every reference.
 */
export function refreshSymbolNames(workspace: Blockly.Workspace): void {
  for (const block of workspace.getAllBlocks(false)) {
    if (block.type === EXPR_SHADOW_TYPE.tokens) {
      const shadow = shadowOf(block);
      if (shadow !== null) {
        renderTokens(shadow);
      }
      continue;
    }
    for (const input of block.inputList) {
      for (const field of input.fieldRow) {
        if (field instanceof B2cSymbolRefField) {
          field.forceRerender();
        }
      }
    }
  }
}
