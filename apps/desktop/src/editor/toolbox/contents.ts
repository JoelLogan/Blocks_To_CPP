/**
 * What each toolbox category shows (docs/spec/04-user-interface.md §4.2, 03 §3.7), built from the
 * generated toolbox metadata (`TOOLBOX`, from catalog/toolbox.toml) and, for the categories that
 * depend on the project, from the latest analysis. None of them depends on the selection (decision
 * D2 of the M2 polish plan), so two builds for the same project give equal items:
 *
 * - **Program**: *when program starts* while the project has none, otherwise a note saying where
 *   the program starts; then the category's other entries;
 * - **Variables**: *Make a variable*, the `create` block, one *set*, and, while a variable of a
 *   whole-number, decimal or character type exists, one *change* (whose menu also gives the other
 *   update operators), each preset to the first variable of `main` it fits (by name), or else to
 *   the first one of the module it fits; then a getter for each variable of the module, grouped
 *   by the function that declares it (`main` first, then the functions by name) and sorted by name
 *   ({@link variableGroups}), at most {@link MAX_LISTED_GETTERS}, then a note of how many more
 *   there are (03 §3.6, decision D3);
 * - **Functions**: the catalog entries, then, under *Your functions*, a call block for every
 *   function of the module shown (calls across modules are `B2C-E0206` in M2), or a hint when it
 *   has none; a module heading names the module when the project has more than one.
 *
 * The other categories are static. A block that declares something (`create`, the counted loop's
 * counter, `define`) shows the first default name of its kind; it gets a free name where it lands
 * on the canvas (./landing.ts). Every text shown comes from the catalog or the project and is
 * rendered by Blockly as SVG text (never HTML); project names show hidden characters as visible
 * placeholders (M2 decision "Hidden characters in block text fields"). No heading reads like a
 * category's name: the continuous toolbox finds a category's place in the flyout by that text.
 */
import type { SymbolInfo } from '@blocks2cpp/b2c-core-wasm';
import {
  MAX_VARIADIC_PARTS,
  TOOLBOX,
  catalogBlock,
  catalogFieldOptions,
  symbolMatchesKinds,
  toolboxCategoryStyle,
  truncateForDisplay,
  visibleInvisibles,
  type CategoryId,
  type TokenJson,
  type ToolboxCategoryJson,
  type ToolboxEntryJson,
} from '@blocks2cpp/blockly-ext';
import type * as Blockly from 'blockly/core';

import { candidateName } from './names';
import {
  catalogPresetExtra,
  catalogPresetFields,
  presetBlock,
  type B2cBlockInfo,
  type BlockPreset,
  type PresetFieldValue,
} from './presets';
import { FUNCTION_TYPE, MAIN_TYPE, bySymbolOrder, declaredName, type SymbolSource } from './scope';
import { startTokensForDeclaredType, startTokensForStaticType } from './values';

/** The dynamic category of Program (*when program starts* only while the project has none). */
export const PROGRAM_CATEGORY = 'B2C_PROGRAM';
/** The dynamic category of Variables (`registerToolboxCategoryCallback`). */
export const VARIABLES_CATEGORY = 'B2C_VARIABLES';
/** The dynamic category of Functions, which holds the call blocks under *Your functions*. */
export const MY_BLOCKS_CATEGORY = 'B2C_MY_BLOCKS';
/** The flyout button that starts *Make a variable*. */
export const MAKE_VARIABLE_BUTTON = 'B2C_MAKE_VARIABLE';

/** The text of the *Make a variable* button. */
export const MAKE_VARIABLE_TEXT = 'Make a variable';
/** The heading above the call blocks (D11 of the M2 polish plan). */
export const YOUR_FUNCTIONS_HEADING = 'Your functions';
/**
 * What the Functions category says under its heading while the module has no function: two short
 * lines, so that each stays readable in the flyout at its narrowest (it cuts off longer text).
 */
