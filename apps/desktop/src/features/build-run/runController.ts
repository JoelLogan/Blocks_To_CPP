/**
 * ► Run and ⟲ Run again (docs/spec/04-user-interface.md §4.5, 02 §2.4.2, 07 §7.6.1):
 *
 * 1. A program that is still running is stopped first, and its *Stopped* shows before anything
 *    else happens.
 * 2. Unless the last successful build built exactly the project as it is now (same content hash,
 *    configuration and compiler), the project is built, and Run waits for that build. A build
 *    replaced by a newer one is followed to the newer one.
 * 3. `run_start` with the build and the console's size. If the backend says the build no longer
 *    matches (`staleBuild`), the project is built again once and the run tried again.
 *
 * ■ Stop ends whichever step is under way: it cancels the build, or stops the program.
 */
import type {
  BuildId,
  Containment,
  Handle,
  IpcClient,
  RunEvent,
  RunId,
  RunStartResponse,
} from '@blocks2cpp/ipc-types';

import type { DialogService } from '../../app/dialogs';
import type { BottomTab, useAppStore } from '../../app/store';
import type { TerminalSize } from '../../panels';
import type { BuildController, BuildFinish } from './buildController';
import type { Clock } from './clock';
import type { ConsoleBridge } from './consoleBridge';
import { failureCode, type FailureCode, failureMessage } from './messages';
import { RunSession } from './runSession';

/** How long Run waits for a stopped program's exit before it starts the new one anyway. */
export const STOP_WAIT_MS = 5000;

/** How often Run follows a build that a newer build replaced. */
const MAX_BUILD_ATTEMPTS = 3;

/** The terminal sizes `run_start` accepts (02 §2.5.6). */
const COLS = { min: 2, max: 1000 } as const;
const ROWS = { min: 1, max: 1000 } as const;

/** A run ID as the backend makes them (02 §2.5.4). */
const RUN_ID = /^rn_[0-9a-f]{32}$/;

/**
 * What the console shows between two runs: the terminal's modes are reset (a program may have
 * left the alternate screen, hidden the cursor or changed colours), then a dim separator line.
 */
export const RUN_SEPARATOR = '\u001b[?1049l\u001b[!p\r\n\u001b[2m── New run ──\u001b[22m\r\n';

/** The backend errors after which Run builds again and tries once more. */
const REBUILD_CODES: ReadonlySet<FailureCode> = new Set<FailureCode>([
  'staleBuild',
  'unknownBuild',
  'buildNotSuccessful',
]);

/** What the run controller needs. */
export interface RunControllerDeps {
  readonly ipc: IpcClient;
  readonly store: typeof useAppStore;
  readonly bridge: ConsoleBridge;
  readonly clock: Clock;
  readonly builds: BuildController;
  readonly dialogs: DialogService;
  /** Shows the first error (the `problems.focusFirstError` command). */
  readonly showFirstError: () => void;
}

/** `size` within the bounds `run_start` accepts. */
export function clampSize(size: TerminalSize): TerminalSize {
  const clamp = (value: number, range: { min: number; max: number }) =>
    Number.isFinite(value)
      ? Math.min(Math.max(Math.floor(value), range.min), range.max)
      : range.min;
  return { cols: clamp(size.cols, COLS), rows: clamp(size.rows, ROWS) };
}

/** See the module documentation. */
export class RunController {
  readonly #deps: RunControllerDeps;
  #session: RunSession | null = null;
  #sessionHandle: Handle | null = null;
  /** Bumped by every Run, Stop and project change: a step whose number is old stops quietly. */
  #generation = 0;

  constructor(deps: RunControllerDeps) {
    this.#deps = deps;
  }

  /** The current (or last) run. */
  get session(): RunSession | null {
    return this.#session;
  }

  /** Runs the program, building first when needed (see the module documentation). */
  async run(): Promise<void> {
    const generation = ++this.#generation;
    const handle = this.#deps.store.getState().project?.handle;
    if (handle === undefined) {
      return;
    }
    const previous = this.#session;
    if (previous !== null && !previous.hasEnded) {
      await this.#stopAndWait(previous);
      if (generation !== this.#generation) {
        return;
      }
    }
    const buildId = await this.#ensureBuilt(generation);
    if (buildId === null || generation !== this.#generation) {
      return;
    }
    await this.#start(buildId, handle, generation, true);
  }

  /** ■ Stop: cancels the running build and stops the running program. */
  stop(): void {
    this.#generation += 1;
    this.#deps.builds.cancel();
    this.#session?.stop();
  }

  /** What the person typed into the console goes to the running program. */
  input(data: string): void {
    this.#session?.typed(data);
  }

  /** The console's new size goes to the running program. */
  resize(size: TerminalSize): void {
    this.#session?.resized(clampSize(size));
  }

  /**
   * The open project changed: the run (which the backend stops when the project closes) is no
   * longer followed, and the console is cleared for the new project.
   */
  reset(): void {
    this.#generation += 1;
    this.#session?.detach();
    this.#session = null;
    this.#sessionHandle = null;
    this.#deps.bridge.clear();
    this.#deps.bridge.setMode('pty');
  }

  /** Stops following everything (the feature is uninstalled). */
  dispose(): void {
    this.#generation += 1;
    this.#session?.detach();
    this.#session = null;
  }

