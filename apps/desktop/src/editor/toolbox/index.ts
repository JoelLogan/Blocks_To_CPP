/**
 * The block editor's toolbox (docs/spec/04-user-interface.md §4.2, 03 §3.6–3.7): the catalog's
 * categories with presets and editable defaults, the dynamic Variables category (with *Make a
 * variable*), Loops (a free counter name) and Functions (with *My Blocks*), in the continuous
 * toolbox or Blockly's category toolbox.
 *
 * - `toolboxPlugin` is the editor plugin (append it to `EDITOR_PLUGINS`).
 * - `toolboxInjectOptions()` gives the starting toolbox and the continuous toolbox's classes for
 *   `Blockly.inject`; without them the plugin uses Blockly's category toolbox (one category at a
 *   time).
 */
export {
  B2C_CATEGORY_KIND,
  LOOPS_CATEGORY,
  MAKE_VARIABLE_BUTTON,
  MAX_LISTED_FUNCTIONS,
  MAX_LISTED_VARIABLES,
  MY_BLOCKS_CATEGORY,
  VARIABLES_CATEGORY,
  functionsContents,
  initialToolboxDefinition,
  loopsContents,
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
  reshapeStartValue,
} from './reshape';
export { SelectionTracker, canvasFocus, selectedBlock, type CanvasFocus } from './selection';
export type { SymbolSource } from './scope';
