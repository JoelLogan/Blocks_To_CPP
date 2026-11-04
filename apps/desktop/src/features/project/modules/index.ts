/**
 * Adding, renaming, deleting and moving modules (docs/plans/m3-contracts/w3-modules.md §1.2):
 * the document operations, the name check, the edits with their dialogs and messages, and the
 * feature that publishes them for the module strip.
 */
export {
  getModuleEdits,
  type KeptDeletion,
  ModuleEdits,
  type ModuleEditsDeps,
  type ModuleEditsState,
  type ModuleStatus,
  REFUSED_TITLE,
  setModuleEdits,
  subscribeModuleEdits,
} from './controller';
export { installModuleEdits, moduleEditsFeature } from './feature';
export { B2C_PREFIX_MESSAGE, checkModuleName, type ModuleNameCheck } from './names';
export {
  addModule,
  canAddModule,
  canDeleteModule,
  deleteModule,
  holdsProgramStart,
  type KeptModule,
  MAX_MODULES,
  MODULE_EDIT_PROBLEMS,
  ModuleEditError,
  type ModuleEditProblem,
  moveModule,
  nextModuleName,
  renameModule,
  restoreModule,
  topLevelBlockCount,
} from './operations';
