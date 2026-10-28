/**
 * `b2c_type`: a type picked from the block's catalog `types` (docs/spec/03-block-language.md §3.5).
 * In M2 the labels are the Friendly names: `std::string` reads `string`; the stored value is the
 * catalog's type name, so files do not change when the C++ label mode arrives (M3).
 *
 * After its `types`, a field lists the module's own types that its class allows
 * (docs/plans/m3-contracts/w2.md §2.3): the `value` class takes structs and enums, `struct` only
 * structs, `enum` only enums, each by name, sorted, from the latest analysis
 * ({@link SymbolProvider.userTypes}); then, where the class allows a list (`value`), a list of each
 * (`list of Student`). The menu is worked out each time it opens; a user type is stored as its
 * term's key (`{"ref":"sym_…"}`) and shown by its current name, so a rename shows at once.
 *
 * A type field whose pattern's class no listed type fits may list none (`types = []`, w2.md §2.1,
 * *new … with*'s struct): its menu then holds only the module's own types, and until one is chosen
 * it shows {@link NO_TYPE_LABEL} and holds no value (the empty text, which a project file never
 * stores).
 */
import * as Blockly from 'blockly/core';

import { blockLabelMode } from '../blocks/labelMode';
import { blockDef } from '../checker/catalog';
import type { TypeClass } from '../generated/catalog';
import { getEditorServices, nameOf, symbolsAt, type SymbolInfo } from '../services';
import { NO_FACTS, termKey, type TypeFactsTs, type UserTypeTs } from '../types/static-type';
import { visibleInvisibles } from '../text';
import { B2cOptionsFieldBase, type OptionPair } from './options-base';
import { createTypePicker, type PickerContext, type TypePicker } from './type-picker';
import { displayTypeName } from './values';

/** Options of a type field. */
export interface TypeFieldConfig {
  /** The type names offered, as the catalog lists them. */
  readonly types: readonly string[];
  /** The initial type (the first one when absent). */
  readonly value?: string;
  /** The accessible name of the open menu. */
  readonly ariaLabel?: string;
  /**
   * The class of the types the field takes besides its `types` (a mutator's row: the row extra's
   * `typeClass`); without it, the class of the field's pattern in its block's catalog definition.
   */
  readonly typeClass?: TypeClass | null;
}

function isTypeList(types: unknown): types is readonly string[] {
  return Array.isArray(types) && types.every((type) => typeof type === 'string');
}

/** What a type field that lists no types shows while nothing is chosen. */
export const NO_TYPE_LABEL = 'choose a struct first';

/** Which of the module's own types a class lets a type field list (w2.md §2.3). */
export interface UserTypeChoice {
  readonly structs: boolean;
  readonly enums: boolean;
  /** A list of each listed type, too (`std::vector<Student>`). */
  readonly lists: boolean;
}

/** The user types a class lists, or `null` for none. */
export function userTypeChoice(cls: TypeClass | null | undefined): UserTypeChoice | null {
  switch (cls) {
    case 'value':
      return { structs: true, enums: true, lists: true };
    case 'struct':
      return { structs: true, enums: false, lists: false };
    case 'enum':
      return { structs: false, enums: true, lists: false };
    default:
      return null;
  }
}

/** The term key of a user type (`{"ref":"sym_x"}`) and of a list of it. */
function userTypeKey(id: string, list: boolean): string {
  const ref = { kind: 'ref', id } as const;
  return termKey(list ? { kind: 'app', name: 'std::vector', args: [ref] } : ref);
}

/**
 * Facts that name user types by their current names (the document's, then the analysis's), so a
 * renamed struct shows its new name before the next analysis.
 */
function namingFacts(): TypeFactsTs {
  let facts: TypeFactsTs = NO_FACTS;
  try {
    facts = getEditorServices().types.facts?.() ?? NO_FACTS;
  } catch {
    // Without facts, names still come from the symbols.
  }
  return {
    userType(id: string): UserTypeTs | null {
      let known: UserTypeTs | null;
      try {
        known = facts.userType(id);
      } catch {
        known = null;
      }
      const name = nameOf(id);
      if (known !== null) {
        return name === null ? known : { ...known, name };
      }
      return name === null ? null : { name, kind: 'struct', comparable: false, printable: false };
    },
  };
}

