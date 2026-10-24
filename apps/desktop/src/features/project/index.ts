/**
 * The project feature (docs/spec/04-user-interface.md §4.10): start page, new, open, recent
 * projects, save, save as, close and the unsaved-changes prompts. `projectFeature` goes into
 * `src/features/index.ts`; other features add start page sections with
 * `registerStartPageSection`, and run their operations on the open project in the lifecycle's
 * queue through a `ProjectLink` (`createProjectFeature({ link })`).
 */
export {
  createProjectFeature,
  installProjectFeature,
  type InstalledProjectFeature,
  projectFeature,
  type ProjectFeatureOptions,
} from './feature';
export { ProjectLink, type ProjectQueue } from './link';
export {
  type LifecycleOptions,
  ProjectLifecycle,
  TEMPLATE_LABELS,
  type UnsavedChoice,
} from './lifecycle';
export {
  createStartPageSections,
  MAX_START_PAGE_SECTIONS,
  registerStartPageSection,
  type StartPageSection,
  type StartPageSectionOptions,
  type StartPageSections,
  startPageSections,
  useStartPageSections,
} from './sections';
