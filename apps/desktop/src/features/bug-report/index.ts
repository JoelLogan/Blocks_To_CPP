/** *Copy bug report* (docs/plans/m3-contracts/w4-editor-tools.md §2.7). */
export { BugReportDialog, type BugReportDialogProps } from './BugReportDialog';
export { copyText } from './copy';
export {
  MAX_REPORT_CPP_CHARS,
  MAX_REPORT_MESSAGE_CHARS,
  bugReport,
  sanitisedMessage,
  topBlockOf,
  withoutFileNames,
  withoutPaths,
  withoutQuotedNames,
  type BugReportInput,
  type BugReportOptions,
} from './report';
