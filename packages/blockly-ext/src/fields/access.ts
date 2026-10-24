/**
 * Reading and writing field values in their project-file shape (docs/spec/05-project-format.md
 * §5.4), whatever Blockly keeps internally. The document sync uses these two functions, so it never
 * depends on Blockly's own serialisation or on how each field stores its value.
 */
import type * as Blockly from 'blockly/core';

import { B2cCheckboxField } from './checkbox';
import { B2cOptionsFieldBase } from './options-base';
import { B2cSymbolDeclField } from './symbol-decl';
import { B2cSymbolRefField } from './symbol-ref';
import { B2cTextInputBase } from './text-input-base';
import { isSymbolDeclValue, isSymbolRefValue, type B2cFieldValue } from './values';

/**
 * A Blocks2Cpp field's value as a project file stores it: a boolean (checkbox), a string (number,
 * text, dropdown, type), `{sym, name}` (declaration) or `{ref}` (reference). Null when the field has
 * no value yet (a declaration without a symbol ID, a reference with nothing chosen) or is not a
 * Blocks2Cpp field.
 */
export function readB2cField(field: Blockly.Field): B2cFieldValue | null {
  if (field instanceof B2cSymbolDeclField) {
    return field.getDecl();
  }
  if (field instanceof B2cSymbolRefField) {
    return field.getRef();
  }
  if (field instanceof B2cCheckboxField) {
    return field.getBoolean();
  }
  if (field instanceof B2cTextInputBase || field instanceof B2cOptionsFieldBase) {
    return field.getValue();
  }
  return null;
}

/**
 * Sets a Blocks2Cpp field from a project-file value. Returns false, and leaves the field unchanged,
 * when the value has the wrong shape for the field or breaks the project text rules, or when the
 * field is not a Blocks2Cpp field.
 */
export function writeB2cField(field: Blockly.Field, value: B2cFieldValue): boolean {
  if (field instanceof B2cSymbolDeclField) {
    return isSymbolDeclValue(value) && field.setDecl(value);
  }
  if (field instanceof B2cSymbolRefField) {
    return isSymbolRefValue(value) && field.setRef(value);
  }
  if (field instanceof B2cCheckboxField) {
    if (typeof value !== 'boolean') {
      return false;
    }
    field.setBoolean(value);
    return true;
  }
  if (field instanceof B2cTextInputBase || field instanceof B2cOptionsFieldBase) {
    if (typeof value !== 'string') {
      return false;
    }
    field.setValue(value);
    return field.getValue() === value;
  }
  return false;
}
