/**
 * The variadic mutators: repeated items (`b2c_mutator_items`), *else if* / *else* parts
 * (`b2c_mutator_if`) and call arguments (`b2c_mutator_call_args`). One engine serves all three,
 * driven by the block's catalog definition (see spec.ts for the label layout).
 *
 * Parts are named as in project files: `ITEM0` … `ITEM{n-1}`; `COND0`/`DO0` … for `control.if`,
 * whose first branch always exists (`plus` 1); `ELSE` while `hasElse` is set.
 */
import * as Blockly from 'blockly/core';

import { blockDef, partIndex } from '../checker/catalog';
import { asUndoStep, canEdit } from './events';
import { buttonRow, keepingFocus, type MutatorController, type RowItem } from './controller';
import { createJoinField } from './fields';
import { mutatorHooks } from './hooks';
import { displayName, MirrorLabel } from './labels';
import { placeParts, REGISTRATION_ANCHOR, removePart, restoreShadow, silently } from './parts';
import type { MemberSpec, VariadicSpec } from './spec';
import {
  MAX_VARIADIC_PARTS,
  readVariadicExtra,
  type VariadicState,
  writeVariadicExtra,
} from './state';
import { B2C_MUTATOR_CALL_ARGS, type B2cMutatorName } from './types';

/** The dummy input holding the mutator's buttons. */
export const BUTTONS_INPUT = 'B2C_BUTTONS';
/** The dummy input holding a joining field the mutator created itself. */
export const JOIN_INPUT = 'B2C_JOIN';
/** The prefix of the label field naming a call argument (`B2C_LABEL_ARG0`). */
export const ARG_LABEL_PREFIX = 'B2C_LABEL_';

/** A symbol reference field's value: `{ref}` (a `b2c_symbol_ref`), or the ID as plain text. */
function refOf(value: unknown): string | null {
  if (typeof value === 'string') {
    return value === '' ? null : value;
  }
  if (typeof value === 'object' && value !== null && 'ref' in value) {
    const ref: unknown = value.ref;
    return typeof ref === 'string' && ref !== '' ? ref : null;
  }
  return null;
}

/** Quoted label words for button text: `"else if"`. */
function quoted(words: readonly string[]): string {
  return `"${words.join(' ')}"`;
}

/** The controller of a block with a variadic mutator. */
export class VariadicController implements MutatorController {
  private readonly block: Blockly.Block;
  private readonly spec: VariadicSpec;
  private readonly mutator: B2cMutatorName;
  /** The name of the field whose symbol labels the arguments (`FUNC`), for call arguments. */
  private readonly refField: string | null;
  private state: VariadicState;
  /** Shadow states of removed value parts, by input name. */
  private readonly shadows = new Map<string, Blockly.serialization.blocks.State>();
  /** The input holding the field that joins copies, placed after the first copy, if any. */
  private readonly joinInput: Blockly.Input | null;

  constructor(block: Blockly.Block, spec: VariadicSpec, mutator: B2cMutatorName) {
    this.block = block;
    this.spec = spec;
    this.mutator = mutator;
    this.refField =
      mutator === B2C_MUTATOR_CALL_ARGS
        ? (blockDef(block.type)?.fields.find((field) => field.kind === 'symbol_ref')?.name ?? null)
        : null;
    this.state = {
      count: spec.count.default,
      flags: new Map(spec.flags.map((flag) => [flag.name, flag.default])),
    };
    this.joinInput = this.setUpJoinField();
    this.reshape(this.state);
  }

  extra(): Record<string, unknown> {
    return writeVariadicExtra(this.spec, this.state);
  }

  apply(extra: unknown): void {
    this.reshape(readVariadicExtra(this.spec, extra));
  }

  refresh(): void {
    this.refreshArgLabels();
    this.markMirrorsDirty();
  }

  // --- Layout -----------------------------------------------------------------------------------

  private copies(state: VariadicState): number {
    return state.count + this.spec.plus;
  }

  private isPartName(name: string): boolean {
    return (
      this.spec.members.some((member) => partIndex(name, member.name) !== null) ||
      this.spec.gated.some((gated) => gated.name === name)
    );
  }

  private readonly isOwned = (input: Blockly.Input): boolean =>
    input === this.joinInput || input.name === BUTTONS_INPUT || this.isPartName(input.name);

  /** The value and statement parts `state` has, in block order. */
  private wantedParts(state: VariadicState): string[] {
    const names: string[] = [];
    for (let copy = 0; copy < this.copies(state); copy += 1) {
      for (const member of this.spec.members) {
        names.push(`${member.name}${String(copy)}`);
      }
    }
    for (const gated of this.spec.gated) {
      if (state.flags.get(gated.flag) === true) {
        names.push(gated.name);
      }
    }
    return names;
  }

