import { type BannerRegistry, banners as appBanners } from '../../app/banners';
import type { Feature } from '../../app/features';
import { AnalysisNoticeBanner } from './AnalysisNoticeBanner';

/** Where the feature puts its banner; tests pass their own registry. */
export interface AnalysisNoticeFeatureOptions {
  /** The window banners (the app's by default). */
  banners?: BannerRegistry;
}

/**
 * The banner's place among the window banners: after Restricted Mode (0), before the settings
 * notices (100).
 */
export const ANALYSIS_BANNER_ORDER = 50;

/**
 * Creates the analysis notice feature: a window banner while the live analysis has a notice
 * (`syncFailed` or `trapRecovered`, see {@link AnalysisNoticeBanner}).
 */
export function createAnalysisNoticeFeature(options: AnalysisNoticeFeatureOptions = {}): Feature {
  const banners = options.banners ?? appBanners;
  return function analysisNoticeFeature(ctx) {
    function Banner() {
      return <AnalysisNoticeBanner store={ctx.store} />;
    }
    return banners.registerBanner('analysisNotice', Banner, { order: ANALYSIS_BANNER_ORDER });
  };
}

/** The analysis notice feature with the app's banners. */
export const analysisNoticeFeature: Feature = createAnalysisNoticeFeature();