/** A type field. */
export class B2cTypeField extends B2cOptionsFieldBase {
  /** Whether the catalog lists types for the field (w2.md §2.1: a field may list none). */
  readonly listsTypes: boolean;
  /** The fixed options: the catalog's types, or {@link NO_TYPE_LABEL} for a field that lists none. */
  private readonly fixed: readonly OptionPair[];
  /** The class given by the creator; `undefined` reads it from the block's definition. */
  private readonly givenClass: TypeClass | null | undefined;
  /** The menu last worked out, for Blockly's cached reads. */
  private lastOptions: Blockly.MenuOption[] | null = null;

  constructor(
    types: readonly string[],
    value: string | null = null,
    ariaLabel = 'Type',
    typeClass?: TypeClass | null,
  ) {
    const fixed: readonly OptionPair[] =
      types.length === 0
        ? [[NO_TYPE_LABEL, '']]
        : types.map((type) => [displayTypeName(type), type] as const);
    super(fixed, value, ariaLabel);
    this.fixed = fixed;
    this.listsTypes = types.length > 0;
    this.givenClass = typeClass;
  }

  /**
   * Whether the field holds a type: always for a field that lists types (a project value is kept
   * as it is); for one that lists none, when something other than the empty text is chosen.
   */
  hasType(): boolean {
    return this.listsTypes || (this.getValue() ?? '') !== '';
  }

  /** The class of the other types the field takes: given, or its pattern's in the block's definition. */
  typeClass(): TypeClass | null {
    if (this.givenClass !== undefined) {
      return this.givenClass;
    }
    const block = this.getSourceBlock();
    const def = block === null ? undefined : blockDef(block.type);
    const pattern = def?.fields.find((field) => field.name === this.name)?.type ?? null;
    if (def === undefined || pattern?.kind !== 'var') {
      return null;
    }
    return Object.hasOwn(def.typeVars, pattern.name) ? (def.typeVars[pattern.name] ?? null) : null;
  }

  /** The module's structs and enums, from the latest analysis. */
  private userTypes(): readonly SymbolInfo[] {
    let symbols: readonly SymbolInfo[] | null;
    try {
      symbols = getEditorServices().symbols.userTypes?.() ?? null;
    } catch {
      symbols = null;
    }
    if (symbols === null) {
      // The scope query lists the module's structs and enums at every block (types.md §2.4).
      const block = this.getSourceBlock();
      symbols = block === null || block.isInFlyout ? [] : symbolsAt(block.id, null);
    }
    return symbols.filter((symbol) => symbol.kind === 'struct' || symbol.kind === 'enum');
  }

  /**
   * The options for the user types the field's class allows (w2.md §2.3): sorted by name, then a
   * list of each where the class allows lists.
   */
  userTypeOptions(): OptionPair[] {
    const choice = userTypeChoice(this.typeClass());
    if (choice === null) {
      return [];
    }
    const listed = this.userTypes()
      .filter((symbol) => (symbol.kind === 'struct' ? choice.structs : choice.enums))
      .map((symbol) => ({ id: symbol.id, name: nameOf(symbol.id) ?? symbol.name }))
      .sort((a, b) => (a.name < b.name ? -1 : a.name > b.name ? 1 : a.id < b.id ? -1 : 1));
    const facts = namingFacts();
    const options: OptionPair[] = [];
    const seen = new Set(this.fixed.map(([, value]) => value));
    for (const list of choice.lists ? [false, true] : [false]) {
      for (const { id } of listed) {
        const key = userTypeKey(id, list);
        if (!seen.has(key)) {
          seen.add(key);
          options.push([visibleInvisibles(displayTypeName(key, facts), { lineBreaks: true }), key]);
        }
      }
    }
    return options;
  }

