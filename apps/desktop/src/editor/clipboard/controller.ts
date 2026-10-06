/**
 * The editor's clipboard actions (docs/spec/05-project-format.md §5.12): copy, cut, paste and
 * duplicate through the compiler core's validated clipboard format.
 *
 * - **Copy** reads the document from the canvas (as the preview does) and asks the core for the
 *   payload (`clipboardMake`): the copied blocks without canvas positions, `refs` for the symbols
 *   they use but do not declare, and their C++ for `text/plain`. The in-app copy keeps it; writing
 *   the system clipboard is the DOM bridge's part (./bridge.ts).
 * - **Cut** copies, then deletes exactly what was copied (a top-level block with its loose stack;
 *   a block in a list alone, the list closing up behind it), as one undo step.
 * - **Paste** hands the payload to the core (`pastePrepare`), which validates it like a project
 *   file, gives the blocks fresh block and symbol IDs from a 256-bit random seed, and binds outside
 *   references again among the symbols visible at the target. A refused payload changes nothing
 *   and is reported with the loader's problems; references that find nothing keep their original
 *   symbol, so the analyser reports them (`B2C-E0201`). Before anything is inserted, the document
 *   with the blocks at the target is loaded too (./candidate.ts): a paste that would take it past
 *   the limits of a project file (05 §5.6) is refused the same way.
 * - **Duplicate** is a copy and a paste in memory, leaving both clipboards alone: a statement's
 *   copy goes directly after it, any other block's next to it on the canvas.
 *
 * Every action runs synchronously, so a copy can still fill the DOM event that asked for it. No
 * action throws: problems are reported through the notifier.
 */
import {
  type BdmBlock,
  type BdmDocument,
  CoreError,
  CoreTrap,
  type CoreWasm,
  type Diagnostic,
  type UnresolvedRef,
} from '@blocks2cpp/b2c-core-wasm';
import * as Blockly from 'blockly/core';

import type { EditorContext } from '../../app/editor-types';
import { readModule } from '../sync/workspaceToBdm';
import {
  anchorForBlock,
  anchorIsLive,
  isLive,
  isProjectBlock,
  ON_CANVAS,
  type PasteAnchor,
  pasteTarget,
  type WorkspacePoint,
} from './anchor';
import { documentWithPasted } from './candidate';
import type { ClipboardData } from './formats';
import { CANVAS_STEP, type InsertedBlocks, insertPasted } from './insert';
import type { ClipboardMemory } from './memory';
import type { ClipboardAction, ClipboardNotifier, UnavailableReason } from './notices';

/** Where the canvas starts for pasted blocks without a block to go next to. */
const VIEW_MARGIN = 40;

/** What a controller works with. */
export interface ClipboardControllerOptions {
  /** The canvas. */
  readonly workspace: Blockly.Workspace;
  /** The app store (the open project and its document). */
  readonly store: EditorContext['store'];
  /** The compiler core as it is now (never kept: a trap replaces it). */
  readonly core: () => CoreWasm | null;
  /** The module whose canvas is shown. */
  readonly activeModuleId: () => string;
  /** The in-app copy. */
  readonly memory: ClipboardMemory;
  /** Tells the user why an action did nothing. */
  readonly notify: ClipboardNotifier;
  /** 64 hex digits of fresh randomness for each paste (`randomSeedHex`). */
  readonly seed: () => string;
  /** Starts a new compiler core after a trap (best effort), or `null` to leave it to the preview. */
  readonly restartCore: (() => Promise<unknown>) | null;
}

/** The outcome of a paste or a duplicate. */
export type PasteOutcome =
  /** The blocks were inserted (`attached`: connected at the target, else on the canvas). */
  | {
      readonly kind: 'pasted';
      readonly blocks: readonly Blockly.Block[];
      readonly attached: boolean;
      readonly unresolved: readonly UnresolvedRef[];
    }
  /** The loader refused the payload (or the document); nothing changed. */
  | { readonly kind: 'refused'; readonly diagnostics: readonly Diagnostic[] }
  /** There was nothing to paste, or nowhere to paste it (no project, a read-only canvas). */
  | { readonly kind: 'nothing' }
  /** The action could not run (the user was told); nothing changed. */
  | { readonly kind: 'failed' };

