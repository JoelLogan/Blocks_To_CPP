/**
 * The static type of the value a reporter or predicate gives (03 §3.11.1 "Output types"), worked
 * out in the editor so the connection checker can answer while a block is being dragged, before
 * the next analysis has seen it.
 *
 * The rules mirror b2c-lang's lowering (crates/b2c-lang/src/lower/expr.rs and typing.rs). Where
 * the editor cannot be sure, the answer is `null` ("not known"), which always connects.
 */
import type * as Blockly from 'blockly/core';

import type { BlockDefJson, InputDefJson, TokenJson } from '../generated/catalog';
import { blockDef } from './catalog';
import { isStaticType, type OutputTypeOracle, type StaticType } from './types';

/**
 * How deep nested reporters are followed. Project files nest at most 128 levels (05 §5.6); a
 * deeper chain is "not known".
 */
const MAX_DEPTH = 128;

/** The longest numeric literal b2c-ir accepts (`NumLit::parse`). */
const MAX_LITERAL_CHARS = 400;

/** The longest digit string b2c-ir reads as an integer (`parse_integer`). */
const MAX_INTEGER_DIGITS = 128;

const INT_MAX = 2147483647n;

/**
 * The static type of the value `block` gives, where `def` is its catalog definition:
 *
 * * a fixed `output` (`bool`, `int`, `double`, `char`, `string`) is that type;
 * * `field:NAME` reads the type chosen in field `NAME` (`math.convert`'s `TO`);
 * * `number` comes from the literal text of the block's number field (`math.number`) or from its
 *   operand types (`math.arithmetic`, with `mod` giving a whole number);
 * * `symbol` asks the oracle (`var.get`, `func.call`), and so does `any` (`logic.ternary`), which
 *   falls back to the common type of its two choices while the oracle does not know the block.
 *
 * Returns `null` when the type is not known, including for blocks without an output.
 */
export function staticOutputType(
  block: Blockly.Block,
  def: BlockDefJson,
  oracle: OutputTypeOracle,
): StaticType | null {
  return outputType(block, def, oracle, 0);
}

function outputType(
  block: Blockly.Block,
  def: BlockDefJson,
  oracle: OutputTypeOracle,
  depth: number,
): StaticType | null {
  const output = def.output;
  if (output === null || depth > MAX_DEPTH) {
    return null;
  }
  switch (output) {
    case 'bool':
    case 'int':
    case 'double':
    case 'char':
    case 'string':
      return output;
    case 'symbol':
      return askOracle(oracle, block);
    case 'any':
      return askOracle(oracle, block) ?? conditionalType(block, def, oracle, depth);
    case 'number':
      return numberType(block, def, oracle, depth);
    default:
      return typeNameToStatic(fieldText(block, output.slice('field:'.length)));
  }
}

/** The oracle's answer, or `null` when it does not know or fails. */
function askOracle(oracle: OutputTypeOracle, block: Blockly.Block): StaticType | null {
  try {
    const answer: unknown = oracle.outputTypeOf(block);
    return isStaticType(answer) ? answer : null;
  } catch {
    // The oracle reads the latest analysis; a failure there must never break dragging.
    return null;
  }
}

/** A field's value as text (`''` when the field is missing or holds something else). */
function fieldText(block: Blockly.Block, name: string): string {
  const value: unknown = block.getField(name)?.getValue();
  if (typeof value === 'string') {
    return value;
  }
  return typeof value === 'number' ? String(value) : '';
}

/** A catalog type name (as type fields store it) as a static type. */
function typeNameToStatic(name: string): StaticType | null {
  switch (name) {
    case 'void':
    case 'bool':
    case 'char':
    case 'int':
    case 'double':
      return name;
    case 'std::string':
    case 'string':
      return 'string';
    default:
      // `auto` and types this editor does not know yet.
      return null;
  }
}

// --- `number` outputs -------------------------------------------------------------------------

function numberType(
  block: Blockly.Block,
  def: BlockDefJson,
  oracle: OutputTypeOracle,
  depth: number,
): StaticType | null {
  const literal = def.fields.find((field) => field.kind === 'number');
  if (literal !== undefined) {
    return literalType(fieldText(block, literal.name));
  }
  const operands = def.inputs
    .filter((input) => input.repeat === null)
    .map((input) => operandType(block, input, oracle, depth));
  const remainder =
    def.fields.some((field) => field.name === 'OP') && fieldText(block, 'OP') === 'mod';
  return arithmeticType(operands, remainder);
}

