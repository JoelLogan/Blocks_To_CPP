/**
 * The one-line statements Quick Insert reads as blocks (docs/spec/03-block-language.md §3.14, the
 * fixed set of docs/plans/m3-contracts/w4-editor-tools.md §2.4): `int x = 5`, `x = y + 1`,
 * `x += 2`, `x++`, `print "hi", x`, `cout << "hi" << x`, `repeat 10`, `while running`, `if x > 3`,
 * `return x`, `break`, `continue`. Pure.
 *
 * A value becomes an expression slot's tokens (05 §5.4): numbers, quoted text and characters,
 * `true` and `false`, the names of the variables given (by symbol ID), and the operators and
 * brackets of expressions; the analysis reads the tokens as it reads any slot, and reports what
 * does not parse. A name that is no variable given gives no block, and nothing typed is ever kept
 * as code: the block is built from the catalog and the tokens only.
 */
import type { SymbolInfo } from '@blocks2cpp/b2c-core-wasm';
import { NAME_ENTRY_PATTERN, type TokenJson } from '@blocks2cpp/blockly-ext';

import { presetBlock, type B2cBlockInfo } from '../toolbox/presets';

/** The longest line read, in characters. */
export const MAX_LINE_CHARS = 200;

/** The most items a `print` line gives (the block's own limit is larger). */
const MAX_PRINT_ITEMS = 16;

/** The type names a declaration may start with, as the type field stores them. */
const DECLARED_TYPES: Readonly<Record<string, string>> = {
  int: 'int',
  double: 'double',
  bool: 'bool',
  char: 'char',
  string: 'std::string',
  'std::string': 'std::string',
  auto: 'auto',
};

/** *change*'s operators by their C++ form. */
const COMPOUND: Readonly<Record<string, string>> = {
  '+=': 'add',
  '-=': 'sub',
  '*=': 'mul',
  '/=': 'div',
  '%=': 'mod',
};

/** The operators a value may hold, longest first. */
const OPERATORS = [
  '&&',
  '||',
  '==',
  '!=',
  '<=',
  '>=',
  '<',
  '>',
  '+',
  '-',
  '*',
  '/',
  '%',
  '!',
  '(',
  ')',
];

/** The variables a line may name, by name (the first of several with one name). */
export type LineNames = ReadonlyMap<string, SymbolInfo>;

/** The variables (and parameters, loop variables and globals) of `symbols`, by name. */
export function lineNames(symbols: readonly SymbolInfo[]): LineNames {
  const names = new Map<string, SymbolInfo>();
  for (const symbol of symbols) {
    const value =
      symbol.kind === 'variable' ||
      symbol.kind === 'parameter' ||
      symbol.kind === 'loopVariable' ||
      symbol.kind === 'global';
    if (value && !names.has(symbol.name)) {
      names.set(symbol.name, symbol);
    }
  }
  return names;
}

/** What a line reads as. */
export interface ParsedLine {
  /** The toolbox item of the block. */
  readonly item: B2cBlockInfo;
  /** What the block is, for the result's text: `create x`, `print`. */
  readonly title: string;
}

/**
 * The tokens of a value: `null` when it holds something else (an unknown name, a `;`, an
 * assignment) or nothing.
 */
export function valueTokens(text: string, names: LineNames): TokenJson[] | null {
  const tokens: TokenJson[] = [];
  let index = 0;
  const source = text.trim();
  while (index < source.length) {
    const rest = source.slice(index);
    const space = /^\s+/u.exec(rest);
    if (space !== null) {
      index += space[0].length;
      continue;
    }
    const number = /^\d+(?:\.\d+)?/u.exec(rest);
    if (number !== null) {
      tokens.push({ num: number[0] });
      index += number[0].length;
      continue;
    }
    if (rest.startsWith('"')) {
      const end = rest.indexOf('"', 1);
      if (end < 0) {
        return null;
      }
      const inner = rest.slice(1, end);
      if (inner.includes('\\')) {
        return null;
      }
      tokens.push({ str: inner });
      index += end + 1;
      continue;
    }
    const char = /^'([^'\\])'/u.exec(rest);
    if (char?.[1] !== undefined) {
      tokens.push({ chr: char[1] });
      index += char[0].length;
      continue;
    }
    const word = /^[A-Za-z_][A-Za-z0-9_]*/u.exec(rest);
    if (word !== null) {
      const name = word[0];
      if (name === 'true' || name === 'false') {
        tokens.push({ kw: name });
      } else {
        const symbol = names.get(name);
        if (symbol === undefined) {
          return null;
        }
        tokens.push({ ref: symbol.id });
      }
      index += name.length;
      continue;
    }
    const op = OPERATORS.find((candidate) => rest.startsWith(candidate));
    if (op === undefined) {
      return null;
    }
    // `=` alone is an assignment, never a value.
    tokens.push({ op });
    index += op.length;
  }
  return tokens.length === 0 || !balanced(tokens) ? null : tokens;
}