  /**
   * The menu: the catalog's types, then the module's own types ({@link userTypeOptions}). It is
   * worked out when the menu opens; a cached read (Blockly's, when a value is set) gives the last
   * menu, or the fixed options, so loading a project asks nothing of the analysis.
   */
  override getOptions(useCache?: boolean): Blockly.MenuOption[] {
    // The base class's constructor sets the options and the value before this class's own fields
    // exist: its fixed options are all there is then.
    // eslint-disable-next-line @typescript-eslint/no-unnecessary-condition -- see above.
    if (this.fixed === undefined) {
      return super.getOptions(useCache);
    }
    if (useCache === true) {
      return this.lastOptions ?? this.fixed.map(([label, value]) => [label, value]);
    }
    const users = this.userTypeOptions();
    const fixed = this.listsTypes || users.length === 0 ? this.fixed : [];
    const options: Blockly.MenuOption[] = [...fixed, ...users].map(([label, value]) => [
      label,
      value,
    ]);
    this.lastOptions = options;
    return options;
  }

  /**
   * What the type picker offers for this field (./type-picker/model.ts): its catalog types, the
   * module's structs and enums ({@link userTypes}, the symbol service's list) and the collections
   * of its class.
   */
  pickerContext(): PickerContext {
    return {
      typeClass: this.typeClass(),
      fixed: this.listsTypes ? this.fixed.map(([, value]) => value) : [],
      userTypes: this.userTypes().map((symbol) => ({
        id: symbol.id,
        name: nameOf(symbol.id) ?? symbol.name,
        kind: symbol.kind === 'enum' ? ('enum' as const) : ('struct' as const),
        module: symbol.module,
      })),
      facts: namingFacts(),
      mode: blockLabelMode(),
    };
  }

  /** The open picker, while there is one (tests). */
  picker: TypePicker | null = null;

  /**
   * Opens the type picker (docs/plans/m3-contracts/w4-editor-tools.md §2.2) in Blockly's drop-down
   * instead of a flat menu. A choice sets the value (one change, so one undo step); Escape or Tab
   * close it without a change, and the focus goes back to the field.
   */
  protected override showEditor_(): void {
    const block = this.getSourceBlock();
    if (block === null) {
      return;
    }
    // Blockly's drop-down takes any field; this one's value type is text.
    const field = this as Blockly.FieldDropdown;
    const close = (): void => {
      Blockly.DropDownDiv.hideIfOwner(field, true);
    };
    const picker = createTypePicker({
      context: this.pickerContext(),
      current: this.getValue(),
      label: this.menuLabel,
      document: block instanceof Blockly.BlockSvg ? block.getSvgRoot().ownerDocument : document,
      onChoose: (key) => {
        close();
        if (key !== this.getValue()) {
          this.setValue(key);
        }
      },
      onCancel: close,
    });
    this.picker = picker;
    Blockly.DropDownDiv.clearContent();
    Blockly.DropDownDiv.getContentDiv().append(picker.element);
    Blockly.DropDownDiv.setColour(
      'var(--b2c-surface-raised, #ffffff)',
      'var(--b2c-border, #c4c4c4)',
    );
    Blockly.DropDownDiv.showPositionedByField(field, () => {
      this.picker = null;
    });
    picker.focus();
  }

  /** A user type (a term key) by its current name; any other value as the base field shows it. */
  protected override getText_(): string | null {
    const value = this.getValue();
    if (value?.startsWith('{') === true) {
      return visibleInvisibles(displayTypeName(value, namingFacts()), { lineBreaks: true });
    }
    return super.getText_();
  }

  /**
   * Builds the field from a block definition: `{type: 'b2c_type', types: [...], value?,
   * typeClass?}`.
   *
   * @throws TypeError when `types` is not a list of text.
   */
  static override fromJson(
    options: Blockly.FieldConfig & {
      readonly types?: unknown;
      readonly value?: unknown;
      readonly ariaLabel?: unknown;
      readonly typeClass?: unknown;
    },
  ): B2cTypeField {
    if (!isTypeList(options.types)) {
      throw new TypeError('b2c_type needs "types": a list of type names.');
    }
    return new this(
      options.types,
      typeof options.value === 'string' ? options.value : null,
      typeof options.ariaLabel === 'string' ? options.ariaLabel : undefined,
      typeof options.typeClass === 'string' ? (options.typeClass as TypeClass) : undefined,
    );
  }
}
