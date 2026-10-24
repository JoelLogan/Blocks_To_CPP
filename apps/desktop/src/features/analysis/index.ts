/**
 * The analysis notice feature: a window banner while the live analysis does not show the canvas as
 * it is now (`syncFailed`) or the compiler core was restarted (`trapRecovered`). Install
 * `analysisNoticeFeature` with the other features (src/features/index.ts).
 */
export { ANALYSIS_NOTICE_TEXT, AnalysisNoticeBanner } from './AnalysisNoticeBanner';
export {
  ANALYSIS_BANNER_ORDER,
  analysisNoticeFeature,
  type AnalysisNoticeFeatureOptions,
  createAnalysisNoticeFeature,
} from './feature';
