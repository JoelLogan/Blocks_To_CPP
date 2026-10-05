/**
 * One editing session on a Blockly workspace: the document shown on the canvas, kept in step with
 * the app's store (02 §2.4.1). Blockly is the live editing state; the BDM derived from it is what
 * is analysed, saved and sent to the backend.
 *
 * - Loading a document builds the shown module's canvas (./bdmToWorkspace.ts) with events off.
 * - Every content change on the canvas (create, delete, change, move, comment, mutation) starts the
 *   debounced preview pipeline (../preview/pipeline.ts), which reads the canvas back
 *   (./workspaceToBdm.ts) and updates the project and the analysis.
 * - Created blocks get fresh symbol IDs where they would clash (./duplicates.ts).
 * - One module is shown at a time; switching keeps the left module's changes in the document and
 *   clears Blockly's undo history (each module has its own canvas).
 * - The viewport is captured only for saving ({@link EditorSession.currentDocument}).
 */
import type { BdmBlock, BdmDocument, BdmViewport, PreviewResult } from '@blocks2cpp/b2c-core-wasm';
import * as Blockly from 'blockly/core';

import type { ProjectState, useAppStore } from '../../app/store';
import type { CoreHost } from '../preview/coreHost';
import { PreviewPipeline } from '../preview/pipeline';
import type { PreviewService } from '../preview/service';
import { loadModule } from './bdmToWorkspace';
import { forEachNode, treeBlockIds } from './bdmTree';
import { DeclIndex, DuplicateGuard } from './duplicates';
import { SyncError } from './errors';
import { placeholderNode } from './placeholders';
import { allBlocks, clearWorkspace } from './traverse';
import {
  captureViewport,
  restoreViewport,
  sameViewState,
  type ViewState,
  viewStateOf,
} from './viewport';
import { readModule, withViewport } from './workspaceToBdm';

/** What the rest of the editor hears from a session. */
export interface SessionHooks {
  /** A document was loaded into the editor (open, reload, recovery). */
  readonly onLoaded?: (doc: BdmDocument) => void;
  /** A document read from the canvas became the project's. */
  readonly onCommitted?: (doc: BdmDocument) => void;
  /** A preview was put in the store. */
  readonly onPreviewed?: (result: PreviewResult) => void;
  /** Created blocks got fresh symbol IDs (old → new). */
  readonly onRenamed?: (syms: ReadonlyMap<string, string>) => void;
}

/** What a session works with. */
export interface EditorSessionOptions {
  readonly workspace: Blockly.Workspace;
  readonly store: typeof useAppStore;
  readonly host: CoreHost;
  /** Runs the preview; the main-thread service by default. */
  readonly service?: PreviewService;
  /** The debounce delay of the pipeline, in milliseconds. */
  readonly debounceMs?: number;
  readonly hooks?: SessionHooks;
}

/** How to select a block. */
export interface SelectOptions {
  /** Scroll the workspace so the block is in the middle. */
  readonly center?: boolean;
}

/** Whether a rendered workspace is in the middle of a drag (its blocks are in flux). */
function isDragging(workspace: Blockly.Workspace): boolean {
  return workspace instanceof Blockly.WorkspaceSvg && workspace.isDragging();
}

/** The module to show: the requested one when the document has it, else the first one. */
function pickModule(doc: BdmDocument, wanted: string | undefined): string | null {
  if (wanted !== undefined && doc.modules.some((module) => module.id === wanted)) {
    return wanted;
  }
  return doc.modules[0]?.id ?? null;
}

/** The ID of the module whose canvas holds block `id` (at any depth), or `null`. */
function moduleOfBlock(doc: BdmDocument, id: string): string | null {
  for (const module of doc.modules) {
    const ids = new Set<string>();
    forEachNode(module.workspace.blocks, (node: BdmBlock) => {
      ids.add(node.id);
    });
    if (ids.has(id)) {
      return module.id;
    }
  }
  return null;
}

/** An editing session; see the module comment. */
export class EditorSession {
  readonly workspace: Blockly.Workspace;
  private readonly store: typeof useAppStore;
  private readonly hooks: SessionHooks;
  private readonly pipeline: PreviewPipeline;
  private readonly index = new DeclIndex();
  private readonly guard: DuplicateGuard;
  private readonly unsubscribe: () => void;
  private shown: string | null = null;
  /** The viewport the shown module had when it was shown, and Blockly's view right after. */
  private shownView: {
    readonly saved: BdmViewport | undefined;
    readonly baseline: ViewState | null;
  } = { saved: undefined, baseline: null };
  /** Viewports of modules that were shown and left, to save with the document. */
  private readonly leftViews = new Map<string, BdmViewport | undefined>();
  private disposed = false;

