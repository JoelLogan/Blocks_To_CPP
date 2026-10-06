/**
 * Keyboard navigation of one block editor (docs/spec/04-user-interface.md §4.7), built on Blockly
 * 12's own keyboard navigation: its focus manager, its line cursor (`↓`/`↑` between blocks, `→`/`←`
 * through fields and inputs) and its navigation rules for the canvas, the toolbox and the flyout.
 * Blockly 12 core has the rules but binds only Escape, Delete, the clipboard keys, undo, redo and
 * `Ctrl+Enter`; this module adds what the M2 editor needs on top: the arrow keys, editing a field
 * with Enter, going between the canvas, the toolbox and its blocks, adding a block from the toolbox
 * and moving a block (./mover.ts).
 *
 * Each action returns whether it handled the key, so that Blockly's own shortcut for the same key
 * (if any) runs otherwise.
 */
import * as Blockly from 'blockly/core';

import type { Announcer } from './announcer';
import { blockOfNode } from './keepFocus';
import { describeNode, type SymbolNames } from './labels';
import { KeyboardMover } from './mover';

/** Where the keyboard focus is, seen from one editor. */
export type FocusArea = 'canvas' | 'flyout' | 'toolbox' | 'elsewhere';

/** A step of the cursor: to the next or previous block, or into or out of a block's parts. */
export type CursorStep = 'next' | 'previous' | 'in' | 'out';

/** What the keyboard of one editor needs. */
export interface EditorKeyboardDeps {
  readonly workspace: Blockly.WorkspaceSvg;
  readonly announcer: Announcer;
  /** The names of the document's symbols, for naming blocks. */
  readonly names: () => SymbolNames;
}

/** The focused node, or `null`. Never throws. */
function focusedNode(): Blockly.IFocusableNode | null {
  try {
    return Blockly.getFocusManager().getFocusedNode();
  } catch (error: unknown) {
    console.warn('The focus could not be read', error);
    return null;
  }
}

/** The focused tree, or `null`. Never throws. */
function focusedTree(): Blockly.IFocusableTree | null {
  try {
    return Blockly.getFocusManager().getFocusedTree();
  } catch (error: unknown) {
    console.warn('The focus could not be read', error);
    return null;
  }
}

/** The block that moves for a node: its block, or for a value slot the block the slot is in. */
function movableBlockOf(node: Blockly.IFocusableNode | null): Blockly.BlockSvg | null {
  let block = blockOfNode(node);
  while (block?.isShadow() === true) {
    block = block.getParent();
  }
  return block;
}

/** The first field of `block` that edits the whole block (a number or text slot), or `null`. */
function fullBlockField(block: Blockly.BlockSvg): Blockly.Field | null {
  if (!block.isSimpleReporter()) {
    return null;
  }
  for (const input of block.inputList) {
    for (const field of input.fieldRow) {
      if (field.isClickable() && field.isFullBlockField()) {
        return field;
      }
    }
  }
  return null;
}

/** The keyboard of one block editor: its cursor, its toolbox and flyout, and moves. */
export class EditorKeyboard {
  readonly workspace: Blockly.WorkspaceSvg;
  readonly announcer: Announcer;
  readonly mover: KeyboardMover;
  private readonly names: () => SymbolNames;
  /**
   * Where the keyboard cursor was on the canvas when the toolbox was opened: a block added from the
   * toolbox starts at the place nearest it.
   */
  private insertionPoint: Blockly.IFocusableNode | null = null;

  constructor(deps: EditorKeyboardDeps) {
    this.workspace = deps.workspace;
    this.announcer = deps.announcer;
    this.names = deps.names;
    this.mover = new KeyboardMover({
      workspace: deps.workspace,
      announce: (text) => {
        deps.announcer.announce(text);
      },
      names: deps.names,
    });
  }

  /** The toolbox's flyout, or `null` (there is none without a toolbox). */
  flyout(): Blockly.IFlyout | null {
    return this.workspace.getFlyout();
  }

  /** The toolbox, or `null`. */
  toolbox(): Blockly.Toolbox | null {
    const toolbox = this.workspace.getToolbox();
    return toolbox instanceof Blockly.Toolbox ? toolbox : null;
  }

  /** Where the keyboard focus is. */
  area(): FocusArea {
    const tree = focusedTree();
    if (tree === null) {
      return 'elsewhere';
    }
    if (tree === this.workspace) {
      return 'canvas';
    }
    const toolbox = this.toolbox();
    if (toolbox !== null && tree === toolbox) {
      return 'toolbox';
    }
    const flyout = this.flyout();
    if (flyout !== null && tree === flyout.getWorkspace()) {
      return 'flyout';
    }
    return 'elsewhere';
  }

