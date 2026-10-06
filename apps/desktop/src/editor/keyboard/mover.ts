/**
 * Moving a block with the keyboard (docs/spec/04-user-interface.md §4.7): `M` picks up the block
 * with the keyboard cursor (or `Enter` on a block in the toolbox adds one), the arrow keys go
 * through the places it can go (./targets.ts), `Enter` puts it there and `Escape` leaves it where
 * it was. While a block is held the keyboard belongs to the move: other keys are not passed on.
 *
 * The block does not travel while the places are tried: the place is highlighted and announced,
 * and only `Enter` changes the canvas, in one Blockly event group, so one `Ctrl+Z` undoes the move.
 * A block added from the toolbox is dropped (or removed on Escape) in the group it was created in,
 * so the whole add is one undo step, as a pointer drag from the toolbox is. The places are the ones
 * a pointer drag could connect to (./targets.ts), and connecting is Blockly's own (with the
 * type-aware connection checker), so the editing session sees the move like any other change. A
 * pointer press, the focus leaving the canvas, or the block or the project going away ends the
 * move without changing anything.
 */
import * as Blockly from 'blockly/core';

import { blockOfNode, isGone } from './keepFocus';
import { describeBlock, describeTarget, type SymbolNames } from './labels';
import {
  dropTargets,
  type DropTarget,
  isCurrentPlace,
  isStillValid,
  movingConnection,
} from './targets';

/** The class a held block gets (./keyboard.css draws it dashed). */
export const MOVING_CLASS = 'b2cKeyboardMoving';

/** How far a block put loose on the canvas lands from where it was, in workspace units. */
export const CANVAS_OFFSET = { x: 48, y: 24 } as const;

/** What a move needs. */
export interface MoverDeps {
  readonly workspace: Blockly.WorkspaceSvg;
  /** Says something to screen readers. */
  readonly announce: (text: string) => void;
  /** The names of the document's symbols, for naming blocks. */
  readonly names: () => SymbolNames;
}

/** How a move starts. */
export interface MoveOptions {
  /** The block was just added from the toolbox: leaving it (Escape) removes it again. */
  readonly inserted?: boolean;
  /**
   * Start at the place nearest this node (where the keyboard cursor was when the toolbox was
   * opened) rather than where the block is. When an added block is removed again, the keyboard
   * focus goes back to this node.
   */
  readonly near?: Blockly.IFocusableNode | null;
  /**
   * The Blockly event group the block was added in: the drop (or the removal on Escape) joins it,
   * so the whole add is one undo step. Without it the drop is a group of its own.
   */
  readonly group?: string;
}

/** A move in progress. */
interface Move {
  readonly block: Blockly.BlockSvg;
  readonly targets: readonly DropTarget[];
  index: number;
  readonly inserted: boolean;
  /** Where the keyboard focus goes back to when an added block is removed again. */
  readonly near: Blockly.IFocusableNode | null;
  /** The event group the move's change joins, or `null` for a group of its own. */
  readonly group: string | null;
  /** The connection drawn as the current place, to un-draw it. */
  highlighted: Blockly.RenderedConnection | null;
  readonly stop: () => void;
}

/**
 * Whether `node` is a live node of `workspace` outside `leaving` (a block about to be removed):
 * the canvas itself, or a block, field or connection of a block that is still on it.
 */
function isLiveNodeOf(
  node: Blockly.IFocusableNode,
  workspace: Blockly.WorkspaceSvg,
  leaving: Blockly.BlockSvg,
): boolean {
  if (node === workspace) {
    return true;
  }
  const block = blockOfNode(node);
  if (block?.workspace !== workspace || isGone(node) || !node.canBeFocused()) {
    return false;
  }
  for (let at: Blockly.BlockSvg | null = block; at !== null; at = at.getParent()) {
    if (at === leaving) {
      return false;
    }
  }
  return true;
}

