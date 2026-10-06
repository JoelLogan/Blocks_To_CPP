/**
 * The editor plugins, attached in this order when the workspace is created and detached in the
 * reverse order when it is disposed: the toolbox (its categories follow the selection, so it
 * comes first), the diagnostics on blocks with two-way highlighting, the clipboard (copy, cut,
 * paste and duplicate through the core's validated format), then the keyboard navigation (it acts
 * on what the others put on the canvas, and leaves the clipboard keys to the clipboard).
 */
import { clipboardPlugin } from '../editor/clipboard';
import { diagnosticsPlugin } from '../editor/diagnostics';
import { keyboardPlugin } from '../editor/keyboard';
import { toolboxPlugin } from '../editor/toolbox';
import type { EditorPlugin } from './editor-types';

export const EDITOR_PLUGINS: EditorPlugin[] = [
  toolboxPlugin,
  diagnosticsPlugin,
  clipboardPlugin,
  keyboardPlugin,
];
