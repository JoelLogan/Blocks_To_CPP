/**
 * The open project's file changed on disk (docs/spec/04-user-interface.md §4.10, 05 §5.10,
 * 08 §8.3): the backend's file watcher says so (`projectChangedOnDisk`), or a save was refused
 * because the file is not what was opened or last saved (`project:changedOnDisk`, from the project
 * feature). The user chooses:
 *
 * - **Reload** (`project_reload`): the backend reads the file again through the whole open flow,
 *   trust re-check included; the document goes through the compiler core's loader
 *   (`openDocumentInEditor`), replaces the workspace and clears the undo history, and the project
 *   takes the trust state from the response, so an outside change to trust-relevant content shows
 *   Restricted Mode. Unsaved changes are discarded. It runs in the project lifecycle's queue
 *   (`ProjectQueue.reloadWith`), so a save never writes the old canvas over the file being read
 *   again, nor does another open replace the project meanwhile. Not offered when the file was
 *   deleted or moved; a reload that finds the file gone asks again without it.
 * - **Keep mine (save as…)**: the project feature's `project.saveAs`.
 * - **Not now** (also what dismissing the dialog means): nothing changes; the next save is refused
 *   again and asks again.
 *
 * One question per project at a time: reports that arrive while it is shown only update what it
 * knows (whether the file is gone). Reports about a project that is not the open one are stale and
 * ignored. Backend errors become sentences for the user; only bugs reject.
 */
import type { Handle, ProjectReloadResponse } from '@blocks2cpp/ipc-types';

import type { FeatureContext } from '../../app/features';
import type { ProjectState } from '../../app/store';
import {
  openDocumentInEditor,
  type OpenDocumentArgs,
  type OpenDocumentResult,
} from '../../editor/load';
import type { ProjectQueue } from '../project/link';
import { errorCode, ipcErrorOf, problemLines, shownName } from '../recovery/text';
import { describeReloadError, type ExternalChangeChoice, externalChangeQuestion } from './messages';

/** What the controller needs besides the feature context (replaceable in tests). */
export interface ExternalChangeOptions {
  /** Shows a document in the editor; `openDocumentInEditor` by default. */
  readonly openInEditor?: (
    ctx: FeatureContext,
    args: OpenDocumentArgs,
  ) => Promise<OpenDocumentResult>;
  /**
   * The project lifecycle's queue, which a reload runs in (the project feature's link in the app).
   * Without it the reload runs at once.
   */
  readonly project?: Pick<ProjectQueue, 'reloadWith'> | null;
}

/** How a reload ended. */
type ReloadOutcome =
  /** Shown. */
  | 'reloaded'
  /** The file is gone: ask again, without *Reload*. */
  | 'gone'
  /** Did not happen; the user was told why (or the project is no longer open). */
  | 'failed';

/** The question being handled for a project. */
interface Active {
  readonly handle: Handle;
  /** Whether the file is gone, as last reported while the question was open. */
  deleted: boolean;
  /** A new report arrived while a reload (or the report of its failure) ran: ask again. */
  changedDuringReload: boolean;
  /** What is running: the question, a reload, or *Save as*. */
  step: 'asking' | 'reloading' | 'savingAs';
  readonly done: Promise<void>;
}

/** Whether a report arrived while `active`'s reload ran (a call: it is read after an await). */
function changedMeanwhile(active: Active): boolean {
  return active.changedDuringReload;
}

/** Handles outside changes of the open project's file; see the module comment. */
export class ExternalChangeController {
  private readonly ctx: FeatureContext;
  private readonly openInEditor: NonNullable<ExternalChangeOptions['openInEditor']>;
  private readonly project: Pick<ProjectQueue, 'reloadWith'> | null;
  private active: Active | null = null;
  private disposed = false;

  constructor(ctx: FeatureContext, options: ExternalChangeOptions = {}) {
    this.ctx = ctx;
    this.openInEditor = options.openInEditor ?? openDocumentInEditor;
    this.project = options.project ?? null;
  }

