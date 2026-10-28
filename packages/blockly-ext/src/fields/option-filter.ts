/**
 * Which options a `b2c_dropdown` menu lists (docs/plans/m3-contracts/w2.md §1.9): the options the
 * editor's mode offers (the app's feature gating, W4: *Show advanced blocks* hides the bitwise forms
 * of *change*), and, for a menu whose choices depend on a variable chosen on the same block, the
 * ones that variable's type takes (*change*'s `OP` lists *add to the end* for text, the arithmetic
 * for numbers).
 *
 * Only the menu is filtered: a value already on a block is never changed or refused for it (a
 * project keeps what it holds, and the analysis reports what does not fit), and the current value
 * is always listed.
 */
import type * as Blockly from 'blockly/core';

import { getEditorServices } from '../services';
import { B2cSymbolRefField } from './symbol-ref';

/** Whether the option `value` of the dropdown `field` of `block` is offered in its menu. */
export type OptionFilter = (block: Blockly.Block, field: string, value: string) => boolean;

let modeFilter: OptionFilter | null = null;

/**
 * Sets what the editor's mode offers (the app's gating service), or with `null` goes back to every
 * option.
 */
export function setOptionFilter(filter: OptionFilter | null): void {
  modeFilter = filter;
}

/** The value types each option takes, by block and field; an option not listed takes any. */
interface TypedOptions {
  /** The block's reference field whose variable the options depend on. */
  readonly variable: string;
  /** The types (as the wire writes them) each option takes. */
  readonly takes: Readonly<Record<string, readonly string[]>>;
}

const NUMBERS = ['int', 'double', 'char'] as const;
const WHOLE = ['int', 'char'] as const;

/**
 * *change*'s operators by the type of its variable (w2.md §1.9): the arithmetic for numbers,
 * *mod* for whole numbers, *add to the end* for text, the bitwise forms for `int`.
 */
const TYPED_OPTIONS: Readonly<Record<string, Readonly<Record<string, TypedOptions>>>> = {
  'var.change': {
    OP: {
      variable: 'VAR',
      takes: {
        add: NUMBERS,
        sub: NUMBERS,
        mul: NUMBERS,
        div: NUMBERS,
        mod: WHOLE,
        append: ['std::string'],
        and: ['int'],
        or: ['int'],
        xor: ['int'],
        shl: ['int'],
        shr: ['int'],
      },
    },
  },
};

/**
 * The type of the variable `block`'s reference field `field` names, as the wire writes a plain
 * type (`"int"`), or null when it is not known (no variable chosen, no analysis yet, an error type
 * or a compound type).
 */
function variableType(block: Blockly.Block, field: string): string | null {
  const reference = block.getField(field);
  if (!(reference instanceof B2cSymbolRefField)) {
    return null;
  }
  const id = reference.getRef()?.ref;
  if (id === undefined) {
    return null;
  }
  const type = getEditorServices().types.symbol?.(id)?.type;
  return typeof type === 'string' && type !== 'error' ? type : null;
}

/** Whether the option `value` of `field` on `block` is offered (see the module comment). */
export function optionOffered(block: Blockly.Block, field: string, value: string): boolean {
  if (modeFilter !== null && !modeFilter(block, field, value)) {
    return false;
  }
  const typed = TYPED_OPTIONS[block.type]?.[field];
  const takes = typed?.takes[value];
  if (typed === undefined || takes === undefined) {
    return true;
  }
  const type = variableType(block, typed.variable);
  return type === null || takes.includes(type);
}