/** A call to the core: its value, or why there is none. */
type CoreCall<T> =
  | { readonly ok: true; readonly value: T }
  | { readonly ok: false; readonly invalidArguments: boolean };

/** The canvas read into the document, and the module shown. */
interface Snapshot {
  readonly doc: BdmDocument;
  readonly json: string;
  readonly moduleId: string;
}

/** The clipboard actions on one canvas (see the module comment). */
export class ClipboardController {
  private readonly options: ClipboardControllerOptions;

  constructor(options: ClipboardControllerOptions) {
    this.options = options;
  }

  /** The canvas this controller works on. */
  get workspace(): Blockly.Workspace {
    return this.options.workspace;
  }

  /** The in-app copy. */
  get memory(): ClipboardMemory {
    return this.options.memory;
  }

  /** Whether blocks can be added or removed: a project is shown and the canvas is editable. */
  canEdit(): boolean {
    return !this.options.workspace.isReadOnly() && this.snapshotModule() !== null;
  }

  /** Whether `block` can be copied: a project block of this canvas while a project is shown. */
  canCopy(block: Blockly.Block | null): block is Blockly.Block {
    return (
      block !== null &&
      isProjectBlock(this.options.workspace, block) &&
      this.snapshotModule() !== null
    );
  }

  /** Whether `block` can be cut: it can be copied and deleted. */
  canCut(block: Blockly.Block | null): block is Blockly.Block {
    return this.canCopy(block) && this.canEdit() && block.isDeletable();
  }

  /** Whether `block` can be duplicated. */
  canDuplicate(block: Blockly.Block | null): block is Blockly.Block {
    return this.canCopy(block) && this.canEdit() && block.isDuplicatable();
  }

  /**
   * The clipboard data for `block` (with everything inside it, and its loose stack if it is a
   * top-level block), without keeping it. `null` when it cannot be made (the user is told why,
   * unless there is simply nothing to copy).
   */
  make(block: Blockly.Block, action: ClipboardAction = 'copy'): ClipboardData | null {
    if (!this.canCopy(block)) {
      return null;
    }
    const snapshot = this.snapshot(action);
    if (snapshot === null) {
      return null;
    }
    const made = this.withCore(action, (core) => core.clipboardMake(snapshot.json, [block.id]));
    if (!made.ok) {
      if (made.invalidArguments) {
        this.options.notify({ kind: 'unavailable', action, reason: 'internal' });
      }
      return null;
    }
    const result = made.value;
    if (!result.ok) {
      this.options.notify({ kind: 'refused', action, diagnostics: result.diagnostics });
      return null;
    }
    return { payload: result.payload, text: result.text ?? null };
  }

  /** Copies `block` into the in-app copy and returns the data (`null`: nothing was copied). */
  copy(block: Blockly.Block): ClipboardData | null {
    const data = this.make(block, 'copy');
    if (data !== null) {
      this.options.memory.set(data);
    }
    return data;
  }

  /**
   * Copies `block` into the in-app copy, then deletes what was copied as one undo step. Returns
   * the data, or `null` when nothing was copied (and so nothing deleted).
   */
  cut(block: Blockly.Block): ClipboardData | null {
    if (!this.canCut(block)) {
      return null;
    }
    const data = this.make(block, 'cut');
    if (data === null) {
      return null;
    }
    this.options.memory.set(data);
    deleteCopied(block);
    return data;
  }

  /**
   * Pastes `payload` (or, with `null`, the in-app copy) at `anchor`. The pasted blocks are
   * selected.
   */
  paste(payload: string | null, anchor: PasteAnchor): PasteOutcome {
    const text = payload ?? this.options.memory.get()?.payload ?? null;
    if (text === null || !this.canEdit()) {
      return { kind: 'nothing' };
    }
    return this.pasteText(text, anchor, 'paste');
  }

  /**
   * Duplicates `block`: a statement's copy goes directly after it, any other block's next to it on
   * the canvas. The clipboards are left alone.
   */
  duplicate(block: Blockly.Block): PasteOutcome {
    if (!this.canDuplicate(block)) {
      return { kind: 'nothing' };
    }
    const data = this.make(block, 'duplicate');
    if (data === null) {
      return { kind: 'failed' };
    }
    const inList = block.getParent() !== null && block.previousConnection?.isConnected() === true;
    const anchor: PasteAnchor = inList
      ? anchorForBlock(block)
      : { kind: 'canvas', near: block, at: null };
    return this.pasteText(data.payload, anchor, 'duplicate');
  }

