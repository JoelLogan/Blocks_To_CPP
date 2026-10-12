/**
 * What each toolbox category shows (docs/spec/04-user-interface.md §4.2, 03 §3.7), built from the
 * generated toolbox metadata (`TOOLBOX`, from catalog/toolbox.toml) and, for the categories that
 * depend on the project, from the latest analysis:
 *
 * - **Variables**: *Make a variable*, the `create` block with a free name, and for each variable in
 *   scope at the selection a getter plus the blocks whose menu may choose it, preset to it: *set*
 *   for a variable that may be changed there, *change* and *update* for one of a number or
 *   character type (03 §3.6);
 * - **Loops**: the catalog entries, the counted loop with a free counter name (`i`, `j`, `k`);
 * - **Functions**: the catalog entries, `define` with a free function name, then, under *Your
 *   functions*, a call block for every function of the module shown (calls across modules are
 *   `B2C-E0206` in M2), or a hint when it has none; a module heading names the module when the
 *   project has more than one.
 *
 * The other categories are static. Every text shown comes from the catalog or the project and is
 * rendered by Blockly as SVG text (never HTML); project names show hidden characters as visible
 * placeholders (M2 decision "Hidden characters in block text fields"). No heading reads like a
 * category's name: the continuous toolbox finds a category's place in the flyout by that text.
 */
