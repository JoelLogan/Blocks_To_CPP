/**
 * The block editor's toolbox (docs/spec/04-user-interface.md §4.2, 03 §3.6–3.7): the catalog's
 * categories with presets and editable defaults, the dynamic Program category (*when program
 * starts* only while the project has none), Variables (the standard set with *Make a variable*, and
 * a getter for each variable of the module) and Functions (with *Your functions*), in the
 * continuous toolbox or Blockly's category toolbox. A block dragged out of it is settled where it
 * lands: a free name, or a variable visible there.
 *
 * - `toolboxPlugin` is the editor plugin (append it to `EDITOR_PLUGINS`).
 * - `toolboxInjectOptions()` gives the starting toolbox and the continuous toolbox's classes for
 *   `Blockly.inject`; without them the plugin uses Blockly's category toolbox (one category at a
 *   time).
 */
export {
  B2C_CATEGORY_KIND,
  MAKE_VARIABLE_BUTTON,
  MAX_LISTED_FUNCTIONS,
  MAX_LISTED_GETTERS,
  MY_BLOCKS_CATEGORY,
  PROGRAM_CATEGORY,
  VARIABLES_CATEGORY,
  functionsContents,
  initialToolboxDefinition,
  programContents,
  staticCategoryContents,
  toolboxDefinition,
  variablesContents,
  type ContentsContext,
  type ModuleInfo,
} from './contents';
export { B2cToolboxCategory } from './category';
export {
  B2cContinuousFlyout,
  B2cContinuousToolbox,
  CONTINUOUS_FLYOUT,
  CONTINUOUS_METRICS,
  CONTINUOUS_TOOLBOX,
} from './continuous';
export { variableNameProblem } from './identifiers';
export { B2cBlockInflater } from './inflater';
export { settleDroppedBlock } from './landing';
export { makeVariable, type MakeVariableContext, type MakeVariableResult } from './makeVariable';
export { candidateName, firstFreeName, type DefaultNameKind } from './names';
export {
  analysisSymbolSource,
  createToolboxPlugin,
  toolboxPlugin,
  type ToolboxPluginOptions,
} from './plugin';
export {
  B2C_BLOCK_KIND,
  PresetError,
  blockState,
  instanceFields,
  presetBlock,
  type B2cBlockInfo,
  type BlockPreset,
} from './presets';
export {
  registerToolboxComponents,
  toolboxInjectOptions,
  type ToolboxInjectOptions,
} from './register';
export {
  START_VALUE_EVENT,
  StartValueChange,
  installStartValueReshaping,
  reshapeSetValue,
  reshapeStartValue,
} from './reshape';
export { SelectionTracker, canvasFocus, selectedBlock, type CanvasFocus } from './selection';
export type { SymbolSource } from './scope';
