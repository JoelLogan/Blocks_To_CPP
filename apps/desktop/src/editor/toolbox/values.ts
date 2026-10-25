/**
 * The start values the toolbox fills in (docs/spec/04-user-interface.md §4.2, M2 decision "Smart
 * defaults for inputs without a catalog default"): a new variable starts at a value of its type
 * (`0`, `0.0`, `true`, `'a'`, `""`), and a call from *Your functions* comes with an argument of each
 * parameter's type.
 */
import type { TypeJson } from '@blocks2cpp/b2c-core-wasm';
import {
  parseType,
  startTokens,
  termFromKey,
  type TokenJson,
  typeFromTerm,
} from '@blocks2cpp/blockly-ext';

/**
 * The value a new variable or argument of a static type starts with, or `null` for none: `void`,
 * `error`, a type the editor cannot read, and, in milestone M3, the library and user types
 * (docs/plans/m3-contracts/slots.md §6). The type is as the analysis sends it: its term
 * (`"int"`, `"std::string"`), docs/plans/m3-contracts/types.md §3.1.
 */
export function startTokensForStaticType(type: TypeJson): readonly TokenJson[] | null {
  const parsed = parseType(type);
  return parsed === null ? null : startTokens(parsed);
}

/**
 * The start value for a type field's value (`var.declare`'s TYPE: `int`, `double`, `bool`, `char`,
 * `std::string`, `auto`, or a term's key), or `null` when the type has none of its own (`auto`
 * takes its type from the value, so its value is left as it is).
 */
export function startTokensForDeclaredType(type: string): readonly TokenJson[] | null {
  const term = termFromKey(type);
  const parsed = term === null ? null : typeFromTerm(term);
  return parsed === null ? null : startTokens(parsed);
}
