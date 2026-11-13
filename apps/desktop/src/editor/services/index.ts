/**
 * The editor services: symbol names and scope, static types, dialogs, and the session hooks that
 * keep them current (docs/spec/03-block-language.md §3.6, 06 §6.5).
 */
import type { CoreWasm, PreviewResult, ScopeIndex } from '@blocks2cpp/b2c-core-wasm';
import { refreshMutatorLabels, refreshSymbolNames } from '@blocks2cpp/blockly-ext';
import type * as Blockly from 'blockly/core';

import type { NO_SCOPE } from '../../app/store';
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
export {
  analysisScope,
  createSymbolProvider,
  createTypeOracle,
  type SymbolServiceDeps,
} from './symbols';

/** The symbol and type services of one editor, and the names they read. */
export interface EditorSymbolServices {
  readonly names: SymbolNames;
  readonly symbols: ReturnType<typeof createSymbolProvider>;
  readonly types: ReturnType<typeof createTypeOracle>;
}

/**
 * Creates the symbol and type services of the editor of `workspace` over the running core and the
 * latest preview.
 */
export function createSymbolServices(deps: {
  readonly core: () => CoreWasm | null;
  readonly preview: () => PreviewResult | null;
  /** The latest preview's scope index (`AnalysisState.scope`); the core answers without it. */
  readonly scope?: () => ScopeIndex | typeof NO_SCOPE | null;
  readonly workspace: Blockly.Workspace;
  /** The ID of the module the canvas shows, or `null` when none is. */
  readonly activeModule: () => string | null;
  /** Whether the open project has several modules; one when absent. */
  readonly multiModule?: () => boolean;
  /** The name of the module with this ID, or `null`. */
  readonly moduleName?: (moduleId: string) => string | null;
}): EditorSymbolServices {
  const names = new SymbolNames();
  return {
    names,
    symbols: createSymbolProvider({
      core: deps.core,
      preview: deps.preview,
      ...(deps.scope === undefined ? {} : { scope: deps.scope }),
      names,
      workspace: deps.workspace,
      activeModule: deps.activeModule,
      ...(deps.multiModule === undefined ? {} : { multiModule: deps.multiModule }),
      ...(deps.moduleName === undefined ? {} : { moduleName: deps.moduleName }),
    }),
    types: createTypeOracle({
      preview: deps.preview,
      core: deps.core,
      workspace: deps.workspace,
      ...(deps.scope === undefined ? {} : { scope: deps.scope }),
    }),
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
