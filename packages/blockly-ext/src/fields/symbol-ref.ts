/**
 * `b2c_symbol_ref`: a reference to a variable or function, chosen from the symbols in scope
 * (docs/spec/03-block-language.md §3.6, 06 §6.5). Its project value is `{"ref": "sym_…"}`.
 *
 * - The options are computed when the menu opens, from the scope query at the block
 *   ({@link SymbolProvider.symbolsAt}), filtered by the field's `kinds`. A block still in the
 *   toolbox's flyout is in no place of the program: it lists every symbol of the module shown, in
 *   the groups the toolbox gives ({@link toolboxSymbolGroups}), with a separator between groups.
 * - The field always shows the current name of the symbol it refers to
 *   ({@link SymbolProvider.nameOf}), so a rename shows at once; a reference whose declaration is
 *   gone shows `missing (sym_…)`. The analyser's diagnostic (B2C-E0201–E0204) explains it.
 * - A field with a type pattern in its block's signature (ADR-0018, slots.md §4.3) lists only the
 *   symbols whose type binds the pattern on its own (class and shape), without looking at the
 *   block's inputs, which are checked against the field and not the reverse. A type the editor
 *   cannot read, or whose class membership is not known, is listed.
 * - A `members` field (the struct field of *… of*, the value of *enum value*; slots.md §4.3,
 *   docs/plans/m3-contracts/w2.md §2.3) lists the members of the user type bound to its variable
 *   (from what is plugged in or chosen elsewhere on the block), in row order, whatever its kinds.
 *   While nothing binds the variable it lists nothing and shows *choose a struct first* (or *an
 *   enum*).
 * - A reference that is out of scope, or no longer of the field's kinds, stays selected and shown
 *   (first in its menu); this field never changes or drops it. (The editor's toolbox gives a
 *   *set* or *change* block just dragged from it another variable when the one it came with is
 *   not visible where it was dropped, docs/spec/04-user-interface.md §4.2.)
 *
 * Blockly's own value (`getValue()`) is the symbol ID, or null when nothing is chosen;
 * {@link B2cSymbolRefField.getRef} gives the project value.
 */
import * as Blockly from 'blockly/core';

import { blockDef } from '../checker/catalog';
import { checkerTypeOracle } from '../checker/oracle';
import { blockTyping } from '../checker/slots';
import { isProjectId } from '../ids';
import {
  getEditorServices,
  nameOf,
  symbolsAt,
  toolboxSymbolGroups,
  type SymbolInfo,
} from '../services';
import { blockSignature, TypeEnv, type TypePattern } from '../types/pattern';
import { NO_FACTS, parseType } from '../types/static-type';
import { visibleInvisibles } from '../text';
import { isSymbolRefValue, type SymbolRefValue } from './values';

/**
 * Which symbols a reference field lists (docs/spec/03-block-language.md §3.6):
 * - `variables`: variables, parameters and loop counters (getters, expression slots);
 * - `assignable`: what a block may change there: the same without `const` variables, `for` loop
 *   counters and read-only parameters (set, ask), which the analyser refuses (B2C-E0308);
 * - `numeric`: the assignable ones holding a whole number, a decimal number or a character, or of
 *   a type the analysis could not tell (change, the update operators): text cannot be counted with
 *   (B2C-E0302) and true/false only with a warning (B2C-W0519);
 * - `functions`: functions (calls);
 * - `enums`: enum definitions, not variables of an enum type (*enum value*'s `ENUM`,
 *   docs/plans/m3-contracts/w2.md §2.3).
 */
export type SymbolRefKinds = 'variables' | 'assignable' | 'numeric' | 'functions' | 'enums';

/** Options of a reference field. */
export interface SymbolRefFieldConfig {
  /** Which symbols the menu lists. Default `variables`. */
  readonly kinds?: SymbolRefKinds;
  /** The initial reference. */
  readonly value?: SymbolRefValue;
  /** The accessible name of the open menu. */
  readonly ariaLabel?: string;
}

const KINDS: readonly SymbolRefKinds[] = [
  'variables',
  'assignable',
  'numeric',
  'functions',
  'enums',
];

/** What the open menu is called, by kinds. */
const MENU_LABELS: Readonly<Record<SymbolRefKinds, string>> = {
  variables: 'Variables in scope',
  assignable: 'Variables in scope',
  numeric: 'Variables in scope',
  functions: 'Functions in scope',
  enums: 'Enums',
};

/** What the menu shows when there is nothing to choose, by kinds. */
const EMPTY_LABELS: Readonly<Record<SymbolRefKinds, string>> = {
  variables: 'no variables here',
  assignable: 'no variables here',
  numeric: 'no variables here',
  functions: 'no functions yet',
  enums: 'no enums yet',
};

