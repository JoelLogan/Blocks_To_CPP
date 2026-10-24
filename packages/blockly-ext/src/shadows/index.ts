/** Internal expression shadows for value inputs (docs/spec/03-block-language.md §3.4, M2). */
export { isExprShadowExtra, registerExpressionShadows, type ExprShadowBlock } from './register';
export {
  ExprShadowError,
  exprShadowState,
  getTokenHighlight,
  readExprShadow,
  refreshSymbolNames,
  setTokenHighlight,
} from './state';
export {
  MAX_EXPR_TOKENS,
  TOKEN_KINDS,
  copyTokens,
  isTokenJson,
  isTokenList,
  tokenDisplay,
  tokensDisplay,
  tokensEqual,
  type TokenKind,
} from './tokens';
export {
  EXPR_SHADOW_TYPE,
  EXPR_VALUE_FIELD,
  isExprShadowType,
  type ExprShadowExtra,
  type ExprShadowType,
  type ExprShadowValue,
  type TokenRange,
} from './types';
