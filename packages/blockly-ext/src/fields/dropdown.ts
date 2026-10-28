/**
 * `b2c_dropdown`: one of the catalog's `[label, value]` options. The value is stored; the label is
 * shown (docs/spec/03-block-language.md §3.11.1).
 */
import type * as Blockly from 'blockly/core';

import { optionOffered } from './option-filter';
import { B2cOptionsFieldBase, isOptionList, type OptionPair } from './options-base';

/** Options of a dropdown field. */
export interface DropdownFieldConfig {
  /** The choices, as [label, value] pairs. */
  readonly options: readonly OptionPair[];
  /** The initial value (the first option when absent). */
  readonly value?: string;
  /** The accessible name of the open menu. */
  readonly ariaLabel?: string;
}

/** A fixed-choice field. */
export class B2cDropdownField extends B2cOptionsFieldBase {
  constructor(options: readonly OptionPair[], value: string | null = null, ariaLabel = 'Choices') {
    super(options, value, ariaLabel);
  }

  /**
   * The options the menu lists: those the editor's mode and the block's chosen variable offer
   * (./option-filter.ts), and always the current value; every option when none would be left.
   */
  override getOptions(useCache?: boolean): Blockly.MenuOption[] {
    const all = super.getOptions(useCache);
    const block = this.getSourceBlock();
    const name = this.name;
    if (block === null || name === undefined) {
      return all;
    }
    const current = this.getValue();
    const offered = all.filter(
      (option) =>
        typeof option[1] !== 'string' ||
        option[1] === current ||
        optionOffered(block, name, option[1]),
    );
    return offered.length > 0 ? offered : all;
  }

  /**
   * Builds the field from a block definition: `{type: 'b2c_dropdown', options: [[label, value], …],
   * value?}`.
   *
   * @throws TypeError when `options` is not a non-empty list of text pairs.
   */
  static override fromJson(
    options: Blockly.FieldConfig & {
      readonly options?: unknown;
      readonly value?: unknown;
      readonly ariaLabel?: unknown;
    },
  ): B2cDropdownField {
    if (!isOptionList(options.options)) {
      throw new TypeError(
        'b2c_dropdown needs "options": a non-empty list of [label, value] text pairs.',
      );
    }
    return new this(
      options.options,
      typeof options.value === 'string' ? options.value : null,
      typeof options.ariaLabel === 'string' ? options.ariaLabel : undefined,
    );
  }
}
