/**
 * Quick fixes (docs/spec/04-user-interface.md §4.4, docs/plans/m3-contracts/w4-editor-tools.md
 * §2.5): the diagnostics with one obvious fix, and the fix as one undoable edit.
 *
 * | Code | Fix |
 * | --- | --- |
 * | `B2C-E0201` (a variable) | *Create variable “score”*: a `create` with the reference's symbol and last-known name, at the start of the body that holds it |
 * | `B2C-E0410` | *Add a “return” at the end*: a `return` with the result type's start value, at the end of the function |
 * | `B2C-W0508` | *Add cases for List, Quit*: a case per missing value of the enum |
 * | `B2C-E0405` | *Add “when program starts”*: the program block, in the view |
 * | `B2C-E0403` | *Give back a value*: the result type's start value in the `return` |
 *
 * A fix is offered only while it still applies (the reference still dangles, the function still
 * has no `return` at its end), checked when it is offered and again when it is applied. Applying
 * one runs in one Blockly event group, so one Undo takes it back, and selects the block it made or
 * changed. Nothing here deletes a block.
 */
import type { SymbolInfo } from '@blocks2cpp/b2c-core-wasm';
import {
  B2cSymbolDeclField,
  B2cSymbolRefField,
  asUndoStep,
  catalogFieldOptions,
  getEditorServices,
  hasB2cMutator,
  type TokenJson,
} from '@blocks2cpp/blockly-ext';
import type { Diagnostic } from '@blocks2cpp/ipc-types';
import * as Blockly from 'blockly/core';

import { blockState, presetBlock } from '../toolbox/presets';
import { giveStartValue } from '../toolbox/reshape';
import type { SymbolSource } from '../toolbox/scope';
import { startTokensForDeclaredType } from '../toolbox/values';

/** One fix of a diagnostic. */
export interface QuickFix {
  /** Stable for the diagnostic: `<code>:<block>`. */
  readonly id: string;
  /** What it does, as a button or menu item reads: `Create variable “score”`. */
  readonly title: string;
  /** Makes the change as one undo step; whether it did (it no longer applied, or failed). */
  apply(): boolean;
}

/** What the fixes read. */
export interface FixContext {
  /** The canvas of the module shown. */
  readonly workspace: Blockly.WorkspaceSvg;
  /** The symbols of the latest analysis. */
  readonly symbols: SymbolSource;
  /** The ID of the module shown. */
  readonly moduleId: string;
}

/** The program block. */
const MAIN = 'program.main';
/** A function definition. */
const DEFINE = 'func.define';
/** The kinds of reference fields that name a variable. */
const VARIABLE_KINDS: ReadonlySet<string> = new Set(['variables', 'assignable', 'changeable']);

/** The diagnostic's block on the canvas, when it is in the module shown. */
function blockOf(diagnostic: Diagnostic, context: FixContext): Blockly.BlockSvg | null {
  const id = diagnostic.primary.block;
  const module = diagnostic.primary.module;
  if (id === undefined || (module !== undefined && module !== context.moduleId)) {
    return null;
  }
  const block = context.workspace.getBlockById(id);
  return block instanceof Blockly.BlockSvg && !block.isDeadOrDying() ? block : null;
}

/** The nearest block around `block` (itself included) of type `type`. */
function enclosing(block: Blockly.Block, type: string): Blockly.Block | null {
  for (let at: Blockly.Block | null = block; at !== null; at = at.getSurroundParent()) {
    if (at.type === type) {
      return at;
    }
  }
  return null;
}

/** The body (*when program starts* or a function) around `block`. */
function bodyBlock(block: Blockly.Block): Blockly.Block | null {
  for (let at: Blockly.Block | null = block; at !== null; at = at.getSurroundParent()) {
    if (at.type === MAIN || at.type === DEFINE) {
      return at;
    }
  }
  return null;
}