/** The place in `targets` nearest a node: a place at its block (or one of its parents). */
function nearestPlace(
  targets: readonly DropTarget[],
  node: Blockly.IFocusableNode | null | undefined,
): number {
  if (node instanceof Blockly.RenderedConnection) {
    const exact = targets.findIndex(
      (target) => target.kind !== 'canvas' && target.connection === node,
    );
    if (exact >= 0) {
      return exact;
    }
  }
  for (let block = blockOfNode(node); block !== null; block = block.getParent()) {
    const at = block;
    const after = targets.findIndex((target) => target.kind === 'after' && target.owner === at);
    if (after >= 0) {
      return after;
    }
    const inside = targets.findIndex((target) => target.kind !== 'canvas' && target.owner === at);
    if (inside >= 0) {
      return inside;
    }
  }
  return -1;
}

/** Moves blocks of one workspace with the keyboard. */
export class KeyboardMover {
  private readonly deps: MoverDeps;
  private move: Move | null = null;

  constructor(deps: MoverDeps) {
    this.deps = deps;
  }

  /** Whether a block is held. */
  get active(): boolean {
    return this.move !== null;
  }

  /** The held block, or `null`. */
  get block(): Blockly.BlockSvg | null {
    return this.move?.block ?? null;
  }

  /** The place the held block would go now, or `null`. */
  get target(): DropTarget | null {
    const move = this.move;
    return move === null ? null : (move.targets[move.index] ?? null);
  }

  /** Whether `block` can be picked up: a movable block of this canvas that can go somewhere. */
  canMove(block: Blockly.BlockSvg): boolean {
    const workspace = this.deps.workspace;
    return (
      block.workspace === workspace &&
      !workspace.isReadOnly() &&
      !block.isInFlyout &&
      !block.isShadow() &&
      !block.isDeadOrDying() &&
      block.isMovable()
    );
  }

  /**
   * Picks up `block`. Returns whether a move started (it does not for a block that cannot move,
   * and says why).
   */
  start(block: Blockly.BlockSvg, options: MoveOptions = {}): boolean {
    this.cancel();
    const name = describeBlock(block, this.deps.names());
    if (!this.canMove(block)) {
      this.deps.announce(`“${name}” cannot be moved.`);
      return false;
    }
    const targets = dropTargets(block);
    const only = targets.length === 1 ? targets[0] : undefined;
    if (only !== undefined && isCurrentPlace(block, only)) {
      // A hat or a definition: it stands alone on the canvas.
      this.deps.announce(
        options.inserted === true
          ? `Added “${name}” loose on the canvas.`
          : `“${name}” has nowhere else to go.`,
      );
      return false;
    }
    const preferred = options.near === undefined ? -1 : nearestPlace(targets, options.near);
    const current = targets.findIndex((target) => isCurrentPlace(block, target));
    const index = preferred >= 0 ? preferred : Math.max(0, current);
    const stop = this.listen(block);
    this.move = {
      block,
      targets,
      index,
      inserted: options.inserted === true,
      near: options.near ?? null,
      group: options.group === undefined || options.group === '' ? null : options.group,
      highlighted: null,
      stop,
    };
    block.addClass(MOVING_CLASS);
    Blockly.keyboardNavigationController.setIsActive(true);
    const verb = options.inserted === true ? 'Adding' : 'Moving';
    this.show(
      `${verb} “${name}”. Arrow keys choose where it goes, Enter puts it there, Escape cancels.`,
    );
    return true;
  }

  /** Goes `delta` places on (wrapping around). */
  step(delta: number): void {
    const move = this.move;
    if (move === null || move.targets.length === 0) {
      return;
    }
    const count = move.targets.length;
    move.index = (((move.index + delta) % count) + count) % count;
    this.show('');
  }

  /** Goes to the first (`first`) or last place. */
  jump(to: 'first' | 'last'): void {
    const move = this.move;
    if (move === null) {
      return;
    }
    move.index = to === 'first' ? 0 : move.targets.length - 1;
    this.show('');
  }

