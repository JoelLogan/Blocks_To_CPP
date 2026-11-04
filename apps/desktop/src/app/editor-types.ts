/**
 * The seam between the shell and the block editor. The editor component (milestone M2, wave 3)
 * publishes an {@link EditorHandle} with {@link setEditorHandle}; features reach the editor only
 * through it. Editor plugins (the toolbox, diagnostics, the clipboard) receive an
 * {@link EditorContext} when the workspace exists and return their clean-up function.
 */
import type { BdmDocument, BdmProject, CoreWasm } from '@blocks2cpp/b2c-core-wasm';
import type * as Blockly from 'blockly/core';

import type { KeptModule } from '../features/project/modules/operations';
import type { useAppStore } from './store';

/** Options for selecting a block. */
export interface SelectBlockOptions {
  /** Scroll the workspace so the block is in the middle. */
  center?: boolean;
}

/**
 * The project settings of the project settings page (05 §5.3): the language, the generator
 * options, the build configurations and defines, and the run arguments and working folder.
 */
export type ProjectSettings = Pick<BdmProject, 'language' | 'options' | 'build' | 'run'>;

/** The project's own data that the editor changes outside the canvas (05 §5.3). */
export interface ProjectInfo {
  /** The project's display name, already checked (04 §4.1: 1–100 characters on one line). */
  name: string;
  /** New project settings, already checked; absent leaves them as they are. */
  settings?: ProjectSettings;
}

/**
 * What a module edit came to (docs/plans/m3-contracts/w3-modules.md §1.2): `done`; `unchecked`,
 * refused because the shown canvas holds a change that cannot be read back (the banner, 04 §4.2);
 * `busy`, nothing changed because a block is being dragged or no project is shown; `refused`, the
 * project's modules do not allow it (a 257th module, deleting the only one, an unknown module).
 */
export type ModuleEditOutcome = 'done' | 'unchecked' | 'busy' | 'refused';

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
  /**
   * Changes the open project's data (its name) as an edit: the project has unsaved changes, and
   * the live preview runs again, so the generated code's header comment follows. Blockly's undo
   * history covers the canvas only, so this is not undone by Undo (04 §4.1). Nothing happens
   * without a project or when nothing changes.
   */
  updateProjectInfo(info: ProjectInfo): void;
  /**
   * Undoes the last change on the canvas, as `Ctrl+Z` on the canvas does; not during a drag. It
   * acts once Blockly has delivered the changes made so far, after the next animation frame or a
   * few.
   */
  undo(): void;
  /** Redoes the last undone change on the canvas; not during a drag, and as {@link undo} acts. */
  redo(): void;
  /**
   * Module edits (w3-modules.md §1.2, §6 W3M-D6): edits of the project, made from the canvas as it
   * is and previewed at once, not on the canvas's Undo. Names must have been checked
   * (features/project/modules/names.ts). Adding shows the new module, and restoring the restored
   * one; deleting the shown module shows its neighbour and keeps the deleted one for
   * {@link restoreModule}.
   */
  addModule(id: string, name: string): ModuleEditOutcome;
  renameModule(id: string, name: string): ModuleEditOutcome;
  deleteModule(id: string): {
    readonly outcome: ModuleEditOutcome;
    readonly kept: KeptModule | null;
  };
  restoreModule(kept: KeptModule): ModuleEditOutcome;
  moveModule(id: string, delta: -1 | 1): ModuleEditOutcome;
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
