/** The type picker (docs/plans/m3-contracts/w4-editor-tools.md §2.2). */
export {
  MAX_PICKER_DEPTH,
  PICKER_GROUP_TITLES,
  acceptsArgument,
  acceptsTerm,
  addArgument,
  argumentEntries,
  collectionLabel,
  pathLabel,
  rootEntries,
  termLabel,
  type OpenCollection,
  type PickerContext,
  type PickerEntry,
  type PickerGroup,
  type PickerUserType,
} from './model';
export { MAX_TYPED_CHARS, parseTypeText } from './parse';
export {
  TYPE_PICKER_CLASS,
  createTypePicker,
  type TypePicker,
  type TypePickerOptions,
} from './picker';
