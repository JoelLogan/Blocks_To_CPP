/**
 * The fields a mutator creates itself: the type, name and mode of a parameter row, and the field
 * that joins repeated parts (`logic.operation`'s `OP`).
 *
 * They are created through Blockly's field registry under the blockly-ext names (`b2c_type`,
 * `b2c_symbol_decl`, `b2c_dropdown`), so they behave like the same fields elsewhere. When a name
 * is not registered (blockly-ext core not loaded, as in this module's own tests), Blockly's
 * built-in dropdown and text fields stand in. Every value is set before the field is attached to
 * the block, so creating a field fires no event.
 */
import * as Blockly from 'blockly/core';

import type { FieldDefJson } from '../generated/catalog';
import { PARAM_MODES, type ParamMode } from './types';

function isRegistered(type: string): boolean {
  return Blockly.registry.hasItem(Blockly.registry.Type.FIELD, type);
}

/** A field from the registry, or `null` when the type is not registered or refuses `config`. */
function fromRegistry(type: string, config: Record<string, unknown>): Blockly.Field | null {
  if (!isRegistered(type)) {
    return null;
  }
  try {
    return Blockly.fieldRegistry.fromJson({ ...config, type });
  } catch {
    // A field class that rejects this configuration: the built-in stand-in still works.
    return null;
  }
}

/** How a type is shown in Friendly mode: `std::string` as `string` (03 §3.5.2). */
export function friendlyTypeName(type: string): string {
  return type === 'std::string' ? 'string' : type;
}

/** How a parameter mode is shown (03 §3.7.7). */
export function friendlyModeName(mode: ParamMode): string {
  return mode === 'read_only' ? 'read-only' : mode;
}

/**
 * A field choosing one of `types` (a `b2c_type`), set to `value`, which may be a type term's key
 * that is not in the menu (a row's library or user type): the `b2c_type` field keeps it, and the
 * plain dropdown used without one gets it as an extra option.
 */
export function createTypeField(types: readonly string[], value: string): Blockly.Field {
  const options = types.includes(value) ? types : [...types, value];
  const field =
    fromRegistry('b2c_type', { types: [...types], value, ariaLabel: 'Parameter type' }) ??
    new Blockly.FieldDropdown(options.map((type) => [friendlyTypeName(type), type]));
  field.setValue(value);
  return field;
}

/** A name field and whether it is a declaration field (`b2c_symbol_decl`) or plain text. */
export interface NameField {
  readonly field: Blockly.Field;
  readonly holdsDecl: boolean;
}

/** A declaration as a project file stores it. */
interface Decl {
  readonly sym: string;
  readonly name: string;
}

/**
 * The declaration API of blockly-ext's `b2c_symbol_decl` field, whose Blockly value is the name
 * while `getDecl`/`setDecl` read and write the whole `{sym, name}`.
 */
interface DeclAccess {
  getDecl(): Decl | null;
  setDecl(value: Decl): boolean;
}

function hasDeclAccess(field: Blockly.Field): field is Blockly.Field & DeclAccess {
  const candidate = field as Partial<DeclAccess>;
  return typeof candidate.getDecl === 'function' && typeof candidate.setDecl === 'function';
}

/** A field declaring the symbol `sym` named `name` (a `b2c_symbol_decl`). */
export function createNameField(sym: string, name: string): NameField {
  const decl = fromRegistry('b2c_symbol_decl', { value: { sym, name } });
  if (decl !== null) {
    if (hasDeclAccess(decl)) {
      decl.setDecl({ sym, name });
    } else {
      decl.setValue({ sym, name });
    }
    return { field: decl, holdsDecl: true };
  }
  return { field: new Blockly.FieldTextInput(name), holdsDecl: false };
}

/**
 * The name a name field holds: from its declaration (`getDecl()`), or its value when that is the
 * name or a `{sym, name}` object.
 */
export function readName(field: NameField): string | null {
  if (hasDeclAccess(field.field)) {
    return field.field.getDecl()?.name ?? null;
  }
  const value: unknown = field.field.getValue();
  if (typeof value === 'string') {
    return value;
  }
  if (typeof value === 'object' && value !== null && 'name' in value) {
    const name: unknown = value.name;
    return typeof name === 'string' ? name : null;
  }
  return null;
}

/** A dropdown (a `b2c_dropdown`) with `[label, value]` options, set to `value`. */
export function createDropdownField(
  options: readonly (readonly [string, string])[],
  value: string,
  ariaLabel: string,
): Blockly.Field {
  const menu: [string, string][] = options.map(([label, option]) => [label, option]);
  const field =
    fromRegistry('b2c_dropdown', { options: menu, value, ariaLabel }) ??
    new Blockly.FieldDropdown(menu);
  field.setValue(value);
  return field;
}

/** The parameter-mode dropdown, set to `mode`. */
export function createModeField(mode: ParamMode): Blockly.Field {
  return createDropdownField(
    PARAM_MODES.map((option) => [friendlyModeName(option), option] as const),
    mode,
    'Parameter mode',
  );
}

/** The field that joins repeated parts, as its catalog definition describes it. */
export function createJoinField(def: FieldDefJson): Blockly.Field {
  const value = typeof def.default === 'string' ? def.default : (def.options[0]?.[1] ?? '');
  return createDropdownField(def.options, value, 'Operator');
}

/** The kinds of field that may be repeated (docs/plans/m3-contracts/w2.md §2.1). */
export const REPEATABLE_FIELD_KINDS: readonly FieldDefJson['kind'][] = [
  'dropdown',
  'checkbox',
  'type',
  'symbol_decl',
];

/**
 * A part of a repeated field (w2.md §2.1), holding the field's catalog default: a dropdown, a
 * checkbox, a type (`b2c_type`, which may list no types) or, for a declaration, the symbol `decl`.
 * Built-in fields stand in where the blockly-ext names are not registered.
 */
export function createMemberField(def: FieldDefJson, decl: Decl): Blockly.Field {
  const text = typeof def.default === 'string' ? def.default : null;
  switch (def.kind) {
    case 'symbol_decl':
      return createNameField(decl.sym, decl.name).field;
    case 'checkbox':
      return (
        fromRegistry('b2c_checkbox', { value: def.default === true }) ??
        new Blockly.FieldCheckbox(def.default === true)
      );
    case 'type': {
      const field =
        fromRegistry('b2c_type', {
          types: [...def.types],
          ...(text === null ? {} : { value: text }),
        }) ??
        new Blockly.FieldDropdown(
          def.types.length === 0
            ? [['', '']]
            : def.types.map((type): [string, string] => [friendlyTypeName(type), type]),
        );
      if (text !== null) {
        field.setValue(text);
      }
      return field;
    }
    default:
      return createDropdownField(def.options, text ?? def.options[0]?.[1] ?? '', 'Choice');
  }
}