import type { SymbolInfo } from '@blocks2cpp/b2c-core-wasm';
import {
  MAX_VARIADIC_PARTS,
  TOOLBOX,
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

import { candidateName, firstFreeName } from './names';
import {
  catalogPresetExtra,
  catalogPresetFields,
  presetBlock,
  type B2cBlockInfo,
  type BlockPreset,
  type PresetFieldValue,
} from './presets';
import {
  resolveScopeAnchor,
  symbolsAtPoint,
  takenFunctionNames,
  takenLoopNames,
  takenVariableNames,
  type SymbolSource,
} from './scope';
import { startTokensForStaticType } from './values';

/** The dynamic category of Variables (`registerToolboxCategoryCallback`). */
export const VARIABLES_CATEGORY = 'B2C_VARIABLES';
/** The dynamic category of Loops (its counted loop gets a free counter name). */
export const LOOPS_CATEGORY = 'B2C_LOOPS';
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

/** The gap between the lines of {@link NO_FUNCTIONS_HINT}, which read as one hint. */
const HINT_LINE_GAP = 4;

/** The toolbox item kind of the toolbox's categories (`B2cToolboxCategory`). */
export const B2C_CATEGORY_KIND = 'b2c_category';

/** The most variables the Variables category lists (each gives up to four blocks). */
export const MAX_LISTED_VARIABLES = 100;
/** The most functions *Your functions* lists. */
export const MAX_LISTED_FUNCTIONS = 200;
/** The longest project name a label shows, in characters (longer ones end with `…`). */
export const MAX_LABEL_NAME_CHARS = 40;

/** The categories whose contents the toolbox builds when they are shown. */
const DYNAMIC_CATEGORY: Readonly<Partial<Record<CategoryId, string>>> = Object.freeze({
  variables: VARIABLES_CATEGORY,
  loops: LOOPS_CATEGORY,
  functions: MY_BLOCKS_CATEGORY,
});

/** The declaration field each block with a default name has, and the kind of name. */
const DEFAULT_NAMED: Readonly<Record<string, { field: string }>> = Object.freeze({
  'var.declare': { field: 'NAME' },
  'control.for_range': { field: 'VAR' },
  'func.define': { field: 'NAME' },
});

/** A module of the open project: its ID and name, in document order. */
export interface ModuleInfo {
  readonly id: string;
  readonly name: string;
}

/** What the dynamic categories read when they are built. */
export interface ContentsContext {
  /** The canvas whose blocks are selected and inserted into. */
  readonly workspace: Blockly.Workspace;
  /** The symbols of the latest analysis. */
  readonly symbols: SymbolSource;
  /** The selected block of the canvas, or `null`. */
  selected(): Blockly.Block | null;
  /** The project's modules in document order (for the module heading of *Your functions*). */
  modules(): readonly ModuleInfo[];
  /** The ID of the module the canvas shows, whose functions *Your functions* lists. */
  activeModuleId(): string;
}

/** A flyout label (Blockly draws it as SVG text). */
function label(text: string): Blockly.utils.toolbox.LabelInfo {
  return { kind: 'label', text, id: undefined };
}

/** A gap between groups of blocks in a flyout. */
function separator(gap: number): Blockly.utils.toolbox.SeparatorInfo {
  return { kind: 'sep', gap, id: undefined, cssconfig: undefined };
}

/** A project name as a label shows it: hidden characters visible, long names cut. */
export function displayName(name: string): string {
  return truncateForDisplay(visibleInvisibles(name), MAX_LABEL_NAME_CHARS);
}

/** The toolbox metadata of a category. */
function categoryJson(id: CategoryId): ToolboxCategoryJson | undefined {
  return TOOLBOX.find((category) => category.id === id);
}

/**
 * The flyout items of a category's catalog entries. Blocks that declare something declare the name
 * `names` gives for their declaration field (each block created from them with a new symbol ID).
 */
function entryItems(
  category: ToolboxCategoryJson,
  names: Readonly<Record<string, string>> = {},
): Blockly.utils.toolbox.FlyoutItemInfo[] {
  const items: Blockly.utils.toolbox.FlyoutItemInfo[] = [];
  for (const entry of category.entries) {
    if (entry.label !== null) {
      items.push(label(entry.label));
    }
    items.push(entryBlock(entry, names));
  }
  return items;
}

/** The block of one catalog entry. */
function entryBlock(
  entry: ToolboxEntryJson,
  names: Readonly<Record<string, string>>,
): B2cBlockInfo {
  const fields: Record<string, PresetFieldValue> = catalogPresetFields(entry.preset?.fields);
  const named = DEFAULT_NAMED[entry.block];
  const name = names[entry.block];
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

/**
 * The `create` block of the Variables category, declaring `name`: the block *Make a variable*
 * inserts (with a new symbol ID, from `blockState`).
 */
export function newVariableBlock(name: string): B2cBlockInfo {
  const entry = categoryJson('variables')?.entries.find((item) => item.block === 'var.declare');
  return entryBlock(entry ?? { block: 'var.declare', label: null, preset: null }, {
    'var.declare': name,
  });
}

/**
 * The names declarations get where nothing is known about the project: the first default name of
 * each kind.
 */
const FIRST_NAMES: Readonly<Record<string, string>> = Object.freeze({
  'var.declare': candidateName('variable', 0),
  'control.for_range': candidateName('loop', 0),
  'func.define': candidateName('function', 0),
});

/**
 * The contents of a category without the project: its catalog entries, with the first default name
 * for each declaration. The dynamic categories use it before the toolbox plugin is attached.
 */
export function staticCategoryContents(id: CategoryId): Blockly.utils.toolbox.FlyoutItemInfo[] {
  const category = categoryJson(id);
  return category === undefined ? [] : entryItems(category, FIRST_NAMES);
}

/** Whether the variable menu of block `type` lists `symbol` (03 §3.6), so a preset may name it. */
function menuLists(type: string, symbol: SymbolInfo): boolean {
  return symbolMatchesKinds(symbol, catalogFieldOptions(type, 'VAR').kinds ?? 'variables');
}

/**
 * The getter of one symbol, then each block that changes a variable and may change this one:
 * *set* with a start value of its type, and *change* and *update* for a number or a character.
 */
function variableItems(symbol: SymbolInfo): B2cBlockInfo[] {
  const ref = { ref: symbol.id };
  const items = [presetBlock('var.get', { fields: { VAR: ref } })];
  if (menuLists('var.set', symbol)) {
    const start = startTokensForStaticType(symbol.type);
    items.push(
      presetBlock('var.set', {
        fields: { VAR: ref },
        ...(start === null ? {} : { inputs: { VALUE: start } }),
      }),
    );
  }
  for (const type of ['var.change', 'var.update']) {
    if (menuLists(type, symbol)) {
      items.push(presetBlock(type, { fields: { VAR: ref } }));
    }
  }
  return items;
}

/** The Variables category, for the current selection. */
export function variablesContents(
  context: ContentsContext,
): Blockly.utils.toolbox.FlyoutItemInfo[] {
  const point = resolveScopeAnchor(context.workspace, context.selected(), context.symbols);
  const name = firstFreeName(
    'variable',
    takenVariableNames(context.workspace, point, context.symbols),
  );
  const items: Blockly.utils.toolbox.FlyoutItemInfo[] = [
    { kind: 'button', text: MAKE_VARIABLE_TEXT, callbackkey: MAKE_VARIABLE_BUTTON },
  ];
  const category = categoryJson('variables');
  if (category !== undefined) {
    items.push(...entryItems(category, { 'var.declare': name }));
  }
  const symbols = symbolsAtPoint(point, context.symbols)
    .filter((symbol) => symbolMatchesKinds(symbol, 'variables'))
    .slice(0, MAX_LISTED_VARIABLES);
  for (const symbol of symbols) {
    items.push(separator(32), ...variableItems(symbol));
  }
  return items;
}

/** The Loops category, for the current selection. */
export function loopsContents(context: ContentsContext): Blockly.utils.toolbox.FlyoutItemInfo[] {
  const category = categoryJson('loops');
  if (category === undefined) {
    return [];
  }
  const point = resolveScopeAnchor(context.workspace, context.selected(), context.symbols);
  const name = firstFreeName('loop', takenLoopNames(point, context.symbols));
  return entryItems(category, { 'control.for_range': name });
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
    const name = firstFreeName('function', takenFunctionNames(context.workspace, context.symbols));
    items.push(...entryItems(category, { 'func.define': name }));
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
    NO_FUNCTIONS_HINT.forEach((line, index) => {
      if (index > 0) {
        items.push(separator(HINT_LINE_GAP));
      }
      items.push(label(line));
    });
    return items;
  }
  const byId = new Map(all.map((symbol) => [symbol.id, symbol]));
  for (const fn of functions) {
    items.push(callItem(fn, byId));
  }
  return items;
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
 * The full toolbox: every category in catalog order (03 §3.7), with Variables, Loops and Functions
 * built when they are shown. The dynamic categories need their callbacks registered on the
 * workspace (the toolbox plugin does that) before the toolbox shows them.
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
