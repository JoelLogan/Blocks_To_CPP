/**
 * The editor plugins, attached in this order when the workspace is created and detached in the
 * reverse order when it is disposed: the toolbox (its categories follow the selection, so it
 * comes first), the diagnostics on blocks with two-way highlighting, then the clipboard (copy,
 * cut, paste and duplicate through the core's validated format).
 */
import { clipboardPlugin } from '../editor/clipboard';
import { diagnosticsPlugin } from '../editor/diagnostics';
import { toolboxPlugin } from '../editor/toolbox';
import type { EditorPlugin } from './editor-types';

export const EDITOR_PLUGINS: EditorPlugin[] = [toolboxPlugin, diagnosticsPlugin, clipboardPlugin];
