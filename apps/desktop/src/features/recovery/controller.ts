/**
 * Restoring and discarding the recovery snapshots of earlier sessions (docs/spec/04-user-interface.md
 * §4.10, 05 §5.10, 08 §8.3.1).
 *
 * - **Restore** opens a snapshot as a project with unsaved changes, under a new handle. It runs
 *   in the project lifecycle's queue ({@link ProjectQueue.replaceWith}), like New and Open: it
 *   waits for an operation in progress, they wait for it, and the start page's buttons are held
 *   back meanwhile. The lifecycle settles the open project's unsaved changes first (*Save*, *Don't
 *   save* or *Cancel*) and closes it only once the restored one is shown. The compiler core is
 *   started next, so a core that cannot start leaves the snapshot untouched; then
 *   `recovery_restore` hands over the document, which goes through the core's loader like any
 *   other (`openDocumentInEditor`), with the trust state the backend decided.
 * - **Discard** deletes a snapshot after the user confirms; it cannot be undone.
 *
 * One operation of the offer runs at a time. Backend errors become sentences for the user; only
 * bugs reject. A restored project that cannot be shown keeps its handle (and with it the backend's
 * snapshot of it), so the work is offered again at the next start instead of being lost.
 */
import type { CoreWasm } from '@blocks2cpp/b2c-core-wasm';
import type {
  Handle,
  RecoveryRestoreResponse,
  SnapshotId,
  SnapshotInfo,
} from '@blocks2cpp/ipc-types';

import type { FeatureContext } from '../../app/features';
import {
  openDocumentInEditor,
  type OpenDocumentArgs,
  type OpenDocumentResult,
} from '../../editor/load';
import { appCoreHost } from '../../editor/preview/coreHost';
import type { ProjectQueue } from '../project/link';
import {
  type RecoveryModel,
  type RecoveryOperation,
  offeredSnapshots,
  withoutSnapshot,
} from './model';
import {
  documentProblem,
  errorCode,
  ipcErrorOf,
  localDateTime,
  problemLines,
  shownName,
  somethingWentWrong,
} from './text';

/** What the controller needs besides the feature context (replaceable in tests). */
export interface RecoveryControllerOptions {
  /** Shows a document in the editor; `openDocumentInEditor` by default. */
  readonly openInEditor?: (
    ctx: FeatureContext,
    args: OpenDocumentArgs,
  ) => Promise<OpenDocumentResult>;
  /**
   * Makes sure the compiler core runs before a snapshot is taken from the backend: by default the
   * published core, else the app's core host starts it.
   *
   * @throws when it cannot be started.
   */
  readonly startCore?: () => Promise<CoreWasm>;
  /**
   * The project lifecycle's queue, which a restore runs in (the project feature's link in the app).
   * Without it nothing can be restored.
   */
  readonly project?: Pick<ProjectQueue, 'replaceWith'> | null;
}

/** Restores and discards snapshots; see the module comment. */
export class RecoveryController {
  private readonly ctx: FeatureContext;
  private readonly model: RecoveryModel;
  private readonly openInEditor: NonNullable<RecoveryControllerOptions['openInEditor']>;
  private readonly startCore: () => Promise<CoreWasm>;
  private readonly project: Pick<ProjectQueue, 'replaceWith'> | null;
  /** The latest `recovery_list` request; older answers are dropped. */
  private listRequest = 0;
  private disposed = false;

  constructor(ctx: FeatureContext, model: RecoveryModel, options: RecoveryControllerOptions = {}) {
    this.ctx = ctx;
    this.model = model;
    this.openInEditor = options.openInEditor ?? openDocumentInEditor;
    this.startCore =
      options.startCore ??
      (() => {
        const core = ctx.core();
        return core === null ? appCoreHost().start() : Promise.resolve(core);
      });
    this.project = options.project ?? null;
  }

  /** Stops: later operations do nothing and resolve `false`. */
  dispose(): void {
    this.disposed = true;
  }

  /** Whether {@link dispose} was called (read through a call, so it is checked after an await). */
  private isDisposed(): boolean {
    return this.disposed;
  }

