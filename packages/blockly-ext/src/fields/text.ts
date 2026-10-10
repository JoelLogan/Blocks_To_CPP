/**
 * `b2c_text`: a piece of text, or a single character with `singleChar` (text.char). Its value is
 * the exact text; the C++ encoders escape it when code is generated (docs/spec/08-security.md §8.4.2).
 */
import type * as Blockly from 'blockly/core';

import { B2cTextInputBase, type TextEntryConfig } from './text-input-base';

/** Options of a text field. */
export interface TextFieldConfig extends TextEntryConfig {
  /** Typing accepts exactly one printable ASCII character (what a C++ `char` literal holds). */
  readonly singleChar?: boolean;
  /** The initial text. */
  readonly value?: string;
}

/**
 * One printable ASCII character, from the space to `~` (U+0020–U+007E): a `char` holds one plain
 * ASCII character (docs/spec/03-block-language.md §3.7.4); anything else is text. A value loaded
 * from a file is kept as it is, and the analyser explains what is wrong with it (B2C-E0312).
 */
const PRINTABLE_ASCII_CHAR = /^[ -~]$/u;

/** A text (or single-character) field. */
export class B2cTextField extends B2cTextInputBase {
  /** Whether typing accepts exactly one printable ASCII character. */
  readonly singleChar: boolean;

  constructor(value: string | null = '', config?: TextFieldConfig) {
    const singleChar = config?.singleChar === true;
    super(value, singleChar ? 'Character' : 'Text', config);
    this.singleChar = singleChar;
  }

  protected acceptsEntry(text: string): boolean {
    return !this.singleChar || PRINTABLE_ASCII_CHAR.test(text);
  }

  /** Builds the field from a block definition: `{type: 'b2c_text', value?, singleChar?}`. */
  static override fromJson(options: Blockly.FieldConfig & TextFieldConfig): B2cTextField {
    return new this(typeof options.value === 'string' ? options.value : '', options);
  }
}
