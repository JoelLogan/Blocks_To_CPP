import { type BannerRegistry, banners as appBanners } from '../../app/banners';
import type { Feature } from '../../app/features';
import { registerLayoutSaver } from '../../app/layout/rememberedLayout';
import { SettingsController } from './controller';
import { LayoutWriter } from './layout';
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
 *
 * It also remembers the editor's layout (05 §5.9 `ui.layout`): a settled change the layout hands on
 * is written at once, the steps of a splitter's series together half a second after the last one
 * (./layout.ts), and what is left at uninstall.
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
    const layoutWriter = new LayoutWriter((layout) => {
      void controller.saveLayout(layout);
    });
    const removers = [
      ctx.screens.registerScreen('settings', SettingsScreen),
      registerLayoutSaver(layoutWriter.change),
      banners.registerBanner('settingsNotices', SettingsBanner, { order: SETTINGS_BANNER_ORDER }),
    ];
    if (ctx.store.getState().settings.value === null) {
      // The bootstrap could not read them: try once more.
      void controller.refresh();
    }
    return () => {
      for (const remove of removers.reverse()) {
        remove();
      }
      layoutWriter.flush();
      controller.dispose();
    };
  };
}

/** The settings feature with the app's registries. */
export const settingsFeature: Feature = createSettingsFeature();
