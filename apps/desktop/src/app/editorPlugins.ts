/**
 * The editor plugins, attached in this order when the workspace is created and detached in the
 * reverse order when it is disposed: the toolbox (its categories follow the selection, so it
 * comes first), then the diagnostics on blocks with two-way highlighting. Milestone M2's wave 4
 * appends the clipboard here.
 */
import { diagnosticsPlugin } from '../editor/diagnostics';
import { toolboxPlugin } from '../editor/toolbox';
import type { EditorPlugin } from './editor-types';

export const EDITOR_PLUGINS: EditorPlugin[] = [toolboxPlugin, diagnosticsPlugin];