  /** Every owned input in block order: the parts, the joining input after the first copy, buttons. */
  private order(buttons: Blockly.Input): Blockly.Input[] {
    const inputs: Blockly.Input[] = [];
    const push = (name: string): void => {
      const input = this.block.getInput(name);
      if (input !== null) {
        inputs.push(input);
      }
    };
    const copies = this.copies(this.state);
    for (let copy = 0; copy < copies; copy += 1) {
      for (const member of this.spec.members) {
        push(`${member.name}${String(copy)}`);
      }
      if (copy === 0 && this.joinInput !== null) {
        inputs.push(this.joinInput);
      }
    }
    if (copies === 0 && this.joinInput !== null) {
      inputs.push(this.joinInput);
    }
    for (const gated of this.spec.gated) {
      if (this.state.flags.get(gated.flag) === true) {
        push(gated.name);
      }
    }
    inputs.push(buttons);
    return inputs;
  }

  /** Shapes the block for `next`: removes, adds and places parts, and rebuilds the buttons. */
  private reshape(next: VariadicState): void {
    const wanted = new Set(this.wantedParts(next));
    for (const input of [...this.block.inputList]) {
      if (this.isPartName(input.name) && !wanted.has(input.name)) {
        removePart(this.block, input.name, this.shadows);
      }
    }
    for (const name of wanted) {
      if (this.block.getInput(name) === null) {
        this.createPart(name);
      }
    }
    this.state = next;
    const buttons = buttonRow(this.block, BUTTONS_INPUT, this.buttonItems());
    this.refreshArgLabels();
    placeParts(this.block, this.order(buttons), this.isOwned, this.spec.anchorArgs);
  }

  /** The member and copy a part name stands for. */
  private memberOf(name: string): { member: MemberSpec; position: number; copy: number } | null {
    for (const [position, member] of this.spec.members.entries()) {
      const copy = partIndex(name, member.name);
      if (copy !== null) {
        return { member, position, copy };
      }
    }
    return null;
  }

  private createPart(name: string): void {
    const gated = this.spec.gated.find((part) => part.name === name);
    if (gated !== undefined) {
      const input = this.block.appendStatementInput(name);
      for (const text of gated.leading) {
        input.appendField(new Blockly.FieldLabel(text));
      }
      return;
    }
    const found = this.memberOf(name);
    if (found === null) {
      return;
    }
    const { member, position, copy } = found;
    const input =
      member.kind === 'value'
        ? this.block.appendValueInput(name)
        : this.block.appendStatementInput(name);
    if (position === 0 && copy === 0) {
      for (const text of this.spec.groupLeading) {
        input.appendField(new Blockly.FieldLabel(text));
      }
    }
    if (position === 0 && copy >= 1) {
      for (const text of this.spec.joinTexts) {
        input.appendField(new Blockly.FieldLabel(text));
      }
      const joinField = this.spec.joinField;
      if (joinField !== null && (this.joinInput === null || copy >= 2)) {
        input.appendField(new MirrorLabel(joinField.name));
      }
    }
    for (const text of member.leading) {
      input.appendField(new Blockly.FieldLabel(text));
    }
    const inputDef = member.input;
    if (inputDef !== null) {
      restoreShadow(input, this.shadows, () => {
        const factory = mutatorHooks().inputShadow;
        return factory === null ? null : factory(this.block, inputDef, name);
      });
    }
  }

  // --- The joining field (logic.operation's OP) -------------------------------------------------

  /**
   * Finds or creates the field that joins copies. A field registration drew alone in a dummy input
   * is adopted (its input moves after the first copy); a field drawn among other fields stays where
   * it is and every later copy shows its text; with no such field, the mutator creates it.
   */
  private setUpJoinField(): Blockly.Input | null {
    const def = this.spec.joinField;
    if (def === null) {
      return null;
    }
    const existing = this.block.getField(def.name);
    if (existing !== null) {
      this.watchJoinField(existing);
      const holder = this.block.inputList.find((input) => input.fieldRow.includes(existing));
      const alone =
        holder?.connection === null &&
        holder.name !== REGISTRATION_ANCHOR &&
        holder.fieldRow.length === 1;
      return alone ? holder : null;
    }
    const field = createJoinField(def);
    const input = this.block.appendDummyInput(JOIN_INPUT);
    input.appendField(field, def.name);
    this.watchJoinField(field);
    return input;
  }

  /** Refreshes the copies of the joining field's text whenever its value changes. */
  private watchJoinField(field: Blockly.Field): void {
    const previous = field.getValidator();
    field.setValidator((value: unknown) => {
      const result: unknown = previous === null ? undefined : previous.call(field, value);
      this.markMirrorsDirty();
      return result;
    });
  }

  private markMirrorsDirty(): void {
    for (const input of this.block.inputList) {
      for (const field of input.fieldRow) {
        if (field instanceof MirrorLabel) {
          field.markDirty();
        }
      }
    }
  }

