/**
 * The settings feature: the Settings page for the machine settings
 * (docs/spec/04-user-interface.md §4.12). Install `settingsFeature` with the other features
 * (src/features/index.ts); other features add sections with `registerSettingsSection`.
 */
export { createSettingsFeature, SETTINGS_BANNER_ORDER, settingsFeature } from './feature';
export type { SettingsFeatureOptions } from './feature';
export {
  CLEAR_CACHE_CONFIRMATION,
  type SettingsAction,
  SettingsController,
  settingsFailureText,
  type SettingsPageState,
} from './controller';
export {
  createSettingsSectionRegistry,
  registerSettingsSection,
  type SettingsSection,
  type SettingsSectionRegistry,
  settingsSections,
  useSettingsSections,
} from './sections';
export { SettingsNoticesBanner, SettingsPage } from './SettingsPage';
export {
  cacheClearedText,
  formatBytes,
  noticeText,
  parseScrollback,
  SCROLLBACK_LIMITS,
  settingLabel,
} from './texts';
