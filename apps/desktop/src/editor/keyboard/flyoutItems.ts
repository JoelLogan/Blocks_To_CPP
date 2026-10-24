/**
 * The items of the toolbox's flyout as the keyboard reaches them (docs/spec/04-user-interface.md
 * §4.7, §4.8). `↓` and `↑` stop on everything the flyout shows: its blocks, its buttons (*Make a
 * variable*) and its labels, which are category headings, other headings (the headings of the
 * Variables category's groups of getters, *In greet*; *Your functions*) and notes (*Your program
 * starts at “when program starts”.*, *…and 3 more*). A note drawn on several lines is one stop,
 * read as a whole, and a getter listed under a group heading is named with its group, so two
 * variables `i` of different functions can be told apart.
 *
 * Blockly's line cursor stops only on blocks, so the flyout's steps use this module's
 * {@link isFlyoutStop} instead (./navigation.ts). The toolbox marks its notes and group headings
 * with the CSS classes below (../toolbox/contents.ts); any other label is a category heading when
 * its text names a category of the toolbox, and a heading otherwise.
 */
import * as Blockly from 'blockly/core';

/** The class of a note's label (its first line, when it has several). */
export const FLYOUT_NOTE_CLASS = 'b2cFlyoutNote';

/** The class of the label of a note's second or later line. */
export const FLYOUT_NOTE_LINE_CLASS = 'b2cFlyoutNoteLine';

/** The class of the heading of a group of getters in the Variables category (*In greet*). */
export const FLYOUT_GROUP_CLASS = 'b2cFlyoutGroup';

/** What a label of the flyout is. */
export type FlyoutLabelKind = 'category' | 'heading' | 'group' | 'note' | 'noteLine';

/** Whether a button or label of the flyout was given CSS class `name`. Never throws. */
function hasClass(button: Blockly.FlyoutButton, name: string): boolean {
  try {
    return button.getSvgRoot().classList.contains(name);
  } catch {
    return false;
  }
}

/** Whether `text` is the name of a category of the toolbox of `workspace`. */
function isCategoryName(workspace: Blockly.WorkspaceSvg, text: string): boolean {
  const toolbox = workspace.getToolbox();
  return (
    toolbox instanceof Blockly.Toolbox &&
    toolbox
      .getToolboxItems()
      .some((item) => item instanceof Blockly.ToolboxCategory && item.getName() === text)
  );
}

/** What label `button` of the flyout is (see the module comment). */
export function flyoutLabelKind(button: Blockly.FlyoutButton): FlyoutLabelKind {
  if (hasClass(button, FLYOUT_NOTE_LINE_CLASS)) {
    return 'noteLine';
  }
  if (hasClass(button, FLYOUT_NOTE_CLASS)) {
    return 'note';
  }
  if (hasClass(button, FLYOUT_GROUP_CLASS)) {
    return 'group';
  }
  return isCategoryName(button.getTargetWorkspace(), button.getButtonText())
    ? 'category'
    : 'heading';
}

/**
 * Whether the keyboard's `↓` and `↑` stop on `node` in the flyout: a block (not one plugged into
 * another), a button, or a label other than the second or later line of a note.
 */
export function isFlyoutStop(node: Blockly.IFocusableNode | null): boolean {
  if (node instanceof Blockly.BlockSvg) {
    // Blockly types it as always there, but a statement block has no output connection.
    const output = node.outputConnection as Blockly.RenderedConnection | null;
    return (output?.targetBlock() ?? null) === null;
  }
  if (node instanceof Blockly.FlyoutButton) {
    return !node.isLabel() || flyoutLabelKind(node) !== 'noteLine';
  }
  return false;
}

/** The flyout items `workspace`'s flyout shows, in order (none without a flyout). */
function flyoutItems(workspace: Blockly.WorkspaceSvg | null): readonly Blockly.FlyoutItem[] {
  return workspace?.getFlyout()?.getContents() ?? [];
}

/** The whole text of the note whose first line is label `button`: its lines, one after another. */
export function flyoutNoteText(button: Blockly.FlyoutButton): string {
  const items = flyoutItems(button.getTargetWorkspace());
  const index = items.findIndex((item) => item.getElement() === button);
  const lines = [button.getButtonText()];
  for (const item of index < 0 ? [] : items.slice(index + 1)) {
    const element = item.getElement();
    if (element instanceof Blockly.FlyoutSeparator) {
      continue;
    }
    if (
      !(element instanceof Blockly.FlyoutButton) ||
      !element.isLabel() ||
      flyoutLabelKind(element) !== 'noteLine'
    ) {
      break;
    }
    lines.push(element.getButtonText());
  }
  return lines.join(' ');
}

/**
 * The heading of the group of getters block `block` of the flyout is listed in (*In greet*): the
 * label nearest above it, when that label is a group heading. `null` for any other block.
 */
export function flyoutGroupOf(block: Blockly.BlockSvg): string | null {
  if (!block.isInFlyout) {
    return null;
  }
  const items = flyoutItems(block.workspace.targetWorkspace);
  const root = block.getRootBlock();
  const index = items.findIndex((item) => item.getElement() === root);
  for (let at = index - 1; at >= 0; at--) {
    const element = items[at]?.getElement();
    if (element instanceof Blockly.FlyoutButton && element.isLabel()) {
      return flyoutLabelKind(element) === 'group' ? element.getButtonText() : null;
    }
  }
  return null;
}
