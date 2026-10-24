/**
 * The recovery feature (docs/spec/04-user-interface.md §4.10, 05 §5.10): autosave into recovery
 * snapshots while the project has unsaved changes, and the start page's *Restore* / *Discard*
 * offer after a crash. The app installs `createRecoveryFeature({ registerStartPageSection })` from
 * `src/features/index.ts`.
 */
export {
  AUTOSAVE_INTERVAL_MS,
  type Autosave,
  type AutosaveDeps,
  type BlurSource,
  exceedsDocumentLimit,
  startAutosave,
} from './autosave';
export { RecoveryController, type RecoveryControllerOptions } from './controller';
export {
  createRecoveryFeature,
  type InstalledRecoveryFeature,
  installRecoveryFeature,
  RECOVERY_SECTION_ID,
  RECOVERY_SECTION_ORDER,
  recoveryFeature,
  type RecoveryFeatureOptions,
  type RegisterStartPageSection,
} from './feature';
export {
  createRecoveryModel,
  MAX_OFFERED_SNAPSHOTS,
  offeredSnapshots,
  type OfferStatus,
  type RecoveryModel,
  type RecoveryOfferState,
  type RecoveryOperation,
} from './model';
export { RecoveryOffer, type RecoveryOfferProps } from './RecoveryOffer';
