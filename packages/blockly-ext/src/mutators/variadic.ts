/**
 * The variadic mutators: repeated items (`b2c_mutator_items`), *else if* / *else* parts
 * (`b2c_mutator_if`) and call arguments (`b2c_mutator_call_args`). One engine serves all three,
 * driven by the block's catalog definition (see spec.ts for the label layout).
 *
 * Parts are named as in project files: `ITEM0` … `ITEM{n-1}`; `COND0`/`DO0` … for `control.if`,
 * whose first branch always exists (`plus` 1); `ELSE` while `hasElse` is set.
 *
 * A call to a function the latest analysis knows has as many arguments as the function has
 * parameters (docs/spec/04-user-interface.md §4.2), so its ⊖/⊕ are hidden while the counts match.
 * When the definition gains or loses a parameter, the call offers only the button that brings it
 * back in line. A parameter with a default value may be left out (w2.md §2.8): the call offers ⊖
 * down to the parameters without one, and ⊕ back up to all of them. Calls in the toolbox show none: the toolbox builds them from the analysis with one
 * argument per parameter, and blocks there cannot be edited.
 *
 * Milestone M3's wave W2 (docs/plans/m3-contracts/w2.md §2.1) adds:
 *
 * - call arguments named by a struct: on a block whose call arguments follow a `type` field (*new
 *   … with*'s `TYPE`) instead of a function reference, the struct's fields label the parts and
 *   set how many there are, as a function's parameters do for a call;
 * - repeated fields (`ERROR0`, `NAME0` of *try*): each copy's fields are created with the copy, in
 *   front of its next input (or on a row `B2C_FIELDS<copy>` of their own), and keep their values
 *   while their copy is removed, so adding it again or undoing brings back the same values (a
 *   declaration its symbol ID). A new declaration gets a fresh symbol ID and the editor's default
 *   name (the `declName` hook);
 * - flag-only branches: a spec without a count has only its flag-gated statements.
 */
import * as Blockly from 'blockly/core';

import { blockDef, partIndex } from '../checker/catalog';
import { MutatorButton } from './buttons';
import { asUndoStep, canEdit } from './events';
import {
  buttonKey,
  buttonRow,
  keepingFocus,
  type MutatorController,
  type RowItem,
} from './controller';
import { createJoinField, createMemberField } from './fields';
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
import { termFromKey } from '../types/static-type';

/** The dummy input holding the mutator's buttons. */
export const BUTTONS_INPUT = 'B2C_BUTTONS';
/** The dummy input holding a joining field the mutator created itself. */
export const JOIN_INPUT = 'B2C_JOIN';
/** The prefix of the label field naming a call argument (`B2C_LABEL_ARG0`). */
export const ARG_LABEL_PREFIX = 'B2C_LABEL_';
/**
 * The prefix of the dummy input holding a copy's repeated fields when no input of the copy follows
 * them (`B2C_FIELDS0`, w2.md §2.1).
 */
export const FIELDS_INPUT_PREFIX = 'B2C_FIELDS';

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

/**
 * The struct a type field's value names (a `{"ref": id}` term, by its key), or `null` for a
 * fundamental type, a template or nothing.
 */
function structRefOf(value: unknown): string | null {
  const term = typeof value === 'string' && value.startsWith('{') ? termFromKey(value) : null;
  return term?.kind === 'ref' ? term.id : null;
}

/**
 * The parameter names of the function a call names, or the field names of the struct a *new …
 * with* names (`null` for a name that is not known), or `null` when that function or struct is not
 * known.
 */
type CalledParameters = {
  readonly names: readonly (string | null)[];
  /** How many of the last ones may be left out (a function's parameters with a default value). */
  readonly optional: number;
} | null;

/** Quoted label words for button text: `"else if"`. */
function quoted(words: readonly string[]): string {
  return `"${words.join(' ')}"`;
}

