/**
 * The build and run feature (docs/spec/04-user-interface.md §4.1, §4.4, §4.5; 02 §2.4.2, §2.4.3).
 * It registers these commands:
 *
 * | Command | Key | What it does |
 * | --- | --- | --- |
 * | `build.start` | Ctrl+B | Builds the project as it is now and shows the Build output tab. |
 * | `run.start` | F5 | Stops a running program, builds when needed, then runs it in the console. |
 * | `run.again` | | The same as `run.start` (⟲ Run again in the console). |
 * | `run.stop` | Shift+F5 | Cancels the running build and stops the running program. |
 * | `problems.focusFirstError` | | Shows the first error in Problems and on its block. |
 *
 * Each command checks the run gate again (./gate.ts). The console panel connects through the
 * {@link ConsoleBridge}, and the feature follows the open project: when it changes, the build and
 * the run of the old one are no longer followed and the console is cleared.
 */
import type { Feature, FeatureContext } from '../../app/features';
import { BuildController, type BuildFinish } from './buildController';
import { type Clock, systemClock } from './clock';
import { type ConsoleBridge, consoleBridge } from './consoleBridge';
import { focusFirstError, gateAllows } from './gate';
import { failureMessage } from './messages';
import { RunController } from './runController';

/** What {@link createBuildRunFeature} can be given; tests pass their own. */
export interface BuildRunFeatureOptions {
  /** The console seam; the app's {@link consoleBridge} by default. */
  readonly bridge?: ConsoleBridge;
  /** The clock; the system clock by default. */
  readonly clock?: Clock;
}

/** Runs a command whose failure only goes to the log (it reports its own errors to the user). */
function trigger(ctx: FeatureContext, id: 'problems.focusFirstError'): void {
  ctx.commands.runCommand(id).catch((error: unknown) => {
    console.error(`Command ${id} failed`, error);
  });
}

/** Whether the open project has an error to show (live, or from the last build). */
function hasErrors(ctx: FeatureContext): boolean {
  const state = ctx.store.getState();
  return (
    state.build.diagnostics.some((diagnostic) => diagnostic.severity === 'error') ||
    (state.analysis.preview?.diagnostics ?? []).some(
      (diagnostic) => diagnostic.severity === 'error',
    )
  );
}

/** Creates the feature; see the module documentation. */
export function createBuildRunFeature(options: BuildRunFeatureOptions = {}): Feature {
  const bridge = options.bridge ?? consoleBridge;
  const clock = options.clock ?? systemClock;

  return function buildRunFeature(ctx: FeatureContext): () => void {
    const builds = new BuildController({
      ipc: ctx.ipc,
      store: ctx.store,
      editor: ctx.editor,
      core: ctx.core,
      dialogs: ctx.dialogs,
    });
    const runs = new RunController({
      ipc: ctx.ipc,
      store: ctx.store,
      bridge,
      clock,
      builds,
      dialogs: ctx.dialogs,
      showFirstError: () => {
        trigger(ctx, 'problems.focusFirstError');
      },
    });

    /** After a build the person asked for: say why it did not start, or show the first error. */
    const afterBuild = async (finish: BuildFinish): Promise<void> => {
      if (finish.kind === 'notStarted') {
        if (finish.code === 'projectErrors') {
          trigger(ctx, 'problems.focusFirstError');
        } else if (finish.code !== 'unknownHandle') {
          await ctx.dialogs.alert(failureMessage(finish.code, 'build'));
        }
        return;
      }
      const failed =
        finish.outcome === 'failed' ||
        finish.outcome === 'projectErrors' ||
        finish.outcome === 'toolchainProblem';
      if (failed && !finish.superseded && hasErrors(ctx)) {
        trigger(ctx, 'problems.focusFirstError');
      }
    };

    const build = (): void => {
      if (!gateAllows('build', ctx)) {
        return;
      }
      const prepared = builds.prepare();
      if (prepared === null) {
        return;
      }
      ctx.store
        .getState()
        .actions.setUi({ screen: 'editor', bottomTab: 'buildOutput', bottomCollapsed: false });
      const active = builds.start(prepared);
      // The command ends here, so pressing Build again after an edit starts a newer build.
      void active?.finished.then(afterBuild).catch((error: unknown) => {
        console.error('Reporting the build failed', error);
      });
    };

    const run = async (): Promise<void> => {
      if (gateAllows('run', ctx)) {
        await runs.run();
      }
    };

    const removers = [
      ctx.commands.registerCommand('build.start', build),
      ctx.commands.registerCommand('run.start', run),
      ctx.commands.registerCommand('run.again', run),
      ctx.commands.registerCommand('run.stop', () => {
        runs.stop();
      }),
      ctx.commands.registerCommand('problems.focusFirstError', () => {
        focusFirstError(ctx);
      }),
      bridge.connect({
        onInput: (data) => {
          runs.input(data);
        },
        onResize: (size) => {
          runs.resize(size);
        },
      }),
      ctx.store.subscribe((state, previous) => {
        if (state.project?.handle !== previous.project?.handle) {
          builds.reset();
          runs.reset();
        }
      }),
    ];

    return () => {
      for (const remove of removers.reverse()) {
        remove();
      }
      runs.dispose();
      builds.reset();
    };
  };
}

/** The build and run feature, installed with the app's other features (src/features/index.ts). */
export const buildRunFeature: Feature = createBuildRunFeature();
