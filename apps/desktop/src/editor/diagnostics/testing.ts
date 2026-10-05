/**
 * Helpers for the diagnostics and highlighting tests (the app never imports this file): Blockly
 * set-up, a rendered workspace, the blocks of a BDM document built with the document's IDs, an
 * editor context and handle over a workspace, and the guessing game.
 *
 * `buildBlocks` is a small stand-in for the editor's sync: it gives every block its document ID,
 * nesting and mutator state, and expression slots their shadows, which is all the badges and the
 * highlighting look at. Fields keep their defaults.
 */
import type { BdmBlock, BdmDocument, CoreWasm } from '@blocks2cpp/b2c-core-wasm';
import {
  b2cLightTheme,
  catalogBlock,
  exprShadowState,
  installIdGenerator,
  isB2cMutatorBlock,
  placeholderState,
  registerB2cBlocks,
  registerB2cMutators,
} from '@blocks2cpp/blockly-ext';
import * as Blockly from 'blockly/core';

import guessingGameText from '../../../../../examples/guessing_game.b2c?raw';
import type { EditorContext, EditorHandle, SelectBlockOptions } from '../../app/editor-types';
import { useAppStore } from '../../app/store';

let registered = false;

/** Registers the ID generator, the mutators and every catalog block type (once per test file). */
export function setUpBlocks(): void {
  if (registered) {
    return;
  }
  registered = true;
  installIdGenerator();
  registerB2cMutators();
  registerB2cBlocks();
}

const workspaces: Blockly.WorkspaceSvg[] = [];

/** A workspace injected into the document with Zelos and the light theme. */
export function renderedWorkspace(): Blockly.WorkspaceSvg {
  setUpBlocks();
  const host = document.createElement('div');
  document.body.append(host);
  const workspace = Blockly.inject(host, {
    renderer: 'zelos',
    theme: b2cLightTheme,
    sounds: false,
  });
  workspaces.push(workspace);
  return workspace;
}

/** Disposes of every workspace {@link renderedWorkspace} made (call it after each test). */
export function disposeWorkspaces(): void {
  for (const workspace of workspaces.splice(0)) {
    workspace.dispose();
  }
  Blockly.Tooltip.hide();
  document.body.replaceChildren();
}

/**
 * Lets Blockly fire its queued events and draw its queued renders (both run on timers), and the
 * plugin's scheduled updates run. Rendering can queue more events, so it goes round a few times.
 */
export async function settle(): Promise<void> {
  for (let round = 0; round < 3; round++) {
    await new Promise((resolve) => setTimeout(resolve, 0));
    Blockly.renderManagement.triggerQueuedRenders();
    await Promise.resolve();
  }
}

/** The type class of a value input of a block type (repeated inputs are numbered: ITEM0, COND1). */
function inputCheck(type: string, name: string) {
  const inputs = catalogBlock(type)?.inputs ?? [];
  const def =
    inputs.find((input) => input.name === name) ??
    inputs.find((input) => input.repeat !== null && /^\d+$/.test(name.slice(input.name.length)));
  return def?.check ?? 'any';
}

/** Builds one block (and everything inside it) with its document ID. */
function buildBlock(workspace: Blockly.WorkspaceSvg, node: BdmBlock): Blockly.BlockSvg {
  if (catalogBlock(node.type) === undefined) {
    const holder = Blockly.serialization.blocks.append(placeholderState(node, 'top'), workspace);
    return holder as Blockly.BlockSvg;
  }
  const block = workspace.newBlock(node.type, node.id);
  if (node.extra !== undefined && isB2cMutatorBlock(block)) {
    block.b2cSetExtra(node.extra);
  }
  for (const [name, value] of Object.entries(node.inputs ?? {})) {
    const connection = block.getInput(name)?.connection;
    if (connection === undefined || connection === null) {
      throw new Error(`${node.type} has no input ${name}`);
    }
    if ('block' in value) {
      connection.connect(buildBlock(workspace, value.block).outputConnection);
    } else {
      connection.setShadowState(
        exprShadowState(value.expr, value.draft === true, inputCheck(node.type, name), false),
      );
    }
  }
  for (const [name, list] of Object.entries(node.statements ?? {})) {
    let connection = block.getInput(name)?.connection ?? null;
    for (const childNode of list) {
      const child = buildBlock(workspace, childNode);
      if (connection === null) {
        throw new Error(`${childNode.type} cannot go into ${node.type}.${name}`);
      }
      connection.connect(child.previousConnection);
      connection = child.nextConnection;
    }
  }
  if (node.collapsed === true) {
    block.setCollapsed(true);
  }
  return block;
}

/**
 * Builds the top-level blocks of `blocks` on `workspace`, each with its document ID, and renders
 * them. Returns the top-level blocks.
 */
export function buildBlocks(
  workspace: Blockly.WorkspaceSvg,
  blocks: readonly BdmBlock[],
): Blockly.BlockSvg[] {
  const roots = blocks.map((node) => {
    const block = buildBlock(workspace, node);
    block.moveBy(node.x ?? 0, node.y ?? 0);
    return block;
  });
  for (const block of workspace.getAllBlocks(false)) {
    block.initSvg();
  }
  for (const root of roots) {
    root.render();
  }
  Blockly.renderManagement.triggerQueuedRenders();
  return roots;
}

/** The block with `id`, or a failed test. */
export function blockById(workspace: Blockly.Workspace, id: string): Blockly.BlockSvg {
  const block = workspace.getBlockById(id);
  if (block === null) {
    throw new Error(`no block ${id}`);
  }
  return block as Blockly.BlockSvg;
}

/** The guessing game (examples/guessing_game.b2c), a trusted test input. */
export const GUESSING_GAME_TEXT: string = guessingGameText;

/** A fresh copy of the guessing game's document. */
export function guessingGame(): BdmDocument {
  return JSON.parse(GUESSING_GAME_TEXT) as BdmDocument;
}

/** A compiler core that no test here may use. */
const NO_CORE = new Proxy({} as CoreWasm, {
  get(_target, property) {
    throw new Error(`the compiler core is not available in this test (${String(property)})`);
  },
});

/** An editor context over `workspace` and the app's store, showing module `mod_main`. */
export function editorContext(
  workspace: Blockly.WorkspaceSvg,
  selectBlock: (id: string, opts?: SelectBlockOptions) => void = () => undefined,
  moduleId: () => string = () => 'mod_main',
): EditorContext {
  return {
    workspace,
    store: useAppStore,
    core: () => NO_CORE,
    selectBlock,
    activeModuleId: moduleId,
  };
}

/**
 * An editor handle over `workspace` whose `selectBlock` selects the block (as the editor does)
 * and calls `onSelect`.
 */
export function editorHandle(
  workspace: Blockly.WorkspaceSvg,
  onSelect: (id: string, opts?: SelectBlockOptions) => void = () => undefined,
): EditorHandle {
  return {
    workspace,
    loadDocument: () => {
      throw new Error('not in this test');
    },
    currentDocument: () => {
      throw new Error('not in this test');
    },
    selectBlock: (id, opts) => {
      onSelect(id, opts);
      workspace.getBlockById(id)?.select();
    },
  };
}