/**
 * The type of the value in one input: the nested block's type, or, while the input is empty, the
 * type of the catalog default the analyser fills in.
 */
function operandType(
  block: Blockly.Block,
  input: InputDefJson,
  oracle: OutputTypeOracle,
  depth: number,
): StaticType | null {
  const child = block.getInput(input.name)?.connection?.targetBlock() ?? null;
  if (child === null) {
    return tokensType(input.default);
  }
  const childDef = blockDef(child.type);
  return childDef === undefined
    ? askOracle(oracle, child)
    : outputType(child, childDef, oracle, depth + 1);
}

/**
 * The type of an arithmetic result after the usual promotions (b2c-lang `binary_type` and
 * `arithmetic_result`): text, `void` (an error already reported at the call) or an error on either
 * side gives `error`; `mod` gives `int`, or `error` with a decimal operand; otherwise `double` if
 * either side is `double`, else `int`.
 */
function arithmeticType(
  operands: readonly (StaticType | null)[],
  remainder: boolean,
): StaticType | null {
  if (operands.some((type) => type === 'error' || type === 'void' || type === 'string')) {
    return 'error';
  }
  if (operands.some((type) => type === null)) {
    return null;
  }
  const decimal = operands.includes('double');
  if (remainder) {
    return decimal ? 'error' : 'int';
  }
  return decimal ? 'double' : 'int';
}

// --- `any` outputs ----------------------------------------------------------------------------

/**
 * The common type of the two choices of a conditional (b2c-lang `conditional_result`): the value
 * inputs with type class `any`, which for `logic.ternary` are THEN and ELSE.
 */
function conditionalType(
  block: Blockly.Block,
  def: BlockDefJson,
  oracle: OutputTypeOracle,
  depth: number,
): StaticType | null {
  const choices = def.inputs.filter((input) => input.check === 'any' && input.repeat === null);
  const [first, second] = choices;
  if (choices.length !== 2 || first === undefined || second === undefined) {
    return null;
  }
  const a = operandType(block, first, oracle, depth);
  const b = operandType(block, second, oracle, depth);
  if (a === 'error' || b === 'error') {
    return 'error';
  }
  if (a === null || b === null) {
    return null;
  }
  if (a === b && a !== 'void') {
    return a;
  }
  const numeric = (type: StaticType): boolean =>
    type === 'int' || type === 'double' || type === 'char';
  if ((numeric(a) || a === 'bool') && (numeric(b) || b === 'bool') && (numeric(a) || numeric(b))) {
    return a === 'double' || b === 'double' ? 'double' : 'int';
  }
  return 'error';
}

// --- Literals and default tokens --------------------------------------------------------------

/**
 * The type of default tokens, as the analyser types them when the input is empty: a single number,
 * text, character or `true`/`false` token. Anything else is not known.
 */
function tokensType(tokens: readonly TokenJson[]): StaticType | null {
  const [token] = tokens;
  if (tokens.length !== 1 || token === undefined) {
    return null;
  }
  if ('num' in token) {
    return literalType(token.num);
  }
  if ('str' in token) {
    return 'string';
  }
  if ('chr' in token) {
    return 'char';
  }
  if ('kw' in token && (token.kw === 'true' || token.kw === 'false')) {
    return 'bool';
  }
  return null;
}

/**
 * The type of a number literal as typed in a number field: `int` unless it has a decimal point or
 * an exponent (and is not hexadecimal), and `error` when b2c-lang rejects it (bad syntax, a leading
 * zero, or a value too large). A leading `-` or `+` is allowed, and `-2147483648` is an `int`.
 * Mirrors `number_block`, `number_literal` and `NumLit::parse`.
 */
