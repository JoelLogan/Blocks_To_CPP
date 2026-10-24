/**
 * `b2c_number`: a number literal kept as the exact text the user typed (docs/spec/05-project-format.md
 * §5.4: "numbers are stored as text"). Blockly's FieldNumber is not used because it normalises
 * (`007` would become `7` and `1e3` would become `1000`). Whether the text is a valid C++ literal is
 * the analyser's decision (B2C-E05xx, with a message on the field).
 */
import type * as Blockly from 'blockly/core';

import { B2cTextInputBase, type TextEntryConfig } from './text-input-base';

/**
 * What can be typed: the characters of C++ number literals (digits, letters for hex digits,
 * prefixes, exponents and suffixes, `.`, digit separators `'`, and exponent signs). No spaces.
 */
export const NUMBER_ENTRY_PATTERN = /^[0-9A-Za-z.'+-]+$/;

/** Options of a number field. */
export interface NumberFieldConfig extends TextEntryConfig {
  /** The initial literal text. */
  readonly value?: string;
}

/** A number literal field. */
export class B2cNumberField extends B2cTextInputBase {
  constructor(value: string | null = '0', config?: NumberFieldConfig) {
    super(value, 'Number', config);
    this.setSpellcheck(false);
  }

  protected acceptsEntry(text: string): boolean {
    return NUMBER_ENTRY_PATTERN.test(text);
  }

  /** Builds the field from a block definition: `{type: 'b2c_number', value?: string}`. */
  static override fromJson(options: Blockly.FieldConfig & NumberFieldConfig): B2cNumberField {
    return new this(typeof options.value === 'string' ? options.value : '0', options);
  }
}
