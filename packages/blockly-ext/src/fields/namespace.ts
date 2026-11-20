/**
 * `b2c_namespace`: the namespace a *use namespace* block nominates (docs/plans/m3-contracts/
 * w3-modules.md §2.9, §2.12). It stores text: a standard namespace as `::` and its name (`::std`),
 * which no symbol ID can be, or a user namespace's symbol ID. Codegen prints only the name the
 * analyser resolved, never the stored text.
 *
 * The menu offers `std` and the top-level namespaces of the module's canvas, each by name once
 * (one namespace may be several *namespace* blocks; the one with the lowest block ID stands for it,
 * as the analyser picks the first). A value the menu does not list, such as a namespace another
 * module declares or one that is gone, is kept and shown by the namespace's current name, or as it
 * is; the analyser reports a namespace that is gone or not top-level (`B2C-E0201`, `B2C-E0207`).
 * Package N2 widens the menu to the other modules' namespaces through the scope query.
 */
import type * as Blockly from 'blockly/core';

import { nameOf } from '../services';
import { B2cOptionsFieldBase, type OptionPair } from './options-base';
import { B2cSymbolDeclField } from './symbol-decl';

/** The stored value of the standard namespace `std`. */
export const STD_NAMESPACE_VALUE = '::std';

/** The block type of a namespace, whose `NAME` field declares it. */
const NAMESPACE_BLOCK = 'program.namespace';

/** The label of a stored standard name (`::std::chrono` → `std::chrono`), or `null`. */
function standardLabel(value: string): string | null {
  return value.startsWith('::') ? value.slice(2) : null;
}

/**
 * The top-level namespaces on a workspace as `[name, symbol ID]` pairs, sorted by name, each name
 * once (the block with the lowest ID stands for a name several blocks share).
 */
export function topLevelNamespaces(workspace: Blockly.Workspace): OptionPair[] {
  const byName = new Map<string, { readonly id: string; readonly sym: string }>();
  for (const block of workspace.getTopBlocks(false)) {
    if (block.type !== NAMESPACE_BLOCK || block.isInsertionMarker()) {
      continue;
    }
    const field = block.getField('NAME');
    const decl = field instanceof B2cSymbolDeclField ? field.getDecl() : null;
    if (decl === null || decl.name === '') {
      continue;
    }
    const known = byName.get(decl.name);
    if (known === undefined || block.id < known.id) {
      byName.set(decl.name, { id: block.id, sym: decl.sym });
    }
  }
  return [...byName]
    .sort(([a], [b]) => (a < b ? -1 : a > b ? 1 : 0))
    .map(([name, { sym }]) => [name, sym]);
}

/** The namespace a *use namespace* block names. */
export class B2cNamespaceField extends B2cOptionsFieldBase {
  constructor(value: string | null = STD_NAMESPACE_VALUE, menuLabel = 'Namespaces') {
    super([['std', STD_NAMESPACE_VALUE]], value, menuLabel);
  }

  /** `std`, the canvas's top-level namespaces, and the current value when it is neither. */
  override getOptions(useCache?: boolean): Blockly.MenuOption[] {
    const options: Blockly.MenuOption[] = [...super.getOptions(useCache)];
    const workspace = this.getSourceBlock()?.workspace;
    if (workspace !== undefined) {
      options.push(
        ...topLevelNamespaces(workspace).map(([name, sym]): Blockly.MenuOption => [name, sym]),
      );
    }
    const current = this.getValue();
    if (current !== null && !options.some((option) => option[1] === current)) {
      options.push([this.getText_() ?? current, current]);
    }
    return options;
  }

  /** A standard name without its leading `::`; a symbol ID as its namespace's current name. */
  protected override getText_(): string | null {
    const value = this.getValue();
    if (value === null) {
      return null;
    }
    const standard = standardLabel(value);
    if (standard !== null) {
      return standard;
    }
    const workspace = this.getSourceBlock()?.workspace;
    const listed =
      workspace === undefined
        ? undefined
        : topLevelNamespaces(workspace).find(([, sym]) => sym === value);
    return listed?.[0] ?? nameOf(value) ?? super.getText_();
  }

  /** Builds the field from a block definition: `{type: 'b2c_namespace', value?}`. */
  static override fromJson(
    options: Blockly.FieldConfig & { readonly value?: unknown },
  ): B2cNamespaceField {
    return new this(typeof options.value === 'string' ? options.value : STD_NAMESPACE_VALUE);
  }
}
