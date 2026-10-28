/**
 * The diagnostics editor plugin (docs/spec/04-user-interface.md §4.3, §4.4): badges, outlines and
 * part marks on the blocks, and the block side of two-way highlighting. Append it to
 * `EDITOR_PLUGINS` (app/editorPlugins.ts).
 */
import type { FieldValue } from '@blocks2cpp/b2c-core-wasm';
import {
  catalogBlock,
  displayTypeName,
  parseTerm,
  registerDiagnosticIcon,
  termKey,
  type TypeFactsTs,
} from '@blocks2cpp/blockly-ext';

import type { EditorPlugin } from '../../app/editor-types';
import { attachHighlightTracking } from '../highlight/tracking';
import { attachBlockDiagnostics } from './badges';
import { type PathCatalog, providePathCatalog } from './catalog';

/**
 * Facts that know each symbol of `names` by its name, so that a user type in a label reads as its
 * name (`list of Student`). A type label reads only the name; the struct-or-enum kind and the
 * classes are not known from names, and labels do not use them.
 */
function namingFacts(names: ReadonlyMap<string, string>): TypeFactsTs {
  return {
    userType(id: string) {
      const name = names.get(id);
      return name === undefined
        ? null
        : { name, kind: 'struct', comparable: false, printable: false };
    },
  };
}

/**
 * A type field's value as its dropdown shows it: a name as it is, a term (a template or a user
 * type, as the document stores it) by its key, and `null` for anything else.
 */
function typeName(type: FieldValue, names: ReadonlyMap<string, string>): string | null {
  if (typeof type === 'string') {
    return displayTypeName(type, namingFacts(names));
  }
  const term = typeof type === 'object' ? parseTerm(type) : null;
  return term === null ? null : displayTypeName(termKey(term), namingFacts(names));
}

/** The block catalog of blockly-ext, as Problems' block paths read it. */
export const BLOCKLY_EXT_PATH_CATALOG: PathCatalog = Object.freeze({
  block: catalogBlock,
  typeName,
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
