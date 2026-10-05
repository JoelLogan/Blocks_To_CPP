/**
 * The app's features, installed in this order when the window starts (see src/app/features.ts).
 *
 * The project feature comes first: it registers the start page, which the recovery feature adds
 * its offer to and the toolchain feature may replace with the setup page once discovery ends
 * without a usable compiler. The trust feature adds a section to the Settings page, so it follows
 * the settings feature.
 */
import { type Feature, type FeatureContext, installAll } from '../app/features';
import { buildRunFeature } from './build-run';
import { externalChangeFeature } from './external-change';
import { projectFeature, registerStartPageSection } from './project';
import { createRecoveryFeature } from './recovery';
import { settingsFeature } from './settings';
import { toolchainFeature } from './toolchain';
import { trustFeature } from './trust';

/** Every feature, in installation order. */
const FEATURES: Feature[] = [
  projectFeature,
  buildRunFeature,
  toolchainFeature,
  settingsFeature,
  trustFeature,
  createRecoveryFeature({ registerStartPageSection }),
  externalChangeFeature,
];

/** Installs every feature; returns the function that uninstalls them again. */
export function installFeatures(ctx: FeatureContext): () => void {
  return installAll(FEATURES, ctx);
}
