/**
 * The toolbox's blocks as Blockly flyout items (docs/spec/04-user-interface.md §4.2): a catalog
 * block with its toolbox preset, its catalog defaults shown as editable expression shadows
 * (03 §3.4), declarations named with a default name, and references preset to a symbol.
 *
 * A declaration gets its symbol ID when the block is created (in the flyout or on the canvas), never
 * in the item itself: every block created from an item declares a new symbol, and two builds of a
 * category for the same project give equal items, so the toolbox can tell that nothing changed.
 *
 * Inputs whose value comes from the catalog default are *absent* shadows: the project file leaves
 * them out until the user changes them, exactly like a block loaded from a file. Values a preset or
 * a symbol's type gives (`= 0` of a new variable, the prompt of *ask*, a call's arguments) are
 * explicit: they are saved.
 */
import {
  catalogBlock,
  exprShadowState,
  newId,
  type BlockDefJson,
  type InputDefJson,
  type SymbolDeclValue,
  type SymbolRefValue,
  type TokenJson,
} from '@blocks2cpp/blockly-ext';
import type * as Blockly from 'blockly/core';

/** The flyout item kind of the toolbox's blocks; `B2cBlockInflater` creates them. */
export const B2C_BLOCK_KIND = 'b2c_block';

/** A field value a preset may give: text, a flag, a declaration or a reference. */
export type PresetFieldValue = string | boolean | SymbolDeclValue | SymbolRefValue;

/** What a toolbox entry gives its block instead of the catalog defaults. */
export interface BlockPreset {
  /** Field values by field name. */
  readonly fields?: Readonly<Record<string, PresetFieldValue>>;
  /** Mutator state (`extra`) by key: counts and flags; keys left out take the catalog default. */
  readonly extra?: Readonly<Record<string, number | boolean>>;
  /** Input tokens by input name (a numbered name such as `ARG0` for a repeated input). */
  readonly inputs?: Readonly<Record<string, readonly TokenJson[]>>;
  /**
   * Declarations by field name (`symbol_decl` fields only): the name each declares. Every block
   * created from the item declares a new symbol with that name.
   */
  readonly declare?: Readonly<Record<string, string>>;
}

/** A block in a toolbox flyout (kind {@link B2C_BLOCK_KIND}). */
export interface B2cBlockInfo extends Blockly.utils.toolbox.BlockInfo {
  kind: typeof B2C_BLOCK_KIND;
  type: string;
  /**
   * Labels shown before a call's arguments (`n:`), one per argument, from the parameter names.
   * They are only shown in the flyout: a block on the canvas gets them from the analysis.
   */
  argLabels?: readonly string[];
  /** The names the block's declaration fields declare, by field name (see `BlockPreset.declare`). */
  declares?: Readonly<Record<string, string>>;
}

/** A preset block could not be described: an unknown block type (a catalog/toolbox mismatch). */
export class PresetError extends Error {
  override readonly name = 'PresetError';
}

/** The catalog definition of `type`. */
export function requireBlockDef(type: string): BlockDefJson {
  const def = catalogBlock(type);
  if (def === undefined) {
    throw new PresetError(`The toolbox names the block ${type}, which the catalog does not have.`);
  }
  return def;
}

/** The block's mutator state: every catalog `extra` key with the preset's value or its default. */
function presetExtra(
  def: BlockDefJson,
  extra: BlockPreset['extra'],
): Record<string, unknown> | undefined {
  if (def.extra.length === 0) {
    return undefined;
  }
  const state: Record<string, unknown> = {};
  for (const key of def.extra) {
    const value = extra?.[key.name];
    switch (key.kind) {
      case 'count':
        state[key.name] = typeof value === 'number' ? value : (key.default ?? key.min ?? 0);
        break;
      case 'flag':
        state[key.name] = typeof value === 'boolean' ? value : key.default === true;
        break;
      case 'params':
        // New definitions start without parameters; the ⊕ button adds them.
        state[key.name] = [];
        break;
    }
  }
  return state;
}

/** The value inputs the block has with `extra`, by name: repeated inputs give one per copy. */
export function valueInputs(
  def: BlockDefJson,
  extra: Readonly<Record<string, unknown>> | undefined,
): { readonly name: string; readonly input: InputDefJson }[] {
  const inputs: { name: string; input: InputDefJson }[] = [];
  for (const input of def.inputs) {
    if (input.repeat === null) {
      inputs.push({ name: input.name, input });
      continue;
    }
    const count = extra?.[input.repeat.count];
    const copies = (typeof count === 'number' ? count : 0) + input.repeat.plus;
    for (let copy = 0; copy < copies; copy += 1) {
      inputs.push({ name: `${input.name}${String(copy)}`, input });
    }
  }
  return inputs;
}

/**
 * The expression shadows of the block's value inputs: a preset value is explicit, a catalog
 * default absent; an input with neither stays empty.
 */
