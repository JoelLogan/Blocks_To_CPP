/**
 * The diagnostics editor plugin (docs/spec/04-user-interface.md §4.3, §4.4): badges, outlines and
 * part marks on the blocks, and the block side of two-way highlighting. Append it to
 * `EDITOR_PLUGINS` (app/editorPlugins.ts).
 */
import { catalogBlock, displayTypeName, registerDiagnosticIcon } from '@blocks2cpp/blockly-ext';

import type { EditorPlugin } from '../../app/editor-types';
import { attachHighlightTracking } from '../highlight/tracking';
import { attachBlockDiagnostics } from './badges';
import { type PathCatalog, providePathCatalog } from './catalog';

/** The block catalog of blockly-ext, as Problems' block paths read it. */
export const BLOCKLY_EXT_PATH_CATALOG: PathCatalog = Object.freeze({
  block: catalogBlock,
  typeName: displayTypeName,
});

/** Diagnostics on blocks and the block side of two-way highlighting. */
export const diagnosticsPlugin: EditorPlugin = {
  name: 'diagnostics',
  attach(ctx) {
    registerDiagnosticIcon();
    // The catalog is the same for every workspace, so it stays provided after detaching.
    providePathCatalog(BLOCKLY_EXT_PATH_CATALOG);
    const detachBadges = attachBlockDiagnostics(ctx);
    const detachTracking = attachHighlightTracking(ctx);
    return () => {
      detachTracking();
      detachBadges();
    };
  },
};
