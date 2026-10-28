/**
 * The rows mutator (`b2c_mutator_params`, 03 §3.7.7, docs/plans/m3-contracts/w2.md §2.1): one row
 * per entry of the block's row extra, with ⊖ on each row and ⊕ after the last.
 *
 * - `func.define`'s `params` rows `[{sym, name, type, mode}, …]` are edited as a type (`b2c_type`),
 *   a name (`b2c_symbol_decl`) and a pass mode (`b2c_dropdown`); a new row is `int param`.
 * - A struct's `fields` rows `[{sym, name, type}, …]` have a type and a name; a new row is
 *   `int field`, then `field2`, … (w2.md §1.5).
 * - An enum's `values` rows `[{sym, name}, …]` have a name only; a new row is `Value`, then
 *   `Value2`, ….
 *
 * A row's fields are named `B2C_PARAM<i>_TYPE`, `_NAME` and `_MODE` (`B2C_FIELD<i>_…` for fields,
 * `B2C_VALUE<i>_NAME` for values); editing them records ordinary field changes, and adding or
 * removing a row records one mutation step with every row's values, so undo restores rows exactly.
 *
 * A block whose value inputs are counted by its rows (`func.define`'s `DEFAULT`, w2.md §2.8) gets
 * one such input per row, `DEFAULT<i>`, which holds the row's fields and draws `= (…)` at the end
 * of the row. The block in it stays with its row: when a row before it is added or removed, the
 * block moves to the row's new input; when its own row is removed, it is unplugged and kept on the
 * canvas, as the other mutators keep a user's block. Those moves belong to the same undo step. Its
 * shadow (a default the file holds as tokens) is kept by the controller and comes back with the row
 * when the removal is undone.
 */
import * as Blockly from 'blockly/core';

import { asUndoStep, canEdit } from './events';
import {
  buttonRow,
  createButton,
  keepingFocus,
  type MutatorController,
  type RowItem,
} from './controller';
import {
  createModeField,
  createNameField,
  createTypeField,
  type NameField,
  readName,
} from './fields';
import { mutatorHooks, randomSymbolId } from './hooks';
import { placeParts, silently } from './parts';
import type { RowKind, RowsSpec } from './spec';
import {
  isShowableName,
  MAX_VARIADIC_PARTS,
  readRowsExtra,
  rowTypeJson,
  rowTypeKey,
  writeRow,
} from './state';
import { partIndex } from '../checker/catalog';
import { PARAM_MODES, type ParamMode, ROW_FIELD_PREFIXES, type RowState } from './types';

/** The prefix of a parameter row's dummy input (`B2C_PARAM0`). */
export const PARAM_ROW_PREFIX = ROW_FIELD_PREFIXES.params;
/** The prefix of each kind of row's dummy input and fields (`B2C_FIELD0`, `B2C_VALUE0_NAME`). */
export const ROW_PREFIXES: Readonly<Record<RowKind, string>> = ROW_FIELD_PREFIXES;
/** The dummy input holding the ⊕ button. */
const BUTTONS_INPUT = 'B2C_BUTTONS';
/**
 * The name a new row of each kind starts with (then `param2`, `field2`, `Value2`, …), as the
 * editor's default names (w2.md §2.13).
 */
const NEW_ROW_NAMES: Readonly<Record<RowKind, string>> = Object.freeze({
  params: 'param',
  fields: 'field',
  values: 'Value',
});
/** What a row of each kind is called on its buttons. */
const ROW_NOUNS: Readonly<Record<RowKind, string>> = Object.freeze({
  params: 'parameter',
  fields: 'field',
  values: 'value',
});
/** The first word of each kind's button keys (`param:add`, `field:0:remove`). */
const BUTTON_KEYS: Readonly<Record<RowKind, string>> = Object.freeze({
  params: 'param',
  fields: 'field',
  values: 'value',
});
/** The aria label of each kind's type field. */
const TYPE_LABELS: Readonly<Record<RowKind, string>> = Object.freeze({
  params: 'Parameter type',
  fields: 'Field type',
  values: 'Value type',
});
/** Symbol IDs: `[A-Za-z0-9_]{1,32}` (05 §5.4). */
const SYMBOL_ID = /^[A-Za-z0-9_]{1,32}$/;
/** What a user may type as a name (03 §3.6); other names from files are kept but not typed. */
const NAME_ENTRY = /^[A-Za-z0-9_]{1,64}$/;