  // --- Call arguments ---------------------------------------------------------------------------

  /** The parameter names of the called function, from the latest analysis, or `null`. */
  private parameterNames(): (string | null)[] | null {
    const symbols = mutatorHooks().symbols;
    if (symbols === null || this.refField === null) {
      return null;
    }
    const ref = refOf(this.block.getField(this.refField)?.getValue());
    if (ref === null) {
      return null;
    }
    try {
      const fn = symbols
        .symbolsAt(this.block.id, null)
        .find((symbol) => symbol.id === ref && symbol.kind === 'function');
      return fn?.params === undefined ? null : fn.params.map((param) => symbols.nameOf(param));
    } catch {
      // The symbols come from the latest analysis; failing there must not break the block.
      return null;
    }
  }

  /** Labels each argument with its parameter's name (`name:`), or with nothing when unknown. */
  private refreshArgLabels(): void {
    const member = this.spec.members[0];
    if (this.mutator !== B2C_MUTATOR_CALL_ARGS || member === undefined) {
      return;
    }
    const names = this.parameterNames();
    for (let copy = 0; copy < this.copies(this.state); copy += 1) {
      const input = this.block.getInput(`${member.name}${String(copy)}`);
      if (input === null) {
        continue;
      }
      const labelName = `${ARG_LABEL_PREFIX}${input.name}`;
      const name = names?.[copy] ?? null;
      const text = name === null || name === '' ? null : `${displayName(name)}:`;
      const existing = this.block.getField(labelName);
      silently(() => {
        if (text === null) {
          input.removeField(labelName, true);
        } else if (existing === null) {
          input.appendField(new Blockly.FieldLabel(text), labelName);
        } else if (existing.getValue() !== text) {
          existing.setValue(text);
        }
      });
    }
  }

  // --- Buttons ----------------------------------------------------------------------------------

  private countLabels(): { add: string; remove: string } {
    if (this.spec.joinTexts.length > 0) {
      const words = quoted(this.spec.joinTexts);
      return { add: `Add ${words}`, remove: `Remove the last ${words}` };
    }
    return this.mutator === B2C_MUTATOR_CALL_ARGS
      ? { add: 'Add an argument', remove: 'Remove the last argument' }
      : { add: 'Add an input', remove: 'Remove the last input' };
  }

  private buttonItems(): RowItem[] {
    const items: RowItem[] = [];
    const { count, flags } = this.state;
    if (this.copies(this.state) === 0) {
      items.push(...this.spec.groupLeading.map((text) => ({ text })));
    }
    const removable = count > this.spec.count.min && count > 0;
    const addable = count < Math.min(this.spec.count.max, MAX_VARIADIC_PARTS);
    if (removable || addable) {
      const labels = this.countLabels();
      items.push(...this.spec.joinTexts.map((text) => ({ text })));
      if (removable) {
        items.push({
          key: 'count:remove',
          action: 'remove',
          label: labels.remove,
          onActivate: () => {
            this.changeCount(-1, 'count:remove');
          },
        });
      }
      if (addable) {
        items.push({
          key: 'count:add',
          action: 'add',
          label: labels.add,
          onActivate: () => {
            this.changeCount(1, 'count:add');
          },
        });
      }
    }
    for (const gated of this.spec.gated) {
      const on = flags.get(gated.flag) === true;
      const words = quoted(gated.leading);
      const key = `flag:${gated.flag}:${on ? 'remove' : 'add'}`;
      items.push(...gated.leading.map((text) => ({ text })));
      items.push({
        key,
        action: on ? 'remove' : 'add',
        label: on ? `Remove ${words}` : `Add ${words}`,
        onActivate: () => {
          this.setFlag(gated.flag, !on, key);
        },
      });
    }
    return items;
  }

  // --- User actions -----------------------------------------------------------------------------

  /** Applies `next` as one undo step, if the block may be edited. */
  private change(key: string, next: VariadicState): void {
    if (!canEdit(this.block)) {
      return;
    }
    keepingFocus(this.block, key, () => {
      asUndoStep(
        this.block,
        () => this.extra(),
        () => {
          this.reshape(next);
        },
      );
    });
  }

  private changeCount(delta: 1 | -1, key: string): void {
    const { count, flags } = this.state;
    const next = count + delta;
    const allowed =
      delta > 0
        ? next <= Math.min(this.spec.count.max, MAX_VARIADIC_PARTS)
        : next >= this.spec.count.min && next >= 0;
    if (allowed) {
      this.change(key, { count: next, flags });
    }
  }

  private setFlag(name: string, value: boolean, key: string): void {
    if (this.state.flags.get(name) === value) {
      return;
    }
    const flags = new Map(this.state.flags);
    flags.set(name, value);
    this.change(key, { count: this.state.count, flags });
  }
}
