/**
 * One editing session on a Blockly workspace: the document shown on the canvas, kept in step with
 * the app's store (02 §2.4.1). Blockly is the live editing state; the BDM derived from it is what
 * is analysed, saved and sent to the backend.
 *
 * - Loading a document builds the shown module's canvas (./bdmToWorkspace.ts) with events off.
 *   Building or emptying the canvas forgets the selected and hovered block of the store.
 * - Every content change on the canvas (create, delete, change, move, comment, mutation) starts the
 *   debounced preview pipeline (../preview/pipeline.ts), which reads the canvas back
 *   (./workspaceToBdm.ts) and updates the project and the analysis.
 * - Created blocks get fresh symbol IDs where they would clash (./duplicates.ts).
 * - One module is shown at a time; switching keeps the left module's changes in the document and
 *   clears Blockly's undo history (each module has its own canvas). A switch asked for during a
 *   block drag (a problem in another module selected, another module chosen) waits until the
 *   drag has ended: the canvas is in flux until then. While the shown canvas does not load
 *   (`syncFailed`), the editor stays on it: the project's document only ever holds documents the
 *   loader accepts, and the undo history that takes the change back is kept.
 * - The viewport is captured only for saving ({@link EditorSession.currentDocument}).
 * - While a block is dragged, saving writes the canvas as it was just before the drag (./drag.ts).
 * - Renaming the project changes the document outside the canvas and previews it again
 *   ({@link EditorSession.updateProjectInfo}); Undo and Redo act on the canvas only.
 */
import type { BdmBlock, BdmDocument, BdmViewport, PreviewResult } from '@blocks2cpp/b2c-core-wasm';
import * as Blockly from 'blockly/core';

import type { ProjectInfo } from '../../app/editor-types';
import type { ProjectState, useAppStore } from '../../app/store';
import { visibleHolder } from '../highlight/reveal';
import type { CoreHost } from '../preview/coreHost';
import { PreviewPipeline } from '../preview/pipeline';
import type { PreviewService } from '../preview/service';
import { loadModule } from './bdmToWorkspace';
import { forEachNode, treeBlockIds } from './bdmTree';
import { listenToBlockDrags } from './drag';
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
import { readModule, readTopBlocks, withBlocks, withViewport } from './workspaceToBdm';

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

/**
 * The view of the shown module: the viewport it was shown with, and what "the user has not moved
 * the view" is checked against when saving.
 */
interface ShownView {
  /** The viewport the module was shown with: the file's, or the one it was left with. */
  readonly saved: BdmViewport | undefined;
  /**
   * Blockly's view once `saved` was shown on a canvas with a size; `null` until then (and for a
   * headless workspace). While it is `null`, the view counts as unchanged.
   */
  baseline: ViewState | null;
  /**
   * The block a selection asked to centre on while this module was shown. Settling the view
   * restores `saved`, so it centres on this block again afterwards; otherwise the deferred
   * settle after Blockly's first render would undo a jump to a block in another module.
   */
  centreOn: string | null;
}

/** Whether a rendered workspace's view has an area (it has none while the editor is hidden). */
function hasViewSize(workspace: Blockly.WorkspaceSvg): boolean {
  const view = workspace.getMetricsManager().getViewMetrics();
  return view.width > 0 && view.height > 0;
}

/**
 * How far inside the view's edge {@link showFirstView} puts blocks that do not fit, in workspace
 * units.
 */
export const FIRST_VIEW_MARGIN = 24;

/**
 * Shows a module that has no saved view (a template, or a file written without one): along each
 * axis its blocks are centred in the view when they fit with {@link FIRST_VIEW_MARGIN} to spare,
 * and otherwise start that margin inside the view. The canvas view starts right of the toolbox's
 * flyout, so the blocks are clear of it (04 §4.1).
 *
 * Without this, the view is wherever Blockly left it. A module built while the editor has no size
 * (the start page opens projects while the editor is hidden) gets the blocks' top-left corner on
 * the view's top-left corner: Blockly pads the scrollable area by half the view's size, which is
 * then 0, so its only scroll position puts the blocks at the edge, against the flyout.
 */
