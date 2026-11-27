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
  SHARED_FIELD,
  SHARED_SHOWN_ON,
  type BlockLayout,
  type LayoutInput,
  type LayoutItem,
  type LayoutOptions,
} from './layout';
export {
  blockLabelParts,
  CPP_OPTION_LABELS,
  hasCppLayout,
  hasFullCppLabel,
  labelParts,
  labelTemplate,
  optionLabel,
  partsMissingFromCppLabel,
  type LabelMode,
  type LabelPartsOptions,
} from './labels';
export { blockLabelMode, setBlockLabelMode } from './labelMode';
export { B2cGearField, GEAR_TEXT, gearPopover, popoverFieldDefs } from './gear';
export {
  applyMenuMarks,
  isSetAside,
  MARK_CLASS,
  toggleUncheckedIndex,
  UNCHECKED_FIELD,
  UNCHECKED_MARK,
  type UncheckedIndex,
  uncheckedIndex,
  uncheckedIndexDef,
} from './marks';
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
  MAX_NAMESPACE_DEPTH,
  NAMESPACE_TYPE,
  isInMouth,
  isMouth,
  isNamespaceMember,
  namespaceDepthAt,
  namespaceHeight,
  registerB2cRenderer,
} from './namespace';
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
