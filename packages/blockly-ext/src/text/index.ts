/** Text rules and safe display of user text (docs/spec/08-security.md §8.4, §8.8). */
export {
  MAX_FIELD_TEXT_BYTES,
  codePointCount,
  isBidiControl,
  isCleanFieldText,
  sanitizeFieldText,
  type FieldTextCheck,
  type FieldTextProblem,
} from './sanitize';
export {
  isInvisible,
  placeholderFor,
  truncateForDisplay,
  visibleInvisibles,
  type VisibleInvisiblesOptions,
} from './invisibles';
