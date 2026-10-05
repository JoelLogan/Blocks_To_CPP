/**
 * The Blocks2Cpp Blockly fields, one per catalog field kind (docs/spec/03-block-language.md §3.6,
 * 05 §5.4). They render user text only as SVG text, and their editors are DOM inputs created with
 * the DOM API (docs/security/custom-field-review-checklist.md).
 */
export { readB2cField, writeB2cField } from './access';
export { B2cCheckboxField, type CheckboxFieldConfig } from './checkbox';
export { B2cDropdownField, type DropdownFieldConfig } from './dropdown';
export { NUMBER_ENTRY_PATTERN, B2cNumberField, type NumberFieldConfig } from './number';
export { B2cOptionsFieldBase, isOptionList, type OptionPair } from './options-base';
export { createCatalogField, registerB2cFields, type CatalogFieldOptions } from './register';
export { B2cSymbolDeclField, type SymbolDeclFieldConfig } from './symbol-decl';
export {
  B2cSymbolRefField,
  missingSymbolLabel,
  symbolMatchesKinds,
  type SymbolRefFieldConfig,
  type SymbolRefKinds,
} from './symbol-ref';
export { B2cTextField, type TextFieldConfig } from './text';
export { B2cTextInputBase, MAX_FIELD_DISPLAY_CHARS, type TextEntryConfig } from './text-input-base';
export { B2cTypeField, type TypeFieldConfig } from './type';
export {
  FIELD_TYPE,
  MAX_NAME_CHARS,
  NAME_ENTRY_PATTERN,
  displayTypeName,
  isStorableName,
  isSymbolDeclValue,
  isSymbolRefValue,
  type B2cFieldType,
  type B2cFieldValue,
  type SymbolDeclValue,
  type SymbolRefValue,
} from './values';
