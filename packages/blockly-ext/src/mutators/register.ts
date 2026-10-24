/**
 * Registration of the four mutators as Blockly mutator extensions, and the members they add to
 * blocks.
 */
import * as Blockly from 'blockly/core';

import { blockDef } from '../checker/catalog';
import type { BlockDefJson } from '../generated/catalog';
import { attachController, controllerOf, type MutatorController } from './controller';
import { MutatorConfigError, MutatorStateError } from './errors';
import { ParamsController } from './params';
import { paramsSpec, type ParamsSpec, variadicSpec, type VariadicSpec } from './spec';
import {
  B2C_MUTATOR_CALL_ARGS,
  B2C_MUTATOR_IF,
  B2C_MUTATOR_ITEMS,
  B2C_MUTATOR_NAMES,
  B2C_MUTATOR_PARAMS,
  type B2cMutatorBlock,
  type B2cMutatorName,
} from './types';
import { VariadicController } from './variadic';

const VARIADIC_SPECS = new Map<string, VariadicSpec>();
const PARAMS_SPECS = new Map<string, ParamsSpec>();

/** The variadic spec of `def` for `mutator`, checked against what that mutator expects. */
function checkedVariadicSpec(def: BlockDefJson, mutator: B2cMutatorName): VariadicSpec {
  const key = `${mutator}|${def.id}`;
  const cached = VARIADIC_SPECS.get(key);
  if (cached !== undefined) {
    return cached;
  }
  const spec = variadicSpec(def);
  if (mutator !== B2C_MUTATOR_IF) {
    const [member] = spec.members;
    if (spec.members.length !== 1 || member?.kind !== 'value' || spec.gated.length > 0) {
      throw new MutatorConfigError(
        def.id,
        `${def.id}: ${mutator} needs exactly one repeated value input.`,
      );
    }
  }
  if (
    mutator === B2C_MUTATOR_CALL_ARGS &&
    !def.fields.some((field) => field.kind === 'symbol_ref')
  ) {
    throw new MutatorConfigError(
      def.id,
      `${def.id}: ${mutator} needs the field naming the function.`,
    );
  }
  VARIADIC_SPECS.set(key, spec);
  return spec;
}

function checkedParamsSpec(def: BlockDefJson): ParamsSpec {
  const cached = PARAMS_SPECS.get(def.id);
  if (cached !== undefined) {
    return cached;
  }
  const spec = paramsSpec(def);
  PARAMS_SPECS.set(def.id, spec);
  return spec;
}

function createController(block: Blockly.Block, mutator: B2cMutatorName): MutatorController {
  const def = blockDef(block.type);
  if (def === undefined) {
    throw new MutatorConfigError(
      block.type,
      `${block.type} is not a catalog block; ${mutator} needs one.`,
    );
  }
  switch (mutator) {
    case B2C_MUTATOR_PARAMS:
      return new ParamsController(block, checkedParamsSpec(def));
    case B2C_MUTATOR_ITEMS:
    case B2C_MUTATOR_IF:
    case B2C_MUTATOR_CALL_ARGS:
      return new VariadicController(block, checkedVariadicSpec(def, mutator), mutator);
  }
}

/**
 * The members every b2c mutator adds to its blocks (Blockly copies them onto each block).
 *
 * `loadExtraState` is Blockly's own path (its serialisation, Blockly-level paste, undo of a
 * mutation): state it cannot show is ignored and the block keeps its parts, as the custom-field
 * review checklist asks. `b2cSetExtra` is the editor's path and reports it with
 * `MutatorStateError`.
 */
const MIXIN = {
  saveExtraState(this: Blockly.Block): Record<string, unknown> | null {
    return controllerOf(this)?.extra() ?? null;
  },
  loadExtraState(this: Blockly.Block, state: unknown): void {
    try {
      controllerOf(this)?.apply(state);
    } catch (error) {
      if (!(error instanceof MutatorStateError)) {
        throw error;
      }
    }
  },
  b2cGetExtra(this: Blockly.Block): Record<string, unknown> {
    return controllerOf(this)?.extra() ?? {};
  },
  b2cSetExtra(this: Blockly.Block, extra: Record<string, unknown>): void {
    controllerOf(this)?.apply(extra);
  },
};

/**
 * Registers the extensions `b2c_mutator_items`, `b2c_mutator_if`, `b2c_mutator_call_args` and
 * `b2c_mutator_params`. Calling it again replaces them, and so does it replace other extensions
 * registered under these names (such as test stand-ins). Blocks created before the call keep the
 * mutators they were created with.
 */
export function registerB2cMutators(): void {
  for (const name of B2C_MUTATOR_NAMES) {
    if (Blockly.Extensions.isRegistered(name)) {
      Blockly.Extensions.unregister(name);
    }
    Blockly.Extensions.registerMutator(name, MIXIN, function (this: Blockly.Block) {
      attachController(this, createController(this, name));
    });
  }
}

/** Whether a block has one of the b2c mutators (so `b2cGetExtra` and `b2cSetExtra`). */
export function isB2cMutatorBlock(block: Blockly.Block): block is B2cMutatorBlock {
  return controllerOf(block) !== undefined;
}

/**
 * Updates what the mutators show from the latest analysis: call arguments are labelled with their
 * parameter names. Call it after each analysis.
 */
export function refreshMutatorLabels(workspace: Blockly.Workspace): void {
  for (const block of workspace.getAllBlocks(false)) {
    controllerOf(block)?.refresh();
  }
}