  private pasteText(text: string, wanted: PasteAnchor, action: ClipboardAction): PasteOutcome {
    const workspace = this.options.workspace;
    const snapshot = this.snapshot(action);
    if (snapshot === null) {
      return { kind: 'failed' };
    }
    let anchor = anchorIsLive(workspace, wanted) ? wanted : ON_CANVAS;
    const seed = this.freshSeed(action);
    if (seed === null) {
      return { kind: 'failed' };
    }
    let prepared = this.withCore(action, (core) =>
      core.pastePrepare(text, snapshot.json, pasteTarget(anchor, snapshot.moduleId), seed),
    );
    if (!prepared.ok && prepared.invalidArguments && anchor.kind !== 'canvas') {
      // The target block is not one the core can see (for example inside a placeholder): the
      // canvas always is. The same seed is fine: nothing was pasted with it.
      anchor = { kind: 'canvas', near: anchor.block, at: null };
      prepared = this.withCore(action, (core) =>
        core.pastePrepare(text, snapshot.json, pasteTarget(anchor, snapshot.moduleId), seed),
      );
    }
    if (!prepared.ok) {
      if (prepared.invalidArguments) {
        this.options.notify({ kind: 'unavailable', action, reason: 'internal' });
      }
      return { kind: 'failed' };
    }
    const result = prepared.value;
    if (!result.ok) {
      this.options.notify({ kind: 'refused', action, diagnostics: result.diagnostics });
      return { kind: 'refused', diagnostics: result.diagnostics };
    }
    const origin = this.canvasOrigin(anchor);
    const problems = this.limitProblems(action, snapshot, result.blocks, anchor, origin);
    if (problems === null) {
      return { kind: 'failed' };
    }
    if (problems.length > 0) {
      this.options.notify({ kind: 'limits', action, diagnostics: problems });
      return { kind: 'refused', diagnostics: problems };
    }
    let inserted: InsertedBlocks;
    try {
      inserted = insertPasted(workspace, result.blocks, anchor, origin);
    } catch (error: unknown) {
      console.error('The pasted blocks could not be inserted', error);
      this.options.notify({ kind: 'unavailable', action, reason: 'internal' });
      return { kind: 'failed' };
    }
    if (inserted.first !== null) {
      select(inserted.first);
    }
    return {
      kind: 'pasted',
      blocks: inserted.roots,
      attached: inserted.attached,
      unresolved: result.unresolved,
    };
  }

  /**
   * The loader's problems with the document once `blocks` are inserted at `anchor` (none: the
   * paste fits), or `null` when the check could not run (the user was told why).
   */
  private limitProblems(
    action: ClipboardAction,
    snapshot: Snapshot,
    blocks: readonly BdmBlock[],
    anchor: PasteAnchor,
    origin: WorkspacePoint,
  ): readonly Diagnostic[] | null {
    let json: string;
    try {
      json = JSON.stringify(
        documentWithPasted(snapshot.doc, snapshot.moduleId, blocks, anchor, origin),
      );
    } catch (error: unknown) {
      console.error('The document with the pasted blocks could not be written', error);
      this.unavailable(action, 'internal');
      return null;
    }
    const checked = this.withCore(action, (core) => core.canonical(json));
    if (!checked.ok) {
      if (checked.invalidArguments) {
        this.unavailable(action, 'internal');
      }
      return null;
    }
    return checked.value.ok ? [] : checked.value.diagnostics;
  }

  /** 64 hex digits of fresh randomness, or `null` when there is none (the user was told). */
  private freshSeed(action: ClipboardAction): string | null {
    try {
      return this.options.seed();
    } catch (error: unknown) {
      console.error('No randomness is available for fresh block IDs', error);
      this.unavailable(action, 'internal');
      return null;
    }
  }