  /**
   * Puts the held block at the current place. Returns whether the move ended (a place that is no
   * longer valid keeps the block held and says so).
   */
  drop(): boolean {
    const move = this.move;
    const target = this.target;
    if (move === null || target === null) {
      return false;
    }
    const { block, inserted, group } = move;
    const names = this.deps.names();
    if (block.isDeadOrDying()) {
      this.cancel();
      return true;
    }
    const name = describeBlock(block, names);
    const where = describeTarget(target, names);
    if (isCurrentPlace(block, target)) {
      this.end();
      this.deps.announce(inserted ? `Added “${name}” ${where}.` : `“${name}” stays where it was.`);
      Blockly.getFocusManager().focusNode(block);
      return true;
    }
    if (!isStillValid(block, target)) {
      this.deps.announce(`“${name}” cannot go there any more.`);
      return false;
    }
    this.end();
    if (this.place(block, target, group)) {
      Blockly.getFocusManager().focusNode(block);
      this.deps.announce(`${inserted ? 'Added' : 'Moved'} “${name}” ${where}.`);
    } else {
      this.deps.announce(`“${name}” could not be moved.`);
    }
    return true;
  }

  /**
   * Leaves the held block where it was and gives it the keyboard focus back. A block just added
   * from the toolbox is removed again, in the add's event group (so undo has nothing to bring
   * back), and the keyboard focus goes back to where the cursor was before the add. Does nothing
   * without a move.
   */
  cancel(): void {
    const move = this.move;
    if (move === null) {
      return;
    }
    this.end();
    const { block } = move;
    if (block.isDeadOrDying()) {
      return;
    }
    if (move.inserted) {
      this.focusAwayFrom(block, move.near);
      Blockly.Events.setGroup(move.group ?? true);
      try {
        block.dispose(false, false);
      } finally {
        Blockly.Events.setGroup(false);
      }
      this.deps.announce('Nothing was added.');
      return;
    }
    Blockly.getFocusManager().focusNode(block);
    this.deps.announce(`“${describeBlock(block, this.deps.names())}” stays where it was.`);
  }

  /** Ends any move (as {@link cancel}) and stops listening. */
  dispose(): void {
    this.cancel();
  }

  /**
   * The keys of a move (called for every key while a block is held): returns whether the key was
   * the move's. Keys that are not are swallowed too, except Tab, which ends the move.
   */
  handleKey(event: KeyboardEvent): boolean {
    if (this.move === null || event.isComposing) {
      return false;
    }
    const plain = !event.ctrlKey && !event.metaKey && !event.altKey;
    const rtl = this.deps.workspace.RTL;
    switch (plain ? event.key : '') {
      case 'ArrowDown':
        this.step(1);
        return true;
      case 'ArrowUp':
        this.step(-1);
        return true;
      case 'ArrowRight':
        this.step(rtl ? -1 : 1);
        return true;
      case 'ArrowLeft':
        this.step(rtl ? 1 : -1);
        return true;
      case 'Home':
        this.jump('first');
        return true;
      case 'End':
        this.jump('last');
        return true;
      case 'Enter':
      case ' ':
        this.drop();
        return true;
      case 'Escape':
        this.cancel();
        return true;
      default:
        return false;
    }
  }

  /** Highlights and announces the current place, after `prefix`. */
  private show(prefix: string): void {
    const move = this.move;
    const target = this.target;
    if (move === null || target === null) {
      return;
    }
    move.highlighted?.unhighlight();
    move.highlighted = null;
    const current = isCurrentPlace(move.block, target);
    if (target.kind !== 'canvas') {
      try {
        target.connection.highlight();
        move.highlighted = target.connection;
        move.block.workspace.scrollBoundsIntoView(target.owner.getBoundingRectangle());
      } catch (error: unknown) {
        console.warn('The place could not be shown', error);
      }
    }
    const where = describeTarget(target, this.deps.names());
    const place = `${current ? 'Where it is now: ' : ''}${where}`;
    const position = `${String(move.index + 1)} of ${String(move.targets.length)}`;
    this.deps.announce(`${prefix === '' ? '' : `${prefix} `}${place}, ${position}.`);
  }

