/**
 * Diagnostics in the editor (docs/spec/04-user-interface.md §4.4): on the blocks (the
 * `diagnosticsPlugin`), and the rows of the Problems panel (`buildProblemItems`).
 *
 * This entry loads Blockly (through the plugin and the badges). The shell's dock panels
 * (app/panels.tsx) import the Blockly-free modules directly instead: ./inputs, ./problemItems and
 * ./catalog.
 */
export {
  BadgeApplier,
  MAX_BADGED_DIAGNOSTICS,
  attachBlockDiagnostics,
  planBadges,
  type BadgePlan,
} from './badges';
export {
  MAX_LABEL_CHARS,
  MAX_PATH_LEVELS,
  PATH_SEPARATOR,
  blockLabel,
  blockPath,
  indexDocument,
  moduleOf,
  shorten,
  type BlockPlace,
  type DocumentIndex,
} from './blockPath';
export { pathCatalog, providePathCatalog, subscribePathCatalog, type PathCatalog } from './catalog';
export {
  codePanelDiagnostics,
  diagnosticInputs,
  diagnosticInputsFrom,
  diagnosticSources,
  isBuildStale,
  type DiagnosticInputs,
  type DiagnosticSources,
} from './inputs';
export { BLOCKLY_EXT_PATH_CATALOG, diagnosticsPlugin } from './plugin';
export { MAX_PROBLEM_ITEMS, buildProblemItems } from './problemItems';
