/**
 * The external-change feature (docs/spec/04-user-interface.md §4.10, 05 §5.10, 08 §8.3): the
 * *Reload* / *Keep mine (save as…)* dialog when the open project's file changes on disk.
 * `externalChangeFeature` goes into `src/features/index.ts`.
 */
export { ExternalChangeController, type ExternalChangeOptions } from './controller';
export {
  externalChangeFeature,
  type InstalledExternalChangeFeature,
  installExternalChangeFeature,
} from './feature';
export {
  describeReloadError,
  type ExternalChangeChoice,
  externalChangeQuestion,
  type ExternalChangeQuestion,
  KEEP_MINE_LABEL,
  LATER_LABEL,
  RELOAD_LABEL,
} from './messages';
