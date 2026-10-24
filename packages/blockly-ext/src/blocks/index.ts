/** Catalog block registration, placeholders and block state (docs/adr/0002-block-editor-blockly.md). */
export {
  CATALOG_FIELD_OPTIONS,
  catalogBlock,
  catalogFieldOptions,
  isCatalogBlockType,
} from './catalog';
export { B2C_CSS_CLASS, registerB2cCss } from './css';
export {
  blockLayout,
  rowName,
  type BlockLayout,
  type LayoutInput,
  type LayoutItem,
} from './layout';
export {
  MUTATOR_FOR_BLOCK,
  MUTATOR_NAME,
  REPEAT_ANCHOR,
  hasB2cMutator,
  type B2cMutatorMixin,
  type BlockExtra,
  type MutatorName,
} from './mutators';
export {
  PLACEHOLDER_TYPE,
  isPlaceholder,
  missingPackOf,
  placeholderState,
  readPlaceholder,
  registerPlaceholderBlock,
  type PlaceholderData,
  type PlaceholderShape,
} from './placeholder';
export { BlockRegistrationError, blockDefinition, registerB2cBlocks } from './register';
export { ZELOS_OUTPUT_SHAPE, applyShape } from './shape';
export { isManuallyDisabled, setManuallyDisabled } from './state';
