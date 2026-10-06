/**
 * The block editor's keys (docs/spec/04-user-interface.md §4.7), as one table: the keyboard plugin
 * binds exactly these, the canvas describes them to screen readers, and the manual accessibility
 * checklist (docs/manual-tests/m2-accessibility.md) lists them. Blockly's own keys and the
 * clipboard's are listed too, so the table is the whole map of the editor's keys.
 */

/** Where a key works. */
export type KeyScope = 'canvas' | 'move' | 'toolbox' | 'flyout';

/** One row of the key map. */
export interface KeyHelp {
  /** Where the key works. */
  readonly scope: KeyScope;
  /** The keys, as people write them (`Ctrl+C`). */
  readonly keys: string;
  /** What they do. */
  readonly action: string;
}

/** Every key of the block editor, by where it works. */
export const KEY_MAP: readonly KeyHelp[] = [
  { scope: 'canvas', keys: '↓ / ↑', action: 'Next or previous block' },
  {
    scope: 'canvas',
    keys: '→ / ←',
    action: 'Next or previous part of a block (fields and inputs)',
  },
  {
    scope: 'canvas',
    keys: 'Enter or Space',
    action: 'Edit the field; on the canvas itself, open the toolbox',
  },
  { scope: 'canvas', keys: 'M', action: 'Move the block (choose where it goes)' },
  { scope: 'canvas', keys: 'T', action: 'Open the toolbox to add a block here' },
  { scope: 'canvas', keys: 'Delete or Backspace', action: 'Delete the block' },
  { scope: 'canvas', keys: 'Ctrl+C / Ctrl+X / Ctrl+V', action: 'Copy, cut or paste blocks' },
  { scope: 'canvas', keys: 'Ctrl+D', action: 'Duplicate the block' },
  { scope: 'canvas', keys: 'Ctrl+Z / Ctrl+Y', action: 'Undo or redo' },
  { scope: 'canvas', keys: 'Ctrl+Enter', action: 'Open the block’s menu' },
  { scope: 'move', keys: '↓ / ↑ (or → / ←)', action: 'Next or previous place for the block' },
  { scope: 'move', keys: 'Enter or Space', action: 'Put the block there' },
  { scope: 'move', keys: 'Escape', action: 'Leave the block where it was' },
  { scope: 'toolbox', keys: '↓ / ↑', action: 'Next or previous category' },
  { scope: 'toolbox', keys: '→ or Enter', action: 'Go to the category’s blocks' },
  { scope: 'toolbox', keys: 'Escape', action: 'Back to the canvas' },
  { scope: 'flyout', keys: '↓ / ↑', action: 'Next or previous block or button' },
  {
    scope: 'flyout',
    keys: 'Enter or Space',
    action: 'Add the block (then choose where it goes), or press the button',
  },
  { scope: 'flyout', keys: '←', action: 'Back to the categories' },
  { scope: 'flyout', keys: 'Escape', action: 'Back to the canvas' },
];

/** What the canvas tells screen readers about its keys (its accessible description). */
export const CANVAS_DESCRIPTION =
  'Arrow keys move between blocks and their parts. Enter edits a field. M moves a block. ' +
  'T opens the toolbox. Delete removes a block. Ctrl+C, Ctrl+X, Ctrl+V and Ctrl+D copy, ' +
  'cut, paste and duplicate. Ctrl+Z undoes.';

/** What the toolbox's blocks tell screen readers about their keys. */
export const FLYOUT_DESCRIPTION =
  'Up and down arrows choose a block. Enter adds it, then arrow keys choose where it goes. ' +
  'Left arrow goes back to the categories, Escape back to the canvas.';

/** The accessible name of the canvas. */
export const CANVAS_LABEL = 'Block canvas';

/** The accessible name of the toolbox's blocks (the flyout). */
export const FLYOUT_LABEL = 'Blocks to add';
