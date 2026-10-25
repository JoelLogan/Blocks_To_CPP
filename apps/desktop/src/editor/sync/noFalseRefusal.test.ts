/**
 * No false refusal (ADR-0018, docs/plans/m3-contracts/slots.md §9), with the real compiler core:
 * for every example project, a seeded sample of (value block, value input) pairs of one module
 * whose parent is attached and enabled; wherever the editor's `candidateFit` is an error (so a drag
 * or a keyboard move would be refused), the document with that connection made has an analyser
 * error at the moved block or at the parent. The editor refuses only what the analyser reports.
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
    const pick = rng(0x5eed + text.length);
    let refused = 0;
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
      if (!isFitError(candidateFit(parent, input, child, oracle))) {
        continue;
      }
      refused += 1;
      // Make the connection in a fresh copy of the module, and analyse it.
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
      const after = analyse(core, readModule(moved, doc, moduleId));
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
    totalRefused += refused;
  });

  it('checked refusals in the samples', () => {
    // The samples hold refusals the analyser had to confirm (text into a number, a value into a
    // `create` of another type, …).
    expect(totalRefused).toBeGreaterThanOrEqual(50);
  });
});