/** The controller of a block with a variadic mutator. */
export class VariadicController implements MutatorController {
  private readonly block: Blockly.Block;
  private readonly spec: VariadicSpec;
  private readonly mutator: B2cMutatorName;
  /**
   * The name of the field whose symbol labels the arguments, for call arguments: a function
   * reference (`FUNC`), or a type field naming a struct (`TYPE`, w2.md §2.1).
   */
  private readonly refField: string | null;
  /** Whether {@link refField} names a struct (a `type` field) rather than a function. */
  private readonly refIsStruct: boolean;
  /** The saved states of the repeated fields of removed copies, by part name (`NAME1`). */
  private readonly fieldStates = new Map<string, unknown>();
  private state: VariadicState;
  /** Shadow states of removed value parts, by input name. */
  private readonly shadows = new Map<string, Blockly.serialization.blocks.State>();
  /** The input holding the field that joins copies, placed after the first copy, if any. */
  private readonly joinInput: Blockly.Input | null;

  constructor(block: Blockly.Block, spec: VariadicSpec, mutator: B2cMutatorName) {
    this.block = block;
    this.spec = spec;
    this.mutator = mutator;
    const fields = blockDef(block.type)?.fields ?? [];
    const reference = fields.find((field) => field.kind === 'symbol_ref');
    const typed = fields.find((field) => field.kind === 'type');
    const ref = mutator === B2C_MUTATOR_CALL_ARGS ? (reference ?? typed ?? null) : null;
    this.refField = ref?.name ?? null;
    this.refIsStruct = ref?.kind === 'type';
    this.state = {
      count: spec.count?.default ?? 0,
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
    const params = this.calledParameters();
    this.refreshArgLabels(params);
    this.refreshCallButtons(params);
    this.markMirrorsDirty();
  }

  // --- Layout -----------------------------------------------------------------------------------

  private copies(state: VariadicState): number {
    return state.count + this.spec.plus;
  }

  private isPartName(name: string): boolean {
    return (
      this.spec.members.some(
        (member) => member.kind !== 'field' && partIndex(name, member.name) !== null,
      ) ||
      (this.trailingFields().length > 0 && partIndex(name, FIELDS_INPUT_PREFIX) !== null) ||
      this.spec.gated.some((gated) => gated.name === name)
    );
  }

  /** The repeated fields after the last input of a copy: shown on a row of their own. */
  private trailingFields(): MemberSpec[] {
    const members = this.spec.members;
    let last = -1;
    members.forEach((member, index) => {
      if (member.kind !== 'field') {
        last = index;
      }
    });
    return members.slice(last + 1);
  }

  /** The inputs of one copy, in order: its inputs and, if it has trailing fields, their row. */
  private copyInputs(copy: number): string[] {
    const names = this.spec.members
      .filter((member) => member.kind !== 'field')
      .map((member) => `${member.name}${String(copy)}`);
    if (this.trailingFields().length > 0) {
      names.push(`${FIELDS_INPUT_PREFIX}${String(copy)}`);
    }
    return names;
  }

  private readonly isOwned = (input: Blockly.Input): boolean =>
    input === this.joinInput || input.name === BUTTONS_INPUT || this.isPartName(input.name);

  /** The value and statement parts `state` has (with rows of fields), in block order. */
  private wantedParts(state: VariadicState): string[] {
    const names: string[] = [];
    for (let copy = 0; copy < this.copies(state); copy += 1) {
      names.push(...this.copyInputs(copy));
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
      for (const name of this.copyInputs(copy)) {
        push(name);
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
        this.saveFieldStates(input);
        removePart(this.block, input.name, this.shadows);
      }
    }
    for (const name of wanted) {
      if (this.block.getInput(name) === null) {
        this.createPart(name);
      }
    }
    this.state = next;
    const params = this.calledParameters();
    const buttons = buttonRow(this.block, BUTTONS_INPUT, this.buttonItems(params));
    this.refreshArgLabels(params);
    placeParts(this.block, this.order(buttons), this.isOwned, this.spec.anchorArgs);
  }

  /**
   * The member and copy an input name stands for, with the repeated fields shown in front of it:
   * a value or statement member, or (`member` `null`) the row of a copy's trailing fields.
   */
  private memberOf(name: string): {
    member: MemberSpec | null;
    position: number;
    copy: number;
    fields: MemberSpec[];
  } | null {
    const members = this.spec.members;
    const fieldsRow = partIndex(name, FIELDS_INPUT_PREFIX);
    if (fieldsRow !== null) {
      const fields = this.trailingFields();
      return { member: null, position: members.length - fields.length, copy: fieldsRow, fields };
    }
    for (const [position, member] of members.entries()) {
      const copy = member.kind === 'field' ? null : partIndex(name, member.name);
      if (copy !== null) {
        // The repeated fields between the previous input of the copy and this one.
        let first = position;
        while (first > 0 && members[first - 1]?.kind === 'field') {
          first -= 1;
        }
        return { member, position: first, copy, fields: members.slice(first, position) };
      }
    }
    return null;
  }

  /** Keeps the states of the repeated fields an input holds, before it is removed. */
  private saveFieldStates(input: Blockly.Input): void {
    for (const field of input.fieldRow) {
      const name = field.name;
      if (name === undefined || !this.isFieldPart(name)) {
        continue;
      }
      try {
        this.fieldStates.set(name, field.saveState());
      } catch {
        this.fieldStates.delete(name);
      }
    }
  }

  /**
   * The declaration a new part of a repeated declaration field starts with: a fresh symbol ID and
   * the editor's default name (the `declName` hook), else the field's name in lower case. Other
   * fields need none (`declares` false gives an empty one, never used).
   */
  private newDecl(declares: boolean, field: string): { sym: string; name: string } {
    if (!declares) {
      return { sym: '', name: '' };
    }
    const hooks = mutatorHooks();
    let name: string | null;
    try {
      name = hooks.declName?.(this.block, field) ?? null;
    } catch {
      // A failing hook must not break the block: the mutators' own name stands in.
      name = null;
    }
    return { sym: hooks.newSymbolId(), name: name ?? field.toLowerCase() };
  }

  /** Whether `name` is a part of one of the repeated fields (`NAME1`). */
  private isFieldPart(name: string): boolean {
    return this.spec.members.some(
      (member) => member.kind === 'field' && partIndex(name, member.name) !== null,
    );
  }

  /**
   * A part of a repeated field: its kept state when its copy comes back, otherwise the catalog
   * default (a declaration with a fresh symbol ID and the editor's default name). Its value is set
   * before it is attached, so creating it fires no event.
   */
  private createFieldPart(member: MemberSpec, name: string): Blockly.Field | null {
    const def = member.field;
    if (def === null) {
      return null;
    }
    const field = createMemberField(def, this.newDecl(def.kind === 'symbol_decl', def.name));
    const kept = this.fieldStates.get(name);
    this.fieldStates.delete(name);
    if (kept !== undefined) {
      try {
        field.loadState(kept);
      } catch {
        // A state the field cannot take back: it keeps its default.
      }
    }
    return field;
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
    const { member, position, copy, fields } = found;
    const input =
      member === null
        ? this.block.appendDummyInput(name)
        : member.kind === 'value'
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
    for (const fieldMember of fields) {
      for (const text of fieldMember.leading) {
        input.appendField(new Blockly.FieldLabel(text));
      }
      const part = `${fieldMember.name}${String(copy)}`;
      const field = this.createFieldPart(fieldMember, part);
      if (field !== null) {
        input.appendField(field, part);
      }
    }
    if (member === null) {
      return;
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

  /**
   * The parameters of the called function as the latest analysis knows them (their names, `null`
   * for a name that is not known), or `null` when the function is not known: nothing is chosen,
   * the reference is missing or not a function, or the analysis has not seen the block yet. For a
   * block whose arguments follow a struct (*new … with*), the struct's fields, in the same way.
   * Always `null` for the other mutators.
   *
   * The editor's `functionParams` (`structFields`) answers without a scope query when it has one;
   * otherwise the function (struct) is looked up among the symbols visible at the block.
   */
  private calledParameters(): CalledParameters {
    const symbols = mutatorHooks().symbols;
    if (symbols === null || this.refField === null) {
      return null;
    }
    const value: unknown = this.block.getField(this.refField)?.getValue();
    const ref = this.refIsStruct ? structRefOf(value) : refOf(value);
    if (ref === null) {
      return null;
    }
    try {
      let params: readonly string[] | null | undefined;
      let optional = 0;
      if (this.refIsStruct) {
        params =
          symbols.structFields === undefined
            ? symbols
                .symbolsAt(this.block.id, null)
                .find((symbol) => symbol.id === ref && symbol.kind === 'struct')?.fields
            : symbols.structFields(this.block.id, ref);
      } else if (symbols.functionParams === undefined) {
        const fn = symbols
          .symbolsAt(this.block.id, null)
          .find((symbol) => symbol.id === ref && symbol.kind === 'function');
        params = fn?.params;
        optional = fn?.optional ?? 0;
      } else {
        params = symbols.functionParams(this.block.id, ref);
        optional = symbols.functionOptional?.(this.block.id, ref) ?? 0;
      }
      return params === undefined || params === null
        ? null
        : {
            names: params.map((param) => symbols.nameOf(param)),
            optional: Math.min(Math.max(optional, 0), params.length),
          };
    } catch {
      // The symbols come from the latest analysis; failing there must not break the block.
      return null;
    }
  }

  /** Labels each argument with its parameter's name (`name:`), or with nothing when unknown. */
  private refreshArgLabels(names: CalledParameters): void {
    const member = this.spec.members[0];
    if (this.mutator !== B2C_MUTATOR_CALL_ARGS || member === undefined) {
      return;
    }
    for (let copy = 0; copy < this.copies(this.state); copy += 1) {
      const input = this.block.getInput(`${member.name}${String(copy)}`);
      if (input === null) {
        continue;
      }
      const labelName = `${ARG_LABEL_PREFIX}${input.name}`;
      const name = names?.names[copy] ?? null;
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
    if (this.mutator === B2C_MUTATOR_CALL_ARGS) {
      return this.refIsStruct
        ? { add: 'Add a field value', remove: 'Remove the last field value' }
        : { add: 'Add an argument', remove: 'Remove the last argument' };
    }
    return { add: 'Add an input', remove: 'Remove the last input' };
  }

  /**
   * Which count buttons the block shows: ⊖ and ⊕ within the catalog's limits, except on a call
   * (see the module comment), where the called function's parameters decide.
   */
  private countButtons(params: CalledParameters): {
    readonly remove: boolean;
    readonly add: boolean;
  } {
    const { count } = this.state;
    if (this.spec.count === null) {
      // Flag-only branches: no count to change.
      return { remove: false, add: false };
    }
    const max = Math.min(this.spec.count.max, MAX_VARIADIC_PARTS);
    let remove = count > this.spec.count.min && count > 0;
    let add = count < max;
    if (this.mutator === B2C_MUTATOR_CALL_ARGS) {
      // A call in the toolbox already has the function's count (the toolbox built it so). A
      // call may leave out the arguments of parameters with a default value.
      const most = this.block.isInFlyout ? count : params?.names.length;
      const least = this.block.isInFlyout ? count : (most ?? 0) - (params?.optional ?? 0);
      if (most !== undefined) {
        remove &&= count > least;
        add &&= count < most;
      }
    }
    return { remove, add };
  }

  /**
   * Shows the count buttons a call needs after an analysis: the called function may have become
   * known, or gained or lost a parameter. Nothing is recorded, and a button that had the keyboard
   * focus hands it to another button of the block, or to the block.
   */
  private refreshCallButtons(params: CalledParameters): void {
    if (this.mutator !== B2C_MUTATOR_CALL_ARGS) {
      return;
    }
    const update = (): void => {
      const buttons = buttonRow(this.block, BUTTONS_INPUT, this.buttonItems(params));
      placeParts(this.block, this.order(buttons), this.isOwned, this.spec.anchorArgs);
    };
    const focused =
      this.block instanceof Blockly.BlockSvg ? Blockly.getFocusManager().getFocusedNode() : null;
    const key =
      focused instanceof MutatorButton && focused.getSourceBlock() === this.block
        ? buttonKey(focused)
        : undefined;
    if (key === undefined) {
      update();
    } else {
      keepingFocus(this.block, key, update);
    }
  }

  private buttonItems(params: CalledParameters): RowItem[] {
    const items: RowItem[] = [];
    const { flags } = this.state;
    if (this.copies(this.state) === 0) {
      items.push(...this.spec.groupLeading.map((text) => ({ text })));
    }
    const { remove: removable, add: addable } = this.countButtons(params);
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
    const spec = this.spec.count;
    if (spec === null) {
      return;
    }
    const next = count + delta;
    const allowed =
      delta > 0 ? next <= Math.min(spec.max, MAX_VARIADIC_PARTS) : next >= spec.min && next >= 0;
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
