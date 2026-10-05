/**
 * Expression-slot tokens (docs/spec/03-block-language.md §3.4, 05 §5.4): checking, comparing, and
 * showing them as text.
 */
import type { TokenJson } from '../generated/catalog';
import { isProjectId } from '../ids';
import { nameOf } from '../services';
import { isCleanFieldText, visibleInvisibles } from '../text';
import { missingSymbolLabel } from '../fields/symbol-ref';

/** The most tokens one slot may hold (05 §5.6, `MAX_EXPR_TOKENS`). */
export const MAX_EXPR_TOKENS = 512;

/** The token kinds (the single key of a token object). */
export const TOKEN_KINDS = ['num', 'str', 'chr', 'ref', 'op', 'kw', 'text'] as const;

/** A token kind. */
export type TokenKind = (typeof TOKEN_KINDS)[number];

/** Whether `value` is a token a project file accepts: one known key with a valid string. */
export function isTokenJson(value: unknown): value is TokenJson {
  if (typeof value !== 'object' || value === null || Array.isArray(value)) {
    return false;
  }
  const keys = Object.keys(value);
  if (keys.length !== 1) {
    return false;
  }
  const kind = keys[0] as TokenKind;
  if (!TOKEN_KINDS.includes(kind)) {
    return false;
  }
  const text: unknown = (value as Record<string, unknown>)[kind];
  return kind === 'ref' ? isProjectId(text) : typeof text === 'string' && isCleanFieldText(text);
}

/** Whether `value` is a token list a project file accepts (at most {@link MAX_EXPR_TOKENS}). */
export function isTokenList(value: unknown): value is readonly TokenJson[] {
  return Array.isArray(value) && value.length <= MAX_EXPR_TOKENS && value.every(isTokenJson);
}

/** A token's kind and text. */
export function tokenParts(token: TokenJson): { kind: TokenKind; text: string } {
  if ('num' in token) return { kind: 'num', text: token.num };
  if ('str' in token) return { kind: 'str', text: token.str };
  if ('chr' in token) return { kind: 'chr', text: token.chr };
  if ('ref' in token) return { kind: 'ref', text: token.ref };
  if ('op' in token) return { kind: 'op', text: token.op };
  if ('kw' in token) return { kind: 'kw', text: token.kw };
  return { kind: 'text', text: token.text };
}

/** A token of a kind with a text. */
export function makeToken(kind: TokenKind, text: string): TokenJson {
  switch (kind) {
    case 'num':
      return { num: text };
    case 'str':
      return { str: text };
    case 'chr':
      return { chr: text };
    case 'ref':
      return { ref: text };
    case 'op':
      return { op: text };
    case 'kw':
      return { kw: text };
    case 'text':
      return { text };
  }
}

/** A fresh copy of a token list (each token a new single-key object). */
export function copyTokens(tokens: readonly TokenJson[]): TokenJson[] {
  return tokens.map((token) => {
    const { kind, text } = tokenParts(token);
    return makeToken(kind, text);
  });
}

/** Whether two token lists are the same. */
export function tokensEqual(a: readonly TokenJson[], b: readonly TokenJson[]): boolean {
  if (a.length !== b.length) {
    return false;
  }
  return a.every((token, index) => {
    const other = b[index];
    if (other === undefined) {
      return false;
    }
    const left = tokenParts(token);
    const right = tokenParts(other);
    return left.kind === right.kind && left.text === right.text;
  });
}

/**
 * How one token reads in a block: numbers, operators and keywords as written, text in double
 * quotes, characters in single quotes, references by the symbol's current name (or
 * `missing (sym_…)`). Invisible characters are shown as placeholders.
 */
export function tokenDisplay(token: TokenJson): string {
  const { kind, text } = tokenParts(token);
  switch (kind) {
    case 'ref': {
      const name = nameOf(text);
      return name === null
        ? missingSymbolLabel(text)
        : visibleInvisibles(name, { lineBreaks: true });
    }
    case 'str':
      return `"${visibleInvisibles(text, { lineBreaks: true })}"`;
    case 'chr':
      return `'${visibleInvisibles(text, { lineBreaks: true })}'`;
    case 'num':
    case 'op':
    case 'kw':
    case 'text':
      return visibleInvisibles(text, { lineBreaks: true });
  }
}

const NO_SPACE_AFTER = new Set(['(', '[']);
const NO_SPACE_BEFORE = new Set([')', ']', ',']);

/** Whether a space separates two neighbouring tokens when an expression is shown. */
function spaceBetween(previous: TokenJson, next: TokenJson): boolean {
  const before = tokenParts(previous);
  const after = tokenParts(next);
  return !(
    (before.kind === 'op' && NO_SPACE_AFTER.has(before.text)) ||
    (after.kind === 'op' && NO_SPACE_BEFORE.has(after.text))
  );
}

/** The text of tokens `start` to `end` (exclusive) of an expression, as it reads in a block. */
export function tokensDisplay(
  tokens: readonly TokenJson[],
  start = 0,
  end = tokens.length,
): string {
  let out = '';
  for (let index = start; index < end; index++) {
    const token = tokens[index];
    if (token === undefined) {
      break;
    }
    const previous = tokens[index - 1];
    if (index > start && previous !== undefined && spaceBetween(previous, token)) {
      out += ' ';
    }
    out += tokenDisplay(token);
  }
  return out;
}

/** Whether the text of a part needs a space before the next part (see {@link tokensDisplay}). */
export function spaceAt(tokens: readonly TokenJson[], index: number): boolean {
  const previous = tokens[index - 1];
  const next = tokens[index];
  return previous !== undefined && next !== undefined && spaceBetween(previous, next);
}