/** The value types `change` and the update operators work on (`error`: not known). */
const NUMERIC_TYPES: ReadonlySet<string> = new Set(['int', 'double', 'char', 'error']);

/** Whether a block may change the symbol where it is visible (03 §3.6). */
function isAssignable(symbol: SymbolInfo): boolean {
  switch (symbol.kind) {
    case 'variable':
    case 'global':
      return symbol.isConst !== true;
    case 'parameter':
      return symbol.mode !== 'read_only';
    case 'loopVariable':
    case 'function':
    case 'struct':
    case 'field':
    case 'enum':
    case 'enumerator':
      return false;
  }
}

/** Whether a symbol belongs in a menu of the given kinds. */
export function symbolMatchesKinds(symbol: SymbolInfo, kinds: SymbolRefKinds): boolean {
  switch (kinds) {
    case 'functions':
      return symbol.kind === 'function';
    case 'enums':
      return symbol.kind === 'enum';
    case 'assignable':
      return isAssignable(symbol);
    case 'numeric':
      return (
        isAssignable(symbol) && typeof symbol.type === 'string' && NUMERIC_TYPES.has(symbol.type)
      );
    case 'variables':
      return isVariable(symbol);
  }
}

/** Whether a symbol holds a value a block can read: a variable, parameter, loop variable or global. */
function isVariable(symbol: SymbolInfo): boolean {
  return (
    symbol.kind === 'variable' ||
    symbol.kind === 'parameter' ||
    symbol.kind === 'loopVariable' ||
    symbol.kind === 'global'
  );
}

/** What a reference shows when its declaration is not known: `missing (sym_x)`. */
export function missingSymbolLabel(symId: string): string {
  return `missing (${symId})`;
}

/** The value of the menu entry shown when there is nothing to choose; it is never accepted. */
const NOTHING = '';

/** What a `members` field shows while nothing binds its variable, by the variable's class. */
export const NO_OWNER_LABELS = Object.freeze({
  struct: 'choose a struct first',
  enum: 'choose an enum first',
});

/** What a `members` field shows when its type has no members. */
const NO_MEMBER_LABELS = Object.freeze({ struct: 'no fields yet', enum: 'no values yet' });

/** A symbol reference field. */
export class B2cSymbolRefField extends Blockly.FieldDropdown {
  /** Which symbols the menu lists. */
  readonly kinds: SymbolRefKinds;
  private menuLabel: string;

