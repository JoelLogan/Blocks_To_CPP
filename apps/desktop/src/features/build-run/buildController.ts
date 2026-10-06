/**
 * The build session of the open project (docs/spec/02-architecture.md §2.4.2, 07 §7.5.4):
 * `build_start` with the project's canonical text and the session's Debug/Release choice, then
 * its channel's `progress`, `diagnostics` and one `finished` event, which update `store.build` and
 * the Build output tab.
 *
 * There is one build per project: a new build replaces the running one (the backend cancels it),
 * and a build that would build the same thing as the running one is not started twice. *Stop*
 * cancels the running build with `build_cancel`.
 *
 * Diagnostics: a build replaces the last build's diagnostics as soon as it reports its own. A
 * cancelled build puts the last build's back, and an up-to-date build (which compiles nothing, so
 * reports no compiler warnings) keeps the compiler messages of the last build of the same content.
 */
import type {
  BuildConfig,
  BuildEvent,
  BuildId,
  BuildOutcome,
  Diagnostic,
  Handle,
  IpcClient,
} from '@blocks2cpp/ipc-types';

import type { DialogService } from '../../app/dialogs';
import { type BuildState, type BuildStatus, useAppStore } from '../../app/store';
import { selectedToolchain } from '../../app/store/selectors';
import {
  buildStartLine,
  diagnosticLines,
  finishedLine,
  GENERATOR_BUG_NOTE,
  labelGeneratorBugs,
  notStartedLine,
  progressLine,
} from './buildOutput';
import { checkBuildEvent } from './channel';
import { type BuildDocument, type DocumentSources, documentToBuild } from './document';
import { failureCode, type FailureCode, failureMessage, unreadableCanvasMessage } from './messages';

/** The most diagnostics one build keeps; more are dropped. */
export const MAX_BUILD_DIAGNOSTICS = 20_000;

/** How many successful builds the controller remembers the toolchain of. */
const REMEMBERED_TOOLCHAINS = 16;

/** A build ID as the backend makes them (02 §2.5.4). */
const BUILD_ID = /^bd_[0-9a-f]{32}$/;

/** What a build builds: the content, the configuration and the compiler. */
export interface BuildKey {
  /** The content hash of the document. */
  readonly hash: string;
  readonly config: BuildConfig;
  /** The toolchain a build would use now (the selected one, or the first usable one). */
  readonly toolchainId: string | null;
}

/** A build about to start: the project, its document and what it would build. */
export interface PreparedBuild {
  readonly handle: Handle;
  readonly projectName: string;
  readonly document: BuildDocument;
  readonly key: BuildKey;
}

/** How a build ended. */
export type BuildFinish =
  | {
      readonly kind: 'finished';
      readonly outcome: BuildOutcome;
      /** `null` only when the backend's answer to `build_start` never came. */
      readonly buildId: BuildId | null;
      readonly projectHash: string | null;
      /** A newer build replaced it before it finished (so the backend cancelled it). */
      readonly superseded: boolean;
      /** How many error diagnostics it reported. */
      readonly errorCount: number;
    }
  | {
      /** `build_start` failed; nothing was built. */
      readonly kind: 'notStarted';
      readonly code: FailureCode;
    };

/** A build that was started (or joined), and how it ends. */
export interface ActiveBuild {
  readonly key: BuildKey;
  /** Resolves when the build has ended; never rejects. */
  readonly finished: Promise<BuildFinish>;
}

/** What the controller needs. */
export interface BuildControllerDeps extends DocumentSources {
  readonly ipc: IpcClient;
  readonly store: typeof useAppStore;
  /** Says why a build cannot start (a canvas the loader refuses). */
  readonly dialogs: Pick<DialogService, 'alert'>;
}