/** Whether the brackets of `tokens` pair up: none closes before it opens, none is left open. */
function balanced(tokens: readonly TokenJson[]): boolean {
  let depth = 0;
  for (const token of tokens) {
    if ('op' in token && token.op === '(') {
      depth += 1;
    } else if ('op' in token && token.op === ')') {
      depth -= 1;
      if (depth < 0) {
        return false;
      }
    }
  }
  return depth === 0;
}

/**
 * Whether `text` is one bracketed whole: it starts with `(` and the `)` that closes it is its last
 * character (`(a) && (b)` is not; quoted text is skipped, as in {@link splitTop}).
 */
function wrapsWhole(text: string): boolean {
  if (!text.startsWith('(') || !text.endsWith(')')) {
    return false;
  }
  let depth = 0;
  let quote: string | null = null;
  for (let index = 0; index < text.length; index += 1) {
    const char = text.charAt(index);
    if (quote !== null) {
      if (char === quote) {
        quote = null;
      }
      continue;
    }
    if (char === '"' || char === "'") {
      quote = char;
    } else if (char === '(') {
      depth += 1;
    } else if (char === ')') {
      depth -= 1;
      if (depth === 0) {
        return index === text.length - 1;
      }
    }
  }
  return false;
}

/** Splits `text` at top-level `sep` (outside quotes and brackets). */
function splitTop(text: string, sep: string): string[] {
  const parts: string[] = [];
  let depth = 0;
  let quote: string | null = null;
  let start = 0;
  for (let index = 0; index < text.length; index += 1) {
    const char = text.charAt(index);
    if (quote !== null) {
      if (char === quote) {
        quote = null;
      }
      continue;
    }
    if (char === '"' || char === "'") {
      quote = char;
    } else if (char === '(') {
      depth += 1;
    } else if (char === ')') {
      depth -= 1;
    } else if (depth === 0 && text.startsWith(sep, index)) {
      parts.push(text.slice(start, index));
      start = index + sep.length;
      index += sep.length - 1;
    }
  }
  parts.push(text.slice(start));
  return parts.map((part) => part.trim());
}

/** A *print* of `parts` (each a value), or `null`. */
function printLine(
  parts: readonly string[],
  names: LineNames,
  newline: boolean,
): ParsedLine | null {
  const items = parts.filter((part) => part !== '');
  if (items.length === 0 || items.length > MAX_PRINT_ITEMS) {
    return null;
  }
  const inputs: Record<string, TokenJson[]> = {};
  for (const [index, part] of items.entries()) {
    const tokens = valueTokens(part, names);
    if (tokens === null) {
      return null;
    }
    inputs[`ITEM${String(index)}`] = tokens;
  }
  return {
    item: presetBlock('io.print', {
      fields: { NEWLINE: newline },
      extra: { itemCount: items.length },
      inputs,
    }),
    title: 'print',
  };
}

/** One block with one value input. */
function withValue(
  type: string,
  input: string,
  value: string,
  names: LineNames,
  title: string,
  fields: Readonly<Record<string, unknown>> = {},
): ParsedLine | null {
  const tokens = valueTokens(value, names);
  return tokens === null
    ? null
    : {
        item: presetBlock(type, {
          fields: fields as Record<string, string | boolean | { ref: string }>,
          inputs: { [input]: tokens },
        }),
        title,
      };
}

