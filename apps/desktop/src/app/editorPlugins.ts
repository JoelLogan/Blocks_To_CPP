/**
 * The editor plugins, attached in this order when the workspace is created and detached in the
 * reverse order when it is disposed. Milestone M2's waves 3 and 4 append the toolbox, the
 * diagnostics and the clipboard here.
 */
import type { EditorPlugin } from './editor-types';

export const EDITOR_PLUGINS: EditorPlugin[] = [];
