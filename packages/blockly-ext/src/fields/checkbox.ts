/**
 * `b2c_checkbox`: a yes/no choice (io.print NEWLINE, var.declare CONST). Blockly's checkbox, whose
 * Blockly value is `'TRUE'`/`'FALSE'`; the project value is a boolean
 * ({@link B2cCheckboxField.getBoolean}, and Blockly's serialisation saves a boolean too).
 */
import * as Blockly from 'blockly/core';

/** Options of a checkbox field. */
export interface CheckboxFieldConfig extends Blockly.FieldCheckboxConfig {
  /** The initial state. */
  readonly value?: boolean;
}

/** A checkbox field. */
export class B2cCheckboxField extends Blockly.FieldCheckbox {
  constructor(value = false, config?: CheckboxFieldConfig) {
    super(value, undefined, config);
  }

  /** The state as a boolean. */
  getBoolean(): boolean {
    return this.getValueBoolean() === true;
  }

  /** Sets the state. */
  setBoolean(value: boolean): void {
    this.setValue(value);
  }

  /** Builds the field from a block definition: `{type: 'b2c_checkbox', value?: boolean}`. */
  static override fromJson(options: Blockly.FieldConfig & CheckboxFieldConfig): B2cCheckboxField {
    return new this(options.value === true, options);
  }
}
