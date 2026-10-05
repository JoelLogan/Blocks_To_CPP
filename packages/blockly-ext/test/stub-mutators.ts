/**
 * Stand-ins for the four Blocks2Cpp mutators (`b2c_mutator_items`, `b2c_mutator_if`,
 * `b2c_mutator_call_args`, `b2c_mutator_params`) for tests of block registration while the real
 * mutators (src/mutators/, registered by `registerB2cMutators()`) are not part of the build.
 *
 * Tests opt in by calling {@link registerStubMutators}; a name that is already registered (for
 * example by the real mutators) is left alone. It is deliberately not done in setup.ts, which runs
 * for every test file, so the real mutators' own tests never meet a stub.
 *
 * A stub follows the mutator contract (src/blocks/mutators.ts) in a simple way: it keeps `extra` as
 * given, and creates the repeated inputs and statements (`NAME0` … `NAME{n-1}`, and statements
 * that exist while a flag is set) in front of the block's repeat anchor. It has no ⊕/⊖ buttons.
 */
import * as Blockly from 'blockly/core';

import { REPEAT_ANCHOR, catalogBlock, MUTATOR_NAME, type BlockExtra } from '../src';
import type { BlockDefJson } from '../src/generated/catalog';

interface StubMutatorBlock extends Blockly.Block {
  b2cExtra: BlockExtra;
  b2cSetExtra(extra: BlockExtra): void;
}

/** The catalog default of every `extra` key of a block. */
function defaultExtra(def: BlockDefJson): BlockExtra {
  const extra: BlockExtra = {};
  for (const key of def.extra) {
    if (key.kind === 'params') {
      extra[key.name] = [];
    } else if (key.default !== null) {
      extra[key.name] = key.default;
    }
  }
  return extra;
}

function count(extra: BlockExtra, key: string): number {
  const value = extra[key];
  return typeof value === 'number' && Number.isInteger(value) && value >= 0 ? value : 0;
}

/** Creates the parts `extra` asks for and removes the others, keeping catalog order. */
function rebuildParts(block: StubMutatorBlock, def: BlockDefJson): void {
  const wanted: { name: string; statement: boolean }[] = [];
  for (const input of def.inputs) {
    if (input.repeat !== null) {
      const parts = count(block.b2cExtra, input.repeat.count) + input.repeat.plus;
      for (let index = 0; index < parts; index++) {
        wanted.push({ name: `${input.name}${String(index)}`, statement: false });
      }
    }
  }
  for (const statement of def.statements) {
    if (statement.repeat !== null) {
      const parts = count(block.b2cExtra, statement.repeat.count) + statement.repeat.plus;
      for (let index = 0; index < parts; index++) {
        wanted.push({ name: `${statement.name}${String(index)}`, statement: true });
      }
    } else if (statement.when !== null && block.b2cExtra[statement.when] === true) {
      wanted.push({ name: statement.name, statement: true });
    }
  }
  const wantedNames = new Set(wanted.map((part) => part.name));
  const owned = new Set(
    [
      ...def.inputs.filter((input) => input.repeat !== null),
      ...def.statements.filter((s) => s.repeat !== null || s.when !== null),
    ].map((part) => part.name),
  );
  for (const input of [...block.inputList]) {
    const base = input.name.replace(/\d+$/, '');
    if ((owned.has(base) || owned.has(input.name)) && !wantedNames.has(input.name)) {
      block.removeInput(input.name);
    }
  }
  for (const part of wanted) {
    if (block.getInput(part.name) === null) {
      if (part.statement) {
        block.appendStatementInput(part.name);
      } else {
        block.appendValueInput(part.name);
      }
      block.moveInputBefore(part.name, REPEAT_ANCHOR);
    }
  }
}

function stubMixin(): object {
  return {
    saveExtraState(this: StubMutatorBlock): BlockExtra {
      return structuredClone(this.b2cExtra);
    },
    loadExtraState(this: StubMutatorBlock, state: unknown): void {
      if (typeof state === 'object' && state !== null && !Array.isArray(state)) {
        this.b2cSetExtra(state as BlockExtra);
      }
    },
    b2cGetExtra(this: StubMutatorBlock): BlockExtra {
      return structuredClone(this.b2cExtra);
    },
    b2cSetExtra(this: StubMutatorBlock, extra: BlockExtra): void {
      const def = catalogBlock(this.type);
      this.b2cExtra = {
        ...(def === undefined ? {} : defaultExtra(def)),
        ...structuredClone(extra),
      };
      if (def !== undefined) {
        rebuildParts(this, def);
      }
    },
  };
}

function stubInit(this: StubMutatorBlock): void {
  const def = catalogBlock(this.type);
  this.b2cExtra = def === undefined ? {} : defaultExtra(def);
  if (def !== undefined) {
    rebuildParts(this, def);
  }
}

/** Registers a stub for each mutator name that is not registered yet. */
export function registerStubMutators(): void {
  for (const name of Object.values(MUTATOR_NAME)) {
    if (!Blockly.Extensions.isRegistered(name)) {
      Blockly.Extensions.registerMutator(name, stubMixin(), stubInit);
    }
  }
}
