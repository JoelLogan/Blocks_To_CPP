/**
 * The parameter rows of `func.define` (`b2c_mutator_params`, 03 §3.7.7): one row per parameter,
 * edited as a type (`b2c_type`), a name (`b2c_symbol_decl`) and a pass mode (`b2c_dropdown`), with
 * ⊖ on each row and ⊕ after the last. The rows are the block's `params` extra:
 * `[{sym, name, type, mode}, …]`, at most 16 (the catalog maximum).
 *
 * A row's fields are named `B2C_PARAM<i>_TYPE`, `_NAME` and `_MODE`; editing them records ordinary
 * field changes, and adding or removing a row records one mutation step with every row's values,
 * so undo restores rows exactly.
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
import { placeParts } from './parts';
import type { ParamsSpec } from './spec';
import { isShowableName, MAX_VARIADIC_PARTS, readParamsExtra } from './state';
import { PARAM_MODES, type ParamMode, type ParamRow } from './types';

/** The prefix of a parameter row's dummy input (`B2C_PARAM0`). */
export const PARAM_ROW_PREFIX = 'B2C_PARAM';
/** The dummy input holding the ⊕ button. */
const BUTTONS_INPUT = 'B2C_BUTTONS';
/** The name a new parameter starts with (then `param2`, `param3`, …). */
const NEW_PARAM_NAME = 'param';
/** Symbol IDs: `[A-Za-z0-9_]{1,32}` (05 §5.4). */
const SYMBOL_ID = /^[A-Za-z0-9_]{1,32}$/;
/** What a user may type as a name (03 §3.6); other names from files are kept but not typed. */
const NAME_ENTRY = /^[A-Za-z0-9_]{1,64}$/;

interface RowFields {
  readonly type: Blockly.Field;
  readonly name: NameField;
  readonly mode: Blockly.Field;
}

function isParamMode(value: unknown): value is ParamMode {
  return typeof value === 'string' && (PARAM_MODES as readonly string[]).includes(value);
}

/** The first of `param`, `param2`, `param3`, … no row uses. */
function freshName(rows: readonly ParamRow[]): string {
  const used = new Set(rows.map((row) => row.name));
  if (!used.has(NEW_PARAM_NAME)) {
    return NEW_PARAM_NAME;
  }
  let suffix = 2;
  while (used.has(`${NEW_PARAM_NAME}${String(suffix)}`)) {
    suffix += 1;
  }
  return `${NEW_PARAM_NAME}${String(suffix)}`;
}

/** A symbol ID for a new parameter, distinct from the rows' IDs. */
function freshSymbolId(rows: readonly ParamRow[]): string {
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

/** The controller of a block with parameter rows. */
export class ParamsController implements MutatorController {
  private readonly block: Blockly.Block;
  private readonly spec: ParamsSpec;
  /** The rows as last applied; the fields hold later edits. */
  private rows: ParamRow[] = [];
  private fields: RowFields[] = [];

  constructor(block: Blockly.Block, spec: ParamsSpec) {
    this.block = block;
    this.spec = spec;
    this.reshape([]);
  }

  extra(): Record<string, unknown> {
    return { [this.spec.name]: this.readRows() };
  }

  apply(extra: unknown): void {
    this.reshape(readParamsExtra(this.spec, extra));
  }

  refresh(): void {
    // Nothing in the rows comes from the analysis.
  }

  /** The rows with the fields' current values (a value a field should not hold is ignored). */
  private readRows(): ParamRow[] {
    return this.rows.map((row, index) => {
      const fields = this.fields[index];
      if (fields === undefined) {
        return { ...row };
      }
      const type: unknown = fields.type.getValue();
      const name = readName(fields.name);
      const mode: unknown = fields.mode.getValue();
      return {
        sym: row.sym,
        name: name !== null && isShowableName(name) ? name : row.name,
        type: typeof type === 'string' && this.spec.types.includes(type) ? type : row.type,
        mode: isParamMode(mode) ? mode : row.mode,
      };
    });
  }

  private readonly isOwned = (input: Blockly.Input): boolean =>
    input.name === BUTTONS_INPUT || input.name.startsWith(PARAM_ROW_PREFIX);

  /** Rebuilds every row for `rows`, then the ⊕ button, and places them. */
  private reshape(rows: readonly ParamRow[]): void {
    for (const input of [...this.block.inputList]) {
      if (input.name.startsWith(PARAM_ROW_PREFIX)) {
        this.block.removeInput(input.name);
      }
    }
    this.rows = rows.map((row) => ({ ...row }));
    this.fields = [];
    const order: Blockly.Input[] = [];
    for (const [index, row] of this.rows.entries()) {
      order.push(this.createRow(index, row));
    }
    order.push(buttonRow(this.block, BUTTONS_INPUT, this.buttonItems()));
    placeParts(this.block, order, this.isOwned, this.spec.anchorArgs);
  }

  private createRow(index: number, row: ParamRow): Blockly.Input {
    const prefix = `${PARAM_ROW_PREFIX}${String(index)}`;
    const input = this.block.appendDummyInput(prefix);
    if (index === 0) {
      for (const text of this.spec.groupLeading) {
        input.appendField(new Blockly.FieldLabel(text));
      }
    }
    const type = createTypeField(this.spec.types, row.type);
    const name = createNameField(row.sym, row.name);
    const mode = createModeField(row.mode);
    if (!name.holdsDecl) {
      // Plain text stands in for the declaration field: limit typing to what a name may be.
      name.field.setValidator((value: unknown) =>
        typeof value === 'string' && NAME_ENTRY.test(value) ? value : null,
      );
    }
    input.appendField(type, `${prefix}_TYPE`);
    input.appendField(name.field, `${prefix}_NAME`);
    input.appendField(mode, `${prefix}_MODE`);
    const key = `param:${String(index)}:remove`;
    input.appendField(
      createButton({
        key,
        action: 'remove',
        label: 'Remove this parameter',
        onActivate: () => {
          this.removeRow(index, key);
        },
      }),
    );
    this.fields.push({ type, name, mode });
    return input;
  }

  private buttonItems(): RowItem[] {
    const items: RowItem[] = [];
    if (this.rows.length === 0) {
      items.push(...this.spec.groupLeading.map((text) => ({ text })));
    }
    if (this.rows.length < Math.min(this.spec.max, MAX_VARIADIC_PARTS)) {
      items.push({
        key: 'param:add',
        action: 'add',
        label: 'Add a parameter',
        onActivate: () => {
          this.addRow();
        },
      });
    }
    return items;
  }

  /** Applies `rows` as one undo step, if the block may be edited. */
  private change(key: string, rows: readonly ParamRow[]): void {
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
    const type = this.spec.types[0];
    if (rows.length >= Math.min(this.spec.max, MAX_VARIADIC_PARTS) || type === undefined) {
      return;
    }
    rows.push({ sym: freshSymbolId(rows), name: freshName(rows), type, mode: 'copy' });
    this.change('param:add', rows);
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