  constructor(options: EditorSessionOptions) {
    this.workspace = options.workspace;
    this.store = options.store;
    this.hooks = options.hooks ?? {};
    this.guard = new DuplicateGuard(this.workspace, this.index, (syms) => {
      this.hooks.onRenamed?.(syms);
    });
    this.pipeline = new PreviewPipeline({
      store: options.store,
      host: options.host,
      read: this.readLive,
      ...(options.service === undefined ? {} : { service: options.service }),
      ...(options.debounceMs === undefined ? {} : { debounceMs: options.debounceMs }),
      onCommitted: (doc) => this.hooks.onCommitted?.(doc),
      onPreviewed: (result) => this.hooks.onPreviewed?.(result),
    });
    this.workspace.addChangeListener(this.onEvent);
    this.unsubscribe = this.store.subscribe((state, previous) => {
      this.onStore(state.project, previous.project);
    });
    const project = this.store.getState().project;
    if (project !== null) {
      this.loadDocument(project.document, { clearUndo: true });
    }
  }

  /** The ID of the module whose canvas is shown, or `null` when no project is shown. */
  shownModuleId(): string | null {
    return this.shown;
  }

  /** Whether a debounced preview run is waiting. */
  get syncPending(): boolean {
    return this.pipeline.pending;
  }

  /**
   * Shows `doc` (which must come from the compiler core's loader) and previews it. The module
   * shown is the project's `activeModuleId` when the document has it, else the first one.
   * `clearUndo` drops Blockly's undo history.
   */
  loadDocument(doc: BdmDocument, options: { clearUndo: boolean }): void {
    const project = this.store.getState().project;
    if (project !== null && project.document !== doc) {
      this.store.getState().actions.updateProject({ document: doc });
    }
    this.pipeline.invalidate();
    this.guard.reset();
    this.leftViews.clear();
    const moduleId = pickModule(doc, project?.activeModuleId);
    if (moduleId === null) {
      this.clearCanvas();
    } else {
      this.show(doc, moduleId, options.clearUndo);
    }
    this.hooks.onLoaded?.(doc);
    if (project !== null) {
      this.store.getState().actions.setAnalysis({ notice: null });
      void this.pipeline.runNow();
    }
  }

  /**
   * The document as the canvas has it now, with the viewports captured: what a save writes.
   *
   * @throws SyncError (`noDocument`) when no project is open.
   */
  currentDocument(): BdmDocument {
    const project = this.store.getState().project;
    if (project === null) {
      throw new SyncError('noDocument', 'No project is open.');
    }
    if (this.shown === null) {
      return project.document;
    }
    let doc = readModule(this.workspace, project.document, this.shown);
    doc = withViewport(doc, this.shown, this.shownViewport(doc, this.shown));
    for (const [moduleId, viewport] of this.leftViews) {
      if (doc.modules.some((module) => module.id === moduleId)) {
        doc = withViewport(doc, moduleId, viewport);
      }
    }
    return doc;
  }

  /** Reads the canvas and previews now, instead of after the debounce delay. */
  flush(): Promise<void> {
    return this.pipeline.runNow();
  }

  /**
   * Selects a block, or the outermost collapsed block it is in, switching to its module first.
   * A block kept inside a placeholder selects the placeholder.
   */
  selectBlock(id: string, options: SelectOptions = {}): void {
    let block = this.workspace.getBlockById(id);
    if (block === null) {
      const project = this.store.getState().project;
      const moduleId = project === null ? null : moduleOfBlock(project.document, id);
      if (moduleId !== null && moduleId !== this.shown) {
        this.switchModule(moduleId);
        block = this.workspace.getBlockById(id);
      }
    }
    block ??= this.placeholderHolding(id);
    if (block === null) {
      return;
    }
    let target = block;
    for (let parent = block.getParent(); parent !== null; parent = parent.getParent()) {
      if (parent.isCollapsed()) {
        target = parent;
      }
    }
    if (
      !(target instanceof Blockly.BlockSvg) ||
      !(this.workspace instanceof Blockly.WorkspaceSvg)
    ) {
      return;
    }
    try {
      Blockly.common.setSelected(target);
      if (options.center === true) {
        this.workspace.centerOnBlock(target.id, true);
      }
    } catch (error: unknown) {
      console.warn('The block could not be selected', error);
    }
  }

  /**
   * Shows module `moduleId`. The shown module's changes stay in the document, and Blockly's undo
   * history is cleared. An unknown module ID is ignored.
   */
  switchModule(moduleId: string): void {
    const project = this.store.getState().project;
    if (project === null || this.shown === null || moduleId === this.shown) {
      return;
    }
    const { actions } = this.store.getState();
    if (!project.document.modules.some((module) => module.id === moduleId)) {
      if (project.activeModuleId !== this.shown) {
        actions.updateProject({ activeModuleId: this.shown });
      }
      return;
    }
    let doc: BdmDocument;
    try {
      doc = readModule(this.workspace, project.document, this.shown);
    } catch (error: unknown) {
      console.error('The shown module could not be read; staying on it', error);
      return;
    }
    this.leftViews.set(this.shown, this.shownViewport(doc, this.shown));
    this.pipeline.invalidate();
    actions.updateProject({ document: doc });
    this.show(doc, moduleId, true);
    void this.pipeline.runNow();
  }

