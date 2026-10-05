/**
 * The project feature (docs/spec/04-user-interface.md §4.10): the start page, the commands
 * `project.new`, `project.open`, `project.save` (`Ctrl+S`), `project.saveAs` and
 * `project.close`, the unsaved-changes prompts (including the window's `closeRequested`), and the
 * dirty flag reported to the backend.
 */
import type { Feature, FeatureContext } from '../../app/features';
import { reportDirtyState } from './dirty';
import { type LifecycleOptions, ProjectLifecycle } from './lifecycle';
import { createProjectModel, type ProjectModel } from './model';
import { startPageSections, type StartPageSections } from './sections';
import { StartPage } from './StartPage';

/** Options for {@link installProjectFeature} (tests replace these). */
export interface ProjectFeatureOptions extends LifecycleOptions {
  /** Where other features add start page sections; the app's registry by default. */
  readonly sections?: StartPageSections;
}

/** An installed project feature: its parts (for tests) and the function that uninstalls it. */
export interface InstalledProjectFeature {
  readonly lifecycle: ProjectLifecycle;
  /** The start page's state: recent projects, the last load failure, the running operation. */
  readonly model: ProjectModel;
  /** Removes the screen, the commands and the event handlers, and stops the lifecycle. */
  readonly uninstall: () => void;
}

/** Installs the project feature into `ctx`; see the module comment. */
export function installProjectFeature(
  ctx: FeatureContext,
  options: ProjectFeatureOptions = {},
): InstalledProjectFeature {
  const model = createProjectModel();
  const lifecycle = new ProjectLifecycle(ctx, model, options);
  const sections = options.sections ?? startPageSections;

  function ProjectStartPage() {
    return <StartPage lifecycle={lifecycle} model={model} store={ctx.store} sections={sections} />;
  }

  /** A command's handler: the lifecycle reports its own failures; the result is not needed. */
  const command =
    (operation: () => Promise<boolean>): (() => Promise<void>) =>
    async () => {
      await operation();
    };

  const removers = [
    ctx.screens.registerScreen('start', ProjectStartPage),
    ctx.commands.registerCommand(
      'project.new',
      command(() => lifecycle.chooseAndCreate()),
    ),
    ctx.commands.registerCommand(
      'project.open',
      command(() => lifecycle.open()),
    ),
    ctx.commands.registerCommand(
      'project.save',
      command(() => lifecycle.save()),
    ),
    ctx.commands.registerCommand(
      'project.saveAs',
      command(() => lifecycle.saveAs()),
    ),
    ctx.commands.registerCommand(
      'project.close',
      command(() => lifecycle.close()),
    ),
    ctx.events.on('closeRequested', () => {
      lifecycle.quit().catch((error: unknown) => {
        console.error('Closing the window failed', error);
      });
    }),
    reportDirtyState(ctx.store, ctx.ipc),
  ];

  let installed = true;
  return {
    lifecycle,
    model,
    uninstall: () => {
      if (!installed) {
        return;
      }
      installed = false;
      lifecycle.dispose();
      for (const remove of removers.reverse()) {
        remove();
      }
    },
  };
}

/** The project feature, for `src/features/index.ts`. */
export const projectFeature: Feature = function projectFeature(ctx) {
  return installProjectFeature(ctx).uninstall;
};