/** The user's block and the shadow of one value input of a row, kept while the rows are rebuilt. */
interface RowPart {
  readonly block: Blockly.Block | null;
  readonly shadow: Blockly.serialization.blocks.State | null;
}

/** A row's fields: a `values` row has no type, and only a `params` row has a mode. */
interface RowFields {
  readonly type: Blockly.Field | null;
  readonly name: NameField;
  readonly mode: Blockly.Field | null;
}

function isParamMode(value: unknown): value is ParamMode {
  return typeof value === 'string' && (PARAM_MODES as readonly string[]).includes(value);
}

/** The first of `base`, `base2`, `base3`, … no row uses (`param`, `field`, `Value`). */
function freshName(rows: readonly RowState[], base: string): string {
  const used = new Set(rows.map((row) => row.name));
  if (!used.has(base)) {
    return base;
  }
  let suffix = 2;
  while (used.has(`${base}${String(suffix)}`)) {
    suffix += 1;
  }
  return `${base}${String(suffix)}`;
}

/** A symbol ID for a new row, distinct from the rows' IDs. */
function freshSymbolId(rows: readonly RowState[]): string {
  const used = new Set(rows.map((row) => row.sym));
  const make = mutatorHooks().newSymbolId;
  for (let attempt = 0; attempt < 8; attempt += 1) {
    let candidate: unknown;
    try {
      candidate = make();
    } catch {
      break;
    }
    if (typeof candidate === 'string' && SYMBOL_ID.test(candidate) && !used.has(candidate)) {
      return candidate;
    }
  }
  // The editor's generator failed or misbehaved; fall back to the built-in one, which cannot
  // collide in practice (about 101 random bits).
  return randomSymbolId();
}

/** The controller of a block with rows: parameters, a struct's fields or an enum's values. */
export class ParamsController implements MutatorController {
  private readonly block: Blockly.Block;
  private readonly spec: RowsSpec;
  /** The prefix of the rows' inputs and fields (`B2C_PARAM`, `B2C_FIELD`, `B2C_VALUE`). */
  private readonly prefix: string;
  /** The rows as last applied; the fields hold later edits. */
  private rows: RowState[] = [];
  private fields: RowFields[] = [];
  /**
   * The shadows of removed rows' value inputs, by row symbol and input (`sym/DEFAULT`), so that a
   * row that comes back (undo) has its default value again, as the variadic mutator keeps a
   * removed part's shadow.
   */
  private readonly shadows = new Map<string, Blockly.serialization.blocks.State>();

  constructor(block: Blockly.Block, spec: RowsSpec) {
    this.block = block;
    this.spec = spec;
    this.prefix = ROW_PREFIXES[spec.rowKind];
    this.reshape([]);
  }

  extra(): Record<string, unknown> {
    return { [this.spec.name]: this.readRows().map((row) => writeRow(this.spec, row)) };
  }

  apply(extra: unknown): void {
    this.reshape(readRowsExtra(this.spec, extra));
  }

  refresh(): void {
    // Nothing in the rows comes from the analysis.
  }

  /** The rows with the fields' current values (a value a field should not hold is ignored). */
  private readRows(): RowState[] {
    return this.rows.map((row, index) => {
      const fields = this.fields[index];
      if (fields === undefined) {
        return { ...row };
      }
      const value: unknown = fields.type?.getValue();
      const type = typeof value === 'string' ? rowTypeKey(this.spec, rowTypeJson(value)) : null;
      const name = readName(fields.name);
      const mode: unknown = fields.mode?.getValue();
      return {
        sym: row.sym,
        name: name !== null && isShowableName(name) ? name : row.name,
        type: row.type === null ? null : (type ?? row.type),
        mode: row.mode === null ? null : isParamMode(mode) ? mode : row.mode,
      };
    });
  }