export const NO_FUNCTIONS_HINT: readonly string[] = Object.freeze([
  'No functions yet.',
  'Define one above.',
]);

/** The gap between the lines of a two-line note, which read as one note. */
const HINT_LINE_GAP = 4;

/** The toolbox item kind of the toolbox's categories (`B2cToolboxCategory`). */
export const B2C_CATEGORY_KIND = 'b2c_category';

/** The most getters the Variables category lists (decision D2 of the M2 polish plan). */
export const MAX_LISTED_GETTERS = 50;
/** The most functions *Your functions* lists. */
export const MAX_LISTED_FUNCTIONS = 200;
/** The longest project name a label shows, in characters (longer ones end with `…`). */
export const MAX_LABEL_NAME_CHARS = 40;

/** The program block's friendly label (`when program starts`), from the catalog. */
export const PROGRAM_LABEL = catalogBlock(MAIN_TYPE)?.label.friendly ?? 'when program starts';

/** The block that changes a number variable: by, down by, times, divided by or mod an amount. */
const CHANGE_TYPE = 'var.change';
/** The block that gives a variable a value. */
const SET_TYPE = 'var.set';
/** The block that reads a variable. */
const GET_TYPE = 'var.get';
/** The reference field of the variable blocks. */
const VAR_FIELD = 'VAR';
/** The value input of *set*. */
const VALUE_INPUT = 'VALUE';
/** The value types *change* is offered for (decision D3). */
const NUMBER_TYPES: ReadonlySet<string> = new Set(['int', 'double', 'char']);

/** The categories whose contents the toolbox builds when they are shown. */
const DYNAMIC_CATEGORY: Readonly<Partial<Record<CategoryId, string>>> = Object.freeze({
  program: PROGRAM_CATEGORY,
  variables: VARIABLES_CATEGORY,
  functions: MY_BLOCKS_CATEGORY,
});

/** A module of the open project: its ID and name, in document order. */
export interface ModuleInfo {
  readonly id: string;
  readonly name: string;
}

/** What the dynamic categories read when they are built. */
export interface ContentsContext {
  /** The canvas of the module shown. */
  readonly workspace: Blockly.Workspace;
  /** The symbols of the latest analysis. */
  readonly symbols: SymbolSource;
  /** The project's modules in document order (for the module headings). */
  modules(): readonly ModuleInfo[];
  /** The ID of the module the canvas shows, whose variables and functions are listed. */
  activeModuleId(): string;
  /**
   * The ID of the module that has *when program starts* (the module shown when its canvas has
   * one), or `null` while the project has none.
   */
  programModuleId(): string | null;
}

/** A flyout label (Blockly draws it as SVG text). */
function label(text: string): Blockly.utils.toolbox.LabelInfo {
  return { kind: 'label', text, id: undefined };
}

/** A gap between groups of blocks in a flyout. */
function separator(gap: number): Blockly.utils.toolbox.SeparatorInfo {
  return { kind: 'sep', gap, id: undefined, cssconfig: undefined };
}

/** A note of several short lines that read as one. */
function note(lines: readonly string[]): Blockly.utils.toolbox.FlyoutItemInfo[] {
  return lines.flatMap((line, index) =>
    index === 0 ? [label(line)] : [separator(HINT_LINE_GAP), label(line)],
  );
}

/** A project name as a label shows it: hidden characters visible, long names cut. */
export function displayName(name: string): string {
  return truncateForDisplay(visibleInvisibles(name), MAX_LABEL_NAME_CHARS);
}

/** The toolbox metadata of a category. */
function categoryJson(id: CategoryId): ToolboxCategoryJson | undefined {
  return TOOLBOX.find((category) => category.id === id);
}

/** The declaration field each block with a default name has, and the kind of name. */
const DEFAULT_NAMED: Readonly<Record<string, { field: string }>> = Object.freeze({
  'var.declare': { field: 'NAME' },
  'control.for_range': { field: 'VAR' },
  'func.define': { field: 'NAME' },
});