  #showTab(tab: BottomTab): void {
    this.#deps.store
      .getState()
      .actions.setUi({ screen: 'editor', bottomTab: tab, bottomCollapsed: false });
  }

  async #stopAndWait(session: RunSession): Promise<void> {
    session.stop();
    let timer: unknown = null;
    const timeout = new Promise<void>((resolve) => {
      timer = this.#deps.clock.setTimeout(resolve, STOP_WAIT_MS);
    });
    await Promise.race([session.ended, timeout]);
    this.#deps.clock.clearTimeout(timer);
    if (!session.hasEnded) {
      console.warn('The running program did not report its end in time; starting the new run');
      session.detach();
    }
  }

  /** The ID of a successful build of the project as it is now, building it when needed. */
  async #ensureBuilt(generation: number): Promise<BuildId | null> {
    const { builds } = this.#deps;
    for (let attempt = 0; attempt < MAX_BUILD_ATTEMPTS; attempt++) {
      const prepared = builds.prepare();
      if (prepared === null) {
        return null;
      }
      const ready = builds.successFor(prepared.key);
      if (ready !== null) {
        return ready;
      }
      this.#showTab('buildOutput');
      const active = builds.start(prepared);
      if (active === null) {
        return null;
      }
      const finish = await active.finished;
      if (generation !== this.#generation) {
        return null;
      }
      const result = await this.#buildResult(finish);
      if (result !== 'retry') {
        return result;
      }
    }
    return null;
  }

  /** What a finished build means for Run: its ID, `retry` (it was replaced), or `null`. */
  async #buildResult(finish: BuildFinish): Promise<BuildId | 'retry' | null> {
    if (finish.kind === 'notStarted') {
      await this.#report(finish.code, 'build');
      return null;
    }
    switch (finish.outcome) {
      case 'built':
      case 'upToDate':
        return finish.buildId;
      case 'cancelled':
        return finish.superseded ? 'retry' : null;
      default:
        this.#buildFailed();
        return null;
    }
  }

  /** After a failed build: the first error when there is one, else the Build output. */
  #buildFailed(): void {
    const state = this.#deps.store.getState();
    const hasError =
      state.build.diagnostics.some((diagnostic) => diagnostic.severity === 'error') ||
      (state.analysis.preview?.diagnostics ?? []).some(
        (diagnostic) => diagnostic.severity === 'error',
      );
    if (hasError) {
      this.#deps.showFirstError();
    } else {
      this.#showTab('buildOutput');
    }
  }

  /** Says why a build (in the run flow) or the program could not start. */
  async #report(code: FailureCode, action: 'build' | 'run'): Promise<void> {
    if (code === 'projectErrors') {
      this.#deps.showFirstError();
      return;
    }
    if (code === 'unknownHandle') {
      // The project was closed meanwhile: nothing to say.
      return;
    }
    await this.#deps.dialogs.alert(failureMessage(code, action));
  }

  /** Whether `session` may still change the store: the newest run of the open project. */
  #isCurrent(session: RunSession, handle: Handle): boolean {
    return (
      this.#session === session &&
      this.#sessionHandle === handle &&
      this.#deps.store.getState().project?.handle === handle
    );
  }

  async #start(
    buildId: BuildId,
    handle: Handle,
    generation: number,
    mayRebuild: boolean,
  ): Promise<void> {
    const { bridge, store, ipc } = this.#deps;
    const session: RunSession = new RunSession({
      ipc,
      bridge,
      clock: this.#deps.clock,
      prelude: bridge.used ? RUN_SEPARATOR : null,
      hooks: {
        onStarted: (started) => {
          if (this.#isCurrent(session, handle)) {
            this.#started(started);
          }
        },
        onExit: (exit) => {
          if (this.#isCurrent(session, handle)) {
            this.#exited(exit);
          }
        },
      },
    });
    this.#session?.detach();
    this.#session = session;
    this.#sessionHandle = handle;

    const before = store.getState().run;
    store.getState().actions.setRun({ status: 'starting', runId: null });
    this.#showTab('console');

    let response: RunStartResponse;
    try {
      response = await ipc.runStart(
        { buildId, runOptions: clampSize(bridge.console().size()) },
        session.onOutput,
        session.onEvent,
      );
    } catch (error: unknown) {
      session.detach();
      if (this.#session === session) {
        this.#session = null;
        this.#sessionHandle = null;
        if (store.getState().project?.handle === handle) {
          store.getState().actions.setRun({
            status: before.exit === null ? 'idle' : 'exited',
            runId: before.runId,
          });
        }
      }
      const code = failureCode(error);
      console.warn('run_start failed', code);
      if (generation !== this.#generation) {
        return;
      }
      if (mayRebuild && REBUILD_CODES.has(code)) {
        this.#deps.builds.forgetSuccess();
        const rebuilt = await this.#ensureBuilt(generation);
        if (rebuilt !== null && generation === this.#generation) {
          await this.#start(rebuilt, handle, generation, false);
        }
        return;
      }
      await this.#report(code, 'run');
      return;
    }

    const runId: unknown = response.runId;
    if (typeof runId !== 'string' || !RUN_ID.test(runId)) {
      console.error('run_start answered without a valid run ID');
      session.detach();
      return;
    }
    session.started(runId as RunId);
    if (this.#isCurrent(session, handle)) {
      store.getState().actions.setRun({ runId: runId as RunId });
    }
  }

  #started(started: { containment: Containment; ideHelpers: boolean; at: number }): void {
    this.#deps.store.getState().actions.setRun({
      status: 'running',
      startedAt: started.at,
      exit: null,
      containment: started.containment,
      ideHelpers: started.ideHelpers,
    });
    this.#showTab('console');
    // Typing goes to the program: move the focus into the terminal once it is shown, unless a
    // dialog has it.
    this.#deps.clock.setTimeout(() => {
      if (document.querySelector('[role="dialog"], [role="alertdialog"]') === null) {
        this.#deps.bridge.console().focus();
      }
    }, 0);
  }

  #exited(exit: Extract<RunEvent, { kind: 'exit' }>): void {
    this.#deps.store.getState().actions.setRun({ status: 'exited', exit });
  }
}