/** The block a line reads as, or `null` when it is none of the forms read (see the module comment). */
export function parseLine(line: string, names: LineNames): ParsedLine | null {
  const text = line.trim().replace(/;\s*$/u, '').trim();
  if (text === '' || text.length > MAX_LINE_CHARS) {
    return null;
  }
  if (text === 'break') {
    return { item: presetBlock('control.break'), title: 'leave loop' };
  }
  if (text === 'continue') {
    return { item: presetBlock('control.continue'), title: 'skip to the next round' };
  }
  if (text === 'return') {
    return { item: presetBlock('func.return'), title: 'return' };
  }
  const keyword = /^(return|repeat|while|if)\s+(.+)$/u.exec(text);
  if (keyword?.[1] !== undefined && keyword[2] !== undefined) {
    // `if (x > 3)`: the brackets around the whole condition are the statement's, not the value's.
    const value = wrapsWhole(keyword[2]) ? keyword[2].slice(1, -1) : keyword[2];
    switch (keyword[1]) {
      case 'return':
        return withValue('func.return', 'VALUE', value, names, 'return');
      case 'repeat':
        return withValue(
          'control.repeat',
          'TIMES',
          value.replace(/\s+times$/u, ''),
          names,
          'repeat',
        );
      case 'while':
        return withValue('control.while', 'COND', value, names, 'repeat while');
      case 'if':
        return withValue('control.if', 'COND0', value, names, 'if');
    }
  }
  const cout = /^(?:std::)?cout\s*<<(.+)$/u.exec(text);
  if (cout?.[1] !== undefined) {
    const parts = splitTop(cout[1], '<<');
    const last = parts.at(-1);
    const newline = last === 'endl' || last === 'std::endl' || last === "'\\n'" || last === '"\\n"';
    return printLine(newline ? parts.slice(0, -1) : parts, names, newline);
  }
  const print = /^print\s+(.+)$/u.exec(text);
  if (print?.[1] !== undefined) {
    return printLine(splitTop(print[1], ','), names, true);
  }
  const declare =
    /^(int|double|bool|char|string|std::string|auto)\s+([A-Za-z0-9_]+)\s*(?:=\s*(.+))?$/u.exec(
      text,
    );
  if (declare?.[1] !== undefined && declare[2] !== undefined) {
    const name = declare[2];
    if (!NAME_ENTRY_PATTERN.test(name)) {
      return null;
    }
    const type = DECLARED_TYPES[declare[1]] ?? 'int';
    const tokens = declare[3] === undefined ? null : valueTokens(declare[3], names);
    if (declare[3] !== undefined && tokens === null) {
      return null;
    }
    return {
      item: presetBlock('var.declare', {
        fields: { TYPE: type, CONST: false },
        declare: { NAME: name },
        ...(tokens === null ? {} : { inputs: { VALUE: tokens } }),
      }),
      title: `create ${name}`,
    };
  }
  const step =
    /^(?:([A-Za-z_][A-Za-z0-9_]*)\s*(\+\+|--)|(\+\+|--)\s*([A-Za-z_][A-Za-z0-9_]*))$/u.exec(text);
  if (step !== null) {
    const name = step[1] ?? step[4] ?? '';
    const symbol = names.get(name);
    const op = (step[2] ?? step[3]) === '++' ? 'add' : 'sub';
    return symbol === undefined
      ? null
      : {
          item: presetBlock('var.change', {
            fields: { VAR: { ref: symbol.id }, OP: op },
            inputs: { BY: [{ num: '1' }] },
          }),
          title: `change ${name}`,
        };
  }
  const assign = /^([A-Za-z_][A-Za-z0-9_]*)\s*([+\-*/%]?=)(?!=)\s*(.+)$/u.exec(text);
  if (assign?.[1] !== undefined && assign[2] !== undefined && assign[3] !== undefined) {
    const symbol = names.get(assign[1]);
    if (symbol === undefined) {
      return null;
    }
    if (assign[2] === '=') {
      return withValue('var.set', 'VALUE', assign[3], names, `set ${assign[1]}`, {
        VAR: { ref: symbol.id },
      });
    }
    const op = COMPOUND[assign[2]];
    return op === undefined
      ? null
      : withValue('var.change', 'BY', assign[3], names, `change ${assign[1]}`, {
          VAR: { ref: symbol.id },
          OP: op,
        });
  }
  return null;
}
