/** Registration of the Blocks2Cpp fields with Blockly's field registry, and fields per catalog kind. */
import * as Blockly from 'blockly/core';

import type { FieldDefJson } from '../generated/catalog';
import { B2cCheckboxField } from './checkbox';
import { B2cDropdownField } from './dropdown';
import { B2cNumberField } from './number';
import { B2cSymbolDeclField } from './symbol-decl';
import { B2cSymbolRefField, type SymbolRefKinds } from './symbol-ref';
import { B2cTextField } from './text';
import { B2cTypeField } from './type';
import { FIELD_TYPE, type B2cFieldType } from './values';

const FIELD_CLASSES: readonly (readonly [B2cFieldType, Blockly.fieldRegistry.RegistrableField])[] =
  [
    [FIELD_TYPE.symbolDecl, B2cSymbolDeclField],
    [FIELD_TYPE.symbolRef, B2cSymbolRefField],
    [FIELD_TYPE.type, B2cTypeField],
    [FIELD_TYPE.number, B2cNumberField],
    [FIELD_TYPE.text, B2cTextField],
    [FIELD_TYPE.dropdown, B2cDropdownField],
    [FIELD_TYPE.checkbox, B2cCheckboxField],
  ];

/**
 * Registers the seven field types (`b2c_symbol_decl`, `b2c_symbol_ref`, `b2c_type`, `b2c_number`,
 * `b2c_text`, `b2c_dropdown`, `b2c_checkbox`), so block definitions and mutators can create them with
 * `Blockly.fieldRegistry.fromJson`. Idempotent.
 */
export function registerB2cFields(): void {
  for (const [name, fieldClass] of FIELD_CLASSES) {
    if (!Blockly.registry.hasItem(Blockly.registry.Type.FIELD, name)) {
      Blockly.fieldRegistry.register(name, fieldClass);
    }
  }
}

/** Per-field choices a block definition makes beyond the catalog field definition. */
export interface CatalogFieldOptions {
  /** For a `symbol_ref` field: which symbols it lists (default `variables`). */
  readonly kinds?: SymbolRefKinds;
  /** For a `text` field: exactly one character may be typed. */
  readonly singleChar?: boolean;
}

/**
 * A new field for a catalog field definition, holding the catalog default (fields without a default
 * start empty: a declaration with no symbol ID, a reference with nothing chosen).
 */
export function createCatalogField(
  def: FieldDefJson,
  options: CatalogFieldOptions = {},
): Blockly.Field {
  const textDefault = typeof def.default === 'string' ? def.default : null;
  switch (def.kind) {
    case 'symbol_decl':
      return new B2cSymbolDeclField(null);
    case 'symbol_ref':
      return new B2cSymbolRefField(
        null,
        options.kinds === undefined ? {} : { kinds: options.kinds },
      );
    case 'type':
      return new B2cTypeField(def.types, textDefault);
    case 'number':
      return new B2cNumberField(textDefault ?? '0');
    case 'text':
      return new B2cTextField(textDefault ?? '', { singleChar: options.singleChar === true });
    case 'dropdown':
      return new B2cDropdownField(def.options, textDefault);
    case 'checkbox':
      return new B2cCheckboxField(def.default === true);
  }
}
