/**
 * A block taken from the toolbox, settled where it lands on the canvas
 * (docs/spec/04-user-interface.md §4.2, 03 §3.6). The toolbox does not depend on where a block
 * will go, so what depends on its place is chosen when it is dropped:
 *
 * - a declaration still named with the toolbox's default name (`value`, `i`, `myFunction`) gets
 *   the first default name that is free there (./names.ts, `takenNamesAtDrop`);
 * - a generic *set*, *change* or update block whose variable the toolbox chose (the user did not
 *   pick another one in its menu while it was in the toolbox) keeps that variable when it is
 *   visible there, and otherwise takes the first variable visible there that it fits, by name; the
 *   value of *set*, while it is still the start value of the old variable's type, becomes the new
 *   type's (./reshape.ts).
 *
 * This runs when the pointer drag that took the block out ends (`B2cBlockDragger.onDragEnd`,
 * ../sync/drag.ts), inside the drag's Blockly event group, so one Undo removes the block together
 * with these changes, and the next preview sees the block as it stays. Getters, and blocks that
 * were already on the canvas, are never changed: a getter whose variable is not visible where it
 * is dropped keeps it and shows `B2C-E0203`, and its menu offers the variables that are.
 *
 * Which blocks came from the toolbox: the inflater remembers the item each flyout block was built
 * from ({@link rememberToolboxItem}), and the flyout tells this module the block it creates from a
 * flyout block ({@link noteCreatedFromToolbox}). The note holds for the event group it was made
 * in, the gesture that took the block out: a block added another way (with the keyboard) and
 * dragged later is left as it is.
 */
import type { StaticType } from '@blocks2cpp/b2c-core-wasm';
import { B2cSymbolDeclField, B2cSymbolRefField } from '@blocks2cpp/blockly-ext';
import * as Blockly from 'blockly/core';

import { fitsGenericBlock } from './contents';
import { firstFreeName, type DefaultNameKind } from './names';
import type { B2cBlockInfo } from './presets';
import { reshapeSetValue } from './reshape';
import { symbolsAtConnection, takenNamesAtDrop, type SymbolSource } from './scope';

/** The kind of default name each block that declares something gets. */
const NAME_KINDS: Readonly<Record<string, DefaultNameKind>> = Object.freeze({
  'var.declare': 'variable',
  'control.for_range': 'loop',
  'func.define': 'function',
});

/** What a block created from the toolbox still has of its toolbox item. */
interface ToolboxOrigin {
  /** The Blockly event group the block was created in (the gesture that took it out). */
  readonly group: string;
  /** Its declaration fields still named with the toolbox's default name, and their kind. */
  readonly defaults: readonly { readonly field: string; readonly kind: DefaultNameKind }[];
  /** Its reference field whose variable the toolbox chose, or `null`. */
  readonly retarget: string | null;
}

/** The toolbox item each block of the flyout was built from. */
const ITEMS = new WeakMap<Blockly.Block, B2cBlockInfo>();

/** What each block just created from the toolbox still has of its item. */
const ORIGINS = new WeakMap<Blockly.Block, ToolboxOrigin>();

/** Remembers the toolbox item a block of the flyout was built from (the inflater calls this). */
export function rememberToolboxItem(block: Blockly.Block, info: B2cBlockInfo): void {
  ITEMS.set(block, info);
}

/** The symbol ID a preset gives a reference field, or `null`. */
function presetRef(value: unknown): string | null {
  if (typeof value !== 'object' || value === null || !('ref' in value)) {
    return null;
  }
  const ref: unknown = value.ref;
  return typeof ref === 'string' ? ref : null;
}

/** The name a declaration field of a block has, or `null`. */
function declarationName(block: Blockly.Block, field: string): string | null {
  const decl = block.getField(field);
  return decl instanceof B2cSymbolDeclField ? decl.getValue() : null;
}

/**
 * Takes note that `created` was just made on the canvas from `original`, a block of the toolbox's
 * flyout (the flyout calls this from `createBlock`): what of the toolbox item `original` was built
 * from it still has. A name or a variable the user changed in the flyout is the user's choice and
 * stays as it is.
 */
export function noteCreatedFromToolbox(original: Blockly.Block, created: Blockly.Block): void {
  const info = ITEMS.get(original);
  if (info === undefined) {
    return;
  }
  const kind = NAME_KINDS[info.type];
  const defaults =
    kind === undefined
      ? []
      : Object.entries(info.declares ?? {})
          .filter(([field, name]) => declarationName(original, field) === name)
          .map(([field]) => ({ field, kind }));
  const field = info.retarget;
  const retarget =
    field !== undefined &&
    (original.getFieldValue(field) as unknown) === presetRef(info.fields?.[field])
      ? field
      : null;
  if (defaults.length > 0 || retarget !== null) {
    ORIGINS.set(created, { group: Blockly.Events.getGroup(), defaults, retarget });
  }
}

/** The type of symbol `symId` in the latest analysis, or `null` when it does not know it. */
export function symbolType(source: SymbolSource, symId: string | null): StaticType | null {
  if (symId === null) {
    return null;
  }
  return source.allSymbols().find((symbol) => symbol.id === symId)?.type ?? null;
}

/** Gives declaration field `field` the first free name of `kind` where the block is, if needed. */
function giveFreeName(
  workspace: Blockly.Workspace,
  block: Blockly.Block,
  field: string,
  kind: DefaultNameKind,
  source: SymbolSource,
): void {
  const decl = block.getField(field);
  if (!(decl instanceof B2cSymbolDeclField)) {
    return;
  }
  const taken = takenNamesAtDrop(workspace, block, kind, source);
  const name = decl.getValue();
  if (name === null || taken.has(name)) {
    decl.setValue(firstFreeName(kind, taken));
  }
}

/**
 * Gives reference field `field` the first variable visible where the block is that the block fits,
 * by name, unless the one it has is visible there. *set*'s value follows the new variable's type
 * while it is still the old one's start value.
 */
function retarget(
  workspace: Blockly.Workspace,
  block: Blockly.Block,
  field: string,
  source: SymbolSource,
): void {
  const ref = block.getField(field);
  if (!(ref instanceof B2cSymbolRefField)) {
    return;
  }
  const visible = symbolsAtConnection(workspace, block, source);
  const current = ref.getValue();
  if (current !== null && visible.some((symbol) => symbol.id === current)) {
    return;
  }
  const next = visible.find((symbol) => fitsGenericBlock(block.type, symbol));
  if (next === undefined) {
    return;
  }
  const before = symbolType(source, current);
  ref.setValue(next.id);
  const reshaped = reshapeSetValue(block, before, next.type);
  if (reshaped !== null) {
    Blockly.Events.fire(reshaped);
  }
}

/**
 * Settles `block`, just dropped on `workspace`, if it came from the toolbox in the gesture whose
 * Blockly event group is open now (see the module comment). The changes are events of that group.
 * Anything else is left alone, and a block is settled at most once.
 */
export function settleDroppedBlock(
  workspace: Blockly.Workspace,
  block: Blockly.Block,
  source: SymbolSource,
): void {
  const origin = ORIGINS.get(block);
  if (origin === undefined) {
    return;
  }
  ORIGINS.delete(block);
  if (
    origin.group === '' ||
    origin.group !== Blockly.Events.getGroup() ||
    block.workspace !== workspace ||
    block.isDisposed()
  ) {
    return;
  }
  for (const { field, kind } of origin.defaults) {
    giveFreeName(workspace, block, field, kind, source);
  }
  if (origin.retarget !== null) {
    retarget(workspace, block, origin.retarget, source);
  }
}
