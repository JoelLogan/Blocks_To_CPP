/**
 * The analyser's conversion rule, mirrored for the editor (03 §3.5.3, 06 §6.6).
 *
 * `staticConversion` is the editor's copy of `b2c_lang::conversion` (../types/static-type.ts, held
 * to it by the shared type fixture). `conversionAllowed` applies it to the type classes of catalog
 * inputs the way the analyser checks an input's `check` (`b2c_lang::check_accepts`), so the
 * connection checker refuses exactly what the analyser would report as an error, and nothing
 * else: gradual typing reports errors only when certain.
 */
import type { TypeClass } from '../generated/catalog';
import { checkAccepts, NO_FACTS, prim, staticConversion } from '../types/static-type';
import { type Compatibility, isStaticType, type StaticConversion, type StaticType } from './types';

export { staticConversion };

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
 * Whether a value of static type `from` may go into an input whose `check` is `to` (03 §3.5.3).
 *
 * * An unknown type (`null`, or a value that is no type) and `error` always connect: the analyser
 *   reports the problem where it is certain.
 * * `void` (a call that gives back nothing) is `invalid` everywhere (`B2C-E0305`).
 * * `any` takes every other value.
 * * `text` takes `std::string` and `char`, like the analyser's question slot; anything else is
 *   `invalid` (`B2C-E0301`), but a user type, whose kind the checker does not know, connects.
 * * `bool`, `integer` and `number` convert to `bool`, `int` and `double` by the conversion rule:
 *   widening is `ok`, narrowing and bool↔number are a `warning` (`B2C-W0518`, `B2C-W0519`), and
 *   anything that does not convert is `invalid`.
 * * The classes of milestone M3 go by membership, a user type connecting; a class this editor does
 *   not know connects.
 */
export function conversionAllowed(from: StaticType | null, to: TypeClass): Compatibility {
  if (!isStaticType(from) || from.kind === 'error') {
    return 'ok';
  }
  if (from.kind === 'prim' && from.name === 'void') {
    return 'invalid';
  }
  switch (to) {
    case 'any':
      return 'ok';
    case 'bool':
      return compatibility(staticConversion(from, prim('bool')));
    case 'integer':
      return compatibility(staticConversion(from, prim('int')));
    case 'number':
      return compatibility(staticConversion(from, prim('double')));
    default: {
      // `text` and the classes of M3; a class from a newer catalog answers null and connects.
      let accepts: boolean | null;
      try {
        accepts = checkAccepts(to, from, NO_FACTS);
      } catch {
        accepts = null;
      }
      return accepts === false ? 'invalid' : 'ok';
    }
  }
}
