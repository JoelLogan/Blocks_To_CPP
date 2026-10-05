/**
 * `b2c_symbol_ref`: a reference to a variable or function, chosen from the symbols in scope
 * (docs/spec/03-block-language.md §3.6, 06 §6.5). Its project value is `{"ref": "sym_…"}`.
 *
 * - The options are computed when the menu opens, from the scope query at the block
 *   ({@link SymbolProvider.symbolsAt}), filtered by the field's `kinds`.
 * - The field always shows the current name of the symbol it refers to
 *   ({@link SymbolProvider.nameOf}), so a rename shows at once; a reference whose declaration is
 *   gone shows `missing (sym_…)`. The analyser's diagnostic (B2C-E0201–E0204) explains it.
 * - A reference that is out of scope stays selected and shown; it is never changed or dropped by
 *   the editor.
 *
 * Blockly's own value (`getValue()`) is the symbol ID, or null when nothing is chosen;
 * {@link B2cSymbolRefField.getRef} gives the project value.
 */
import * as Blockly from 'blockly/core';

import { isProjectId } from '../ids';
import { nameOf, symbolsAt, type SymbolInfo } from '../services';
import { visibleInvisibles } from '../text';
import { isSymbolRefValue, type SymbolRefValue } from './values';

/**
 * Which symbols a reference field lists:
 * - `variables`: variables, parameters and loop counters (getters, expression slots);
 * - `assignable`: the same without `const` variables (set, change, update, ask);
 * - `functions`: functions (calls).
 */
export type SymbolRefKinds = 'variables' | 'assignable' | 'functions';

/** Options of a reference field. */
export interface SymbolRefFieldConfig {
  /** Which symbols the menu lists. Default `variables`. */
  readonly kinds?: SymbolRefKinds;
  /** The initial reference. */
  readonly value?: SymbolRefValue;
  /** The accessible name of the open menu. */
  readonly ariaLabel?: string;
}

const KINDS: readonly SymbolRefKinds[] = ['variables', 'assignable', 'functions'];

/** Whether a symbol belongs in a menu of the given kinds. */
export function symbolMatchesKinds(symbol: SymbolInfo, kinds: SymbolRefKinds): boolean {
  switch (kinds) {
    case 'functions':
      return symbol.kind === 'function';
    case 'assignable':
      return symbol.kind !== 'function' && !(symbol.kind === 'variable' && symbol.isConst === true);
    case 'variables':
      return symbol.kind !== 'function';
  }
}

/** What a reference shows when its declaration is not known: `missing (sym_x)`. */
export function missingSymbolLabel(symId: string): string {
  return `missing (${symId})`;
}

/** The value of the menu entry shown when there is nothing to choose; it is never accepted. */
const NOTHING = '';

/** A symbol reference field. */
export class B2cSymbolRefField extends Blockly.FieldDropdown {
  /** Which symbols the menu lists. */
  readonly kinds: SymbolRefKinds;
  private menuLabel: string;

  constructor(value: SymbolRefValue | null = null, config?: SymbolRefFieldConfig) {
    super(Blockly.Field.SKIP_SETUP);
    this.kinds =
      config?.kinds !== undefined && KINDS.includes(config.kinds) ? config.kinds : 'variables';
    this.menuLabel =
      config?.ariaLabel ??
      (this.kinds === 'functions' ? 'Functions in scope' : 'Variables in scope');
    this.maxDisplayLength = 40;
    // The generator runs again each time the menu opens, so the list is always current.
    this.setOptions(function (this: Blockly.FieldDropdown): Blockly.MenuOption[] {
      return (this as B2cSymbolRefField).menuOptions();
    });
    if (value !== null) {
      this.setRef(value);
    }
  }

  /** The reference, or null when nothing is chosen. */
  getRef(): SymbolRefValue | null {
    const value = this.getValue();
    return value === null ? null : { ref: value };
  }

  /** Sets the reference; false (and no change) when it is not a reference a project accepts. */
  setRef(value: SymbolRefValue): boolean {
    if (!isSymbolRefValue(value)) {
      return false;
    }
    this.setValue(value.ref);
    return true;
  }

  /**
   * The block and input whose scope the menu lists. An expression shadow is never part of the
   * project, so its slot is asked for: the parent block with the shadow's input name.
   */
  scopeAnchor(): { blockId: string; input: string | null } | null {
    const block = this.getSourceBlock();
    if (block === null) {
      return null;
    }
    const parent = block.isShadow() ? block.getParent() : null;
    if (parent !== null) {
      return { blockId: parent.id, input: parent.getInputWithBlock(block)?.name ?? null };
    }
    return { blockId: block.id, input: null };
  }

  /** The menu: the matching symbols in scope, the current reference first when it is not one of them. */
  menuOptions(): Blockly.MenuOption[] {
    const anchor = this.scopeAnchor();
    const symbols =
      anchor === null
        ? []
        : symbolsAt(anchor.blockId, anchor.input).filter((symbol) =>
            symbolMatchesKinds(symbol, this.kinds),
          );
    const options: Blockly.MenuOption[] = [];
    const seenNames = new Map<string, number>();
    const current = this.getValue();
    let currentListed = false;
    for (const symbol of symbols) {
      if (!isProjectId(symbol.id) || typeof symbol.name !== 'string') {
        continue;
      }
      const label = visibleInvisibles(symbol.name, { lineBreaks: true });
      const count = (seenNames.get(label) ?? 0) + 1;
      seenNames.set(label, count);
      options.push([count === 1 ? label : `${label} (${String(count)})`, symbol.id]);
      currentListed ||= symbol.id === current;
    }
    if (current !== null && !currentListed) {
      options.unshift([this.labelFor(current), current]);
    }
    if (options.length === 0) {
      options.push([
        this.kinds === 'functions' ? 'no functions yet' : 'no variables here',
        NOTHING,
      ]);
    }
    return options;
  }

  /** The label for a symbol ID: its current name, or `missing (id)`. */
  labelFor(symId: string): string {
    const name = nameOf(symId);
    return name === null
      ? missingSymbolLabel(symId)
      : visibleInvisibles(name, { lineBreaks: true });
  }

  /** Accepts a symbol ID, or a `{ref}` value. Refuses everything else, including the empty entry. */
  protected override doClassValidation_(newValue?: unknown): string | null {
    if (isSymbolRefValue(newValue)) {
      return newValue.ref;
    }
    return isProjectId(newValue) ? newValue : null;
  }

  /** Always the current name, never a cached menu label. */
  protected override getText_(): string | null {
    const value = this.getValue();
    return value === null ? '?' : this.labelFor(value);
  }

  /** Opens the menu (Blockly checks the current reference) and gives it an accessible name. */
  protected override showEditor_(e?: MouseEvent): void {
    super.showEditor_(e);
    this.menu_?.getElement()?.setAttribute('aria-label', this.menuLabel);
  }

  /** Saves `{ref}` (or null) for Blockly's serialisation. */
  override saveState(): SymbolRefValue | null {
    return this.getRef();
  }

  /**
   * Loads what {@link saveState} saved, or a bare symbol ID. It does not compute the menu: loading
   * a project must not query the scope for every reference.
   */
  override loadState(state: unknown): void {
    const value = this.doClassValidation_(state);
    if (value !== null) {
      this.setValue(value);
    }
  }

  /** Builds the field from a block definition: `{type: 'b2c_symbol_ref', kinds?, value?: {ref}}`. */
  static override fromJson(options: Blockly.FieldConfig & SymbolRefFieldConfig): B2cSymbolRefField {
    return new this(isSymbolRefValue(options.value) ? options.value : null, options);
  }
}