/** Runs `change` as one undo step (its own Blockly event group); whether it succeeded. */
function asOneStep(change: () => Blockly.BlockSvg | null): boolean {
  const outer = Blockly.Events.getGroup();
  if (outer === '') {
    Blockly.Events.setGroup(true);
  }
  let made: Blockly.BlockSvg | null;
  try {
    made = change();
  } catch (error: unknown) {
    console.error('The quick fix could not be applied', error);
    made = null;
  } finally {
    if (outer === '') {
      Blockly.Events.setGroup(false);
    }
  }
  if (made !== null) {
    Blockly.common.setSelected(made);
  }
  return made !== null;
}

/** Creates a block from its state on `workspace`, recorded for undo. */
function create(
  workspace: Blockly.WorkspaceSvg,
  state: Blockly.serialization.blocks.State,
): Blockly.BlockSvg | null {
  const block = Blockly.serialization.blocks.append(state, workspace, { recordUndo: true });
  return block instanceof Blockly.BlockSvg ? block : null;
}

/** The last statement in the statement input `name` of `block`, or `null` when it is empty. */
function lastStatement(block: Blockly.Block, name: string): Blockly.Block | null {
  let at = block.getInputTargetBlock(name);
  while (at?.getNextBlock() !== null && at !== null) {
    at = at.getNextBlock();
  }
  return at;
}

// --- B2C-E0201: create the variable again ------------------------------------------------------

/** Whether a declaration of symbol `sym` is on the canvas. */
function declaredOnCanvas(workspace: Blockly.Workspace, sym: string): boolean {
  return workspace
    .getAllBlocks(false)
    .some((block) =>
      block.inputList.some((input) =>
        input.fieldRow.some(
          (field) => field instanceof B2cSymbolDeclField && field.getSymbolId() === sym,
        ),
      ),
    );
}

/** The dangling variable reference of the diagnostic's block: its field and symbol. */
function danglingVariable(
  block: Blockly.Block,
  diagnostic: Diagnostic,
  context: FixContext,
): { readonly sym: string; readonly name: string } | null {
  const part = diagnostic.primary.part;
  const known = new Set(context.symbols.allSymbols().map((symbol) => symbol.id));
  for (const input of block.inputList) {
    for (const field of input.fieldRow) {
      if (!(field instanceof B2cSymbolRefField) || field.name === undefined) {
        continue;
      }
      if (part.kind === 'field' && part.name !== field.name) {
        continue;
      }
      const kinds = catalogFieldOptions(block.type, field.name).kinds ?? 'variables';
      const sym = field.getRef()?.ref;
      if (!VARIABLE_KINDS.has(kinds) || sym === undefined || known.has(sym)) {
        continue;
      }
      if (declaredOnCanvas(context.workspace, sym)) {
        continue;
      }
      let name: string | null;
      try {
        name = getEditorServices().symbols.nameOf(sym);
      } catch {
        name = null;
      }
      if (name !== null && name !== '') {
        return { sym, name };
      }
    }
  }
  return null;
}

function createVariableFix(diagnostic: Diagnostic, context: FixContext): QuickFix | null {
  const block = blockOf(diagnostic, context);
  const found = block === null ? null : danglingVariable(block, diagnostic, context);
  if (block === null || found === null) {
    return null;
  }
  return {
    id: `${diagnostic.code}:${block.id}`,
    title: `Create variable “${found.name}”`,
    apply() {
      const still = danglingVariable(block, diagnostic, context);
      if (block.isDeadOrDying() || still === null) {
        return false;
      }
      return asOneStep(() => {
        const start = startTokensForDeclaredType('int');
        const item = presetBlock('var.declare', {
          fields: { TYPE: 'int', NAME: { sym: still.sym, name: still.name }, CONST: false },
          ...(start === null ? {} : { inputs: { VALUE: start } }),
        });
        const made = create(context.workspace, blockState(item));
        if (made === null) {
          return null;
        }
        const body = bodyBlock(block);
        const mouth = body?.getInput('BODY')?.connection ?? null;
        if (
          mouth !== null &&
          context.workspace.connectionChecker.canConnect(made.previousConnection, mouth, false)
        ) {
          made.previousConnection.connect(mouth);
        } else {
          const root = block.getRootBlock();
          const at = root.getRelativeToSurfaceXY();
          made.moveTo(new Blockly.utils.Coordinate(at.x, at.y - 60));
          made.bumpNeighbours();
        }
        return made;
      });
    },
  };
}