  /** Stops: later reports are ignored and a running question ends after its answer. */
  dispose(): void {
    this.disposed = true;
  }

  /** Whether {@link dispose} was called (read through a call, so it is checked after an await). */
  private isDisposed(): boolean {
    return this.disposed;
  }

  /**
   * The file of `handle` changed on disk. `deleted`: whether it was deleted or moved, or `null`
   * when that is not known (a refused save). Resolves when the question it raised (or joined) is
   * settled; never rejects.
   */
  notify(handle: Handle, deleted: boolean | null): Promise<void> {
    if (this.isDisposed()) {
      return Promise.resolve();
    }
    const active = this.active;
    if (active?.handle === handle) {
      if (active.step === 'reloading') {
        active.changedDuringReload = true;
      }
      if (deleted !== null && active.step !== 'savingAs') {
        active.deleted = deleted;
      }
      return active.done;
    }
    const project = this.ctx.store.getState().project;
    if (project?.handle !== handle || project.fileName === null) {
      // A project that is no longer open (or never had a file): nothing to ask about.
      return Promise.resolve();
    }
    if (active !== null) {
      // A question about another project is still open; its project was replaced meanwhile.
      return active.done.then(() => this.notify(handle, deleted));
    }
    let finish: () => void = () => undefined;
    const done = new Promise<void>((resolve) => {
      finish = resolve;
    });
    const next: Active = {
      handle,
      deleted: deleted ?? false,
      changedDuringReload: false,
      step: 'asking',
      done,
    };
    this.active = next;
    this.handle(next)
      .catch((error: unknown) => {
        console.error('Handling an outside change of the project file failed', error);
      })
      .finally(() => {
        if (this.active === next) {
          this.active = null;
        }
        finish();
      });
    return done;
  }

  /** Asks, and acts on the answer, until the question is settled. */
  private async handle(active: Active): Promise<void> {
    for (;;) {
      const project = this.openProject(active.handle);
      if (project === null || this.isDisposed()) {
        return;
      }
      active.step = 'asking';
      const question = externalChangeQuestion(project, active.deleted);
      const choice = await this.ctx.dialogs.choose<ExternalChangeChoice>({
        ...question,
        cancel: 'later',
      });
      const now = this.openProject(active.handle);
      if (now === null || this.isDisposed()) {
        return;
      }
      switch (choice) {
        case 'later':
          return;
        case 'keepMine':
          active.step = 'savingAs';
          await this.keepMine();
          return;
        case 'reload': {
          // Even when a report said meanwhile that the file is gone: the reload finds out.
          active.step = 'reloading';
          active.changedDuringReload = false;
          const outcome = await this.reloadInQueue(active.handle);
          if (outcome === 'gone') {
            active.deleted = true;
            continue;
          }
          if (changedMeanwhile(active)) {
            // Changed again while it was being read (or while a failure was explained): what is
            // shown may be outdated, so the question is asked again about the file as it is now.
            continue;
          }
          return;
        }
      }
    }
  }

  /** The open project when it is still `handle`, else `null`. */
  private openProject(handle: Handle): ProjectState | null {
    const project = this.ctx.store.getState().project;
    return project?.handle === handle ? project : null;
  }

  /** *Keep mine (save as…)*: the project feature's `project.saveAs`. */
  private async keepMine(): Promise<void> {
    if (!this.ctx.commands.hasCommand('project.saveAs')) {
      console.error('No project.saveAs command is registered');
      await this.ctx.dialogs.alert({
        title: 'The project was not saved',
        message:
          'Something went wrong in Blocks2Cpp, so Save as… is not available. (Error: noSaveAsCommand)',
      });
      return;
    }
    try {
      // It reports its own failures and cancellations.
      await this.ctx.commands.runCommand('project.saveAs');
    } catch (error: unknown) {
      console.error('Save as… failed after an outside change', error);
    }
  }