/**
 * The names declarations show in the toolbox: the first default name of each kind. A block gets a
 * free name where it lands on the canvas (./landing.ts), so the flyout does not depend on where
 * the block will go.
 */
const FIRST_NAMES: Readonly<Record<string, string>> = Object.freeze({
  'var.declare': candidateName('variable', 0),
  'control.for_range': candidateName('loop', 0),
  'func.define': candidateName('function', 0),
});

/** The block of one catalog entry, its declaration (if any) named with the first default name. */
function entryBlock(entry: ToolboxEntryJson): B2cBlockInfo {
  const fields: Record<string, PresetFieldValue> = catalogPresetFields(entry.preset?.fields);
  const named = DEFAULT_NAMED[entry.block];
  const name = FIRST_NAMES[entry.block];
  const declare =
    named !== undefined && name !== undefined && !Object.hasOwn(fields, named.field)
      ? { [named.field]: name }
      : undefined;
  const preset: BlockPreset = {
    fields,
    extra: catalogPresetExtra(entry.preset?.extra),
    ...(entry.preset?.inputs === undefined ? {} : { inputs: entry.preset.inputs }),
    ...(declare === undefined ? {} : { declare }),
  };
  return presetBlock(entry.block, preset);
}

/** The flyout items of a category's catalog entries. */
function entryItems(category: ToolboxCategoryJson): Blockly.utils.toolbox.FlyoutItemInfo[] {
  const items: Blockly.utils.toolbox.FlyoutItemInfo[] = [];
  for (const entry of category.entries) {
    if (entry.label !== null) {
      items.push(label(entry.label));
    }
    items.push(entryBlock(entry));
  }
  return items;
}

/** The catalog entry of the `create` block (Variables), or a bare one. */
function createEntry(): ToolboxEntryJson {
  return (
    categoryJson('variables')?.entries.find((item) => item.block === 'var.declare') ?? {
      block: 'var.declare',
      label: null,
      preset: null,
    }
  );
}

/**
 * The `create` block *Make a variable* inserts: the Variables category's, of type `type` (a
 * `var.declare` TYPE value, `int` by default) with that type's start value, declaring `name` (with
 * a new symbol ID, from `blockState`).
 */
export function newVariableBlock(name: string, type = 'int'): B2cBlockInfo {
  const entry = createEntry();
  const start = startTokensForDeclaredType(type);
  const fields: Record<string, PresetFieldValue> = {
    ...catalogPresetFields(entry.preset?.fields),
    TYPE: type,
  };
  return presetBlock(entry.block, {
    fields,
    extra: catalogPresetExtra(entry.preset?.extra),
    ...(start === null ? {} : { inputs: { VALUE: start } }),
    declare: { NAME: name },
  });
}

/**
 * The contents of a category without the project: its catalog entries, with the first default name
 * for each declaration. The dynamic categories use it before the toolbox plugin is attached.
 */
export function staticCategoryContents(id: CategoryId): Blockly.utils.toolbox.FlyoutItemInfo[] {
  const category = categoryJson(id);
  return category === undefined ? [] : entryItems(category);
}

/** The function that declares a variable, as the Variables category groups it. */
export type VariableOwner =
  /** *when program starts*. */
  | { readonly kind: 'main' }
  /** A function definition (its block ID and name). */
  | { readonly kind: 'function'; readonly blockId: string; readonly name: string }
  /** Neither: a declaration that is not (or no longer) inside `main` or a function. */
  | { readonly kind: 'other' };

/** The variables one function declares, sorted by name, then ID. */
export interface VariableGroup {
  readonly owner: VariableOwner;
  readonly symbols: readonly SymbolInfo[];
}

const MAIN_OWNER: VariableOwner = Object.freeze({ kind: 'main' });
const OTHER_OWNER: VariableOwner = Object.freeze({ kind: 'other' });

