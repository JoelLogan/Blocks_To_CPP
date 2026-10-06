/**
 * The app's features, installed in this order when the window starts (see src/app/features.ts).
 *
 * The project feature comes first: it registers the start page, which the recovery feature adds
 * its offer to and the toolchain feature may replace with the setup page once discovery ends
 * without a usable compiler. The trust feature adds a section to the Settings page, so it follows
 * the settings feature. Restoring a snapshot, reloading the file and autosave act on the open
 * project, so the recovery and external-change features use the project lifecycle's queue through
 * `projectLink`, which the project feature binds while it is installed.
 */
import { type Feature, type FeatureContext, installAll } from '../app/features';
import { analysisNoticeFeature } from './analysis';
import { buildRunFeature } from './build-run';
import { createExternalChangeFeature } from './external-change';
import { createProjectFeature, ProjectLink, registerStartPageSection } from './project';
import { createRecoveryFeature } from './recovery';
import { settingsFeature } from './settings';
import { toolchainFeature } from './toolchain';
import { trustFeature } from './trust';

/** The project lifecycle's queue, for the features that act on the open project. */
const projectLink = new ProjectLink();

/** Every feature, in installation order. */
const FEATURES: Feature[] = [
  createProjectFeature({ link: projectLink }),
  buildRunFeature,
  analysisNoticeFeature,
  toolchainFeature,
  settingsFeature,
  trustFeature,
  createRecoveryFeature({ registerStartPageSection, project: projectLink }),
  createExternalChangeFeature({ project: projectLink }),
];

/** Installs every feature; returns the function that uninstalls them again. */
export function installFeatures(ctx: FeatureContext): () => void {
  return installAll(FEATURES, ctx);
}
