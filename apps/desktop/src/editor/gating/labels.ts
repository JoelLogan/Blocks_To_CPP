/**
 * Block labels in the two display modes (docs/spec/03-block-language.md §3.5.2). They live in
 * blockly-ext, which lays blocks out with them (packages/blockly-ext/src/blocks/labels.ts); the
 * gating module gives them to the rest of the app with the mode they read.
 */
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
} from '@blocks2cpp/blockly-ext';