/** The function that declares `symbol`, from where its declaring block is on the canvas. */
function ownerOf(
  workspace: Blockly.Workspace,
  symbol: SymbolInfo,
  functionNames: ReadonlyMap<string, string>,
): VariableOwner {
  const root = workspace.getBlockById(symbol.declBlock)?.getRootBlock() ?? null;
  if (root?.type === MAIN_TYPE) {
    return MAIN_OWNER;
  }
  if (root?.type === FUNCTION_TYPE) {
    return {
      kind: 'function',
      blockId: root.id,
      name: functionNames.get(root.id) ?? declaredName(root) ?? '',
    };
  }
  return OTHER_OWNER;
}

/** The order of groups: `main`, then the functions by name (then block ID), then the others. */
function byOwnerOrder(a: VariableGroup, b: VariableGroup): number {
  const rank = (owner: VariableOwner): number =>
    owner.kind === 'main' ? 0 : owner.kind === 'function' ? 1 : 2;
  const difference = rank(a.owner) - rank(b.owner);
  if (difference !== 0 || a.owner.kind !== 'function' || b.owner.kind !== 'function') {
    return difference;
  }
  const [x, y] = [a.owner, b.owner];
  if (x.name !== y.name) {
    return x.name < y.name ? -1 : 1;
  }
  return x.blockId < y.blockId ? -1 : x.blockId > y.blockId ? 1 : 0;
}

/**
 * Every variable of module `moduleId` the latest analysis knows (variables, parameters and loop
 * counters), grouped by the function that declares it: `main` first, then the functions by name,
 * then anything else; each group sorted by name, then ID. Empty groups are left out.
 */
export function variableGroups(
  workspace: Blockly.Workspace,
  source: SymbolSource,
  moduleId: string,
): VariableGroup[] {
  const all = source.allSymbols();
  const functionNames = new Map(
    all.filter((symbol) => symbol.kind === 'function').map((fn) => [fn.declBlock, fn.name]),
  );
  const groups = new Map<string, { owner: VariableOwner; symbols: SymbolInfo[] }>();
  for (const symbol of all) {
    if (symbol.module !== moduleId || !symbolMatchesKinds(symbol, 'variables')) {
      continue;
    }
    const owner = ownerOf(workspace, symbol, functionNames);
    const key = owner.kind === 'function' ? `function:${owner.blockId}` : owner.kind;
    const group = groups.get(key) ?? { owner, symbols: [] };
    group.symbols.push(symbol);
    groups.set(key, group);
  }
  return [...groups.values()]
    .map((group) => ({ owner: group.owner, symbols: group.symbols.sort(bySymbolOrder) }))
    .sort(byOwnerOrder);
}

/** The heading of a group of getters: *In when program starts*, *In greet*. */
export function groupHeading(owner: VariableOwner): string | null {
  switch (owner.kind) {
    case 'main':
      return `In ${PROGRAM_LABEL}`;
    case 'function':
      return `In ${displayName(owner.name)}`;
    case 'other':
      return null;
  }
}

/** The note after the getters when there are more variables than the category lists. */
export function moreVariablesNote(count: number): string {
  return `…and ${String(count)} more`;
}

/**
 * Whether block `type`'s variable menu lists `symbol` (03 §3.6) and, for *change*, whether it
 * holds a whole number, a decimal number or a character (decision D3): the variables a generic
 * block of the toolbox may be preset to, and re-targeted to where it is dropped (./landing.ts).
 */
export function fitsGenericBlock(type: string, symbol: SymbolInfo): boolean {
  const kinds = catalogFieldOptions(type, VAR_FIELD).kinds ?? 'variables';
  return symbolMatchesKinds(symbol, kinds) && (type === SET_TYPE || NUMBER_TYPES.has(symbol.type));
}

