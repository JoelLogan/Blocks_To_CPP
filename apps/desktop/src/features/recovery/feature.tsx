/**
 * The recovery feature (docs/spec/04-user-interface.md §4.10, 05 §5.10): autosave of the open
 * project's unsaved changes into recovery snapshots, and the start page's offer to restore or
 * discard the snapshots of earlier sessions.
 *
 * The offer is a start page section. The start page belongs to the project feature, which provides
 * `registerStartPageSection`; this feature takes that function as an option
 * ({@link RecoveryFeatureOptions.registerStartPageSection}), so it does not depend on the project
 * feature's modules:
 *
 * ```ts
 * import { projectFeature, registerStartPageSection } from './project';
 * import { createRecoveryFeature } from './recovery';
 *
 * const FEATURES = [projectFeature, createRecoveryFeature({ registerStartPageSection })];
 * ```
 */
import type { ComponentType } from 'react';

import type { Feature, FeatureContext } from '../../app/features';
import { type Autosave, type BlurSource, startAutosave } from './autosave';
import { RecoveryController, type RecoveryControllerOptions } from './controller';
import { createRecoveryModel, type RecoveryModel } from './model';
import { RecoveryOffer } from './RecoveryOffer';

/** The start page section's ID. */
export const RECOVERY_SECTION_ID = 'recovery';

/** Where the offer goes among the start page's sections: first. */
export const RECOVERY_SECTION_ORDER = 0;

/**
 * Adds a section to the start page and returns the function that removes it again: the project
 * feature's `registerStartPageSection` (src/features/project/sections.ts).
 */
export type RegisterStartPageSection = (
  id: string,
  component: ComponentType,
  options?: { readonly order?: number },
) => () => void;

/** Options for {@link installRecoveryFeature} (the app passes the start page; tests the rest). */
export interface RecoveryFeatureOptions extends RecoveryControllerOptions {
  /**
   * Adds the offer to the start page. Without it the offer is not shown (autosave still runs),
   * and an error is logged, because the snapshots of a crash would then never be offered.
   */
  readonly registerStartPageSection?: RegisterStartPageSection | null;
  /** The window whose `blur` writes a snapshot; the app's `window` by default. */
  readonly window?: BlurSource | null;
  /** The time between snapshots, in milliseconds (30 s by default). */
  readonly intervalMs?: number;
}

/** An installed recovery feature: its parts (for tests) and the function that uninstalls it. */
export interface InstalledRecoveryFeature {
  /** The offer's state: the snapshots and the running operation. */
  readonly model: RecoveryModel;
  readonly controller: RecoveryController;
  readonly autosave: Autosave;
  /** The start page section, bound to this installation. */
  readonly Offer: ComponentType;
  /** Stops autosave, removes the section and stops the controller. */
  readonly uninstall: () => void;
}

/** The app's window, where there is one (not in tests that run without a DOM). */
function defaultWindow(): BlurSource | null {
  return typeof window === 'undefined' ? null : window;
}

/**
 * Installs the recovery feature into `ctx`: starts autosave, reads the snapshots offered for
 * restore (`recovery_list`, once, at start-up) and adds the offer to the start page.
 */
export function installRecoveryFeature(
  ctx: FeatureContext,
  options: RecoveryFeatureOptions = {},
): InstalledRecoveryFeature {
  const model = createRecoveryModel();
  const controller = new RecoveryController(ctx, model, options);
  const autosave = startAutosave({
    store: ctx.store,
    ipc: ctx.ipc,
    window: options.window === undefined ? defaultWindow() : options.window,
    ...(options.intervalMs === undefined ? {} : { intervalMs: options.intervalMs }),
  });

  function RecoveryStartPageSection() {
    return <RecoveryOffer model={model} controller={controller} />;
  }

  const register = options.registerStartPageSection ?? null;
  let removeSection: () => void = () => undefined;
  if (register === null) {
    console.error('The recovery feature has no start page section; unsaved work is not offered');
  } else {
    removeSection = register(RECOVERY_SECTION_ID, RecoveryStartPageSection, {
      order: RECOVERY_SECTION_ORDER,
    });
  }
  void controller.refresh();

  let installed = true;
  return {
    model,
    controller,
    autosave,
    Offer: RecoveryStartPageSection,
    uninstall: () => {
      if (!installed) {
        return;
      }
      installed = false;
      controller.dispose();
      autosave.stop();
      removeSection();
    },
  };
}

/** A recovery feature with `options` (the app passes `registerStartPageSection`). */
export function createRecoveryFeature(options: RecoveryFeatureOptions = {}): Feature {
  return function recoveryFeature(ctx) {
    return installRecoveryFeature(ctx, options).uninstall;
  };
}

/**
 * The recovery feature without a start page: autosave only. The app installs
 * `createRecoveryFeature({ registerStartPageSection })` instead (see the module comment).
 */
export const recoveryFeature: Feature = createRecoveryFeature();
