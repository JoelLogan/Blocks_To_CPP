/**
 * The external-change feature (docs/spec/04-user-interface.md §4.10, 05 §5.10): when the open
 * project's file changes on disk, the backend's `projectChangedOnDisk` event, or a save the backend
 * refused with `changedOnDisk` (the project feature's `project:changedOnDisk` event), shows the
 * *Reload* / *Keep mine (save as…)* dialog (see `ExternalChangeController`).
 */
import type { Feature, FeatureContext } from '../../app/features';
import { ExternalChangeController, type ExternalChangeOptions } from './controller';

/** An installed external-change feature: its controller (for tests) and its uninstaller. */
export interface InstalledExternalChangeFeature {
  readonly controller: ExternalChangeController;
  /** Stops listening; a question being shown ends after its answer. */
  readonly uninstall: () => void;
}

/** Installs the external-change feature into `ctx`. */
export function installExternalChangeFeature(
  ctx: FeatureContext,
  options: ExternalChangeOptions = {},
): InstalledExternalChangeFeature {
  const controller = new ExternalChangeController(ctx, options);
  const removers = [
    ctx.events.on('projectChangedOnDisk', (event) => {
      void controller.notify(event.handle, event.deleted);
    }),
    // A save refused because the file changed: whether it is gone is not known.
    ctx.events.on('project:changedOnDisk', (event) => {
      void controller.notify(event.handle, null);
    }),
  ];

  let installed = true;
  return {
    controller,
    uninstall: () => {
      if (!installed) {
        return;
      }
      installed = false;
      for (const remove of removers) {
        remove();
      }
      controller.dispose();
    },
  };
}

/**
 * An external-change feature with `options` (the app passes the project feature's link as
 * `project`, so a reload runs in the project lifecycle's queue).
 */
export function createExternalChangeFeature(options: ExternalChangeOptions = {}): Feature {
  return function externalChangeFeature(ctx) {
    return installExternalChangeFeature(ctx, options).uninstall;
  };
}

/** The external-change feature without the project lifecycle's queue. */
export const externalChangeFeature: Feature = createExternalChangeFeature();
