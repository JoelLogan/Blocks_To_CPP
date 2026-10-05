/**
 * The seam between the shell and the block editor. The editor component (milestone M2, wave 3)
 * publishes an {@link EditorHandle} with {@link setEditorHandle}; features reach the editor only
 * through it. Editor plugins (the toolbox, diagnostics, the clipboard) receive an
 * {@link EditorContext} when the workspace exists and return their clean-up function.
 */
import type { BdmDocument, CoreWasm } from '@blocks2cpp/b2c-core-wasm';
import type * as Blockly from 'blockly/core';

import type { useAppStore } from './store';

/** Options for selecting a block. */
export interface SelectBlockOptions {
  /** Scroll the workspace so the block is in the middle. */
  center?: boolean;
}

/** What the rest of the app may do with the block editor. */
export interface EditorHandle {
  /**
   * Replaces the workspace's content with `doc`, which must come from the compiler core's loader.
   * `clearUndo` drops the undo history (a newly opened or reloaded project).
   */
  loadDocument(doc: BdmDocument, opts: { clearUndo: boolean }): void;
  /** The document as the workspace currently has it. */
  currentDocument(): BdmDocument;
  /** Selects a block (or its collapsed ancestor) and optionally centres it. */
  selectBlock(id: string, opts?: SelectBlockOptions): void;
  /** The Blockly workspace itself. */
  workspace: Blockly.WorkspaceSvg;
}

/** What an editor plugin gets when it is attached to the workspace. */
export interface EditorContext {
  workspace: Blockly.WorkspaceSvg;
  store: typeof useAppStore;
  /**
   * The compiler core as it is now, or `null` before it has started: the editor passes `getCore`
   * from `app/core.ts`, like `FeatureContext.core`.
   *
   * Plugins must call it each time they need the core and must never keep the instance it returns
   * (nor anything that holds it, such as a symbol provider built over it): after a WebAssembly
   * trap the preview pipeline replaces the core (`resetCore`, `initCore`, `setCore`) without
   * recreating the workspace or its plugins, and the old instance throws `CoreTrap` on every call.
   */
  core: () => CoreWasm | null;
  /** Selects a block (or its collapsed ancestor) and optionally centres it. */
  selectBlock(id: string, opts?: SelectBlockOptions): void;
  /** The ID of the module whose canvas is shown. */
  activeModuleId(): string;
}

/** A part of the editor that attaches to the workspace (see `EDITOR_PLUGINS`). */
export interface EditorPlugin {
  /** A short name for logs and tests. */
  name: string;
  /** Attaches to the workspace and returns the function that detaches again. */
  attach(ctx: EditorContext): () => void;
}

let current: EditorHandle | null = null;

/** Publishes the editor (or, with `null`, withdraws it when the workspace is disposed). */
export function setEditorHandle(handle: EditorHandle | null): void {
  current = handle;
}

/** The editor, or `null` while there is no workspace. */
export function getEditorHandle(): EditorHandle | null {
  return current;
}
