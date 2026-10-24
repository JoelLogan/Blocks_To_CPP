/**
 * Field value shapes as a project file stores them (docs/spec/05-project-format.md §5.4–5.5), and
 * the rules a field applies to them.
 */
import { isProjectId } from '../ids';
import { codePointCount, isCleanFieldText } from '../text';

/** The registry names of the Blocks2Cpp fields, one per catalog field kind. */
export const FIELD_TYPE = Object.freeze({
  symbolDecl: 'b2c_symbol_decl',
  symbolRef: 'b2c_symbol_ref',
  type: 'b2c_type',
  number: 'b2c_number',
  text: 'b2c_text',
  dropdown: 'b2c_dropdown',
  checkbox: 'b2c_checkbox',
} as const);

/** A Blocks2Cpp field registry name. */
export type B2cFieldType = (typeof FIELD_TYPE)[keyof typeof FIELD_TYPE];

/** A declaration: `{"sym": "sym_x", "name": "score"}`. The `sym` never changes by editing. */
export interface SymbolDeclValue {
  readonly sym: string;
  readonly name: string;
}

/** A reference to a symbol by ID: `{"ref": "sym_x"}`. */
export interface SymbolRefValue {
  readonly ref: string;
}

/** Any field value of a project file. */
export type B2cFieldValue = boolean | string | SymbolDeclValue | SymbolRefValue;

/** The longest name a declaration may have, in code points (05 §5.6, `MAX_IDENT_LEN`). */
export const MAX_NAME_CHARS = 64;

/**
 * What a user may type as a name: ASCII letters, digits and `_`, 1–64 characters
 * (docs/spec/03-block-language.md §3.6). Keywords, `__`, a leading digit or underscore and reserved
 * names are allowed here and reported by the analyser on the field (B2C-E0220), with a reason.
 */
export const NAME_ENTRY_PATTERN = /^[A-Za-z0-9_]{1,64}$/;

/**
 * Whether a name may be stored: the loader's rules (text rules and at most 64 code points). Names
 * from a file can be anything the loader accepts; only typing is limited to
 * {@link NAME_ENTRY_PATTERN}.
 */
export function isStorableName(name: unknown): name is string {
  return (
    typeof name === 'string' && isCleanFieldText(name) && codePointCount(name) <= MAX_NAME_CHARS
  );
}

/** Whether `value` is a declaration a project file accepts. */
export function isSymbolDeclValue(value: unknown): value is SymbolDeclValue {
  if (typeof value !== 'object' || value === null || Array.isArray(value)) {
    return false;
  }
  const keys = Object.keys(value);
  if (keys.length !== 2 || !Object.hasOwn(value, 'sym') || !Object.hasOwn(value, 'name')) {
    return false;
  }
  const { sym, name } = value as { sym: unknown; name: unknown };
  return isProjectId(sym) && isStorableName(name);
}

/** Whether `value` is a reference a project file accepts. */
export function isSymbolRefValue(value: unknown): value is SymbolRefValue {
  if (typeof value !== 'object' || value === null || Array.isArray(value)) {
    return false;
  }
  const keys = Object.keys(value);
  return (
    keys.length === 1 && Object.hasOwn(value, 'ref') && isProjectId((value as { ref: unknown }).ref)
  );
}

/** The labels type dropdowns show (docs/spec/03-block-language.md §3.5.2): `std::string` reads `string`. */
export function displayTypeName(type: string): string {
  return type === 'std::string' ? 'string' : type;
}