  /** Reads the snapshots offered for restore (`recovery_list`). Never rejects. */
  async refresh(): Promise<void> {
    if (this.isDisposed()) {
      return;
    }
    const request = ++this.listRequest;
    try {
      const response = await this.ctx.ipc.recoveryList();
      if (request === this.listRequest && !this.isDisposed()) {
        this.model.setState({ status: 'ready', snapshots: offeredSnapshots(response.snapshots) });
      }
    } catch (error: unknown) {
      if (request === this.listRequest && !this.isDisposed()) {
        console.warn('Could not read the recovery snapshots', errorCode(error));
        this.model.setState({ status: 'failed', snapshots: [] });
      }
    }
  }

  /**
   * Restores the snapshot `snapshotId` as the window's project (see the module comment). Resolves
   * whether it is shown; `false` when another operation is running, the user cancelled, or it
   * failed (after telling the user why).
   */
  restore(snapshotId: SnapshotId): Promise<boolean> {
    return this.exclusive({ kind: 'restore', snapshotId }, async (snapshot) => {
      const project = this.project;
      if (project === null) {
        console.error('The recovery feature has no project lifecycle; nothing was restored');
        await this.ctx.dialogs.alert({
          title: 'Nothing was restored',
          message: somethingWentWrong('the unsaved work was not restored', 'noProjectLifecycle'),
        });
        return false;
      }
      // The lifecycle settles the open project first, and closes it once this one is shown.
      return project.replaceWith(() => this.restoreNow(snapshot));
    });
  }

  /**
   * Deletes the snapshot `snapshotId` after the user confirms. Resolves whether it is gone.
   */
  discard(snapshotId: SnapshotId): Promise<boolean> {
    return this.exclusive({ kind: 'discard', snapshotId }, async (snapshot) => {
      const name = shownName(snapshot.projectName);
      const when = localDateTime(snapshot.savedAt);
      const confirmed = await this.ctx.dialogs.confirm({
        title: `Discard the unsaved work on “${name}”?`,
        message: `${when === null ? 'It was saved automatically' : `It was saved automatically on ${when}`}. Discarded work cannot be recovered.`,
        confirmLabel: 'Discard',
        cancelLabel: 'Keep it',
        destructive: true,
      });
      if (!confirmed) {
        return false;
      }
      try {
        await this.ctx.ipc.recoveryDiscard({ snapshotId });
      } catch (error: unknown) {
        // `unknownSnapshot`: it is gone already (restored or discarded elsewhere), as asked.
        if (ipcErrorOf(error)?.code !== 'unknownSnapshot') {
          console.warn('Could not discard a recovery snapshot', errorCode(error));
          await this.ctx.dialogs.alert({
            title: 'The unsaved work was not discarded',
            message: describeDiscardError(error),
          });
          return false;
        }
      }
      withoutSnapshot(this.model, snapshotId);
      return true;
    });
  }

  // ---- One operation at a time -----------------------------------------------------------------

  /** Runs `task` on the offered snapshot unless another operation is running. */
  private async exclusive(
    operation: RecoveryOperation,
    task: (snapshot: SnapshotInfo) => Promise<boolean>,
  ): Promise<boolean> {
    if (this.isDisposed() || this.model.getState().busy !== null) {
      return false;
    }
    const snapshot = this.model
      .getState()
      .snapshots.find((candidate) => candidate.snapshotId === operation.snapshotId);
    if (snapshot === undefined) {
      return false;
    }
    this.model.setState({ busy: operation });
    try {
      return await task(snapshot);
    } finally {
      this.model.setState({ busy: null });
    }
  }

  // ---- Restoring -------------------------------------------------------------------------------