  constructor(value: SymbolRefValue | null = null, config?: SymbolRefFieldConfig) {
    super(Blockly.Field.SKIP_SETUP);
    this.kinds =
      config?.kinds !== undefined && KINDS.includes(config.kinds) ? config.kinds : 'variables';
    this.menuLabel = config?.ariaLabel ?? MENU_LABELS[this.kinds];
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

  /**
   * The symbols the menu lists, in groups shown one after the other: for a block in the toolbox's
   * flyout, the toolbox's groups when it gives them; otherwise one group, what is in scope at the
   * {@link scopeAnchor}.
   */
  private symbolGroups(): readonly (readonly SymbolInfo[])[] {
    const groups = this.getSourceBlock()?.isInFlyout === true ? toolboxSymbolGroups() : null;
    if (groups !== null) {
      return groups;
    }
    const anchor = this.scopeAnchor();
    return anchor === null ? [] : [symbolsAt(anchor.blockId, anchor.input)];
  }

  /**
   * The field's own type pattern and its block's variables, when the block's signature gives it
   * one (not a `members` field), or `null`.
   */
  private typePattern(): { pattern: TypePattern; env: () => TypeEnv } | null {
    const block = this.getSourceBlock();
    const def = block === null ? undefined : blockDef(block.type);
    const sig = def === undefined ? null : blockSignature(def);
    const pattern = sig?.fields.get(this.name ?? '') ?? null;
    if (sig === null || pattern === null || sig.members.has(this.name ?? '')) {
      return null;
    }
    return { pattern, env: () => new TypeEnv(sig) };
  }

  /** Whether a symbol's type binds the field's pattern on its own (true when not known). */
  private static fitsPattern(
    symbol: SymbolInfo,
    typed: { pattern: TypePattern; env: () => TypeEnv },
  ): boolean {
    const type = parseType(symbol.kind === 'function' ? symbol.returns : symbol.type);
    if (type === null) {
      return true;
    }
    let facts = NO_FACTS;
    try {
      facts = getEditorServices().types.facts?.() ?? NO_FACTS;
    } catch {
      // Without facts, a user type's membership is not known, and it is listed.
    }
    const fit = typed.env().bind(typed.pattern, { kind: 'known', type }, { field: 'F' }, facts);
    return fit === 'fits' || fit === 'unknown';
  }

  /**
   * The type variable whose members the field lists, and whether it is an enum's, when the field
   * is a `members` field of its block's signature; otherwise `null`.
   */
  private membersOf(): { variable: string; kind: 'struct' | 'enum' } | null {
    const block = this.getSourceBlock();
    const def = block === null ? undefined : blockDef(block.type);
    const name = this.name ?? '';
    if (def === undefined || !Object.hasOwn(def.members, name)) {
      return null;
    }
    const variable = def.members[name];
    if (variable === undefined) {
      return null;
    }
    const cls = Object.hasOwn(def.typeVars, variable) ? def.typeVars[variable] : undefined;
    return { variable, kind: cls === 'enum' ? 'enum' : 'struct' };
  }

  /**
   * The members of the user type bound to `variable` at the block, in row order: `null` while
   * nothing binds it to a struct or enum the analysis knows.
   */
  private boundMembers(variable: string): readonly string[] | null {
    const block = this.getSourceBlock();
    if (block === null) {
      return null;
    }
    const oracle = checkerTypeOracle();
    let owner = null;
    try {
      owner = blockTyping(block, oracle)?.bound.get(variable)?.type ?? null;
    } catch {
      owner = null;
    }
    if (owner?.kind !== 'named') {
      return null;
    }
    let info: SymbolInfo | null;
    try {
      info = oracle.symbol?.(owner.id) ?? null;
    } catch {
      info = null;
    }
    if (info?.kind === 'struct') {
      return info.fields ?? [];
    }
    if (info?.kind === 'enum') {
      return info.values ?? [];
    }
    // Without the analysis's symbol, the scope query lists each member with its owner.
    const anchor = this.scopeAnchor();
    const found =
      anchor === null
        ? []
        : symbolsAt(anchor.blockId, anchor.input).filter(
            (symbol) =>
              (symbol.kind === 'field' || symbol.kind === 'enumerator') &&
              symbol.owner === owner.id,
          );
    return found.length === 0 ? null : found.map((symbol) => symbol.id);
  }

  /**
   * A `members` field's menu (w2.md §2.3): the members of the bound type in row order, the current
   * member first when it is not one of them (it stays shown, also when it no longer fits; the
   * analyser reports `B2C-E0207`), and *choose a struct first* while nothing is bound.
   */
  private memberOptions(of: { variable: string; kind: 'struct' | 'enum' }): Blockly.MenuOption[] {
    const options: Blockly.MenuOption[] = [];
    const members = this.boundMembers(of.variable);
    const current = this.getValue();
    const seenNames = new Map<string, number>();
    for (const id of members ?? []) {
      if (!isProjectId(id)) {
        continue;
      }
      const label = this.labelFor(id);
      const count = (seenNames.get(label) ?? 0) + 1;
      seenNames.set(label, count);
      options.push([count === 1 ? label : `${label} (${String(count)})`, id]);
    }
    if (current !== null && !(members ?? []).includes(current)) {
      options.unshift([this.labelFor(current), current]);
    }
    if (options.length === 0) {
      const empty = members === null ? NO_OWNER_LABELS[of.kind] : NO_MEMBER_LABELS[of.kind];
      options.push([empty, NOTHING]);
    }
    return options;
  }

  /**
   * The menu: the matching symbols in scope (for a block in the toolbox's flyout, the module's,
   * with a separator between groups), the current reference first when it is not one of them; for
   * a `members` field, the members of the bound type ({@link memberOptions}).
   */
  menuOptions(): Blockly.MenuOption[] {
    const members = this.membersOf();
    if (members !== null) {
      return this.memberOptions(members);
    }
    const options: Blockly.MenuOption[] = [];
    const seenNames = new Map<string, number>();
    const current = this.getValue();
    const typed = this.typePattern();
    let currentListed = false;
    for (const group of this.symbolGroups()) {
      const before = options.length;
      for (const symbol of group) {
        if (
          !isProjectId(symbol.id) ||
          typeof symbol.name !== 'string' ||
          !symbolMatchesKinds(symbol, this.kinds) ||
          (typed !== null && !B2cSymbolRefField.fitsPattern(symbol, typed))
        ) {
          continue;
        }
        if (before > 0 && options.length === before) {
          options.push(Blockly.FieldDropdown.SEPARATOR);
        }
        const label = visibleInvisibles(symbol.name, { lineBreaks: true });
        const count = (seenNames.get(label) ?? 0) + 1;
        seenNames.set(label, count);
        options.push([count === 1 ? label : `${label} (${String(count)})`, symbol.id]);
        currentListed ||= symbol.id === current;
      }
    }
    if (current !== null && !currentListed) {
      options.unshift([this.labelFor(current), current]);
    }
    if (options.length === 0) {
      options.push([EMPTY_LABELS[this.kinds], NOTHING]);
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