// --- B2C-E0410: add a return at the end --------------------------------------------------------

/** The start value of a function's result type (its RETURNS field), or none. */
function resultStart(define: Blockly.Block): readonly TokenJson[] {
  const returns: unknown = define.getFieldValue('RETURNS');
  return typeof returns === 'string' ? (startTokensForDeclaredType(returns) ?? []) : [];
}

function addReturnFix(diagnostic: Diagnostic, context: FixContext): QuickFix | null {
  const block = blockOf(diagnostic, context);
  const define = block === null ? null : enclosing(block, DEFINE);
  const applies = (fn: Blockly.Block): boolean =>
    !fn.isDeadOrDying() && lastStatement(fn, 'BODY')?.type !== 'func.return';
  if (define === null || !applies(define)) {
    return null;
  }
  return {
    id: `${diagnostic.code}:${define.id}`,
    title: 'Add a “return” block at the end',
    apply() {
      if (!applies(define)) {
        return false;
      }
      return asOneStep(() => {
        const start = resultStart(define);
        const item = presetBlock(
          'func.return',
          start.length === 0 ? {} : { inputs: { VALUE: start } },
        );
        const made = create(context.workspace, blockState(item));
        if (made === null) {
          return null;
        }
        const last = lastStatement(define, 'BODY');
        const target = last === null ? define.getInput('BODY')?.connection : last.nextConnection;
        if (target === null || target === undefined) {
          made.dispose(false);
          return null;
        }
        made.previousConnection.connect(target);
        return made;
      });
    },
  };
}

// --- B2C-E0403: give the return a value --------------------------------------------------------

function returnValueFix(diagnostic: Diagnostic, context: FixContext): QuickFix | null {
  const block = blockOf(diagnostic, context);
  if (block?.type !== 'func.return') {
    return null;
  }
  const define = enclosing(block, DEFINE);
  const start = define === null ? [] : resultStart(define);
  if (start.length === 0) {
    return null;
  }
  return {
    id: `${diagnostic.code}:${block.id}`,
    title: 'Give back a value',
    apply() {
      if (block.isDeadOrDying()) {
        return false;
      }
      return asOneStep(() => {
        // Nothing when the return holds a value already (the analysis was out of date).
        const change = giveStartValue(block, 'VALUE', start);
        if (change === null) {
          return null;
        }
        Blockly.Events.fire(change);
        return block;
      });
    },
  };
}

// --- B2C-W0508: add the missing cases ----------------------------------------------------------

/** The enum a switch's value has, and the enumerators its cases do not name yet. */
function missingCases(
  block: Blockly.BlockSvg,
): { readonly enumId: string; readonly missing: readonly SymbolInfo[] } | null {
  const types = getEditorServices().types;
  const value = block.getInputTargetBlock('VALUE');
  if (value === null || types.symbol === undefined) {
    return null;
  }
  let type;
  try {
    type = types.outputTypeOf(value);
  } catch {
    return null;
  }
  if (type?.kind !== 'named') {
    return null;
  }
  const enumSymbol = types.symbol(type.id);
  if (enumSymbol?.kind !== 'enum') {
    return null;
  }
  const named = new Set<string>();
  for (const input of block.inputList) {
    if (!/^CASE\d+$/u.test(input.name)) {
      continue;
    }
    const field = input.connection?.targetBlock()?.getField('VALUE');
    const ref = field instanceof B2cSymbolRefField ? field.getRef()?.ref : undefined;
    if (ref !== undefined) {
      named.add(ref);
    }
  }
  const values = (enumSymbol as { values?: readonly string[] }).values ?? [];
  const missing = values
    .filter((id) => !named.has(id))
    .map((id) => types.symbol?.(id) ?? null)
    .filter((symbol): symbol is SymbolInfo => symbol !== null);
  return missing.length === 0 ? null : { enumId: enumSymbol.id, missing };
}

