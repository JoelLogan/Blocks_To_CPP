/**
 * Restoring and discarding the recovery snapshots of earlier sessions (docs/spec/04-user-interface.md
 * §4.10, 05 §5.10, 08 §8.3.1).
 *
 * - **Restore** opens a snapshot as a project with unsaved changes, under a new handle. The
 *   compiler core is started first, so a core that cannot start leaves the snapshot untouched;
 *   then `recovery_restore` hands over the document, which goes through the core's loader like
 *   any other (`openDocumentInEditor`), with the trust state the backend decided. One project per
 *   window: an open project with unsaved changes is settled first (*Save*, *Don't save* or
 *   *Cancel*), and closed only once the restored one is shown.
 * - **Discard** deletes a snapshot after the user confirms; it cannot be undone.
 *
 * One operation runs at a time. Backend errors become sentences for the user; only bugs reject.
 * A restored project that cannot be shown keeps its handle (and with it the backend's snapshot of
 * it), so the work is offered again at the next start instead of being lost.
 */
import type { CoreWasm } from '@blocks2cpp/b2c-core-wasm';
import type {
  Handle,
  RecoveryRestoreResponse,
  SnapshotId,
  SnapshotInfo,
} from '@blocks2cpp/ipc-types';

import type { FeatureContext } from '../../app/features';
import type { ProjectState } from '../../app/store';
import {
  openDocumentInEditor,
  type OpenDocumentArgs,
  type OpenDocumentResult,
} from '../../editor/load';
import { appCoreHost } from '../../editor/preview/coreHost';
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
}

/** The answers of the unsaved-changes prompt. */
type UnsavedChoice = 'save' | 'discard' | 'cancel';

/** Restores and discards snapshots; see the module comment. */
export class RecoveryController {
  private readonly ctx: FeatureContext;
  private readonly model: RecoveryModel;
  private readonly openInEditor: NonNullable<RecoveryControllerOptions['openInEditor']>;
  private readonly startCore: () => Promise<CoreWasm>;
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
      const previous = await this.settleOpenProject();
      if (previous === 'cancel') {
        return false;
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
        return false;
      }

      let restored: RecoveryRestoreResponse;
      try {
        restored = await this.ctx.ipc.recoveryRestore({ snapshotId });
      } catch (error: unknown) {
        await this.reportRestoreFailure(snapshot, error);
        return false;
      }
      // The backend has a new project for it now and will not offer the snapshot again.
      withoutSnapshot(this.model, snapshotId);
      if (!(await this.show(snapshot, restored))) {
        return false;
      }
      if (previous !== null && previous.handle !== restored.handle) {
        await this.closeHandle(previous.handle);
      }
      return true;
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
   * Before a restore replaces the open project: resolves the project to close afterwards (or
   * `null` when none is open), or `cancel` when the user cancelled or its save did not happen.
   */
  private async settleOpenProject(): Promise<ProjectState | null | 'cancel'> {
    const project = this.ctx.store.getState().project;
    if (project?.dirty !== true) {
      return project;
    }
    const choice = await this.ctx.dialogs.choose<UnsavedChoice>({
      title: `Save changes to “${shownName(project.document.project.name)}”?`,
      message:
        "The restored project replaces the one that is open. If you don't save, your changes to it will be lost.",
      choices: [
        { id: 'save', label: 'Save', primary: true },
        { id: 'discard', label: "Don't save", destructive: true },
        { id: 'cancel', label: 'Cancel' },
      ],
      cancel: 'cancel',
    });
    switch (choice) {
      case 'cancel':
        return 'cancel';
      case 'discard':
        return project;
      case 'save':
        return (await this.saveOpenProject(project)) ? project : 'cancel';
    }
  }

  /** Saves the open project with the project feature's `project.save`; resolves whether it did. */
  private async saveOpenProject(project: ProjectState): Promise<boolean> {
    if (!this.ctx.commands.hasCommand('project.save')) {
      console.error('No project.save command is registered; the restore was not started');
      await this.ctx.dialogs.alert({
        title: 'Nothing was restored',
        message: somethingWentWrong('the open project could not be saved', 'noSaveCommand'),
      });
      return false;
    }
    try {
      await this.ctx.commands.runCommand('project.save');
    } catch (error: unknown) {
      console.error('Saving the open project before a restore failed', error);
    }
    // The save reports its own failures; whether it happened shows in the store.
    const now = this.ctx.store.getState().project;
    return now?.handle !== project.handle || !now.dirty;
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

  /** Closes the backend's handle, logging a failure (the handle is gone either way). */
  private async closeHandle(handle: Handle): Promise<void> {
    try {
      await this.ctx.ipc.projectClose({ handle });
    } catch (error: unknown) {
      console.warn('project_close failed', errorCode(error));
    }
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
