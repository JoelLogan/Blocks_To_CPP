/**
 * The analyser's conversion rule, mirrored for the editor (03 §3.5.3, 06 §6.6).
 *
 * `staticConversion` is a line-by-line copy of `b2c_lang::conversion` (crates/b2c-lang/src/
 * typing.rs). `conversionAllowed` applies it to the type classes of catalog inputs the way the
 * analyser checks each kind of input (crates/b2c-lang/src/lower/convert.rs, stmt.rs, expr.rs), so
 * the connection checker refuses exactly what the analyser would report as an error, and nothing
 * else: gradual typing reports errors only when certain.
 */
import type { TypeClass } from '../generated/catalog';
import { type Compatibility, isStaticType, type StaticConversion, type StaticType } from './types';

/** `int`, `double` and `char`: the types b2c-lang calls numbers. */
function isNumber(type: StaticType): boolean {
  return type === 'int' || type === 'double' || type === 'char';
}

/**
 * The conversion from a value of type `from` to a place of type `to`, exactly as
 * `b2c_lang::conversion` decides it:
 *
 * 1. `error` on either side, or the same type on both: `same`.
 * 2. `bool` to a number, or a number to `bool`: `boolNumber`.
 * 3. `int` or `char` to `double`, `char` to `int`: `widening`.
 * 4. `double` to `int` or `char`, `int` to `char`: `narrowing`.
 * 5. Anything else (text with any other type, `void` with any other type): `invalid`.
 */
export function staticConversion(from: StaticType, to: StaticType): StaticConversion {
  if (from === 'error' || to === 'error' || from === to) {
    return 'same';
  }
  if ((from === 'bool' && isNumber(to)) || (isNumber(from) && to === 'bool')) {
    return 'boolNumber';
  }
  if (
    ((from === 'int' || from === 'char') && to === 'double') ||
    (from === 'char' && to === 'int')
  ) {
    return 'widening';
  }
  if ((from === 'double' && (to === 'int' || to === 'char')) || (from === 'int' && to === 'char')) {
    return 'narrowing';
  }
  return 'invalid';
}

/** What the editor does with each conversion: only analyser errors are refused. */
function compatibility(conversion: StaticConversion): Compatibility {
  switch (conversion) {
    case 'same':
    case 'widening':
      return 'ok';
    case 'narrowing':
    case 'boolNumber':
      return 'warning';
    case 'invalid':
      return 'invalid';
  }
}

/**
 * Whether a value of static type `from` may go into an input of type class `to` (03 §3.5.3).
 *
 * * An unknown type (`null`, or a type this editor does not know) and `error` always connect: the
 *   analyser reports the problem where it is certain.
 * * `void` (a call that gives back nothing) is `invalid` everywhere (`B2C-E0305`).
 * * `any` takes every other value.
 * * `text` takes `string` and `char`, like the analyser's question slot; anything else is
 *   `invalid` (`B2C-E0301`).
 * * `bool`, `integer` and `number` convert to `bool`, `int` and `double` by the conversion rule:
 *   widening is `ok`, narrowing and bool↔number are a `warning` (`B2C-W0518`, `B2C-W0519`), and
 *   text is `invalid`.
 * * An unknown type class connects.
 */
export function conversionAllowed(from: StaticType | null, to: TypeClass): Compatibility {
  if (!isStaticType(from) || from === 'error') {
    return 'ok';
  }
  if (from === 'void') {
    return 'invalid';
  }
  switch (to) {
    case 'any':
      return 'ok';
    case 'text':
      return from === 'string' || from === 'char' ? 'ok' : 'invalid';
    case 'bool':
      return compatibility(staticConversion(from, 'bool'));
    case 'integer':
      return compatibility(staticConversion(from, 'int'));
    case 'number':
      return compatibility(staticConversion(from, 'double'));
    default:
      // A type class from a newer catalog: let the analyser decide.
      return 'ok';
  }
}