/** The most cases one fix adds (the block's own limit is 64 parts). */
const MAX_ADDED_CASES = 32;

function addCasesFix(diagnostic: Diagnostic, context: FixContext): QuickFix | null {
  const block = blockOf(diagnostic, context);
  if (block?.type !== 'control.switch' || !hasB2cMutator(block)) {
    return null;
  }
  const found = missingCases(block);
  if (found === null) {
    return null;
  }
  const names = found.missing.slice(0, 3).map((symbol) => symbol.name);
  const more = found.missing.length > 3 ? ` and ${String(found.missing.length - 3)} more` : '';
  return {
    id: `${diagnostic.code}:${block.id}`,
    title: `Add ${found.missing.length === 1 ? 'a case' : 'cases'} for ${names.join(', ')}${more}`,
    apply() {
      const now = block.isDeadOrDying() ? null : missingCases(block);
      if (now === null || !hasB2cMutator(block)) {
        return false;
      }
      return asOneStep(() => {
        const extra = block.b2cGetExtra();
        const count = typeof extra['caseCount'] === 'number' ? extra['caseCount'] : 0;
        const adding = now.missing.slice(0, MAX_ADDED_CASES);
        asUndoStep(
          block,
          () => block.b2cGetExtra(),
          () => {
            block.b2cSetExtra({ ...extra, caseCount: Math.min(64, count + adding.length) });
          },
        );
        adding.forEach((symbol, index) => {
          const input = block.getInput(`CASE${String(count + index)}`);
          const made = create(context.workspace, {
            type: 'type.enum_value',
            fields: { ENUM: { ref: now.enumId }, VALUE: { ref: symbol.id } },
          });
          if (input?.connection != null && made !== null) {
            made.outputConnection.connect(input.connection);
          }
        });
        return block;
      });
    },
  };
}

// --- B2C-E0405: add the program block ----------------------------------------------------------

function addMainFix(diagnostic: Diagnostic, context: FixContext): QuickFix | null {
  const applies = (): boolean =>
    !context.workspace.getAllBlocks(false).some((block) => block.type === MAIN);
  if (!applies() || context.workspace.isReadOnly()) {
    return null;
  }
  return {
    id: `${diagnostic.code}:project`,
    title: 'Add “when program starts”',
    apply() {
      if (!applies()) {
        return false;
      }
      return asOneStep(() => {
        const made = create(context.workspace, blockState(presetBlock(MAIN)));
        if (made === null) {
          return null;
        }
        const view = context.workspace.getMetricsManager().getViewMetrics(true);
        made.moveTo(new Blockly.utils.Coordinate(view.left + 40, view.top + 40));
        made.bumpNeighbours();
        return made;
      });
    },
  };
}

/** The fix makers by diagnostic code. */
const FIXES: Readonly<
  Record<string, (diagnostic: Diagnostic, context: FixContext) => QuickFix | null>
> = {
  'B2C-E0201': createVariableFix,
  'B2C-E0410': addReturnFix,
  'B2C-W0508': addCasesFix,
  'B2C-E0405': addMainFix,
  'B2C-E0403': returnValueFix,
};

/** The codes that have a quick fix. */
export const FIXABLE_CODES: readonly string[] = Object.keys(FIXES);

/** The fixes of `diagnostic` that apply now (none for most codes). */
export function fixesFor(diagnostic: Diagnostic, context: FixContext): QuickFix[] {
  const make = Object.hasOwn(FIXES, diagnostic.code) ? FIXES[diagnostic.code] : undefined;
  if (make === undefined || context.workspace.isReadOnly()) {
    return [];
  }
  try {
    const fix = make(diagnostic, context);
    return fix === null ? [] : [fix];
  } catch (error: unknown) {
    console.error('The quick fixes could not be worked out', error);
    return [];
  }
}