/** The variable a generic block is preset to: the first of `main` it fits, else of the module. */
function presetVariable(type: string, groups: readonly VariableGroup[]): SymbolInfo | null {
  const main = groups.find((group) => group.owner.kind === 'main');
  return (
    main?.symbols.find((symbol) => fitsGenericBlock(type, symbol)) ??
    groups.flatMap((group) => group.symbols).find((symbol) => fitsGenericBlock(type, symbol)) ??
    null
  );
}

/**
 * A generic block of the Variables category (*set*, *change*), preset to `symbol`, or to no
 * variable: the toolbox chose it, so a drop may re-target it.
 */
function genericItem(type: string, symbol: SymbolInfo | null): B2cBlockInfo {
  const start = type === SET_TYPE && symbol !== null ? startTokensForStaticType(symbol.type) : null;
  return presetBlock(type, {
    ...(symbol === null ? {} : { fields: { [VAR_FIELD]: { ref: symbol.id } } }),
    ...(start === null ? {} : { inputs: { [VALUE_INPUT]: start } }),
    retarget: VAR_FIELD,
  });
}

/** The Variables category (see the module comment). */
export function variablesContents(
  context: ContentsContext,
): Blockly.utils.toolbox.FlyoutItemInfo[] {
  const items: Blockly.utils.toolbox.FlyoutItemInfo[] = [
    { kind: 'button', text: MAKE_VARIABLE_TEXT, callbackkey: MAKE_VARIABLE_BUTTON },
  ];
  const category = categoryJson('variables');
  if (category !== undefined) {
    items.push(...entryItems(category));
  }
  const groups = variableGroups(context.workspace, context.symbols, context.activeModuleId());
  items.push(genericItem(SET_TYPE, presetVariable(SET_TYPE, groups)));
  const changed = presetVariable(CHANGE_TYPE, groups);
  if (changed !== null) {
    items.push(genericItem(CHANGE_TYPE, changed));
  }
  let listed = 0;
  let total = 0;
  for (const group of groups) {
    total += group.symbols.length;
    const shown = group.symbols.slice(0, Math.max(0, MAX_LISTED_GETTERS - listed));
    if (shown.length === 0) {
      continue;
    }
    const heading = groupHeading(group.owner);
    if (heading !== null) {
      items.push(label(heading));
    }
    for (const symbol of shown) {
      items.push(presetBlock(GET_TYPE, { fields: { [VAR_FIELD]: { ref: symbol.id } } }));
    }
    listed += shown.length;
  }
  if (total > listed) {
    items.push(label(moreVariablesNote(total - listed)));
  }
  return items;
}

/**
 * The groups of symbols a block still in the toolbox's flyout lists in its menus: the module's
 * variables grouped like the Variables category, then its functions (the menus keep what fits
 * them).
 */
export function toolboxSymbolGroups(context: ContentsContext): SymbolInfo[][] {
  const active = context.activeModuleId();
  const functions = context.symbols
    .allSymbols()
    .filter((symbol) => symbol.kind === 'function' && symbol.module === active)
    .sort(bySymbolOrder);
  const groups = variableGroups(context.workspace, context.symbols, active).map((group) => [
    ...group.symbols,
  ]);
  return functions.length === 0 ? groups : [...groups, functions];
}

/** What *Program* says instead of offering *when program starts* (see {@link programNote}). */
export const PROGRAM_HERE_NOTE: readonly string[] = Object.freeze([
  'Your program starts at',
  `“${PROGRAM_LABEL}”.`,
]);

/**
 * The note *Program* shows while the project has *when program starts*: where it is (this
 * module's canvas, or another module, named).
 */
export function programNote(programModuleId: string, context: ContentsContext): readonly string[] {
  if (programModuleId === context.activeModuleId()) {
    return PROGRAM_HERE_NOTE;
  }
  const name =
    context.modules().find((module) => module.id === programModuleId)?.name ?? programModuleId;
  return ['Your program starts in', `the module “${displayName(name)}”.`];
}