  /** Says what the keyboard cursor is on now. */
  announceFocus(): void {
    const text = describeNode(focusedNode(), this.names());
    if (text !== '') {
      this.announcer.announce(text);
    }
  }

  /**
   * Moves the cursor of the canvas or of the flyout one `step`. With nothing focused in it yet, the
   * first (or for `previous`/`out` the last) node is focused. Returns whether the key was handled.
   */
  step(step: CursorStep): boolean {
    const area = this.area();
    if (area !== 'canvas' && area !== 'flyout') {
      return false;
    }
    const workspace = area === 'canvas' ? this.workspace : this.flyout()?.getWorkspace();
    if (workspace === undefined) {
      return false;
    }
    Blockly.keyboardNavigationController.setIsActive(true);
    const cursor = workspace.getCursor();
    const node = focusedNode();
    let reached: Blockly.IFocusableNode | null;
    if (node === null || node === workspace) {
      reached = this.firstOrLast(cursor, step);
    } else if (area === 'flyout' && (step === 'in' || step === 'out')) {
      // The toolbox's blocks are a list: → does nothing and ← goes back to the categories.
      return step === 'out' ? this.focusToolbox() : true;
    } else {
      switch (step) {
        case 'next':
          reached = cursor.next();
          break;
        case 'previous':
          reached = cursor.prev();
          break;
        case 'in':
          reached = cursor.in();
          break;
        case 'out':
          reached = cursor.out();
          break;
      }
    }
    if (reached === null && (node === null || node === workspace)) {
      this.announcer.announce(
        area === 'canvas' ? 'The canvas is empty. Press T to add a block.' : 'No blocks here.',
      );
      return true;
    }
    this.announceFocus();
    return true;
  }

  /**
   * Enter or Space: edits the focused field, adds the focused toolbox block, presses the focused
   * toolbox button, or opens the toolbox from the canvas. Returns whether the key was handled.
   */
  activate(): boolean {
    switch (this.area()) {
      case 'canvas':
        return this.activateOnCanvas(focusedNode());
      case 'flyout':
        return this.activateInFlyout(focusedNode());
      case 'toolbox':
        return this.enterFlyout();
      case 'elsewhere':
        return false;
    }
  }

  /** `M`: picks up the block the cursor is on. Returns whether the key was handled. */
  pickUp(): boolean {
    if (this.area() !== 'canvas') {
      return false;
    }
    const block = movableBlockOf(focusedNode());
    if (block === null) {
      this.announcer.announce('Go to a block first, then press M to move it.');
      return true;
    }
    this.mover.start(block);
    return true;
  }

  /**
   * `T`: opens the toolbox, remembering where the cursor was on the canvas (a block added from the
   * toolbox starts there). Returns whether the key was handled.
   */
  openToolbox(): boolean {
    if (this.area() !== 'canvas') {
      return false;
    }
    return this.focusToolbox();
  }

  /** Escape: from the toolbox or its blocks back to the canvas. Returns whether it was handled. */
  escape(): boolean {
    const area = this.area();
    if (area !== 'toolbox' && area !== 'flyout') {
      return false;
    }
    Blockly.keyboardNavigationController.setIsActive(true);
    Blockly.getFocusManager().focusTree(this.workspace);
    this.announceFocus();
    return true;
  }

  /**
   * → or Enter on a toolbox category: goes to the category's blocks (its label in the flyout).
   * Returns whether the key was handled.
   */
  enterFlyout(): boolean {
    if (this.area() !== 'toolbox') {
      return false;
    }
    const flyout = this.flyout();
    if (flyout?.isVisible() !== true) {
      return false;
    }
    Blockly.keyboardNavigationController.setIsActive(true);
    const selected = this.toolbox()?.getSelectedItem();
    const name = selected instanceof Blockly.ToolboxCategory ? selected.getName() : null;
    const label =
      name === null
        ? undefined
        : flyout
            .getContents()
            .map((item) => item.getElement())
            .find(
              (element): element is Blockly.FlyoutButton =>
                element instanceof Blockly.FlyoutButton &&
                element.isLabel() &&
                element.getButtonText() === name,
            );
    if (label === undefined) {
      Blockly.getFocusManager().focusTree(flyout.getWorkspace());
    } else {
      Blockly.getFocusManager().focusNode(label);
    }
    this.announceFocus();
    return true;
  }

