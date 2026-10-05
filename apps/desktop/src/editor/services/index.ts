/**
 * The editor services: symbol names and scope, static types, dialogs, and the session hooks that
 * keep them current (docs/spec/03-block-language.md §3.6, 06 §6.5).
 */
import type { CoreWasm, PreviewResult } from '@blocks2cpp/b2c-core-wasm';
import { refreshMutatorLabels, refreshSymbolNames } from '@blocks2cpp/blockly-ext';
import type * as Blockly from 'blockly/core';

import type { SessionHooks } from '../sync/session';
import { SymbolNames } from './names';
import { createSymbolProvider, createTypeOracle } from './symbols';

export {
  defaultInputShadow,
  type EditorServiceSet,
  fieldDialogs,
  installEditorServices,
  registerEditorBlocks,
} from './install';
export { MAX_LAST_KNOWN_NAMES, SymbolNames } from './names';
export { createSymbolProvider, createTypeOracle, type SymbolServiceDeps } from './symbols';

/** The symbol and type services of one editor, and the names they read. */
export interface EditorSymbolServices {
  readonly names: SymbolNames;
  readonly symbols: ReturnType<typeof createSymbolProvider>;
  readonly types: ReturnType<typeof createTypeOracle>;
}

/** Creates the symbol and type services over the running core and the latest preview. */
export function createSymbolServices(deps: {
  readonly core: () => CoreWasm | null;
  readonly preview: () => PreviewResult | null;
}): EditorSymbolServices {
  const names = new SymbolNames();
  return {
    names,
    symbols: createSymbolProvider({ core: deps.core, preview: deps.preview, names }),
    types: createTypeOracle({ preview: deps.preview }),
  };
}

/**
 * The session hooks that keep the services current and the canvas labelled: names from every
 * committed document (reset on load), fresh IDs of duplicates named at once, references redrawn
 * when a name changed, and call argument labels redrawn after each analysis.
 */
export function servicesSessionHooks(
  services: EditorSymbolServices,
  workspace: Blockly.Workspace,
): SessionHooks {
  const relabel = (): void => {
    refreshSymbolNames(workspace);
    refreshMutatorLabels(workspace);
  };
  return {
    onLoaded: (doc) => {
      services.names.reset();
      services.names.update(doc);
      relabel();
    },
    onCommitted: (doc) => {
      if (services.names.update(doc)) {
        refreshSymbolNames(workspace);
      }
    },
    onPreviewed: () => {
      refreshMutatorLabels(workspace);
    },
    onRenamed: (syms) => {
      services.names.alias(syms);
    },
  };
}