/**
 * The Program category: its catalog entries, with *when program starts* only while the project has
 * none (a second one is `B2C-E0406`); otherwise a note saying where the program starts.
 */
export function programContents(context: ContentsContext): Blockly.utils.toolbox.FlyoutItemInfo[] {
  const category = categoryJson('program');
  if (category === undefined) {
    return [];
  }
  const where = context.programModuleId();
  const items: Blockly.utils.toolbox.FlyoutItemInfo[] = [];
  for (const entry of category.entries) {
    if (entry.block === MAIN_TYPE && where !== null) {
      items.push(...note(programNote(where, context)));
      continue;
    }
    if (entry.label !== null) {
      items.push(label(entry.label));
    }
    items.push(entryBlock(entry));
  }
  return items;
}

/** The parameters of a function, in order, from the analysis. */
function parametersOf(
  fn: Extract<SymbolInfo, { kind: 'function' }>,
  byId: ReadonlyMap<string, SymbolInfo>,
): (SymbolInfo | undefined)[] {
  return fn.params.slice(0, MAX_VARIADIC_PARTS).map((id) => byId.get(id));
}

/** The call block of a function: a statement for `void`, a reporter otherwise. */
function callItem(
  fn: Extract<SymbolInfo, { kind: 'function' }>,
  byId: ReadonlyMap<string, SymbolInfo>,
): B2cBlockInfo {
  const params = parametersOf(fn, byId);
  const inputs: Record<string, readonly TokenJson[]> = {};
  params.forEach((param, index) => {
    const start = param === undefined ? null : startTokensForStaticType(param.type);
    if (start !== null) {
      inputs[`ARG${String(index)}`] = start;
    }
  });
  const labels = params.map((param) => (param === undefined ? '' : `${displayName(param.name)}:`));
  return presetBlock(
    fn.returns === 'void' ? 'func.call_stmt' : 'func.call',
    { fields: { FUNC: { ref: fn.id } }, extra: { argCount: params.length }, inputs },
    labels.some((text) => text !== '') ? labels : undefined,
  );
}

/** The heading that names the module whose functions *Your functions* lists. */
export function moduleHeading(name: string): string {
  return `Module: ${displayName(name)}`;
}

/**
 * The Functions category: its catalog entries, then *Your functions*: a call block for each
 * function of the module shown, or a hint while it has none. Calls to another module's functions
 * cannot be used in M2 (`B2C-E0206`), so they are not offered; with more than one module, a
 * heading names the module the calls belong to.
 */
export function functionsContents(
  context: ContentsContext,
): Blockly.utils.toolbox.FlyoutItemInfo[] {
  const items: Blockly.utils.toolbox.FlyoutItemInfo[] = [];
  const category = categoryJson('functions');
  if (category !== undefined) {
    items.push(...entryItems(category));
  }
  items.push(label(YOUR_FUNCTIONS_HEADING));
  const active = context.activeModuleId();
  const modules = context.modules();
  if (modules.length > 1) {
    items.push(
      label(moduleHeading(modules.find((module) => module.id === active)?.name ?? active)),
    );
  }
  const all = context.symbols.allSymbols();
  const functions = all
    .filter(
      (symbol): symbol is Extract<SymbolInfo, { kind: 'function' }> =>
        symbol.kind === 'function' && symbol.module === active,
    )
    .slice(0, MAX_LISTED_FUNCTIONS);
  if (functions.length === 0) {
    items.push(...note(NO_FUNCTIONS_HINT));
    return items;
  }
  const byId = new Map(all.map((symbol) => [symbol.id, symbol]));
  for (const fn of functions) {
    items.push(callItem(fn, byId));
  }
  return items;
}