  /** Whether `name` is a part of one of the value inputs the rows count (`DEFAULT2`). */
  private isRowInput(name: string): boolean {
    return this.spec.rowInputs.some((input) => partIndex(name, input.name) !== null);
  }

  private readonly isOwned = (input: Blockly.Input): boolean =>
    input.name === BUTTONS_INPUT ||
    input.name.startsWith(this.prefix) ||
    this.isRowInput(input.name);

  /**
   * Takes the user's blocks (and the shadows) out of the rows' value inputs before the rows are
   * rebuilt for `next`, by row symbol and input: a block whose row stays is unplugged to be
   * connected again, one whose row goes is unplugged and bumped away, as removed parts' blocks are.
   */
  private takeRowParts(next: readonly RowState[]): Map<string, RowPart> {
    const kept = new Map<string, RowPart>();
    const staying = new Set(next.map((row) => row.sym));
    for (const [index, row] of this.rows.entries()) {
      for (const def of this.spec.rowInputs) {
        const connection = this.block.getInput(`${def.name}${String(index)}`)?.connection ?? null;
        if (connection === null) {
          continue;
        }
        const shadow = connection.getShadowState(true);
        const target = connection.targetBlock();
        const user = target !== null && !target.isShadow() ? target : null;
        if (user !== null) {
          user.unplug(false);
        }
        const key = `${row.sym}/${def.name}`;
        if (staying.has(row.sym)) {
          kept.set(key, { block: user, shadow });
          continue;
        }
        if (shadow === null) {
          this.shadows.delete(key);
        } else {
          this.shadows.set(key, shadow);
        }
        if (user instanceof Blockly.BlockSvg && !user.isInFlyout) {
          const offset = Blockly.config.snapRadius;
          user.moveBy(user.RTL ? -offset : offset, offset, ['bump']);
        }
      }
    }
    return kept;
  }

  /**
   * Puts the parts `takeRowParts` kept back into the rows' new inputs, and a removed row's shadow
   * into the row when it comes back.
   */
  private restoreRowParts(kept: ReadonlyMap<string, RowPart>): void {
    for (const [index, row] of this.rows.entries()) {
      for (const def of this.spec.rowInputs) {
        const key = `${row.sym}/${def.name}`;
        const removed = this.shadows.get(key);
        this.shadows.delete(key);
        const part =
          kept.get(key) ?? (removed === undefined ? undefined : { block: null, shadow: removed });
        const connection = this.block.getInput(`${def.name}${String(index)}`)?.connection ?? null;
        if (part === undefined || connection === null) {
          continue;
        }
        if (part.shadow !== null) {
          try {
            connection.setShadowState(part.shadow);
          } catch {
            connection.setShadowState(null);
          }
        }
        const output = part.block?.outputConnection ?? null;
        if (output !== null && !(part.block?.isDeadOrDying() ?? true)) {
          try {
            connection.connect(output);
          } catch {
            // A block the input no longer takes stays on the canvas.
          }
        }
      }
    }
  }

  /** Rebuilds every row for `rows`, then the ⊕ button, and places them. */
  private reshape(rows: readonly RowState[]): void {
    const kept = this.takeRowParts(rows);
    for (const input of [...this.block.inputList]) {
      if (input.name.startsWith(this.prefix) || this.isRowInput(input.name)) {
        silently(() => this.block.removeInput(input.name));
      }
    }
    this.rows = rows.map((row) => ({ ...row }));
    this.fields = [];
    const order: Blockly.Input[] = [];
    for (const [index, row] of this.rows.entries()) {
      order.push(...this.createRow(index, row));
    }
    order.push(buttonRow(this.block, BUTTONS_INPUT, this.buttonItems()));
    placeParts(this.block, order, this.isOwned, this.spec.anchorArgs);
    this.restoreRowParts(kept);
  }