  /** Detaches from the workspace and the store. */
  dispose(): void {
    if (this.disposed) {
      return;
    }
    this.disposed = true;
    this.workspace.removeChangeListener(this.onEvent);
    this.unsubscribe();
    this.pipeline.dispose();
  }

  /** Builds module `moduleId` of `doc` on the canvas and restores its view. */
  private show(doc: BdmDocument, moduleId: string, clearUndo: boolean): void {
    loadModule(this.workspace, doc, moduleId);
    this.shown = moduleId;
    if (clearUndo) {
      this.workspace.clearUndo();
    }
    const saved = this.leftViews.has(moduleId)
      ? this.leftViews.get(moduleId)
      : doc.modules.find((module) => module.id === moduleId)?.workspace.viewport;
    this.leftViews.delete(moduleId);
    if (saved !== undefined) {
      restoreViewport(this.workspace, saved);
    }
    this.shownView = { saved, baseline: viewStateOf(this.workspace) };
    this.index.rebuild(this.workspace);
    const project = this.store.getState().project;
    if (project !== null && project.activeModuleId !== moduleId) {
      this.store.getState().actions.updateProject({ activeModuleId: moduleId });
    }
  }

  /** Empties the canvas (the project was closed). */
  private clearCanvas(): void {
    this.pipeline.invalidate();
    clearWorkspace(this.workspace);
    this.workspace.clearUndo();
    this.shown = null;
    this.shownView = { saved: undefined, baseline: null };
    this.leftViews.clear();
    this.guard.reset();
    this.index.rebuild(this.workspace);
  }

  /**
   * The shown module's viewport for saving: the one it was shown with while the user has not
   * scrolled or zoomed, else the current one. A headless workspace keeps the document's.
   */
  private shownViewport(doc: BdmDocument, moduleId: string): BdmViewport | undefined {
    const state = viewStateOf(this.workspace);
    const baseline = this.shownView.baseline;
    if (state === null || baseline === null) {
      return doc.modules.find((module) => module.id === moduleId)?.workspace.viewport;
    }
    if (sameViewState(state, baseline)) {
      return this.shownView.saved;
    }
    return captureViewport(this.workspace) ?? this.shownView.saved;
  }

  /** The placeholder on the canvas whose kept data holds block `id`, or `null`. */
  private placeholderHolding(id: string): Blockly.Block | null {
    for (const block of allBlocks(this.workspace)) {
      const kept = placeholderNode(block);
      if (kept !== null && treeBlockIds([kept]).includes(id)) {
        return block;
      }
    }
    return null;
  }

  /** The open project's document read from the canvas, for the pipeline. */
  private readonly readLive = (): BdmDocument | null => {
    const project = this.store.getState().project;
    if (project === null || this.shown === null) {
      return null;
    }
    if (isDragging(this.workspace)) {
      // The blocks are in flux; read once the drag has ended.
      this.pipeline.schedule();
      return null;
    }
    try {
      return readModule(this.workspace, project.document, this.shown);
    } catch (error: unknown) {
      console.error('The canvas could not be read', error);
      return null;
    }
  };

  /** Blockly's events: content changes start the pipeline; created blocks are checked. */
  private readonly onEvent = (event: Blockly.Events.Abstract): void => {
    if (this.shown === null || this.disposed) {
      return;
    }
    if (event.isUiEvent) {
      // An opened or closed comment changes its saved `pinned`.
      if (
        event instanceof Blockly.Events.BubbleOpen &&
        event.bubbleType === Blockly.Events.BubbleType.COMMENT
      ) {
        this.pipeline.schedule();
      }
      return;
    }
    if (event instanceof Blockly.Events.BlockCreate) {
      const id = event.blockId;
      if (id !== undefined && id !== '') {
        this.guard.onCreated(id);
      }
    } else if (event instanceof Blockly.Events.BlockDelete) {
      for (const id of event.ids ?? []) {
        this.index.forget(id);
      }
    } else if (event instanceof Blockly.Events.BlockChange) {
      const id = event.blockId;
      const block = id === undefined ? null : this.workspace.getBlockById(id);
      if (block !== null) {
        this.index.refresh(block);
      }
    }
    this.pipeline.schedule();
  };

  /** Follows the store: a closed project empties the canvas; a new active module is shown. */
  private onStore(project: ProjectState | null, previous: ProjectState | null): void {
    if (this.disposed) {
      return;
    }
    if (project === null) {
      if (previous !== null) {
        this.clearCanvas();
      }
      return;
    }
    // A new project or document comes with a `loadDocument` call; only a change of the module
    // alone is a request to switch.
    if (previous?.handle !== project.handle || project.document !== previous.document) {
      return;
    }
    const wanted = project.activeModuleId;
    if (this.shown !== null && wanted !== this.shown && wanted !== previous.activeModuleId) {
      this.switchModule(wanted);
    }
  }
}
