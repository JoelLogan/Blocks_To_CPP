/**
 * No false refusal (ADR-0018, docs/plans/m3-contracts/slots.md §9), with the real compiler core:
 * for every example project, a seeded sample of (value block, value input) pairs of one module
 * whose parent is attached and enabled; wherever the editor's `candidateFit` is an error (so a drag
 * or a keyboard move would be refused), the document with that connection made has an analyser
 * error at the moved block or at the parent. The editor refuses only what the analyser reports.
 *
 * Parked parents are checked apart (ADR-0018 §7, its owner point 6): the editor types a parked
 * block's slots from its own parts and refuses a wrong drop there, which the analyser reports only
 * once the stack is attached (ADR-0012). So each example's bodies are parked (`main`'s and every
 * function's statements taken off as loose stacks), refusals into a parked parent are sampled
 * there, and each one is checked in the example as it was, with the parent attached: the
 * connection made there has an analyser error at the moved block or at the parent.
 */
import type { BdmDocument, CoreWasm, PreviewResult } from '@blocks2cpp/b2c-core-wasm';
import { candidateFit, isFitError } from '@blocks2cpp/blockly-ext';
import * as Blockly from 'blockly/core';
import { afterEach, describe, expect, it } from 'vitest';

import { createTypeOracle } from '../services/symbols';
import { loadModule } from './bdmToWorkspace';
import {
  disposeWorkspaces,
  EXAMPLE_PROJECTS,
  headlessWorkspace,
  loadText,
  testCore,
} from './testing';
import { readModule } from './workspaceToBdm';

const core = await testCore();

afterEach(() => {
  disposeWorkspaces();
});

/** How many refusals the samples checked, over every example. */
let totalRefused = 0;

/** The most pairs sampled per example. */
const SAMPLES = 600;

/** A tiny deterministic generator (xorshift), so the sample never changes between runs. */
function rng(seed: number): (below: number) => number {
  let state = seed;
  return (below) => {
    state ^= state << 13;
    state ^= state >>> 17;
    state ^= state << 5;
    return Math.abs(state) % Math.max(below, 1);
  };
}

/** Whether a block is attached to `main` or a function, through enabled blocks only. */
function attachedAndEnabled(block: Blockly.Block): boolean {
  for (let current: Blockly.Block | null = block; current !== null; current = current.getParent()) {
    if (!current.isEnabled()) {
      return false;
    }
  }
  const root = block.getRootBlock();
  return root.type === 'program.main' || root.type === 'func.define';
}

/** The value inputs of the blocks of `workspace` whose block is attached and enabled. */
function valueInputs(workspace: Blockly.Workspace): [Blockly.Block, string][] {
  const inputs: [Blockly.Block, string][] = [];
  for (const block of workspace.getAllBlocks(true)) {
    if (block.isShadow() || !attachedAndEnabled(block)) {
      continue;
    }
    for (const input of block.inputList) {
      if (input.type === Blockly.inputs.inputTypes.VALUE && input.connection !== null) {
        inputs.push([block, input.name]);
      }
    }
  }
  return inputs;
}

/** Whether a block is in a parked stack (loose on the canvas, not under a hat) and enabled. */
function parkedAndEnabled(block: Blockly.Block): boolean {
  for (let current: Blockly.Block | null = block; current !== null; current = current.getParent()) {
    if (!current.isEnabled()) {
      return false;
    }
  }
  const root = block.getRootBlock();
  return root.type !== 'program.main' && root.type !== 'func.define';
}

/** Parks every body of `workspace`: the statements of `main` and of each function, as loose stacks. */
function parkBodies(workspace: Blockly.Workspace): void {
  for (const root of workspace.getTopBlocks(false)) {
    if (root.type !== 'program.main' && root.type !== 'func.define') {
      continue;
    }
    const first = root.getInputTargetBlock('BODY');
    // Without healing: the statements below the first go with it, as one stack.
    first?.unplug(false);
  }
}

/** The (child, parent, input) pairs sampled from `values` and `inputs`, without impossible ones. */
function* sampled(
  values: readonly Blockly.Block[],
  inputs: readonly [Blockly.Block, string][],
  seed: number,
): Generator<[Blockly.Block, Blockly.Block, string]> {
  const pick = rng(seed);
  for (let i = 0; i < SAMPLES && values.length > 0 && inputs.length > 0; i += 1) {
    const child = values[pick(values.length)];
    const target = inputs[pick(inputs.length)];
    if (child === undefined || target === undefined) {
      continue;
    }
    const [parent, input] = target;
    // A block cannot go into itself or into a block inside it.
    if (child === parent || child.getDescendants(false).includes(parent)) {
      continue;
    }
    yield [child, parent, input];
  }
}

/**
 * The analysis of module `moduleId` of `doc` with `child` connected to input `input` of `parent`
 * (in a fresh copy of the module, the child taken out of where it was).
 */