function presetInputs(
  def: BlockDefJson,
  extra: Readonly<Record<string, unknown>> | undefined,
  inputs: BlockPreset['inputs'],
): Record<string, Blockly.serialization.blocks.ConnectionState> | undefined {
  const states: Record<string, Blockly.serialization.blocks.ConnectionState> = {};
  for (const { name, input } of valueInputs(def, extra)) {
    const preset = inputs !== undefined && Object.hasOwn(inputs, name) ? inputs[name] : undefined;
    const tokens = preset ?? input.default;
    if (tokens.length === 0) {
      continue;
    }
    states[name] = { shadow: exprShadowState(tokens, false, input.check, preset === undefined) };
  }
  return Object.keys(states).length === 0 ? undefined : states;
}

/**
 * The declarations of a preset, checked against the block: each must name a `symbol_decl` field.
 *
 * @throws PresetError for any other field (a toolbox/catalog mismatch).
 */
function presetDeclarations(
  def: BlockDefJson,
  declare: BlockPreset['declare'],
): Record<string, string> | undefined {
  if (declare === undefined) {
    return undefined;
  }
  const out: Record<string, string> = {};
  for (const [field, name] of Object.entries(declare)) {
    if (
      !def.fields.some((candidate) => candidate.name === field && candidate.kind === 'symbol_decl')
    ) {
      throw new PresetError(`The block ${def.id} has no declaration field ${field}.`);
    }
    out[field] = name;
  }
  return Object.keys(out).length === 0 ? undefined : out;
}

/** The block's field values: the preset's, for fields the block has. */
function presetFields(
  def: BlockDefJson,
  fields: BlockPreset['fields'],
): Record<string, PresetFieldValue> | undefined {
  if (fields === undefined) {
    return undefined;
  }
  const states: Record<string, PresetFieldValue> = {};
  for (const field of def.fields) {
    if (Object.hasOwn(fields, field.name)) {
      const value = fields[field.name];
      if (value !== undefined) {
        states[field.name] = value;
      }
    }
  }
  return Object.keys(states).length === 0 ? undefined : states;
}

/**
 * The flyout item for a catalog block with a preset.
 *
 * @throws PresetError when `type` is not a catalog block.
 */
export function presetBlock(
  type: string,
  preset: BlockPreset = {},
  argLabels?: readonly string[],
): B2cBlockInfo {
  const def = requireBlockDef(type);
  const extraState = presetExtra(def, preset.extra);
  const info: B2cBlockInfo = { kind: B2C_BLOCK_KIND, type };
  const fields = presetFields(def, preset.fields);
  if (fields !== undefined) {
    info.fields = fields;
  }
  const declares = presetDeclarations(def, preset.declare);
  if (declares !== undefined) {
    info.declares = declares;
  }
  if (extraState !== undefined) {
    info.extraState = extraState;
  }
  const inputs = presetInputs(def, extraState, preset.inputs);
  if (inputs !== undefined) {
    info.inputs = inputs;
  }
  if (argLabels !== undefined && argLabels.length > 0) {
    info.argLabels = argLabels;
  }
  return info;
}

/**
 * The field values a new block of a toolbox item starts with: the item's, plus a declaration with a
 * new symbol ID for each name it declares. Each call declares new symbols.
 */
export function instanceFields(info: B2cBlockInfo): Record<string, unknown> | undefined {
  if (info.declares === undefined) {
    return info.fields;
  }
  const fields: Record<string, unknown> = { ...info.fields };
  for (const [field, name] of Object.entries(info.declares)) {
    const decl: SymbolDeclValue = { sym: newId('sym'), name };
    fields[field] = decl;
  }
  return fields;
}

/** The Blockly block state of a toolbox block, to create it on a canvas (new symbols each call). */
export function blockState(info: B2cBlockInfo): Blockly.serialization.blocks.State {
  const state: Blockly.serialization.blocks.State = { type: info.type };
  const fields = instanceFields(info);
  if (fields !== undefined) {
    state.fields = fields;
  }
  if (info.extraState !== undefined) {
    state.extraState = info.extraState as unknown;
  }
  if (info.inputs !== undefined) {
    state.inputs = info.inputs;
  }
  return state;
}

/** Field values from a toolbox entry's preset (generated from catalog/toolbox.toml). */
export function catalogPresetFields(
  fields: Readonly<Record<string, unknown>> | undefined,
): Record<string, PresetFieldValue> {
  const out: Record<string, PresetFieldValue> = {};
  for (const [name, value] of Object.entries(fields ?? {})) {
    if (typeof value === 'string' || typeof value === 'boolean') {
      out[name] = value;
    }
  }
  return out;
}

/** Mutator state from a toolbox entry's preset: counts and flags. */
export function catalogPresetExtra(
  extra: Readonly<Record<string, unknown>> | undefined,
): Record<string, number | boolean> {
  const out: Record<string, number | boolean> = {};
  for (const [name, value] of Object.entries(extra ?? {})) {
    if (typeof value === 'number' || typeof value === 'boolean') {
      out[name] = value;
    }
  }
  return out;
}