  /** Ends the move without changing the canvas: un-draws it and stops listening. */
  private end(): void {
    const move = this.move;
    if (move === null) {
      return;
    }
    this.move = null;
    move.stop();
    try {
      move.highlighted?.unhighlight();
    } catch (error: unknown) {
      console.warn('The place could not be un-drawn', error);
    }
    if (!move.block.isDeadOrDying()) {
      move.block.removeClass(MOVING_CLASS);
    }
  }

  /**
   * Moves the keyboard focus off `block`, which is about to be removed, to `near` (when it is
   * still a live node of this canvas) or else to the canvas itself, if the focus is on the block
   * or a part of it. Blockly's own choice when a focused block is removed can be a block nested in
   * it, removed a moment later, which would leave the focus on nothing.
   */
  private focusAwayFrom(block: Blockly.BlockSvg, near: Blockly.IFocusableNode | null): void {
    try {
      const manager = Blockly.getFocusManager();
      const focused = manager.getFocusedNode();
      if (focused === null || !block.getSvgRoot().contains(focused.getFocusableElement())) {
        return;
      }
      const workspace = this.deps.workspace;
      manager.focusNode(near !== null && isLiveNodeOf(near, workspace, block) ? near : workspace);
    } catch (error: unknown) {
      console.warn('The keyboard focus could not be moved off the block', error);
    }
  }

  /**
   * Connects `block` at `target` in one undoable step (in `group` when given: the add the block
   * came from). Returns whether it worked.
   */
  private place(block: Blockly.BlockSvg, target: DropTarget, group: string | null): boolean {
    const workspace = this.deps.workspace;
    const from = movingConnection(block);
    const start = block.getRelativeToSurfaceXY();
    Blockly.Events.setGroup(group ?? true);
    try {
      block.unplug(true);
      if (target.kind === 'canvas') {
        const now = block.getRelativeToSurfaceXY();
        block.moveBy(start.x + CANVAS_OFFSET.x - now.x, start.y + CANVAS_OFFSET.y - now.y, [
          'keyboard',
        ]);
      } else if (from !== null) {
        target.connection.connect(from);
      }
      block.bringToFront();
      return true;
    } catch (error: unknown) {
      console.error('The block could not be moved', error);
      return false;
    } finally {
      Blockly.Events.setGroup(false);
      workspace.resizeContents();
    }
  }

  /**
   * Listens for what ends a move from outside: a pointer press, the focus leaving the canvas, the
   * block being deleted, or the canvas being cleared. Returns the function that stops listening.
   */
  private listen(block: Blockly.BlockSvg): () => void {
    const workspace = this.deps.workspace;
    const injection = workspace.getInjectionDiv();
    const document = injection.ownerDocument;
    const onPointerDown = () => {
      this.cancel();
    };
    const onFocusOut = (event: FocusEvent) => {
      const next = event.relatedTarget;
      if (next instanceof Node && injection.contains(next)) {
        return;
      }
      this.cancel();
    };
    const onChange = (event: Blockly.Events.Abstract) => {
      if (
        (event instanceof Blockly.Events.BlockDelete && event.ids?.includes(block.id) === true) ||
        event instanceof Blockly.Events.FinishedLoading ||
        block.isDeadOrDying()
      ) {
        this.end();
      }
    };
    document.addEventListener('pointerdown', onPointerDown, true);
    injection.addEventListener('focusout', onFocusOut);
    workspace.addChangeListener(onChange);
    return () => {
      document.removeEventListener('pointerdown', onPointerDown, true);
      injection.removeEventListener('focusout', onFocusOut);
      workspace.removeChangeListener(onChange);
    };
  }
}