/** One build session, from `build_start` to its `finished` event. */
interface Session {
  readonly handle: Handle;
  readonly key: BuildKey;
  readonly finished: Promise<BuildFinish>;
  readonly resolve: (finish: BuildFinish) => void;
  /** The build store's diagnostics before this build reported any. */
  readonly previous: Pick<BuildState, 'diagnostics' | 'diagnosticsHash'>;
  buildId: BuildId | null;
  /** Whether `build_start` has answered (with an ID or an error). */
  answered: boolean;
  /** A `finished` event that arrived before `build_start` answered. */
  early: Extract<BuildEvent, { kind: 'finished' }> | null;
  done: boolean;
  cancelRequested: boolean;
  superseded: boolean;
  diagnostics: Diagnostic[];
  generatorBugNoted: boolean;
  lastProgress: string | null;
}

function sameKey(a: BuildKey, b: BuildKey): boolean {
  return a.hash === b.hash && a.config === b.config && a.toolchainId === b.toolchainId;
}

/** The build slice's status after an outcome. */
function statusOf(outcome: BuildOutcome): BuildStatus {
  switch (outcome) {
    case 'built':
    case 'upToDate':
      return 'succeeded';
    case 'cancelled':
      return 'cancelled';
    default:
      return 'failed';
  }
}

function isCompilerMessage(diagnostic: Diagnostic): boolean {
  return diagnostic.source === 'compiler' || diagnostic.source === 'linker';
}

/** See the module documentation. */
export class BuildController {
  readonly #deps: BuildControllerDeps;
  #current: Session | null = null;
  /** The toolchain of recent successful builds, so a change of compiler builds again. */
  readonly #toolchains = new Map<BuildId, string | null>();

  constructor(deps: BuildControllerDeps) {
    this.#deps = deps;
  }

  /**
   * What a build of the open project would build now, or `null` without a project, and when the
   * canvas does not read back as a project the loader accepts: then nothing may be built, and a
   * dialog names the loader's problems (Run builds through here too).
   */
  prepare(): PreparedBuild | null {
    const state = this.#deps.store.getState();
    const project = state.project;
    const found = documentToBuild(this.#deps);
    if (project === null || found === null) {
      return null;
    }
    if (found.kind === 'unreadable') {
      this.#deps.dialogs
        .alert(unreadableCanvasMessage(found.diagnostics))
        .catch((error: unknown) => {
          console.error('Could not say why the build did not start', error);
        });
      return null;
    }
    const { document } = found;
    return {
      handle: project.handle,
      projectName: project.document.project.name,
      document,
      key: {
        hash: document.hash,
        config: state.build.config,
        toolchainId: selectedToolchain(state)?.id ?? null,
      },
    };
  }

  /**
   * The ID of the last successful build when it built exactly `key` (07 §7.6.1: Run needs no
   * build then), or `null`.
   */
  successFor(key: BuildKey): BuildId | null {
    const success = this.#deps.store.getState().build.lastSuccess;
    if (success?.projectHash !== key.hash || success.config !== key.config) {
      return null;
    }
    const toolchain = this.#toolchains.get(success.buildId);
    return toolchain === undefined || toolchain === key.toolchainId ? success.buildId : null;
  }

  /** Forgets the last successful build (the backend said it no longer matches its program). */
  forgetSuccess(): void {
    this.#deps.store.getState().actions.setBuild({ lastSuccess: null });
  }

  /** The running build, or `null`. */
  active(): ActiveBuild | null {
    const current = this.#current;
    return current !== null && !current.done ? current : null;
  }

  /**
   * Starts a build of `prepared` (by default, of the project as it is now). When the running
   * build builds the same thing, that one is returned instead. Returns `null` without a project.
   */
  start(prepared: PreparedBuild | null = this.prepare()): ActiveBuild | null {
    if (prepared === null) {
      return null;
    }
    const { actions, build } = this.#deps.store.getState();
    // The diagnostics of the last build that finished: a cancelled build puts them back.
    let previous: Session['previous'] = {
      diagnostics: build.diagnostics,
      diagnosticsHash: build.diagnosticsHash,
    };
    const running = this.#current;
    if (running !== null && !running.done) {
      if (running.handle === prepared.handle && sameKey(running.key, prepared.key)) {
        return running;
      }
      // The backend cancels it when the new one starts; its `finished` event says so. What it
      // reported so far is not a finished build's.
      running.superseded = true;
      if (running.handle === prepared.handle) {
        previous = running.previous;
      }
    }

    let resolve: (finish: BuildFinish) => void = () => undefined;
    const finished = new Promise<BuildFinish>((done) => {
      resolve = done;
    });
    const session: Session = {
      handle: prepared.handle,
      key: prepared.key,
      finished,
      resolve,
      previous,
      buildId: null,
      answered: false,
      early: null,
      done: false,
      cancelRequested: false,
      superseded: false,
      diagnostics: [],
      generatorBugNoted: false,
      lastProgress: null,
    };
    this.#current = session;

    actions.setBuild({ status: 'building', buildId: null, progress: null, output: [] });
    actions.appendBuildOutput([buildStartLine(prepared.projectName, prepared.key.config)]);

    this.#deps.ipc
      .buildStart(
        {
          handle: prepared.handle,
          document: prepared.document.text,
          config: prepared.key.config,
        },
        (message) => {
          this.#onEvent(session, message);
        },
      )
      .then(
        (response) => {
          this.#answered(session, response.buildId);
        },
        (error: unknown) => {
          this.#notStarted(session, failureCode(error));
        },
      );
    return session;
  }

  /** Cancels the running build (`build_cancel`); nothing happens when none runs. */
  cancel(): void {
    const current = this.#current;
    if (current === null || current.done) {
      return;
    }
    current.cancelRequested = true;
    if (current.buildId !== null) {
      this.#sendCancel(current.buildId);
    }
  }

  /**
   * Forgets the running build because the project changed (the backend cancels the builds of a
   * closed project). Whoever waits for it hears `cancelled`; the store is not touched.
   */
  reset(): void {
    const current = this.#current;
    this.#current = null;
    this.#toolchains.clear();
    if (current !== null && !current.done) {
      current.done = true;
      current.resolve({
        kind: 'finished',
        outcome: 'cancelled',
        buildId: current.buildId,
        projectHash: null,
        superseded: false,
        errorCount: 0,
      });
    }
  }

  /** Whether `session` may still change the store: the newest build of the open project. */
  #isCurrent(session: Session): boolean {
    return (
      this.#current === session && this.#deps.store.getState().project?.handle === session.handle
    );
  }

