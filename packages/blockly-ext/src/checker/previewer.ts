/**
 * The drag preview of a wrap (docs/spec/03-block-language.md §3.3 "Placing blocks", ./wrap.ts):
 * while an empty statement C-block dragged with the pointer would wrap statements, a grey copy of
 * it (an insertion marker) stands in their place with the statements in its first mouth, as the
 * drop will leave them. Every other preview is Blockly's own.
 *
 * Blockly 12.5.1's `InsertionMarkerPreviewer` makes its markers in private methods
 * (`previewMarker`, `createInsertionMarker`), and its `hideInsertionMarker` disposes a marker
 * together with what is connected inside it, so a wrap preview through it would delete the user's
 * statements. This previewer therefore makes its own marker from the protected
 * `serializeBlockToInsertionMarker`, as Blockly does, keeps it to itself, and on hiding puts the
 * statements back exactly where they were before it disposes the marker (ADR-0014: public and
 * protected members only, checked against Blockly 12.5.1). The changes are made with Blockly's
 * events off, as Blockly's own previews are, so the editing session and the undo history never
 * see them.
 *
 * Register it with `registerB2cConnectionPreviewer()` (done when this module loads) and inject with
 * `plugins: { connectionPreviewer: 'b2c_previewer' }`.
 */
import * as Blockly from 'blockly/core';

import { firstMouth, wrapPlace, type WrapPlace } from './wrap';

/** The name the previewer is registered under (`plugins: { connectionPreviewer: … }`). */
export const B2C_PREVIEWER_NAME = 'b2c_previewer';

/** A wrap preview on the canvas. */
interface ShownWrap {
  /** The dragged connection the preview is for (the C-block's previous connection). */
  readonly dragged: Blockly.Connection;
  readonly wrap: WrapPlace;
  /** The grey copy of the C-block that stands in the statements' place. */
  readonly marker: Blockly.BlockSvg;
}

/** The previewer of the drag of each block, while it is dragged (see {@link previewedWrap}). */
const PREVIEWERS = new WeakMap<Blockly.Block, B2cConnectionPreviewer>();

/**
 * Blockly's insertion-marker previewer, with the preview of a wrap: Blockly's block drag strategy
 * makes one for each drag (the registry's `CONNECTION_PREVIEWER`), and shows and hides previews
 * through it as the dragged block moves.
 */
export class B2cConnectionPreviewer extends Blockly.InsertionMarkerPreviewer {
  /** The canvas the block is dragged on. */
  private readonly canvas: Blockly.WorkspaceSvg;

  /** The wrap preview shown, if any. */
  private shown: ShownWrap | null = null;

  constructor(draggedBlock: Blockly.BlockSvg) {
    super(draggedBlock);
    this.canvas = draggedBlock.workspace;
    PREVIEWERS.set(draggedBlock, this);
  }

  /**
   * Shows that the dragged block would replace `replacedBlock` at `staticConn`, or, for a wrap,
   * the wrap.
   */
  override previewReplacement(
    draggedConn: Blockly.RenderedConnection,
    staticConn: Blockly.RenderedConnection,
    replacedBlock: Blockly.BlockSvg,
  ): void {
    if (this.previewWrap(draggedConn, staticConn)) {
      return;
    }
    this.hideWrap();
    super.previewReplacement(draggedConn, staticConn, replacedBlock);
  }

  /** Shows where the dragged block would connect, or, for a wrap, the wrap. */
  override previewConnection(
    draggedConn: Blockly.RenderedConnection,
    staticConn: Blockly.RenderedConnection,
  ): void {
    if (this.previewWrap(draggedConn, staticConn)) {
      return;
    }
    this.hideWrap();
    super.previewConnection(draggedConn, staticConn);
  }

  /** Hides every preview; the statements of a wrap preview go back where they were. */
  override hidePreview(): void {
    this.hideWrap();
    super.hidePreview();
  }

  /** The wrap the preview shows now, or `null`. */
  shownWrap(): WrapPlace | null {
    return this.shown?.wrap ?? null;
  }

  /**
   * The wrap that connecting `draggedConn` to `staticConn` stands for, or `null`: the rule of
   * ./wrap.ts (`wrapPlace`). The W5-2 spike's measurement overrides it to preview other rules.
   */
  protected wrapAt(
    draggedConn: Blockly.RenderedConnection,
    staticConn: Blockly.RenderedConnection,
  ): WrapPlace | null {
    return wrapPlace(draggedConn, staticConn);
  }