function analyseConnected(
  wasm: CoreWasm,
  doc: BdmDocument,
  moduleId: string,
  child: Blockly.Block,
  parent: Blockly.Block,
  input: string,
): PreviewResult {
  const moved = headlessWorkspace();
  loadModule(moved, doc, moduleId);
  const movedChild = moved.getBlockById(child.id);
  const movedParent = moved.getBlockById(parent.id);
  const connection = movedParent?.getInput(input)?.connection ?? null;
  if (movedChild?.outputConnection == null || connection === null) {
    throw new Error('the copy lacks a block');
  }
  movedChild.unplug(true);
  connection.targetBlock()?.unplug(false);
  connection.connect(movedChild.outputConnection);
  return analyse(wasm, readModule(moved, doc, moduleId));
}

/** Expects an analyser error at `child` or `parent` in `after`. */
function expectErrorAt(
  after: PreviewResult,
  child: Blockly.Block,
  parent: Blockly.Block,
  input: string,
): void {
  const errors = after.diagnostics.filter(
    (diagnostic) =>
      diagnostic.severity === 'error' &&
      (diagnostic.primary.block === child.id || diagnostic.primary.block === parent.id),
  );
  expect(
    errors.length,
    `${child.type} ${child.id} into ${parent.type} ${parent.id}.${input}: ${after.diagnostics
      .map((diagnostic) => `${diagnostic.code}@${String(diagnostic.primary.block)}`)
      .join(', ')}`,
  ).toBeGreaterThan(0);
}

/** The analysis of `doc`. */
function analyse(wasm: CoreWasm, doc: BdmDocument): PreviewResult {
  return wasm.update(JSON.stringify(doc), { indentWidth: 4 });
}

describe.skipIf(core === null)('no false refusal', () => {
  it.each(Object.entries(EXAMPLE_PROJECTS))('in %s', (_name, text) => {
    if (core === null) {
      return;
    }
    const doc = loadText(core, text);
    const moduleId = doc.modules[0]?.id;
    if (moduleId === undefined) {
      return;
    }
    const workspace = headlessWorkspace();
    loadModule(workspace, doc, moduleId);
    const preview = analyse(core, doc);
    const oracle = createTypeOracle({ preview: () => preview, core: () => core, workspace });
    const values = workspace
      .getAllBlocks(true)
      .filter((block) => !block.isShadow() && block.outputConnection !== null);
    const inputs = valueInputs(workspace);
    let refused = 0;
    for (const [child, parent, input] of sampled(values, inputs, 0x5eed + text.length)) {
      if (!isFitError(candidateFit(parent, input, child, oracle))) {
        continue;
      }
      refused += 1;
      expectErrorAt(
        analyseConnected(core, doc, moduleId, child, parent, input),
        child,
        parent,
        input,
      );
    }
    totalRefused += refused;
  });

  it('checked refusals in the samples', () => {
    // The samples hold refusals the analyser had to confirm (text into a number, a value into a
    // `create` of another type, …).
    expect(totalRefused).toBeGreaterThanOrEqual(50);
  });
});

/** How many refusals into parked parents the samples checked, over every example. */
let parkedRefused = 0;

describe.skipIf(core === null)('no false refusal into parked parents, once attached', () => {
  it.each(Object.entries(EXAMPLE_PROJECTS))('in %s', (_name, text) => {
    if (core === null) {
      return;
    }
    const doc = loadText(core, text);
    const moduleId = doc.modules[0]?.id;
    if (moduleId === undefined) {
      return;
    }
    const workspace = headlessWorkspace();
    loadModule(workspace, doc, moduleId);
    parkBodies(workspace);
    // The editor works with the analysis of the canvas as it is: the bodies parked.
    const preview = analyse(core, readModule(workspace, doc, moduleId));
    const oracle = createTypeOracle({ preview: () => preview, core: () => core, workspace });
    const values = workspace
      .getAllBlocks(true)
      .filter((block) => !block.isShadow() && block.outputConnection !== null);
    const inputs: [Blockly.Block, string][] = [];
    for (const block of workspace.getAllBlocks(true)) {
      if (block.isShadow() || !parkedAndEnabled(block)) {
        continue;
      }
      for (const input of block.inputList) {
        if (input.type === Blockly.inputs.inputTypes.VALUE && input.connection !== null) {
          inputs.push([block, input.name]);
        }
      }
    }
    for (const [child, parent, input] of sampled(values, inputs, 0xbeef + text.length)) {
      if (!isFitError(candidateFit(parent, input, child, oracle))) {
        continue;
      }
      parkedRefused += 1;
      // Attached again: the example as it was, with the connection made.
      expectErrorAt(
        analyseConnected(core, doc, moduleId, child, parent, input),
        child,
        parent,
        input,
      );
    }
  });

  it('checked refusals into parked parents', () => {
    // The parked bodies hold typed blocks (a `create` of a type, a `set` of a known variable, a
    // call), so the samples refuse some drops there too.
    expect(parkedRefused).toBeGreaterThanOrEqual(20);
  });
});
