/**
 * The app's features, installed in this order when the window starts (see src/app/features.ts).
 * Milestone M2's wave 4 appends the project, build and run, toolchain, settings, trust, recovery
 * and external-change features here.
 */
import { type Feature, type FeatureContext, installAll } from '../app/features';

/** Every feature, in installation order. */
const FEATURES: Feature[] = [];

/** Installs every feature; returns the function that uninstalls them again. */
export function installFeatures(ctx: FeatureContext): () => void {
  return installAll(FEATURES, ctx);
}
