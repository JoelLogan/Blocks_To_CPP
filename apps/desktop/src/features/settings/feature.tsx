import { type BannerRegistry, banners as appBanners } from '../../app/banners';
import type { Feature } from '../../app/features';
import { SettingsController } from './controller';
import { type SettingsSectionRegistry, settingsSections } from './sections';
import { SettingsNoticesBanner, SettingsPage } from './SettingsPage';

/** Where the settings feature puts its parts; tests pass their own registries. */
export interface SettingsFeatureOptions {
  /** The window banners (the app's by default). */
  banners?: BannerRegistry;
  /** The Settings page's extra sections (the app's by default). */
  sections?: SettingsSectionRegistry;
}

/** The order of the settings banner among the window banners (after Restricted Mode). */
export const SETTINGS_BANNER_ORDER = 100;

/**
 * Creates the settings feature (docs/spec/04-user-interface.md §4.12): the `settings` screen
 * with the code style, *Run on errors*, the console scrollback, a link to the toolchain page and
 * *Clear build cache*, plus the sections other features add. It reads the settings with
 * `settings_get` (again each time the page opens) and changes them with partial
 * `settings_update` calls; every change is saved at once and applies live through the store.
 *
 * When `settings.json` had problems at start-up, a window banner says so until the person opens
 * the page or dismisses it; the page lists the notices.
 */
export function createSettingsFeature(options: SettingsFeatureOptions = {}): Feature {
  const banners = options.banners ?? appBanners;
  const sections = options.sections ?? settingsSections;
  return function settingsFeature(ctx) {
    const controller = new SettingsController(ctx);
    function SettingsScreen() {
      return <SettingsPage controller={controller} screens={ctx.screens} sections={sections} />;
    }
    function SettingsBanner() {
      return (
        <SettingsNoticesBanner
          controller={controller}
          onOpen={() => {
            void ctx.commands.runCommand('settings.open');
          }}
        />
      );
    }
    const removers = [
      ctx.screens.registerScreen('settings', SettingsScreen),
      banners.registerBanner('settingsNotices', SettingsBanner, { order: SETTINGS_BANNER_ORDER }),
    ];
    if (ctx.store.getState().settings.value === null) {
      // The bootstrap could not read them: try once more.
      void controller.refresh();
    }
    return () => {
      controller.dispose();
      for (const remove of removers.reverse()) {
        remove();
      }
    };
  };
}

/** The settings feature with the app's registries. */
export const settingsFeature: Feature = createSettingsFeature();