  #sendCancel(buildId: BuildId): void {
    this.#deps.ipc.buildCancel({ buildId }).catch((error: unknown) => {
      // A build that has just finished (or a closed project) cannot be cancelled; that is fine.
      console.debug('build_cancel failed', failureCode(error));
    });
  }

  #answered(session: Session, buildId: unknown): void {
    session.answered = true;
    if (typeof buildId !== 'string' || !BUILD_ID.test(buildId)) {
      console.error('build_start answered without a valid build ID');
      this.#notStarted(session, 'internal');
      return;
    }
    session.buildId = buildId as BuildId;
    if (this.#isCurrent(session) && !session.done) {
      this.#deps.store.getState().actions.setBuild({ buildId: session.buildId });
    }
    if (session.cancelRequested && !session.done) {
      this.#sendCancel(session.buildId);
    }
    if (session.early !== null) {
      const early = session.early;
      session.early = null;
      this.#finish(session, early);
    }
  }

  #notStarted(session: Session, code: FailureCode): void {
    session.answered = true;
    if (session.done) {
      return;
    }
    if (session.early !== null) {
      // The build ran (its `finished` event came) even though the answer was lost.
      const early = session.early;
      session.early = null;
      this.#finish(session, early);
      return;
    }
    session.done = true;
    console.warn('build_start failed', code);
    if (this.#isCurrent(session)) {
      const { actions } = this.#deps.store.getState();
      actions.setBuild({ status: 'failed', progress: null });
      actions.appendBuildOutput([notStartedLine(failureMessage(code, 'build').message)]);
    }
    session.resolve({ kind: 'notStarted', code });
  }

  #onEvent(session: Session, message: unknown): void {
    if (session.done || session.early !== null) {
      return;
    }
    const event = checkBuildEvent(message);
    if (event === null) {
      console.warn('Ignored a build event of an unknown shape');
      return;
    }
    const current = this.#isCurrent(session);
    const { actions } = this.#deps.store.getState();
    switch (event.kind) {
      case 'progress': {
        if (!current) {
          return;
        }
        actions.setBuild({
          progress: { stage: event.stage, done: event.done, total: event.total },
        });
        const line = progressLine(event.stage, event.done, event.total);
        if (line.text !== session.lastProgress) {
          session.lastProgress = line.text;
          actions.appendBuildOutput([line]);
        }
        return;
      }
      case 'diagnostics': {
        const room = MAX_BUILD_DIAGNOSTICS - session.diagnostics.length;
        if (room <= 0) {
          return;
        }
        const labelled = labelGeneratorBugs(
          event.items.slice(0, room),
          this.#deps.store.getState().project?.document ?? null,
        );
        session.diagnostics = session.diagnostics.concat(labelled.items);
        if (!current) {
          return;
        }
        actions.setBuild({
          diagnostics: [...session.diagnostics],
          diagnosticsHash: session.key.hash,
        });
        const lines = diagnosticLines(labelled.items);
        if (labelled.generatorBug && !session.generatorBugNoted) {
          session.generatorBugNoted = true;
          lines.push(GENERATOR_BUG_NOTE);
        }
        actions.appendBuildOutput(lines);
        return;
      }
      case 'finished':
        if (!session.answered) {
          // The ID is needed for Run: wait for `build_start`'s answer.
          session.early = event;
          return;
        }
        this.#finish(session, event);
        return;
    }
  }

  #finish(session: Session, event: Extract<BuildEvent, { kind: 'finished' }>): void {
    if (session.done) {
      return;
    }
    session.done = true;
    const errorCount = session.diagnostics.filter(
      (diagnostic) => diagnostic.severity === 'error',
    ).length;
    const projectHash = event.projectHash ?? session.key.hash;
    if (this.#isCurrent(session)) {
      const { actions } = this.#deps.store.getState();
      const patch: Partial<BuildState> = { status: statusOf(event.outcome), progress: null };
      switch (event.outcome) {
        case 'cancelled':
          // A cancelled build says nothing: show the last finished build's diagnostics again
          // (a build this one replaced may have reported some meanwhile).
          patch.diagnostics = session.previous.diagnostics;
          patch.diagnosticsHash = session.previous.diagnosticsHash;
          break;
        case 'upToDate': {
          const kept =
            session.previous.diagnosticsHash === projectHash
              ? session.previous.diagnostics.filter(isCompilerMessage)
              : [];
          patch.diagnostics = [...session.diagnostics, ...kept];
          patch.diagnosticsHash = projectHash;
          break;
        }
        default:
          patch.diagnostics = session.diagnostics;
          patch.diagnosticsHash = projectHash;
      }
      if ((event.outcome === 'built' || event.outcome === 'upToDate') && session.buildId !== null) {
        patch.lastSuccess = {
          buildId: session.buildId,
          projectHash,
          config: session.key.config,
        };
        this.#rememberToolchain(session.buildId, session.key.toolchainId);
      }
      actions.setBuild(patch);
      actions.appendBuildOutput([finishedLine(event.outcome, event.elapsedMs, errorCount)]);
    }
    session.resolve({
      kind: 'finished',
      outcome: event.outcome,
      buildId: session.buildId,
      projectHash: event.projectHash,
      superseded: session.superseded,
      errorCount,
    });
  }

  #rememberToolchain(buildId: BuildId, toolchainId: string | null): void {
    this.#toolchains.set(buildId, toolchainId);
    while (this.#toolchains.size > REMEMBERED_TOOLCHAINS) {
      const oldest = this.#toolchains.keys().next();
      if (oldest.done === true) {
        break;
      }
      this.#toolchains.delete(oldest.value);
    }
  }
}