  /**
   * Starts the compiler core, takes the snapshot from the backend and shows it. Resolves the
   * restored project's handle, or `null` when nothing is shown (after telling the user why).
   */
  private async restoreNow(snapshot: SnapshotInfo): Promise<Handle | null> {
    if (this.isDisposed()) {
      return null;
    }
    try {
      await this.startCore();
    } catch (error: unknown) {
      console.error('The compiler core could not start before a restore', error);
      await this.ctx.dialogs.alert({
        title: 'Nothing was restored',
        message:
          'The Blocks2Cpp compiler core could not start, so the unsaved work was not restored. It is still offered here. Restart Blocks2Cpp and try again.',
      });
      return null;
    }

    let restored: RecoveryRestoreResponse;
    try {
      restored = await this.ctx.ipc.recoveryRestore({ snapshotId: snapshot.snapshotId });
    } catch (error: unknown) {
      await this.reportRestoreFailure(snapshot, error);
      return null;
    }
    // The backend has a new project for it now and will not offer the snapshot again.
    withoutSnapshot(this.model, snapshot.snapshotId);
    return (await this.show(snapshot, restored)) ? restored.handle : null;
  }

  /**
   * Shows the restored project. When it cannot be shown, says so and leaves its handle open: the
   * backend keeps its snapshot, which is offered again at the next start.
   */
  private async show(snapshot: SnapshotInfo, restored: RecoveryRestoreResponse): Promise<boolean> {
    const name = shownName(snapshot.projectName);
    let result: OpenDocumentResult;
    try {
      result = await this.openInEditor(this.ctx, {
        handle: restored.handle,
        documentText: restored.document,
        trust: restored.trust,
        fileName: restored.fileName,
        migratedFrom: null,
        // A snapshot holds changes that were never saved: the project starts with unsaved changes.
        savedText: null,
      });
    } catch (error: unknown) {
      console.error('The compiler core could not show a restored project', error);
      await this.ctx.dialogs.alert({
        title: `“${name}” could not be shown`,
        message:
          'The Blocks2Cpp compiler core stopped, so the restored project could not be shown. Your unsaved work is kept and will be offered again the next time Blocks2Cpp starts.',
      });
      return false;
    }
    if (!result.ok) {
      console.warn(
        'The compiler core refused a restored project',
        result.diagnostics.map((diagnostic) => diagnostic.code).join(', '),
      );
      await this.ctx.dialogs.alert({
        title: `“${name}” could not be shown`,
        message: `Blocks2Cpp found these problems in the restored project:\n${problemLines(result.diagnostics).join('\n')}\nYour unsaved work is kept and will be offered again the next time Blocks2Cpp starts.`,
      });
      return false;
    }
    return true;
  }

  /** Tells the user why `recovery_restore` failed; drops a snapshot that is gone from the offer. */
  private async reportRestoreFailure(snapshot: SnapshotInfo, error: unknown): Promise<void> {
    const code = errorCode(error);
    console.warn('A recovery snapshot could not be restored', code);
    const name = shownName(snapshot.projectName);
    const ipcError = ipcErrorOf(error);
    if (ipcError?.code === 'unknownSnapshot') {
      withoutSnapshot(this.model, snapshot.snapshotId);
      await this.ctx.dialogs.alert({
        title: 'Nothing was restored',
        message: `The unsaved work on “${name}” is no longer available. Another Blocks2Cpp window may have restored or discarded it.`,
      });
      void this.refresh();
      return;
    }
    const problem = ipcError === null ? null : documentProblem(ipcError);
    let message: string;
    if (problem !== null) {
      message = `The saved work on “${name}” is not a project Blocks2Cpp can open:\n${problem}\nYou can discard it.`;
    } else if (ipcError?.code === 'tooManyHandles') {
      message = 'Too many projects are open. Close a project and try again.';
    } else if (ipcError?.code === 'io') {
      message =
        ipcError.kind === 'permissionDenied'
          ? `Blocks2Cpp is not allowed to read the unsaved work on “${name}”.`
          : `The unsaved work on “${name}” could not be read. Try again later.`;
    } else {
      message = somethingWentWrong('the unsaved work was not restored', code);
    }
    await this.ctx.dialogs.alert({ title: 'Nothing was restored', message });
  }
}

/** What a failed `recovery_discard` means for the user. */
function describeDiscardError(error: unknown): string {
  const ipcError = ipcErrorOf(error);
  if (ipcError?.code === 'io') {
    return ipcError.kind === 'permissionDenied'
      ? 'Blocks2Cpp is not allowed to delete it.'
      : 'It could not be deleted. Try again later.';
  }
  return somethingWentWrong('the unsaved work was not discarded', errorCode(error));
}
