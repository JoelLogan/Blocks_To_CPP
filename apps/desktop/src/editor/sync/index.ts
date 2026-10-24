/**
 * The BDM ⇄ Blockly sync (docs/spec/02-architecture.md §2.4.1, 05 §5.4): building a module's
 * canvas from a document, reading it back, fresh IDs for duplicated blocks, and the editing
 * session that ties them to the store and the live preview.
 */
export { buildBlockTree, type BuildPlace, loadModule, withoutEvents } from './bdmToWorkspace';
export {
  childNodes,
  type FoundDecl,
  forEachNode,
  nodeDecls,
  renameInTrees,
  stableJson,
  treeBlockIds,
  treeDecls,
} from './bdmTree';
export { DeclIndex, declaredSyms, DuplicateGuard } from './duplicates';
export { SyncError, type SyncErrorKind } from './errors';
export {
  clampCoordinate,
  clampZoom,
  MAX_COORDINATE,
  MAX_REMAP_MEMO,
  MAX_ZOOM,
  MIN_ZOOM,
  SYNC_DEBOUNCE_MS,
} from './limits';
export {
  EditorSession,
  type EditorSessionOptions,
  type SelectOptions,
  type SessionHooks,
} from './session';
export { allBlocks, clearWorkspace, descendantsOf } from './traverse';
export { captureViewport, restoreViewport } from './viewport';
export { readBlockTrees, readModule, readTopBlocks, withViewport } from './workspaceToBdm';
