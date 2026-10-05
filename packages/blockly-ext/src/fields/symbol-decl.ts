/**
 * `b2c_symbol_decl`: the name of a declaration (var.declare NAME, control.for_range VAR,
 * func.define NAME, parameter rows). Its project value is `{"sym": "sym_…", "name": "score"}`
 * (docs/spec/05-project-format.md §5.5).
 *
 * The field edits the name; the symbol ID belongs to the block instance and never changes by
 * editing, so references (which store the ID) follow a rename at once. Blockly's own value
 * (`getValue()`) is the name, which keeps Blockly's editor, events and undo working as for any text
 * field; {@link B2cSymbolDeclField.getDecl} and {@link B2cSymbolDeclField.setDecl} read and write the
 * whole declaration, and Blockly's serialisation (copy, duplicate, undo of a delete) saves both.
 */
import type * as Blockly from 'blockly/core';

import { newId } from '../ids';
import { B2cTextInputBase, type TextEntryConfig } from './text-input-base';
import {
  NAME_ENTRY_PATTERN,
  isStorableName,
  isSymbolDeclValue,
  type SymbolDeclValue,
} from './values';

/** Options of a declaration field. */
export interface SymbolDeclFieldConfig extends TextEntryConfig {
  /** The initial declaration. */
  readonly value?: SymbolDeclValue;
}

/** A declaration name field. */
export class B2cSymbolDeclField extends B2cTextInputBase {
  /** The declared symbol's ID; null until the field gets a declaration or the user names it. */
  private sym: string | null = null;

  constructor(value: SymbolDeclValue | null = null, config?: SymbolDeclFieldConfig) {
    super(null, 'Name', config);
    this.setSpellcheck(false);
    if (value !== null) {
      this.setDecl(value);
    }
  }

  /** The declaration, or null while the field has no symbol ID yet. */
  getDecl(): SymbolDeclValue | null {
    const name = this.getValue();
    return this.sym === null || name === null ? null : { sym: this.sym, name };
  }

  /**
   * Sets the whole declaration (loading, paste, ID remapping). Returns false, and changes nothing,
   * when the value is not a declaration a project file accepts.
   */
  setDecl(value: SymbolDeclValue): boolean {
    if (!isSymbolDeclValue(value)) {
      return false;
    }
    const previous = this.sym;
    this.sym = value.sym;
    this.setValue(value.name);
    if (this.getValue() !== value.name) {
      // Refused (only possible while the editor is open and the name breaks the entry rule).
      this.sym = previous;
      return false;
    }
    return true;
  }

  /** The symbol ID, or null. */
  getSymbolId(): string | null {
    return this.sym;
  }

  protected acceptsEntry(text: string): boolean {
    return NAME_ENTRY_PATTERN.test(text);
  }

  protected override doClassValidation_(newValue?: unknown): string | null {
    const text = super.doClassValidation_(newValue);
    return text !== null && isStorableName(text) ? text : null;
  }

  protected override doValueUpdate_(newValue: string): void {
    super.doValueUpdate_(newValue);
    // A name typed into a block that has no declaration yet starts a new symbol.
    if (this.sym === null && this.isBeingEdited_) {
      this.sym = newId('sym');
    }
  }

  /** Saves `{sym, name}` (or null without a symbol ID) for Blockly's serialisation. */
  override saveState(): SymbolDeclValue | null {
    return this.getDecl();
  }

  /** Loads what {@link saveState} saved. Anything else is ignored. */
  override loadState(state: unknown): void {
    if (isSymbolDeclValue(state)) {
      this.setDecl(state);
    }
  }

  /** Builds the field from a block definition: `{type: 'b2c_symbol_decl', value?: {sym, name}}`. */
  static override fromJson(
    options: Blockly.FieldConfig & SymbolDeclFieldConfig,
  ): B2cSymbolDeclField {
    const value = isSymbolDeclValue(options.value) ? options.value : null;
    return new this(value, options);
  }
}