  /**
   * Shows the wrap that connecting `draggedConn` to `staticConn` stands for; returns `false`
   * (showing nothing) when the pair is not a wrap.
   */
  private previewWrap(
    draggedConn: Blockly.RenderedConnection,
    staticConn: Blockly.RenderedConnection,
  ): boolean {
    if (this.wrapAt(draggedConn, staticConn) === null) {
      return false;
    }
    const shown = this.shown;
    if (shown !== null && shown.dragged === draggedConn && shown.wrap.place === staticConn) {
      return true;
    }
    Blockly.Events.disable();
    try {
      super.hidePreview();
      this.hideWrap();
      // Asked again now that no preview stands in for anything.
      const wrap = this.wrapAt(draggedConn, staticConn);
      this.shown = wrap === null ? null : this.showWrap(draggedConn, wrap);
    } finally {
      Blockly.Events.enable();
    }
    return true;
  }

  /**
   * Puts a marker copy of the C-block at the wrap's place and the wrapped statements into its
   * first mouth. Blockly's events must be off.
   */
  private showWrap(dragged: Blockly.Connection, wrap: WrapPlace): ShownWrap | null {
    if (!(wrap.block instanceof Blockly.BlockSvg)) {
      return null;
    }
    // A copy of the C-block: it has the previous connection the wrap was found for.
    const marker = this.createMarker(wrap.block);
    const markerPrevious = marker.previousConnection;
    const markerMouth = firstMouth(marker);
    const statements = wrap.wrapped.previousConnection;
    if (markerMouth === null || statements === null) {
      marker.dispose(false);
      return null;
    }
    // Drawn on its own first, so that its connections are where it is drawn (as Blockly does).
    void marker.queueRender();
    Blockly.renderManagement.triggerQueuedRenders();
    statements.disconnect();
    markerPrevious.connect(wrap.place);
    markerMouth.connect(statements);
    void Blockly.renderManagement.finishQueuedRenders().then(() => {
      if (!marker.isDeadOrDying()) {
        marker.getSvgRoot().setAttribute('visibility', 'visible');
      }
    });
    return { dragged, wrap, marker };
  }

  /**
   * An insertion marker copy of `block`, hidden until it is drawn in place: Blockly's
   * `createInsertionMarker` (private in Blockly 12.5.1), from the protected
   * `serializeBlockToInsertionMarker`.
   */
  private createMarker(block: Blockly.BlockSvg): Blockly.BlockSvg {
    const state = this.serializeBlockToInsertionMarker(block);
    const marker = Blockly.serialization.blocks.append(state, this.canvas) as Blockly.BlockSvg;
    // Expression slots made while the block was set up become markers too.
    for (const part of marker.getDescendants(false)) {
      part.setInsertionMarker(true);
    }
    marker.initSvg();
    marker.getSvgRoot().setAttribute('visibility', 'hidden');
    return marker;
  }

  /** Hides the wrap preview, if one is shown: the statements go back, the marker goes. */
  private hideWrap(): void {
    const shown = this.shown;
    if (shown === null) {
      return;
    }
    this.shown = null;
    Blockly.Events.disable();
    try {
      restore(shown);
    } finally {
      Blockly.Events.enable();
    }
  }
}

/**
 * Takes the statements out of a wrap preview's marker and connects them where they were, then
 * disposes the marker. A user block still attached to the marker after that (none should be) is
 * set loose on the canvas rather than disposed with it.
 *
 * The canvas is drawn before the marker is disposed: Blockly 12.5.1's render queue keeps every
 * block that was queued as a top block until it draws it, and never draws a disposed one, so a
 * marker disposed right after it was unplugged (queued) would stay in memory for good.
 */
function restore({ wrap, marker }: ShownWrap): void {
  const statements = firstMouth(marker)?.targetBlock()?.previousConnection ?? null;
  statements?.disconnect();
  marker.unplug(false);
  const place = wrap.place;
  if (
    statements !== null &&
    place.targetConnection === null &&
    !place.getSourceBlock().isDeadOrDying() &&
    !statements.getSourceBlock().isDeadOrDying()
  ) {
    place.connect(statements);
  }
  for (const child of marker.getChildren(false)) {
    if (!child.isInsertionMarker()) {
      console.error('A wrap preview still held a block; it is set loose', child.id);
      child.unplug(false);
    }
  }
  Blockly.renderManagement.triggerQueuedRenders(marker.workspace);
  marker.dispose(false);
}

/**
 * The wrap the drag preview of `block` (a block being dragged) shows now, or `null`: what its drop
 * will do, read before Blockly ends the drag (which hides the preview first).
 */
export function previewedWrap(block: Blockly.Block): WrapPlace | null {
  return PREVIEWERS.get(block)?.shownWrap() ?? null;
}

/**
 * Registers `B2cConnectionPreviewer` as Blockly's connection previewer named `b2c_previewer`.
 * Calling it again (or loading this module again, as hot reloading does) replaces the registration.
 */
export function registerB2cConnectionPreviewer(): void {
  Blockly.registry.register(
    Blockly.registry.Type.CONNECTION_PREVIEWER,
    B2C_PREVIEWER_NAME,
    B2cConnectionPreviewer,
    true,
  );
}

registerB2cConnectionPreviewer();