  /**
   * The inputs of one row: a dummy input holding its fields and ⊖, or, on a block whose value
   * inputs the rows count, those inputs (`DEFAULT<i>`, the first holding the row's fields, each
   * after `=`) and a dummy input holding ⊖.
   */
  private createRow(index: number, row: RowState): Blockly.Input[] {
    const prefix = `${this.prefix}${String(index)}`;
    const [firstValue, ...otherValues] = this.spec.rowInputs;
    const input =
      firstValue === undefined
        ? this.block.appendDummyInput(prefix)
        : this.block.appendValueInput(`${firstValue.name}${String(index)}`);
    if (index === 0) {
      for (const text of this.spec.groupLeading) {
        input.appendField(new Blockly.FieldLabel(text));
      }
    }
    const kind = this.spec.rowKind;
    const type =
      row.type === null
        ? null
        : createTypeField(this.spec.types, row.type, TYPE_LABELS[kind], this.spec.typeClass);
    const name = createNameField(row.sym, row.name);
    const mode = row.mode === null ? null : createModeField(row.mode);
    if (!name.holdsDecl) {
      // Plain text stands in for the declaration field: limit typing to what a name may be.
      name.field.setValidator((value: unknown) =>
        typeof value === 'string' && NAME_ENTRY.test(value) ? value : null,
      );
    }
    if (type !== null) {
      input.appendField(type, `${prefix}_TYPE`);
    }
    input.appendField(name.field, `${prefix}_NAME`);
    if (mode !== null) {
      input.appendField(mode, `${prefix}_MODE`);
    }
    const inputs = [input];
    if (firstValue !== undefined) {
      input.appendField(new Blockly.FieldLabel('='));
      for (const def of otherValues) {
        inputs.push(
          this.block
            .appendValueInput(`${def.name}${String(index)}`)
            .appendField(new Blockly.FieldLabel('=')),
        );
      }
    }
    const buttons = firstValue === undefined ? input : this.block.appendDummyInput(prefix);
    if (buttons !== input) {
      inputs.push(buttons);
    }
    const key = `${this.buttonPrefix()}:${String(index)}:remove`;
    buttons.appendField(
      createButton({
        key,
        action: 'remove',
        label: `Remove this ${ROW_NOUNS[kind]}`,
        onActivate: () => {
          this.removeRow(index, key);
        },
      }),
    );
    this.fields.push({ type, name, mode });
    return inputs;
  }

  /** The prefix of the rows' button keys (`param:add`, `field:0:remove`, `value:add`). */
  private buttonPrefix(): string {
    return BUTTON_KEYS[this.spec.rowKind];
  }

  private buttonItems(): RowItem[] {
    const items: RowItem[] = [];
    if (this.rows.length === 0) {
      items.push(...this.spec.groupLeading.map((text) => ({ text })));
    }
    if (this.rows.length < Math.min(this.spec.max, MAX_VARIADIC_PARTS)) {
      items.push({
        key: `${this.buttonPrefix()}:add`,
        action: 'add',
        label: `Add a ${ROW_NOUNS[this.spec.rowKind]}`,
        onActivate: () => {
          this.addRow();
        },
      });
    }
    return items;
  }

  /** Applies `rows` as one undo step, if the block may be edited. */
  private change(key: string, rows: readonly RowState[]): void {
    if (!canEdit(this.block)) {
      return;
    }
    keepingFocus(this.block, key, () => {
      asUndoStep(
        this.block,
        () => this.extra(),
        () => {
          this.reshape(rows);
        },
      );
    });
  }

  private addRow(): void {
    const rows = this.readRows();
    const kind = this.spec.rowKind;
    // A new field is an `int` (the first listed type), as a new parameter is; a value has no type.
    const type = kind === 'values' ? null : (this.spec.types[0] ?? null);
    const full = rows.length >= Math.min(this.spec.max, MAX_VARIADIC_PARTS);
    if (full || (kind !== 'values' && type === null)) {
      return;
    }
    rows.push({
      sym: freshSymbolId(rows),
      name: freshName(rows, NEW_ROW_NAMES[kind]),
      type,
      mode: kind === 'params' ? 'copy' : null,
    });
    this.change(`${this.buttonPrefix()}:add`, rows);
  }

  private removeRow(index: number, key: string): void {
    const rows = this.readRows();
    if (index >= rows.length) {
      return;
    }
    rows.splice(index, 1);
    this.change(key, rows);
  }
}