export function literalType(raw: string): 'int' | 'double' | 'error' {
  let text = raw.trim();
  let negative = false;
  if (text.startsWith('-')) {
    negative = true;
    text = text.slice(1);
  } else if (text.startsWith('+')) {
    text = text.slice(1);
  }
  if (text.length === 0 || text.length > MAX_LITERAL_CHARS) {
    return 'error';
  }
  const hex = text.startsWith('0x') || text.startsWith('0X');
  if (!hex && /[.eE]/.test(text)) {
    const cleaned = stripSeparators(text, 10);
    return cleaned !== null && isDecimalFloat(cleaned) && Number.isFinite(Number(cleaned))
      ? 'double'
      : 'error';
  }
  const value = parseInteger(text);
  if (value === null) {
    return 'error';
  }
  return value <= (negative ? INT_MAX + 1n : INT_MAX) ? 'int' : 'error';
}

function isDigit(char: string, radix: 2 | 10 | 16): boolean {
  switch (radix) {
    case 2:
      return char === '0' || char === '1';
    case 10:
      return char >= '0' && char <= '9';
    case 16:
      return /^[0-9a-fA-F]$/.test(char);
  }
}

/**
 * Removes C++14 digit separators. As in b2c-ir, a `'` must stand between two digits of `radix`;
 * otherwise the literal is rejected (`null`).
 */
function stripSeparators(text: string, radix: 2 | 10 | 16): string | null {
  // Literals are ASCII; any other character is simply not a digit, so code units are enough.
  for (let index = 0; index < text.length; index += 1) {
    if (text.charAt(index) !== "'") {
      continue;
    }
    if (index === 0 || index === text.length - 1) {
      return null;
    }
    if (!isDigit(text.charAt(index - 1), radix) || !isDigit(text.charAt(index + 1), radix)) {
      return null;
    }
  }
  return text.replaceAll("'", '');
}

/** Whether every character of a non-empty string is a digit of `radix`. */
function allDigits(text: string, radix: 2 | 10 | 16): boolean {
  switch (radix) {
    case 2:
      return /^[01]+$/.test(text);
    case 10:
      return /^[0-9]+$/.test(text);
    case 16:
      return /^[0-9a-fA-F]+$/.test(text);
  }
}

/** A decimal, hexadecimal (`0x`) or binary (`0b`) integer literal, or `null` (b2c-ir `parse_integer`). */
function parseInteger(text: string): bigint | null {
  let radix: 2 | 10 | 16 = 10;
  let digits = text;
  if (text.startsWith('0x') || text.startsWith('0X')) {
    radix = 16;
    digits = text.slice(2);
  } else if (text.startsWith('0b') || text.startsWith('0B')) {
    radix = 2;
    digits = text.slice(2);
  }
  const cleaned = stripSeparators(digits, radix);
  if (cleaned === null) {
    return null;
  }
  // A leading zero would make C++ read the number as octal.
  if (radix === 10 && cleaned.length > 1 && cleaned.startsWith('0')) {
    return null;
  }
  if (cleaned.length > MAX_INTEGER_DIGITS || !allDigits(cleaned, radix)) {
    return null;
  }
  const prefix = radix === 16 ? '0x' : radix === 2 ? '0b' : '';
  return BigInt(prefix + cleaned);
}

/** C++ decimal floating-point syntax without suffix or sign (b2c-ir `is_decimal_float`). */
function isDecimalFloat(text: string): boolean {
  const exponentAt = text.search(/[eE]/);
  const mantissa = exponentAt === -1 ? text : text.slice(0, exponentAt);
  const exponent = exponentAt === -1 ? null : text.slice(exponentAt + 1);
  const dot = mantissa.indexOf('.');
  const intPart = dot === -1 ? mantissa : mantissa.slice(0, dot);
  const fracPart = dot === -1 ? null : mantissa.slice(dot + 1);
  const allDigits = (s: string): boolean => /^[0-9]*$/.test(s);
  if (!allDigits(intPart) || (fracPart !== null && !allDigits(fracPart))) {
    return false;
  }
  if (intPart.length === 0 && (fracPart === null || fracPart.length === 0)) {
    return false;
  }
  if (fracPart === null && exponent === null) {
    return false;
  }
  if (exponent === null) {
    return true;
  }
  const expDigits =
    exponent.startsWith('+') || exponent.startsWith('-') ? exponent.slice(1) : exponent;
  return expDigits.length > 0 && allDigits(expDigits);
}