function showFirstView(workspace: Blockly.WorkspaceSvg): void {
  if (workspace.getTopBlocks(false).length === 0) {
    return;
  }
  const blocks = workspace.getBlocksBoundingBox();
  const view = workspace.getMetricsManager().getViewMetrics(true);
  const start = (from: number, to: number, room: number): number =>
    to - from + 2 * FIRST_VIEW_MARGIN <= room
      ? (from + to) / 2 - room / 2
      : from - FIRST_VIEW_MARGIN;
  const left = start(blocks.left, blocks.right, view.width);
  const top = start(blocks.top, blocks.bottom, view.height);
  workspace.scroll(-left * workspace.scale, -top * workspace.scale);
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

/** Whether module `moduleId` of `doc` holds block `id` (at any depth). */
function moduleHasBlock(doc: BdmDocument, moduleId: string, id: string): boolean {
  const module = doc.modules.find((candidate) => candidate.id === moduleId);
  return module !== undefined && treeBlockIds(module.workspace.blocks).includes(id);
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

/** A module switch asked for during a block drag, done once the drag has ended. */
interface WaitingSwitch {
  readonly moduleId: string;
  /** The block to select once the module is shown, when a selection asked for the switch. */
  readonly select: { readonly id: string; readonly options: SelectOptions } | null;
}

/** An editing session; see the module comment. */
export class EditorSession {
  readonly workspace: Blockly.Workspace;
  private readonly store: typeof useAppStore;
  private readonly host: CoreHost;
  private readonly hooks: SessionHooks;
  private readonly pipeline: PreviewPipeline;
  private readonly index = new DeclIndex();
  private readonly guard: DuplicateGuard;
  private readonly unsubscribe: () => void;
  private shown: string | null = null;
  /** The view of the shown module (see {@link ShownView}). */
  private shownView: ShownView = { saved: undefined, baseline: null, centreOn: null };
  /** Viewports of modules that were shown and left, to save with the document. */
  private readonly leftViews = new Map<string, BdmViewport | undefined>();
  /** The shown module's blocks just before the block drag in progress, or `null`. */
  private preDrag: { readonly moduleId: string; readonly blocks: readonly BdmBlock[] } | null =
    null;
  /** The module switch asked for during the block drag in progress, or `null`. */
  private waitingSwitch: WaitingSwitch | null = null;
  private readonly stopDragListening: () => void;
  private disposed = false;

  constructor(options: EditorSessionOptions) {
    this.workspace = options.workspace;
    this.store = options.store;
    this.host = options.host;
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
    this.stopDragListening =
      this.workspace instanceof Blockly.WorkspaceSvg
        ? listenToBlockDrags(this.workspace, {
            onDragStart: (block) => {
              this.beforeDrag(block);
            },
            onDragEnd: () => {
              this.preDrag = null;
              if (this.waitingSwitch !== null) {
                // Blockly's gesture ends the drag after its dragger has finished the drop.
                queueMicrotask(this.switchAfterDrag);
              }
            },
          })
        : () => undefined;
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
   * While a block is dragged, the canvas as it was just before the drag: the dragged block is
   * still where it was, and no drag preview stands in for anything.
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
    const preDrag = this.preDrag;
    let doc =
      preDrag !== null && preDrag.moduleId === this.shown && isDragging(this.workspace)
        ? withBlocks(project.document, this.shown, preDrag.blocks)
        : readModule(this.workspace, project.document, this.shown);
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
   * Gives the open project the name `info.name` (already checked by the caller): the project's
   * document takes it at once and has unsaved changes, and the preview runs now. The canvas reads
   * back onto that document, so the run commits the canvas with the new name and works out
   * `dirty` exactly (renaming back to the saved name leaves nothing unsaved). While the canvas
   * does not load (`syncFailed`) the name still counts as unsaved, so it is never lost silently.
   *
   * Blockly's undo history covers the canvas only: the rename is not undone by Undo. An unchanged
   * name, or no open project, changes nothing.
   */
  updateProjectInfo(info: ProjectInfo): void {
    const project = this.store.getState().project;
    if (this.disposed || project === null || project.document.project.name === info.name) {
      return;
    }
    const document: BdmDocument = {
      ...project.document,
      project: { ...project.document.project, name: info.name },
    };
    this.store.getState().actions.updateProject({ document, dirty: true });
    void this.pipeline.runNow();
  }

  /**
   * Undoes (or, with `redo`, redoes) the last change on the canvas, as Blockly's own `Ctrl+Z` and
   * `Ctrl+Y` do: its drop-downs and field editors close first, and nothing happens during a drag.
   */
  undo(redo: boolean): void {
    const workspace = this.workspace;
    if (this.disposed || isDragging(workspace)) {
      return;
    }
    if (workspace instanceof Blockly.WorkspaceSvg) {
      workspace.hideChaff();
    }
    workspace.undo(redo);
  }

  /**
   * Selects a block, or the outermost collapsed block it is nested in, switching to its module
   * first. A collapsed statement before it in the same list does not hide it, so it is not taken
   * instead. A block kept inside a placeholder selects the placeholder. A block of another module
   * asked for during a block drag is selected once the drag has ended (see
   * {@link switchModule}).
   */
  selectBlock(id: string, options: SelectOptions = {}): void {
    let block = this.workspace.getBlockById(id);
    if (block === null) {
      const project = this.store.getState().project;
      const moduleId = project === null ? null : moduleOfBlock(project.document, id);
      if (moduleId !== null && moduleId !== this.shown) {
        if (isDragging(this.workspace)) {
          this.waitingSwitch = { moduleId, select: { id, options } };
          return;
        }
        this.switchModule(moduleId);
        block = this.workspace.getBlockById(id);
      }
    }
    block ??= this.placeholderHolding(id);
    if (block === null) {
      return;
    }
    const target = visibleHolder(block);
    if (
      !(target instanceof Blockly.BlockSvg) ||
      !(this.workspace instanceof Blockly.WorkspaceSvg)
    ) {
      return;
    }
    try {
      Blockly.common.setSelected(target);
      if (options.center === true) {
        // The module may have just been shown, and its view still settles once Blockly has
        // rendered it: that settle centres on the block again.
        this.shownView.centreOn = target.id;
        this.workspace.centerOnBlock(target.id, true);
      }
    } catch (error: unknown) {
      console.warn('The block could not be selected', error);
    }
  }

  /**
   * Shows module `moduleId`. The shown module's changes stay in the document, and Blockly's undo
   * history is cleared. An unknown module ID is ignored.
   *
   * During a block drag the switch waits until the drag has ended: the canvas is in flux (a drag
   * preview may stand in a list, or hold the statements a wrap would take in its mouth), and
   * Blockly cannot clear a canvas whose block it is dragging. The last switch asked for wins.
   *
   * A shown module whose canvas does not load (its read is refused by the compiler core's loader,
   * `syncFailed`) is not left: the project's document would get a document that no save, build
   * or recovery snapshot can write, and clearing the undo history would take away the Undo that
   * gets back to the last version that loads (04 §4.4). The store goes back to the shown module.
   */
  switchModule(moduleId: string): void {
    const project = this.store.getState().project;
    if (project === null || this.shown === null) {
      return;
    }
    if (isDragging(this.workspace)) {
      this.waitingSwitch = { moduleId, select: null };
      return;
    }
    if (moduleId === this.shown) {
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
    if (!this.loads(doc)) {
      console.warn('The shown module does not load; staying on it until the change is undone');
      if (project.activeModuleId !== this.shown) {
        actions.updateProject({ activeModuleId: this.shown });
      }
      return;
    }
    this.leftViews.set(this.shown, this.shownViewport(doc, this.shown));
    this.pipeline.invalidate();
    actions.updateProject({ document: doc });
    this.show(doc, moduleId, true);
    void this.pipeline.runNow();
  }

  /**
   * Fits a rendered workspace to its container (`Blockly.svgResize`); call it whenever the editor
   * is shown or resized. Resizing moves Blockly's scroll position, which is not the user's doing:
   * a view the user has not moved stays unchanged for saving, and the shown module's saved view,
   * which waits while the editor has no size, is shown now.
   */
  resize(): void {
    const workspace = this.workspace;
    if (!(workspace instanceof Blockly.WorkspaceSvg)) {
      return;
    }
    const view = this.shownView;
    const before = viewStateOf(workspace);
    const unchanged =
      view.baseline !== null && before !== null && sameViewState(before, view.baseline);
    Blockly.svgResize(workspace);
    if (this.shown === null || this.disposed) {
      return;
    }
    if (view.baseline === null) {
      this.settleView(view);
    } else if (unchanged) {
      view.baseline = viewStateOf(workspace);
    }
  }

  /** Detaches from the workspace and the store. */
  dispose(): void {
    if (this.disposed) {
      return;
    }
    this.disposed = true;
    this.preDrag = null;
    this.waitingSwitch = null;
    this.stopDragListening();
    this.workspace.removeChangeListener(this.onEvent);
    this.unsubscribe();
    this.pipeline.dispose();
  }

  /**
   * Keeps the shown module's blocks as they are just before a drag of `block` starts (Blockly has
   * not detached it yet), for {@link currentDocument}.
   *
   * When the preview pipeline is idle ({@link PreviewPipeline.isIdle}), the project's document
   * already holds them: it is the canvas as the pipeline last read it, and nothing has changed
   * since. Reading the whole canvas instead grows with the canvas (about 10 ms at 1,000 blocks in
   * a debug build), and it delayed every drag's start. Otherwise (an edit waits for its preview or
   * its `update()` still runs, or the canvas did not load) the canvas is read now; a block the
   * project's document does not have yet and that sits on its own is one the drag takes out of the
   * toolbox, which is not part of the canvas before the drag. The project's document never has
   * that block.
   *
   * A change Blockly has made but not yet announced (it delivers its events after the next
   * animation frame, so a drag that starts before then is heard of first) is not in the project's
   * document: a save during that drag writes the canvas without it, and the project still has
   * unsaved changes afterwards, once the pipeline reads the canvas after the drop.
   */
  private beforeDrag(block: Blockly.BlockSvg): void {
    this.preDrag = null;
    const project = this.store.getState().project;
    if (project === null || this.shown === null || this.disposed) {
      return;
    }
    const committed = this.pipeline.isIdle()
      ? project.document.modules.find((module) => module.id === this.shown)
      : undefined;
    let blocks: readonly BdmBlock[];
    if (committed === undefined) {
      blocks = readTopBlocks(this.workspace);
      if (block.getParent() === null && !moduleHasBlock(project.document, this.shown, block.id)) {
        blocks = blocks.filter((node) => node.id !== block.id);
      }
    } else {
      blocks = committed.workspace.blocks;
    }
    this.preDrag = { moduleId: this.shown, blocks };
  }

  /**
   * Whether `doc`, read from the canvas, loads: the running compiler core's loader accepts it
   * (`canonical()`, which the core answers synchronously). Without a running core, or when the
   * core fails, nothing is known, and it counts as loading (the pipeline restarts the core).
   */
  private loads(doc: BdmDocument): boolean {
    const core = this.host.current();
    if (core === null) {
      return true;
    }
    try {
      return core.canonical(JSON.stringify(doc)).ok;
    } catch (error: unknown) {
      console.error('The compiler core could not check the shown module', error);
      return true;
    }
  }

  /**
   * Does the module switch asked for during the block drag that has just ended (see
   * {@link switchModule}), unless another drag has started since (its end does it), the session
   * was disposed, or another document was shown in the meantime.
   */
  private readonly switchAfterDrag = (): void => {
    const waiting = this.waitingSwitch;
    if (waiting === null || this.disposed || isDragging(this.workspace)) {
      return;
    }
    this.waitingSwitch = null;
    if (waiting.select === null) {
      this.switchModule(waiting.moduleId);
    } else {
      this.selectBlock(waiting.select.id, waiting.select.options);
    }
  };

  /** Builds module `moduleId` of `doc` on the canvas and restores its view. */
  private show(doc: BdmDocument, moduleId: string, clearUndo: boolean): void {
    this.preDrag = null;
    this.waitingSwitch = null;
    loadModule(this.workspace, doc, moduleId);
    this.forgetMarkedBlocks();
    this.shown = moduleId;
    if (clearUndo) {
      this.workspace.clearUndo();
    }
    const saved = this.leftViews.has(moduleId)
      ? this.leftViews.get(moduleId)
      : doc.modules.find((module) => module.id === moduleId)?.workspace.viewport;
    this.leftViews.delete(moduleId);
    const view: ShownView = { saved, baseline: null, centreOn: null };
    this.shownView = view;
    this.settleView(view);
    this.index.rebuild(this.workspace);
    const project = this.store.getState().project;
    if (project !== null && project.activeModuleId !== moduleId) {
      this.store.getState().actions.updateProject({ activeModuleId: moduleId });
    }
  }

  /**
   * Forgets the selected and the hovered block of the store (`ui.selection`, `ui.hoverBlock`) once
   * the canvas has been rebuilt or emptied (a module switch, a reload, a project closed): Blockly
   * replaces the blocks without events, so the highlight tracking (../highlight/tracking.ts) hears
   * of neither, and the code panel would go on highlighting a block that is gone, or that is no
   * longer selected when the new canvas has a block with the same ID (04 §4.3).
   */
  private forgetMarkedBlocks(): void {
    const { ui, actions } = this.store.getState();
    if (ui.selection !== null || ui.hoverBlock !== null) {
      actions.setUi({ selection: null, hoverBlock: null });
    }
  }

  /** Empties the canvas (the project was closed). */
  private clearCanvas(): void {
    this.pipeline.invalidate();
    this.preDrag = null;
    this.waitingSwitch = null;
    clearWorkspace(this.workspace);
    this.forgetMarkedBlocks();
    this.workspace.clearUndo();
    this.shown = null;
    this.shownView = { saved: undefined, baseline: null, centreOn: null };
    this.leftViews.clear();
    this.guard.reset();
    this.index.rebuild(this.workspace);
  }

  /**
   * Shows `view`'s saved viewport, or for a module without one its first view
   * ({@link showFirstView}), and takes its baseline, once the workspace has a size: at once, and
   * again after Blockly's first render of the canvas, which sizes the content and moves the scroll
   * position (that is not the user scrolling). A workspace without a size (the editor is hidden
   * while a project opens from the start page) cannot show a view; {@link resize} settles it once
   * the editor is shown, with the toolbox's flyout already fitted to the room it has. A block a
   * selection centred on in the meantime is centred on again after each settle; the baseline is
   * taken before that, so the jump counts as a move.
   */
  private settleView(view: ShownView): void {
    const workspace = this.workspace;
    if (!(workspace instanceof Blockly.WorkspaceSvg) || !hasViewSize(workspace)) {
      return;
    }
    const settle = (): void => {
      if (view.saved === undefined) {
        showFirstView(workspace);
      } else {
        restoreViewport(workspace, view.saved);
      }
      view.baseline = viewStateOf(workspace);
      if (view.centreOn !== null && workspace.getBlockById(view.centreOn) !== null) {
        workspace.centerOnBlock(view.centreOn, true);
      }
    };
    settle();
    void Blockly.renderManagement.finishQueuedRenders().then(() => {
      if (!this.disposed && this.shownView === view && hasViewSize(workspace)) {
        settle();
      }
    });
  }

  /**
   * The shown module's viewport for saving: the one it was shown with while the user has not
   * scrolled or zoomed, else the current one. A headless workspace keeps the document's.
   */
  private shownViewport(doc: BdmDocument, moduleId: string): BdmViewport | undefined {
    const state = viewStateOf(this.workspace);
    if (state === null) {
      return doc.modules.find((module) => module.id === moduleId)?.workspace.viewport;
    }
    const baseline = this.shownView.baseline;
    if (baseline === null || sameViewState(state, baseline)) {
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