  /**
   * *Reload* in the project lifecycle's queue: once it is its turn, the project must still be
   * open.
   */
  private async reloadInQueue(handle: Handle): Promise<ReloadOutcome> {
    const reloadNow = (): Promise<ReloadOutcome> => {
      const project = this.openProject(handle);
      return project === null || this.isDisposed()
        ? Promise.resolve('failed')
        : this.reload(project);
    };
    if (this.project === null) {
      return reloadNow();
    }
    let outcome: ReloadOutcome = 'failed';
    await this.project.reloadWith(async () => {
      outcome = await reloadNow();
      return outcome === 'reloaded';
    });
    return outcome;
  }

  /** *Reload*: reads the file again and shows it; see the module comment. */
  private async reload(project: ProjectState): Promise<ReloadOutcome> {
    const { handle } = project;
    const name = shownName(project.document.project.name);
    let response: ProjectReloadResponse;
    try {
      response = await this.ctx.ipc.projectReload({ handle });
    } catch (error: unknown) {
      const ipcError = ipcErrorOf(error);
      const code = errorCode(error);
      console.warn('The project file could not be reloaded', code);
      if (
        ipcError?.code === 'notFound' ||
        (ipcError?.code === 'io' && ipcError.kind === 'notFound')
      ) {
        return 'gone';
      }
      // `unknownHandle`: closed meanwhile; `noPath`: it has no file. Nothing to reload either way.
      if (ipcError?.code !== 'unknownHandle' && ipcError?.code !== 'noPath') {
        await this.ctx.dialogs.alert({
          title: `“${name}” could not be reloaded`,
          message: describeReloadError(ipcError, code),
        });
      }
      return 'failed';
    }

    const now = this.openProject(handle);
    if (now === null) {
      return 'failed';
    }
    let result: OpenDocumentResult;
    try {
      result = await this.openInEditor(this.ctx, {
        handle,
        documentText: response.document,
        trust: response.trust,
        fileName: now.fileName,
        migratedFrom: response.migratedFrom,
        // The file's own text: no unsaved changes.
        savedText: response.document,
      });
    } catch (error: unknown) {
      console.error('The compiler core could not show a reloaded project', error);
      await this.keepEditorVersion(handle, response);
      await this.ctx.dialogs.alert({
        title: `“${name}” could not be reloaded`,
        message:
          'The Blocks2Cpp compiler core stopped, so the file could not be shown. Your blocks were not changed; they now count as unsaved changes, and saving them replaces the file. Restart Blocks2Cpp to see the file.',
      });
      return 'failed';
    }
    if (!result.ok) {
      console.warn(
        'The compiler core refused a reloaded project',
        result.diagnostics.map((diagnostic) => diagnostic.code).join(', '),
      );
      await this.keepEditorVersion(handle, response);
      await this.ctx.dialogs.alert({
        title: `“${name}” could not be reloaded`,
        message: `Blocks2Cpp found these problems in the file:\n${problemLines(result.diagnostics).join('\n')}\nYour blocks were not changed; they now count as unsaved changes, and saving them replaces the file.`,
      });
      return 'failed';
    }
    return 'reloaded';
  }

  /**
   * After the backend reloaded the file but the editor could not show it: the editor keeps its
   * blocks, which now differ from the file, so they count as unsaved changes (in the store and,
   * because a reload clears the flag there, in the backend), and the project takes the reloaded
   * file's trust state, which the backend enforces from now on.
   */
  private async keepEditorVersion(handle: Handle, response: ProjectReloadResponse): Promise<void> {
    if (this.openProject(handle) === null) {
      return;
    }
    this.ctx.store
      .getState()
      .actions.updateProject({ trust: response.trust, savedCanonicalText: null, dirty: true });
    try {
      await this.ctx.ipc.projectSetDirty({ handle, dirty: true });
    } catch (error: unknown) {
      console.warn('Could not report unsaved changes to the backend', errorCode(error));
    }
  }
}
