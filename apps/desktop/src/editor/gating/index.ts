/**
 * Expert mode and feature gating (docs/spec/03-block-language.md §3.5.2, 06 §6.10): what the
 * editor offers for its mode, the project's standard and the toolchain, and how blocks read.
 */
export {
  blockGate,
  type Capability,
  categoryGate,
  type EditorMode,
  gate,
  type Gate,
  gateText,
  type LabelMode,
  optionGate,
  type Requirement,
  standardAtLeast,
  typeGate,
} from './gate';
export {
  blockLabelParts,
  CPP_OPTION_LABELS,
  hasFullCppLabel,
  labelParts,
  labelTemplate,
  optionLabel,
  partsMissingFromCppLabel,
} from './labels';
export { installOptionGate } from './install';
export { editorMode, modeKey } from './mode';
export { AWAITED, REQUIREMENTS, requirementKey, type RequirementKind } from './table';
