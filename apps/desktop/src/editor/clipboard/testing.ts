/**
 * Test support for the clipboard tests (the app never imports this file): an editing session on a
 * workspace with the real compiler core and the clipboard plugin attached, with an in-test copy,
 * notifier and command registry, plus helpers to find blocks in a document.
 */
import type { BdmBlock, BdmDocument, CoreWasm } from '@blocks2cpp/b2c-core-wasm';
import * as Blockly from 'blockly/core';

import { type CommandRegistry, createCommandRegistry } from '../../app/commands';
import { getCore } from '../../app/core';
import type { EditorContext } from '../../app/editor-types';
import { useAppStore } from '../../app/store';
import { GUESSING_GAME_TEXT } from '../diagnostics/testing';
import { forEachNode } from '../sync/bdmTree';
import { loadText, startSession, type TestSession, testCore, WASM_REQUIRED } from '../sync/testing';
import { ClipboardController } from './controller';
import { ClipboardMemory } from './memory';
import type { ClipboardNotice } from './notices';
import { createClipboardPlugin, type EditorClipboard } from './plugin';

/** Whether the compiler core is built (`pnpm --filter @blocks2cpp/b2c-core-wasm build`). */
const CORE_BUILT =
  Object.keys(import.meta.glob('../../../../../packages/b2c-core-wasm/pkg/b2c_core_wasm_bg.wasm'))
    .length > 0;

/**
 * Whether the tests that need the real compiler core run: it is built, or the run requires it
 * (`B2C_REQUIRE_WASM`, as in CI), in which case a missing build fails them.
 */
export const WITH_CORE = CORE_BUILT || WASM_REQUIRED;

/** The real compiler core, or a failed test. */
export async function requireCore(): Promise<CoreWasm> {
  const core = await testCore();
  if (core === null) {
    throw new Error('the compiler core is not built');
  }
  return core;
}

/** The guessing game as the core loads it, optionally with its text changed first. */
export function loadGuessingGame(
  core: CoreWasm,
  edit: (text: string) => string = (text) => text,
): BdmDocument {
  return loadText(core, edit(GUESSING_GAME_TEXT));
}

/** What a seeded block or symbol ID looks like (`SeededIds`: a prefix and 17 base-62 digits). */
export const FRESH_BLOCK_ID = /^blk_[0-9A-Za-z]{17}$/;
export const FRESH_SYMBOL_ID = /^sym_[0-9A-Za-z]{17}$/;

/** A deterministic seed for a paste: 64 hex digits from a counter. */
export function counterSeeds(): () => string {
  let next = 1;
  return () => {
    const value = next.toString(16).padStart(64, '0');
    next += 1;
    return value;
  };
}

/** The editor context a plugin gets for a session. */
export function contextFor(session: TestSession, workspace: Blockly.WorkspaceSvg): EditorContext {
  return {
    workspace,
    store: useAppStore,
    core: () => getCore(),
    selectBlock: (id, options) => {
      session.session.selectBlock(id, options);
    },
    activeModuleId: () => session.session.shownModuleId() ?? '',
  };
}

/** A session with the clipboard plugin attached to its rendered workspace. */
export interface ClipboardTestEditor {
  readonly session: TestSession;
  readonly workspace: Blockly.WorkspaceSvg;
  readonly clipboard: EditorClipboard;
  readonly memory: ClipboardMemory;
  readonly notices: ClipboardNotice[];
  readonly commands: CommandRegistry;
  /** The document the canvas shows now. */
  document(): BdmDocument;
  /** Detaches the plugin and ends the session. */
  dispose(): void;
}

/** Starts a session on `workspace` (a rendered one) showing `doc`, with the plugin attached. */
export function clipboardEditor(
  core: CoreWasm,
  doc: BdmDocument,
  workspace: Blockly.WorkspaceSvg,
  options: { memory?: ClipboardMemory } = {},
): ClipboardTestEditor {
  const session = startSession(core, doc, { workspace });
  const notices: ClipboardNotice[] = [];
  const memory = options.memory ?? new ClipboardMemory();
  const commands = createCommandRegistry();
  const attached: EditorClipboard[] = [];
  const plugin = createClipboardPlugin({
    memory,
    notify: (notice) => notices.push(notice),
    commands,
    seed: counterSeeds(),
    restartCore: null,
    onAttached: (clipboard) => {
      attached.push(clipboard);
    },
  });
  const detach = plugin.attach(contextFor(session, workspace));
  const clipboard = attached[0];
  if (clipboard === undefined) {
    throw new Error('the clipboard plugin did not attach');
  }
  return {
    session,
    workspace,
    clipboard,
    memory,
    notices,
    commands,
    document: () => session.session.currentDocument(),
    dispose() {
      detach();
      session.dispose();
    },
  };
}

/** A clipboard controller on a (headless or rendered) session's workspace, without the plugin. */
export function sessionController(
  session: TestSession,
  options: { memory?: ClipboardMemory; notices?: ClipboardNotice[] } = {},
): ClipboardController {
  const notices = options.notices ?? [];
  return new ClipboardController({
    workspace: session.workspace,
    store: useAppStore,
    core: () => getCore(),
    activeModuleId: () => session.session.shownModuleId() ?? '',
    memory: options.memory ?? new ClipboardMemory(),
    notify: (notice) => notices.push(notice),
    seed: counterSeeds(),
    restartCore: null,
  });
}

/** Every block node of a document, by ID (stacks and nested blocks included). */
export function nodesById(doc: BdmDocument): Map<string, BdmBlock> {
  const found = new Map<string, BdmBlock>();
  for (const module of doc.modules) {
    forEachNode(module.workspace.blocks, (node) => {
      found.set(node.id, node);
    });
  }
  return found;
}

/** The node with `id` in `doc`, or a failed test. */
export function nodeOf(doc: BdmDocument, id: string): BdmBlock {
  const node = nodesById(doc).get(id);
  if (node === undefined) {
    throw new Error(`the document has no block ${id}`);
  }
  return node;
}

/** The workspace block with `id`, or a failed test. */
export function canvasBlock(workspace: Blockly.Workspace, id: string): Blockly.BlockSvg {
  const block = workspace.getBlockById(id);
  if (!(block instanceof Blockly.BlockSvg)) {
    throw new Error(`the canvas has no block ${id}`);
  }
  return block;
}

/** A statement list of a node, or a failed test. */
export function listOf(node: BdmBlock, input: string): BdmBlock[] {
  const list = node.statements?.[input];
  if (list === undefined) {
    throw new Error(`block ${node.id} has no statement list ${input}`);
  }
  return list;
}

/** Lets Blockly fire its queued events (they run on a timer). */
export async function settle(): Promise<void> {
  for (let round = 0; round < 3; round++) {
    await new Promise((resolve) => setTimeout(resolve, 0));
  }
}