  /** Where blocks pasted on the canvas start for `anchor`. */
  private canvasOrigin(anchor: PasteAnchor): WorkspacePoint {
    if (anchor.kind === 'canvas' && anchor.at !== null) {
      return anchor.at;
    }
    const near = anchor.kind === 'canvas' ? anchor.near : anchor.block;
    if (near !== null && isLive(near)) {
      const xy = near.getRelativeToSurfaceXY();
      return { x: xy.x + CANVAS_STEP, y: xy.y + CANVAS_STEP };
    }
    const workspace = this.options.workspace;
    if (workspace instanceof Blockly.WorkspaceSvg) {
      try {
        const view = workspace.getMetricsManager().getViewMetrics(true);
        return { x: view.left + VIEW_MARGIN, y: view.top + VIEW_MARGIN };
      } catch (error: unknown) {
        console.warn('The visible part of the canvas could not be measured', error);
      }
    }
    return { x: VIEW_MARGIN, y: VIEW_MARGIN };
  }

  /** The shown module's ID, when a project is open and its document has that module. */
  private snapshotModule(): string | null {
    const project = this.options.store.getState().project;
    if (project === null) {
      return null;
    }
    const moduleId = this.options.activeModuleId();
    return project.document.modules.some((module) => module.id === moduleId) ? moduleId : null;
  }

  /** The document as the canvas has it now, as JSON for the core, or `null` (told when broken). */
  private snapshot(action: ClipboardAction): Snapshot | null {
    const project = this.options.store.getState().project;
    const moduleId = this.snapshotModule();
    if (project === null || moduleId === null) {
      return null;
    }
    let doc: BdmDocument;
    try {
      doc = readModule(this.options.workspace, project.document, moduleId);
    } catch (error: unknown) {
      console.error('The canvas could not be read for the clipboard', error);
      this.options.notify({ kind: 'unavailable', action, reason: 'internal' });
      return null;
    }
    return { doc, json: JSON.stringify(doc), moduleId };
  }

  /** Runs `call` on the current core, turning every failure into a notice (see {@link CoreCall}). */
  private withCore<T>(action: ClipboardAction, call: (core: CoreWasm) => T): CoreCall<T> {
    const core = this.options.core();
    if (core === null) {
      this.unavailable(action, 'coreNotStarted');
      return { ok: false, invalidArguments: false };
    }
    try {
      return { ok: true, value: call(core) };
    } catch (error: unknown) {
      if (error instanceof CoreError && error.kind === 'invalidArguments') {
        console.warn(`The compiler core refused the ${action} request`, error);
        return { ok: false, invalidArguments: true };
      }
      if (error instanceof CoreTrap) {
        console.error(`The compiler core stopped during ${action}`, error);
        this.restart(core);
        this.unavailable(action, 'coreStopped');
      } else {
        console.error(`The ${action} failed in the compiler core`, error);
        this.unavailable(action, 'internal');
      }
      return { ok: false, invalidArguments: false };
    }
  }

  private unavailable(action: ClipboardAction, reason: UnavailableReason): void {
    this.options.notify({ kind: 'unavailable', action, reason });
  }

  /**
   * Starts a new core after `trapped` stopped, without waiting for it; nothing to do when the
   * preview has already replaced it.
   */
  private restart(trapped: CoreWasm): void {
    const restart = this.options.restartCore;
    if (restart === null || this.options.core() !== trapped) {
      return;
    }
    restart().catch((error: unknown) => {
      console.error('The compiler core could not be restarted', error);
    });
  }
}

/**
 * Deletes what a copy of `block` holds, as one undo step: a top-level block with its loose stack
 * (the blocks chained below it), any other block alone (a statement list closes up behind it).
 */
function deleteCopied(block: Blockly.Block): void {
  const outerGroup = Blockly.Events.getGroup();
  if (outerGroup === '') {
    Blockly.Events.setGroup(true);
  }
  try {
    const heal = block.getParent() !== null && block.outputConnection === null;
    if (block instanceof Blockly.BlockSvg) {
      block.workspace.hideChaff();
      block.dispose(heal, true);
    } else {
      block.dispose(heal);
    }
  } catch (error: unknown) {
    console.error('The cut blocks could not be deleted', error);
  } finally {
    if (outerGroup === '') {
      Blockly.Events.setGroup(false);
    }
  }
}

/** Selects a pasted block (Blockly 12: focuses it, which scrolls it into view). */
function select(block: Blockly.Block): void {
  if (!(block instanceof Blockly.BlockSvg)) {
    return;
  }
  try {
    Blockly.common.setSelected(block);
  } catch (error: unknown) {
    console.warn('The pasted block could not be selected', error);
  }
}
