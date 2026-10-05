/**
 * The trust feature: the Restricted Mode banner with *Trust…*, and *Revoke trust* on the Settings
 * page (docs/spec/08-security.md §8.3, 04 §4.10). Install `trustFeature` with the other features
 * (src/features/index.ts), after `settingsFeature`.
 */
export {
  createTrustFeature,
  RESTRICTED_BANNER_ORDER,
  TRUST_SECTION_ORDER,
  trustFeature,
  type TrustFeatureOptions,
} from './feature';
export {
  type TrustAction,
  TrustController,
  type TrustMessage,
  type TrustState,
} from './controller';
export { RestrictedModeBanner, RUN_BUTTON_SELECTOR } from './RestrictedModeBanner';
export { TrustSection } from './TrustSection';
export {
  MARK_OF_THE_WEB_TEXT,
  RESTRICTED_MODE_TEXT,
  restrictedReasonText,
  trustedText,
  trustMessageText,
} from './texts';