/** What the dynamic categories read of a symbol: its ID, name, kind, type and kind's details. */
function symbolKey(symbol: SymbolInfo): unknown[] {
  const common = [symbol.id, symbol.name, symbol.kind, symbol.type];
  switch (symbol.kind) {
    case 'variable':
      return [...common, symbol.isConst];
    case 'parameter':
      return [...common, symbol.mode];
    case 'function':
      return [...common, symbol.params, symbol.returns];
    case 'loopVariable':
      return common;
  }
}

/**
 * What the dynamic categories show depends on, as one string: the module shown and the project's
 * modules, where the program starts, the module's variables (with their kinds, types and the
 * functions declaring them) and its functions (with their parameters and results). Equal keys give
 * equal contents, so the toolbox rebuilds its flyout only when the key changes (04 §4.2).
 */
export function dynamicContentsKey(context: ContentsContext): string {
  const active = context.activeModuleId();
  const groups = variableGroups(context.workspace, context.symbols, active).map((group) => [
    group.owner.kind === 'function' ? [group.owner.blockId, group.owner.name] : group.owner.kind,
    group.symbols.map(symbolKey),
  ]);
  const functions = context.symbols
    .allSymbols()
    .filter((symbol) => symbol.kind === 'function' && symbol.module === active)
    .map(symbolKey);
  return JSON.stringify([active, context.modules(), context.programModuleId(), groups, functions]);
}

/** The key a category definition carries so its toolbox item can find its icon. */
export const CATEGORY_KEY = 'b2cCategory';

/** A category definition of ours: Blockly's, plus the catalog category it shows. */
export type B2cCategoryInfo = Blockly.utils.toolbox.CategoryInfo & {
  readonly [CATEGORY_KEY]: CategoryId;
};

/** The category definition of one toolbox category, dynamic or static. */
function categoryDefinition(category: ToolboxCategoryJson, dynamic: boolean): B2cCategoryInfo {
  const custom = dynamic ? DYNAMIC_CATEGORY[category.id] : undefined;
  const common = {
    kind: B2C_CATEGORY_KIND,
    name: category.name,
    // Blockly makes a unique ID; a fixed one could repeat in the document (two canvases).
    id: undefined,
    categorystyle: toolboxCategoryStyle(category.id),
    colour: undefined,
    cssconfig: undefined,
    hidden: undefined,
    [CATEGORY_KEY]: category.id,
  };
  if (custom === undefined) {
    return { ...common, contents: staticCategoryContents(category.id) };
  }
  // Blockly's type for a dynamic category has no `name`, but its toolbox item reads one.
  return { ...common, custom };
}

/** The catalog category a toolbox category definition shows, or `null` for one not of ours. */
export function categoryOfDefinition(definition: unknown): CategoryId | null {
  if (typeof definition !== 'object' || definition === null || !(CATEGORY_KEY in definition)) {
    return null;
  }
  const id: unknown = (definition as Record<string, unknown>)[CATEGORY_KEY];
  return TOOLBOX.find((category) => category.id === id)?.id ?? null;
}

/**
 * The full toolbox: every category in catalog order (03 §3.7), with Program, Variables and
 * Functions built when they are shown. The dynamic categories need their callbacks registered on
 * the workspace (the toolbox plugin does that) before the toolbox shows them.
 */
export function toolboxDefinition(): Blockly.utils.toolbox.ToolboxInfo {
  return {
    kind: 'categoryToolbox',
    contents: TOOLBOX.map((category) => categoryDefinition(category, true)),
  };
}

/**
 * The toolbox to inject a workspace with, before the toolbox plugin is attached: the same
 * categories, all static (dynamic ones show their catalog entries only), so injecting needs no
 * callbacks. The plugin replaces it with {@link toolboxDefinition}.
 */
export function initialToolboxDefinition(): Blockly.utils.toolbox.ToolboxInfo {
  return {
    kind: 'categoryToolbox',
    contents: TOOLBOX.map((category) => categoryDefinition(category, false)),
  };
}
