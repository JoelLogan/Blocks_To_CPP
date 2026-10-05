/**
 * The start values the toolbox fills in (docs/spec/04-user-interface.md §4.2, M2 decision "Smart
 * defaults for inputs without a catalog default"): a new variable starts at a value of its type
 * (`0`, `0.0`, `true`, `'a'`, `""`), and a call from *My Blocks* comes with an argument of each
 * parameter's type.
 */
import type { StaticType } from '@blocks2cpp/b2c-core-wasm';
import type { TokenJson } from '@blocks2cpp/blockly-ext';

/** The value a new variable or argument of a static type starts with, or `null` for none. */
export function startTokensForStaticType(type: StaticType): readonly TokenJson[] | null {
  switch (type) {
    case 'int':
      return [{ num: '0' }];
    case 'double':
      return [{ num: '0.0' }];
    case 'bool':
      return [{ kw: 'true' }];
    case 'char':
      return [{ chr: 'a' }];
    case 'string':
      return [{ str: '' }];
    case 'void':
    case 'error':
      return null;
  }
}

/**
 * The start value for a `var.declare` TYPE (the type field's values: `int`, `double`, `bool`,
 * `char`, `std::string`, `auto`), or `null` when the type has none of its own (`auto` takes its
 * type from the value, so its value is left as it is).
 */
export function startTokensForDeclaredType(type: string): readonly TokenJson[] | null {
  switch (type) {
    case 'int':
      return startTokensForStaticType('int');
    case 'double':
      return startTokensForStaticType('double');
    case 'bool':
      return startTokensForStaticType('bool');
    case 'char':
      return startTokensForStaticType('char');
    case 'std::string':
      return startTokensForStaticType('string');
    default:
      return null;
  }
}