  /** Ends a move and forgets the insertion point. */
  dispose(): void {
    this.mover.dispose();
    this.insertionPoint = null;
  }

  /**
   * Where the cursor starts when nothing in its tree has the focus: the first node going forwards,
   * the last block (`previous`) or the last node (`out`) going backwards. Focuses it.
   */
  private firstOrLast(cursor: Blockly.LineCursor, step: CursorStep): Blockly.IFocusableNode | null {
    const first = cursor.getFirstNode();
    if (first === null) {
      return null;
    }
    if (step === 'next' || step === 'in') {
      cursor.setCurNode(first);
      return first;
    }
    if (step === 'out') {
      const last = cursor.getLastNode() ?? first;
      cursor.setCurNode(last);
      return last;
    }
    // The block before the first one, going round: the last block.
    cursor.setCurNode(first);
    return cursor.prev() ?? first;
  }

  /** Focuses the toolbox (its selected category), remembering the canvas cursor. */
  private focusToolbox(): boolean {
    const toolbox = this.toolbox();
    if (toolbox === null) {
      return false;
    }
    if (this.area() === 'canvas') {
      this.insertionPoint = focusedNode();
    }
    Blockly.keyboardNavigationController.setIsActive(true);
    Blockly.getFocusManager().focusTree(toolbox);
    this.announcer.announce(
      'Toolbox. Up and down arrows choose a category, right arrow goes to its blocks.',
    );
    return true;
  }

  private activateOnCanvas(node: Blockly.IFocusableNode | null): boolean {
    if (node instanceof Blockly.Field) {
      if (!node.isClickable()) {
        this.announcer.announce('This field cannot be changed here.');
        return true;
      }
      node.showEditor();
      return true;
    }
    if (node instanceof Blockly.BlockSvg) {
      const field = fullBlockField(node);
      if (field !== null) {
        field.showEditor();
      } else {
        this.announcer.announce(
          'Right arrow goes to the block’s fields, M moves it, Delete removes it.',
        );
      }
      return true;
    }
    if (node instanceof Blockly.RenderedConnection || node === this.workspace || node === null) {
      if (this.workspace.isReadOnly()) {
        return false;
      }
      return this.focusToolbox();
    }
    return false;
  }

  private activateInFlyout(node: Blockly.IFocusableNode | null): boolean {
    if (node instanceof Blockly.FlyoutButton) {
      if (node.isLabel()) {
        this.announcer.announce('Down arrow goes to the category’s blocks.');
        return true;
      }
      const callback = this.workspace.getButtonCallback(node.callbackKey);
      if (callback === null) {
        return false;
      }
      callback(node);
      return true;
    }
    if (node instanceof Blockly.BlockSvg) {
      return this.addFromFlyout(node);
    }
    return false;
  }

  /**
   * Adds a copy of a toolbox block to the canvas and lets the keyboard choose where it goes.
   *
   * The whole add is one Blockly event group, as a pointer drag from the toolbox is: the copy's
   * creation, Blockly's move of it next to the toolbox, and then the drop (or, on Escape, its
   * removal), so one `Ctrl+Z` takes an added block away and an add that was cancelled leaves
   * nothing for undo to bring back. Blockly turns the canvas's resizing off while it creates the
   * copy (a pointer drag turns it on again when it ends); it is turned on again at once here.
   */
  private addFromFlyout(original: Blockly.BlockSvg): boolean {
    const flyout = this.flyout();
    if (flyout === null || this.workspace.isReadOnly()) {
      return false;
    }
    if (!original.isEnabled()) {
      this.announcer.announce('This block cannot be added now.');
      return true;
    }
    let added: Blockly.BlockSvg;
    const outerGroup = Blockly.Events.getGroup();
    if (outerGroup === '') {
      Blockly.Events.setGroup(true);
    }
    const group = Blockly.Events.getGroup();
    try {
      added = flyout.createBlock(original);
    } catch (error: unknown) {
      console.error('The block could not be added from the toolbox', error);
      this.announcer.announce('The block could not be added.');
      return true;
    } finally {
      if (outerGroup === '') {
        Blockly.Events.setGroup(false);
      }
      this.workspace.setResizesEnabled(true);
    }
    Blockly.getFocusManager().focusNode(added);
    this.mover.start(added, { inserted: true, near: this.insertionPoint, group });
    return true;
  }
}
