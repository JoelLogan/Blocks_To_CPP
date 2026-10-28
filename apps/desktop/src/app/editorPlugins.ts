/**
 * The editor plugins, attached in this order when the workspace is created and detached in the
 * reverse order when it is disposed: the toolbox (it remembers the canvas selection for *Make a
 * variable*, so it comes first), the diagnostics on blocks with two-way highlighting, the
 * clipboard (copy, cut, paste and duplicate through the core's validated format), the menus
 * (Blockly's other menu items as the editor offers them), Quick Insert and toolbox search (they
 * read the toolbox's blocks), then the keyboard navigation (it acts
 * on what the others put on the canvas, and leaves the clipboard keys to the clipboard). The last
 * one closes Blockly's pop-ups before one of the app's dialogs opens.
 */
import { clipboardPlugin } from '../editor/clipboard';
import { diagnosticsPlugin } from '../editor/diagnostics';
import { keyboardPlugin } from '../editor/keyboard';
import { menusPlugin } from '../editor/menus';
import { quickInsertPlugin } from '../editor/quick-insert';
import { toolboxPlugin } from '../editor/toolbox';
import { blocklyPopupsPlugin } from './dialogs';
import type { EditorPlugin } from './editor-types';

export const EDITOR_PLUGINS: EditorPlugin[] = [
  toolboxPlugin,
  diagnosticsPlugin,
  clipboardPlugin,
  menusPlugin,
  quickInsertPlugin,
  keyboardPlugin,
  blocklyPopupsPlugin(),
];
