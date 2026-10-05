/**
 * The common base of the fixed-choice fields (`b2c_dropdown`, `b2c_type`). It is Blockly's dropdown
 * with three changes:
 *
 * - a value from a project file that is not one of the options is kept and shown as it is, instead
 *   of being refused. The catalog check reports it (B2C-E06xx); the editor must not silently change
 *   the project. Such a value is shown with invisible characters as placeholders.
 * - option labels are text only (never images or HTML elements), shown exactly as given: Blockly's
 *   trimming of common words into separate labels is off.
 * - the open menu has an accessible name.
 */
import * as Blockly from 'blockly/core';

import { isCleanFieldText, visibleInvisibles } from '../text';

/** One choice: what the menu shows, and the value stored. */
export type OptionPair = readonly [label: string, value: string];

/** Whether `options` is a non-empty list of text [label, value] pairs. */
export function isOptionList(options: unknown): options is readonly OptionPair[] {
  return (
    Array.isArray(options) &&
    options.length > 0 &&
    options.every(
      (option) =>
        Array.isArray(option) &&
        option.length === 2 &&
        typeof option[0] === 'string' &&
        typeof option[1] === 'string',
    )
  );
}

/** A dropdown with a fixed list of text options. */
export abstract class B2cOptionsFieldBase extends Blockly.FieldDropdown {
  /** The accessible name of the open menu. */
  protected menuLabel = '';
  private labels = new Map<string, string>();

  protected constructor(options: readonly OptionPair[], value: string | null, menuLabel: string) {
    super(Blockly.Field.SKIP_SETUP);
    this.maxDisplayLength = 40;
    this.menuLabel = menuLabel;
    if (!isOptionList(options)) {
      throw new TypeError('A Blocks2Cpp dropdown needs at least one [label, value] text pair.');
    }
    this.labels = new Map(
      options.map(([label, optionValue]) => [
        optionValue,
        visibleInvisibles(label, { lineBreaks: true }),
      ]),
    );
    // Blockly makes the first option the value here; `value` replaces it below.
    this.setOptions([...this.labels].map(([optionValue, label]) => [label, optionValue]));
    if (value !== null) {
      this.setValue(value);
    }
  }

  /** The values of the options, in menu order. */
  optionValues(): string[] {
    return [...this.labels.keys()];
  }

  /** Accepts any text that follows the project text rules (see the module comment). */
  protected override doClassValidation_(newValue?: unknown): string | null {
    return typeof newValue === 'string' && isCleanFieldText(newValue) ? newValue : null;
  }

  /** The label of the value: its option's label, or the value itself when it is not an option. */
  protected override getText_(): string | null {
    const value = this.getValue();
    if (value === null) {
      return null;
    }
    return this.labels.get(value) ?? visibleInvisibles(value, { lineBreaks: true });
  }

  /** Labels are shown exactly as given (no common prefix or suffix is split off). */
  protected override trimOptions(options: Blockly.MenuOption[]): { options: Blockly.MenuOption[] } {
    return { options };
  }

  protected override showEditor_(e?: MouseEvent): void {
    super.showEditor_(e);
    this.menu_?.getElement()?.setAttribute('aria-label', this.menuLabel);
  }
}
