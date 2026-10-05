import { type BannerRegistry, banners as appBanners } from '../../app/banners';
import type { Feature } from '../../app/features';
import { type SettingsSectionRegistry, settingsSections } from '../settings/sections';
import { TrustController } from './controller';
import { RestrictedModeBanner } from './RestrictedModeBanner';
import { TrustSection } from './TrustSection';

/** Where the trust feature puts its parts; tests pass their own registries. */
export interface TrustFeatureOptions {
  /** The window banners (the app's by default). */
  banners?: BannerRegistry;
  /** The Settings page's extra sections (the app's by default). */
  sections?: SettingsSectionRegistry;
}

/** The Restricted Mode banner comes first among the window banners. */
export const RESTRICTED_BANNER_ORDER = 0;

/** The *This project* section comes after the Settings page's own sections. */
export const TRUST_SECTION_ORDER = 100;

/**
 * Creates the trust feature (docs/spec/08-security.md §8.3, 04 §4.10): a persistent Restricted
 * Mode banner whenever the open project is restricted, with *Trust…* (`trust_grant`; a cancelled
 * dialog leaves the trust unchanged), and a *This project* section on the Settings page with
 * *Revoke trust* (`trust_revoke`) when the project is trusted by its own record. The banner and
 * the section only show the backend's answers; Build and Run are gated by the shell and checked
 * again by the backend.
 */
export function createTrustFeature(options: TrustFeatureOptions = {}): Feature {
  const banners = options.banners ?? appBanners;
  const sections = options.sections ?? settingsSections;
  return function trustFeature(ctx) {
    const controller = new TrustController(ctx);
    function RestrictedBanner() {
      return <RestrictedModeBanner controller={controller} />;
    }
    function ProjectTrustSection() {
      return <TrustSection controller={controller} />;
    }
    const removers = [
      banners.registerBanner('restrictedMode', RestrictedBanner, {
        order: RESTRICTED_BANNER_ORDER,
      }),
      sections.registerSection('projectTrust', ProjectTrustSection, {
        order: TRUST_SECTION_ORDER,
      }),
    ];
    return () => {
      controller.dispose();
      for (const remove of removers.reverse()) {
        remove();
      }
    };
  };
}

/** The trust feature with the app's registries. */
export const trustFeature: Feature = createTrustFeature();
