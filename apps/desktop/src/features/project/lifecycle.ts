/**
 * The project lifecycle (docs/spec/04-user-interface.md §4.10, 05 §5.2, §5.10): new, open,
 * recent projects, save, save as, close and quit, with the unsaved-changes prompt.
 *
 * - **One project per window.** Opening or creating another project first asks about unsaved
 *   changes; the current project is closed (`project_close`) only once the new one is shown, so a
 *   cancelled dialog or a file that does not load leaves it as it was.
 * - **Every document goes through the compiler core's loader** (`openDocumentInEditor`). A file
 *   the loader refuses shows its `B2C-E01xx` problems and nothing opens; its handle is closed.
 * - **Saving** writes what the editor shows: the canvas with the viewports captured
 *   (`EditorHandle.currentDocument`), `generator` naming this app and catalog, serialised by the
 *   core's canonical writer (an unchanged project saves byte for byte the same, F5). A project
 *   without a file is saved with *Save as…*; a file that changed on disk is never overwritten: the
 *   external-change feature takes over through the `project:changedOnDisk` event.
 * - **Closing the window** with unsaved changes: the backend sends `closeRequested`; the user
 *   chooses *Save*, *Don't save* or *Cancel*, then `app_quit` ends the app.
 *
 * Operations run one at a time, in the order they were asked for, so a close never overtakes a
 * save. Other features' operations on the open project take their place in the same queue
 * (./link.ts): restoring a recovery snapshot and reloading after an outside change. A save waits
 * for an autosave snapshot that is being written before it sends the project, so the backend never
 * gets a snapshot after the save that deleted it. Backend errors become messages for the user;
 * only bugs reject.
 */
import {
  type BdmDocument,
  type BdmGenerator,
  type CanonicalResult,
  CoreTrap,
  type CoreWasm,
  MAX_DOCUMENT_BYTES,
} from '@blocks2cpp/b2c-core-wasm';
import type { Handle, ProjectOpened, RecentEntry, RecentId, Template } from '@blocks2cpp/ipc-types';

import type { FeatureContext } from '../../app/features';
import type { ProjectState } from '../../app/store';
import {
  openDocumentInEditor,
  type OpenDocumentArgs,
  type OpenDocumentResult,
} from '../../editor/load';
import { appCoreHost } from '../../editor/preview/coreHost';
import { withGenerator, withSavedLayout } from './document';
import {
  describeOpenError,
  describeQuitError,
  describeSaveError,
  errorCode,
  ipcErrorOf,
  type LoadProblems,
  loadProblems,
} from './errors';
import type { ProjectQueue } from './link';
import { type LifecycleOperation, type ProjectModel, shownRecentEntries } from './model';
import { MAX_SHOWN_PATH_CHARS, shownName, shownText } from './text';

/** The answers of the unsaved-changes prompt. */
export type UnsavedChoice = 'save' | 'discard' | 'cancel';

/** How the templates are named in the app (04 §4.10). */
export const TEMPLATE_LABELS: Readonly<Record<Template, string>> = {
  empty: 'Empty project',
  helloWorld: 'Hello World',
};

/** What a lifecycle needs besides the feature context (replaceable in tests). */
export interface LifecycleOptions {
  /** Shows a document in the editor; `openDocumentInEditor` by default. */
  readonly openInEditor?: (
    ctx: FeatureContext,
    args: OpenDocumentArgs,
  ) => Promise<OpenDocumentResult>;
}

/** A document ready to be written, and what it was made from. */
interface PreparedSave {
  /** The document with the viewports captured and `generator` set. */
  readonly doc: BdmDocument;
  /** Its canonical text: what is sent and written. */
  readonly text: string;
  /** Its content hash. */
  readonly hash: string;
  /** The project's live canonical text when the save started, to notice edits made meanwhile. */
  readonly base: string;
}

/** A project's name for dialogs. */
function projectName(project: ProjectState): string {
  return shownName(project.document.project.name);
}

/** The problems of a file that did not load, as one block of text for an alert. */
function problemsText(problems: LoadProblems): string {
  const lines = problems.problems.map((problem) => `${problem.code}: ${problem.message}`);
  if (problems.omitted > 0) {
    lines.push(
      `…and ${String(problems.omitted)} more ${problems.omitted === 1 ? 'problem' : 'problems'}.`,
    );
  }
  return lines.join('\n');
}

/** The project lifecycle of one window; see the module comment. */
export class ProjectLifecycle implements ProjectQueue {
  private readonly ctx: FeatureContext;
  private readonly model: ProjectModel;
  private readonly openInEditor: NonNullable<LifecycleOptions['openInEditor']>;
  /** The end of the queue of operations. */
  private tail: Promise<unknown> = Promise.resolve();
  /** How many operations are queued or running. */
  private pending = 0;
  /** A quit in progress, so a second close request does not ask twice. */
  private quitting: Promise<boolean> | null = null;
  /** The latest `recent_list` request; older answers are dropped. */
  private recentRequest = 0;
  /** How many saves are under way (a Save as inside a save counts twice). */
  private saving = 0;
  /** What every save waits for before it sends the project ({@link addSaveBarrier}). */
  private readonly saveBarriers = new Set<() => Promise<void>>();
  private disposed = false;

  constructor(ctx: FeatureContext, model: ProjectModel, options: LifecycleOptions = {}) {
    this.ctx = ctx;
    this.model = model;
    this.openInEditor = options.openInEditor ?? openDocumentInEditor;
  }

  /** Stops: later operations do nothing and resolve `false`. */
  dispose(): void {
    this.disposed = true;
  }

  // ---- Operations ------------------------------------------------------------------------------

  /** Creates a project from `template` (after the unsaved-changes prompt) and shows it. */
  newProject(template: Template): Promise<boolean> {
    return this.exclusive('new', async () => {
      if (!(await this.settleUnsaved())) {
        return false;
      }
      return this.createNow(template);
    });
  }

  /** `project.new`: after the unsaved-changes prompt, asks for the template, then creates it. */
  chooseAndCreate(): Promise<boolean> {
    return this.exclusive('new', async () => {
      if (!(await this.settleUnsaved())) {
        return false;
      }
      const choice = await this.ctx.dialogs.choose<Template | 'cancel'>({
        title: 'New project',
        message:
          'Start from an empty project, or from Hello World, a small program that prints a greeting.',
        choices: [
          { id: 'empty', label: TEMPLATE_LABELS.empty, primary: true },
          { id: 'helloWorld', label: TEMPLATE_LABELS.helloWorld },
          { id: 'cancel', label: 'Cancel' },
        ],
        cancel: 'cancel',
      });
      if (choice === 'cancel') {
        return false;
      }
      return this.createNow(choice);
    });
  }

  /** `project.open`: after the unsaved-changes prompt, the backend's native open dialog. */
  open(): Promise<boolean> {
    return this.exclusive('open', async () => {
      if (!(await this.settleUnsaved())) {
        return false;
      }
      let response;
      try {
        response = await this.ctx.ipc.projectOpenDialog();
      } catch (error: unknown) {
        await this.reportOpenFailure(error, null);
        return false;
      }
      if (response.status === 'cancelled') {
        return false;
      }
      return this.showOpened(response);
    });
  }

  /** Opens a recent-list entry (after the unsaved-changes prompt). */
  openRecent(entry: RecentEntry): Promise<boolean> {
    return this.exclusive('open', async () => {
      if (!(await this.settleUnsaved())) {
        return false;
      }
      let opened;
      try {
        opened = await this.ctx.ipc.projectOpenRecent({ recentId: entry.recentId });
      } catch (error: unknown) {
        await this.reportOpenFailure(error, entry);
        return false;
      }
      return this.showOpened(opened);
    });
  }

  /**
   * `project.save` (`Ctrl+S`): writes the project to its file, or asks for one with *Save as…*
   * when it has none. Resolves whether it was saved.
   */
  save(): Promise<boolean> {
    return this.exclusive('save', () => this.saveNow());
  }

  /** `project.saveAs`: writes the project to a file the user picks. Resolves whether it was saved. */
  saveAs(): Promise<boolean> {
    return this.exclusive('saveAs', async () => {
      const project = this.ctx.store.getState().project;
      return project === null ? false : this.saveAsNow(project, null);
    });
  }

  /** `project.close`: after the unsaved-changes prompt, closes the project and shows the start page. */
  close(): Promise<boolean> {
    return this.exclusive('close', async () => {
      const project = this.ctx.store.getState().project;
      if (project === null) {
        this.ctx.store.getState().actions.setUi({ screen: 'start' });
        return true;
      }
      if (!(await this.settleUnsaved())) {
        return false;
      }
      await this.forget(project.handle);
      void this.refreshRecent();
      return true;
    });
  }

  /**
   * The window is closing with unsaved changes (`closeRequested`): asks *Save*, *Don't save* or
   * *Cancel*, then quits with `app_quit`. A second request while the first is being answered
   * returns the same promise. Resolves whether the app was told to quit.
   */
  quit(): Promise<boolean> {
    this.quitting ??= this.exclusive('quit', async () => {
      const project = this.ctx.store.getState().project;
      if (project?.dirty === true) {
        const choice = await this.askUnsaved(project);
        if (choice === 'cancel') {
          return false;
        }
        if (choice === 'save' && !(await this.saveNow())) {
          return false;
        }
        if (choice === 'discard') {
          // Closing deletes the recovery snapshot of changes the user chose to drop (05 §5.10).
          await this.forget(project.handle);
        }
      }
      try {
        await this.ctx.ipc.appQuit();
        return true;
      } catch (error: unknown) {
        console.error('app_quit failed', errorCode(error));
        await this.ctx.dialogs.alert({
          title: 'Blocks2Cpp could not quit',
          message: describeQuitError(error),
        });
        return false;
      }
    }).finally(() => {
      this.quitting = null;
    });
    return this.quitting;
  }

  /** Reads the recent-projects list again (the start page shows it). */
  async refreshRecent(): Promise<void> {
    const request = ++this.recentRequest;
    this.model.setState((state) => ({
      recent: {
        status: state.recent.status === 'ready' ? 'ready' : 'loading',
        entries: state.recent.entries,
      },
    }));
    try {
      const response = await this.ctx.ipc.recentList();
      if (request === this.recentRequest) {
        this.model.setState({
          recent: { status: 'ready', entries: shownRecentEntries(response.entries) },
        });
      }
    } catch (error: unknown) {
      if (request === this.recentRequest) {
        console.warn('Could not read the recent projects', errorCode(error));
        this.model.setState({ recent: { status: 'failed', entries: [] } });
      }
    }
  }

  /** Removes an entry from the recent-projects list (the file is not touched). */
  async removeRecent(recentId: RecentId): Promise<void> {
    try {
      await this.ctx.ipc.recentRemove({ recentId });
    } catch (error: unknown) {
      // `unknownRecent`: it is gone already, which is what was asked.
      if (ipcErrorOf(error)?.code !== 'unknownRecent') {
        console.warn('Could not remove a recent project', errorCode(error));
        await this.ctx.dialogs.alert({
          title: 'The entry was not removed',
          message: 'The list of recent projects could not be changed. Try again later.',
        });
      }
    }
    await this.refreshRecent();
  }

  /** Hides the start page's report of a file that did not load. */
  dismissLoadFailure(): void {
    this.model.setState({ loadFailure: null });
  }

  // ---- For other features (./link.ts) ----------------------------------------------------------

  /**
   * Replaces the open project with the one `show` shows (a restored recovery snapshot), as the
   * operation `restore`: after the unsaved-changes prompt, `show` runs and resolves the handle it
   * showed, or `null` when it showed none (having told the user why); the replaced project is
   * closed only once the new one is shown. Resolves whether it was shown.
   */
  replaceWith(show: () => Promise<Handle | null>): Promise<boolean> {
    return this.exclusive('restore', async () => {
      if (!(await this.settleUnsaved())) {
        return false;
      }
      const previous = this.ctx.store.getState().project;
      const shown = await show();
      if (shown === null) {
        return false;
      }
      this.model.setState({ loadFailure: null });
      if (previous !== null && previous.handle !== shown) {
        await this.closeHandle(previous.handle);
      }
      return true;
    });
  }

  /**
   * Runs `reload` (`project_reload` and showing the file again after an outside change) as the
   * operation `reload`, so it never overlaps a save or another open. Resolves its result, or
   * `false` once the lifecycle is stopped.
   */
  reloadWith(reload: () => Promise<boolean>): Promise<boolean> {
    return this.exclusive('reload', reload);
  }

  /** Whether a save (Save, Save as, or the save of an unsaved-changes prompt) is under way. */
  isSaving(): boolean {
    return this.saving > 0;
  }

  /**
   * Adds `wait`, which every save awaits right before it sends the project to the backend
   * (autosave: its snapshot write in progress). Returns the function that removes it again.
   */
  addSaveBarrier(wait: () => Promise<void>): () => void {
    const entry = () => wait();
    this.saveBarriers.add(entry);
    return () => {
      this.saveBarriers.delete(entry);
    };
  }

  // ---- The queue -------------------------------------------------------------------------------

  /** Runs `task` after every operation asked for before it; `busy` names the running one. */
  private exclusive(operation: LifecycleOperation, task: () => Promise<boolean>): Promise<boolean> {
    const run = async (): Promise<boolean> => {
      if (this.disposed) {
        return false;
      }
      this.model.setState({ busy: operation });
      return task();
    };
    this.pending += 1;
    const result = this.tail.then(run, run);
    const done = (): void => {
      this.pending -= 1;
      if (this.pending === 0) {
        this.model.setState({ busy: null });
      }
    };
    this.tail = result.then(done, done);
    return result;
  }

  // ---- Unsaved changes -------------------------------------------------------------------------

  /** Asks *Save*, *Don't save* or *Cancel* about the project's unsaved changes. */
  private askUnsaved(project: ProjectState): Promise<UnsavedChoice> {
    return this.ctx.dialogs.choose<UnsavedChoice>({
      title: `Save changes to “${projectName(project)}”?`,
      message: "If you don't save, your changes will be lost.",
      choices: [
        { id: 'save', label: 'Save', primary: true },
        { id: 'discard', label: "Don't save", destructive: true },
        { id: 'cancel', label: 'Cancel' },
      ],
      cancel: 'cancel',
    });
  }

  /**
   * Before the project is replaced or closed: resolves `true` when it may go (no unsaved changes,
   * saved, or the user chose *Don't save*), `false` when the user cancelled or the save failed.
   */
  private async settleUnsaved(): Promise<boolean> {
    const project = this.ctx.store.getState().project;
    if (project?.dirty !== true) {
      return true;
    }
    switch (await this.askUnsaved(project)) {
      case 'save':
        return this.saveNow();
      case 'discard':
        return true;
      case 'cancel':
        return false;
    }
  }

  // ---- Opening ---------------------------------------------------------------------------------

  /** `project_new`, then shows the new project. */
  private async createNow(template: Template): Promise<boolean> {
    let created;
    try {
      created = await this.ctx.ipc.projectNew({ template });
    } catch (error: unknown) {
      await this.reportOpenFailure(error, null);
      return false;
    }
    // An untouched new project has no unsaved changes: its template text counts as saved.
    return this.show(
      {
        handle: created.handle,
        documentText: created.document,
        trust: created.trust,
        fileName: null,
        migratedFrom: null,
        savedText: created.document,
      },
      TEMPLATE_LABELS[template],
    );
  }

  /** Shows a project the backend opened from its file. */
  private showOpened(opened: ProjectOpened): Promise<boolean> {
    return this.show(
      {
        handle: opened.handle,
        documentText: opened.document,
        trust: opened.trust,
        fileName: opened.fileName,
        migratedFrom: opened.migratedFrom,
        savedText: opened.document,
      },
      shownName(opened.fileName),
    );
  }

  /**
   * Loads the backend's document with the compiler core and shows it, then closes the project it
   * replaces. A document the core refuses is reported and its new handle closed; the previous
   * project stays open.
   */
  private async show(args: OpenDocumentArgs, name: string): Promise<boolean> {
    const previous = this.ctx.store.getState().project;
    let result: OpenDocumentResult;
    try {
      result = await this.openInEditor(this.ctx, args);
    } catch (error: unknown) {
      console.error('The compiler core could not open a project', error);
      await this.closeHandle(args.handle);
      await this.ctx.dialogs.alert({
        title: 'The project could not be opened',
        message:
          'The Blocks2Cpp compiler core could not start, so the project was not opened. Restart Blocks2Cpp and try again.',
      });
      return false;
    }
    if (!result.ok) {
      console.warn(
        'The compiler core refused a project',
        result.diagnostics.map((diagnostic) => diagnostic.code).join(', '),
      );
      await this.closeHandle(args.handle);
      await this.showLoadFailure(name, loadProblems(result.diagnostics));
      return false;
    }
    this.model.setState({ loadFailure: null });
    if (previous !== null && previous.handle !== args.handle) {
      await this.closeHandle(previous.handle);
    }
    void this.refreshRecent();
    return true;
  }

  /**
   * Shows the problems of a file that did not load: on the start page when no project is open,
   * else in a dialog over the project that stays open.
   */
  private async showLoadFailure(name: string | null, problems: LoadProblems): Promise<void> {
    const { project, actions } = this.ctx.store.getState();
    if (project === null) {
      this.model.setState({ loadFailure: { name, ...problems } });
      actions.setUi({ screen: 'start' });
      return;
    }
    await this.ctx.dialogs.alert({
      title: name === null ? 'The project could not be opened' : `“${name}” could not be opened`,
      message: problemsText(problems),
    });
  }

  /** Tells the user why an open failed; offers to drop a recent entry whose file is gone. */
  private async reportOpenFailure(error: unknown, entry: RecentEntry | null): Promise<void> {
    console.warn('A project could not be opened', errorCode(error));
    const failure = describeOpenError(error);
    switch (failure.kind) {
      case 'load':
        await this.showLoadFailure(entry === null ? null : shownName(entry.projectName), failure);
        break;
      case 'notFound':
        if (entry === null) {
          await this.ctx.dialogs.alert({
            title: 'The project could not be opened',
            message: 'The file no longer exists.',
          });
        } else {
          await this.offerRemoval(entry);
        }
        break;
      case 'unknownRecent':
        // The list was out of date; show the current one.
        await this.refreshRecent();
        break;
      case 'message':
        await this.ctx.dialogs.alert({
          title: 'The project could not be opened',
          message: failure.message,
        });
        break;
    }
  }

  /** A recent entry's file is gone: says so and offers to remove the entry (04 §4.10). */
  private async offerRemoval(entry: RecentEntry): Promise<void> {
    const remove = await this.ctx.dialogs.confirm({
      title: `“${shownName(entry.projectName)}” was not found`,
      message: `The project file is no longer at ${shownText(entry.displayPath, MAX_SHOWN_PATH_CHARS)}. It may have been moved, renamed or deleted. Remove it from the recent projects?`,
      confirmLabel: 'Remove from list',
      cancelLabel: 'Keep',
    });
    if (remove) {
      await this.removeRecent(entry.recentId);
    }
  }

  // ---- Saving ----------------------------------------------------------------------------------

  /** Runs `task`, a save, counted in {@link isSaving}. */
  private async whileSaving(task: () => Promise<boolean>): Promise<boolean> {
    this.saving += 1;
    try {
      return await task();
    } finally {
      this.saving -= 1;
    }
  }

  /** Waits for every save barrier (a snapshot being written); a failing one is only logged. */
  private async passSaveBarriers(): Promise<void> {
    await Promise.all(
      [...this.saveBarriers].map((wait) =>
        Promise.resolve()
          .then(wait)
          .catch((error: unknown) => {
            console.warn('A save waited in vain for another write', error);
          }),
      ),
    );
  }

  /** Saves to the project's file; see {@link save}. */
  private saveNow(): Promise<boolean> {
    return this.whileSaving(() => this.saveToFile());
  }

  /** {@link saveNow} itself. */
  private async saveToFile(): Promise<boolean> {
    const project = this.ctx.store.getState().project;
    if (project === null) {
      return false;
    }
    if (project.fileName === null) {
      return this.saveAsNow(project, null);
    }
    const prepared = await this.prepare(project);
    if (prepared === null) {
      return false;
    }
    await this.passSaveBarriers();
    let response;
    try {
      response = await this.ctx.ipc.projectSave({
        handle: project.handle,
        document: prepared.text,
      });
    } catch (error: unknown) {
      switch (ipcErrorOf(error)?.code) {
        case 'noPath':
          return this.saveAsNow(project, prepared);
        case 'changedOnDisk':
          // Never overwritten (05 §5.10): the external-change feature offers Reload or Keep mine.
          console.warn('The project file changed on disk; it was not saved');
          this.ctx.events.emit({ kind: 'project:changedOnDisk', handle: project.handle });
          return false;
        default:
          return this.reportSaveFailure(error);
      }
    }
    this.adoptSaved(project.handle, prepared, response.savedAt, null);
    if (project.trust.source === 'createdHere') {
      // The first save records trust for the file (08 §8.3).
      await this.refreshTrust(project.handle);
    }
    return true;
  }

  /** Saves under a name the user picks in the backend's native dialog. */
  private saveAsNow(project: ProjectState, ready: PreparedSave | null): Promise<boolean> {
    return this.whileSaving(() => this.saveToChosenFile(project, ready));
  }

  /** {@link saveAsNow} itself. */
  private async saveToChosenFile(
    project: ProjectState,
    ready: PreparedSave | null,
  ): Promise<boolean> {
    const prepared = ready ?? (await this.prepare(project));
    if (prepared === null) {
      return false;
    }
    await this.passSaveBarriers();
    let response;
    try {
      response = await this.ctx.ipc.projectSaveAsDialog({
        handle: project.handle,
        document: prepared.text,
      });
    } catch (error: unknown) {
      return this.reportSaveFailure(error);
    }
    if (response.status === 'cancelled') {
      return false;
    }
    this.adoptSaved(project.handle, prepared, response.savedAt, response.fileName);
    // Save As records trust for the new file, or keeps a restricted project restricted (08 §8.3).
    await this.refreshTrust(project.handle);
    void this.refreshRecent();
    return true;
  }

  /** Tells the user why a save failed; resolves `false`. */
  private async reportSaveFailure(error: unknown): Promise<false> {
    console.warn('The project was not saved', errorCode(error));
    await this.ctx.dialogs.alert({
      title: 'The project was not saved',
      message: describeSaveError(error),
    });
    return false;
  }

  /**
   * The document to write: what the editor shows (with the viewports), with `generator` set,
   * serialised by the core's canonical writer and checked against the size limit. `null` after
   * telling the user why it cannot be saved.
   */
  private async prepare(project: ProjectState): Promise<PreparedSave | null> {
    let shown: BdmDocument;
    const editor = this.ctx.editor();
    try {
      shown = editor === null ? project.document : editor.currentDocument();
    } catch (error: unknown) {
      console.error('The editor could not read the blocks to save', error);
      return this.refuseSave(
        'The blocks could not be read from the editor, so the project was not saved. This is a bug in Blocks2Cpp.',
      );
    }

    let doc: BdmDocument;
    let canonical: CanonicalResult;
    try {
      ({ doc, canonical } = await this.withCore((core) => {
        const generated = withGenerator(shown, this.generator(core));
        return { doc: generated, canonical: core.canonical(JSON.stringify(generated)) };
      }));
    } catch (error: unknown) {
      console.error('The compiler core could not prepare a save', error);
      return this.refuseSave(
        'The Blocks2Cpp compiler core could not start, so the project was not saved. Your changes are still in the editor.',
      );
    }
    if (!canonical.ok) {
      const codes = canonical.diagnostics.map((diagnostic) => diagnostic.code).join(', ');
      console.error(`The editor's document does not load (${codes}); this is a bug`);
      return this.refuseSave(
        `The project was not saved, because the editor made a document Blocks2Cpp cannot read back. This is a bug in Blocks2Cpp. (Problems: ${shownText(codes, 200)})`,
      );
    }
    if (new TextEncoder().encode(canonical.text).length > MAX_DOCUMENT_BYTES) {
      return this.refuseSave(
        `The project was not saved: a project file can be at most ${String(MAX_DOCUMENT_BYTES / (1024 * 1024))} MiB.`,
      );
    }
    return { doc, text: canonical.text, hash: canonical.hash, base: project.canonicalText };
  }

  /** Shows why a save cannot start; resolves `null`. */
  private async refuseSave(message: string): Promise<null> {
    await this.ctx.dialogs.alert({ title: 'The project was not saved', message });
    return null;
  }

  /** `generator` for a save: this app's version and the catalog the core was built with. */
  private generator(core: CoreWasm): BdmGenerator {
    const version = core.version();
    return {
      app: this.ctx.store.getState().appInfo?.appVersion ?? version.app,
      catalog: version.catalog,
    };
  }

  /**
   * Makes a written document the project's saved state. When the project did not change while it
   * was being saved, the written document becomes the project's; otherwise the newer edits stay
   * and only the saved layout (`generator`, viewports) is taken over, so they still count as
   * unsaved.
   */
  private adoptSaved(
    handle: Handle,
    prepared: PreparedSave,
    savedAt: string,
    fileName: string | null,
  ): void {
    const { project, actions } = this.ctx.store.getState();
    if (project?.handle !== handle) {
      return;
    }
    const named = fileName === null ? {} : { fileName };
    if (project.canonicalText === prepared.base) {
      actions.updateProject({
        ...named,
        document: prepared.doc,
        canonicalText: prepared.text,
        contentHash: prepared.hash,
        savedCanonicalText: prepared.text,
        savedAt,
        dirty: false,
        migratedFrom: null,
      });
      return;
    }
    const document = withSavedLayout(project.document, prepared.doc);
    let canonical: CanonicalResult | null = null;
    try {
      canonical = this.ctx.core()?.canonical(JSON.stringify(document)) ?? null;
    } catch (error: unknown) {
      console.warn('The compiler core could not check the saved state', error);
    }
    actions.updateProject({
      ...named,
      document,
      ...(canonical?.ok === true
        ? {
            canonicalText: canonical.text,
            contentHash: canonical.hash,
            dirty: canonical.text !== prepared.text,
          }
        : { dirty: true }),
      savedCanonicalText: prepared.text,
      savedAt,
      migratedFrom: null,
    });
  }

  /** Asks the backend for the project's trust again (it changes when a save records it). */
  private async refreshTrust(handle: Handle): Promise<void> {
    try {
      const { trust } = await this.ctx.ipc.trustGet({ handle });
      const { project, actions } = this.ctx.store.getState();
      if (project?.handle === handle) {
        actions.updateProject({ trust });
      }
    } catch (error: unknown) {
      console.warn('Could not read the trust state after saving', errorCode(error));
    }
  }

  // ---- Closing ---------------------------------------------------------------------------------

  /** Closes the backend's handle, logging a failure (the handle is gone either way). */
  private async closeHandle(handle: Handle): Promise<void> {
    try {
      await this.ctx.ipc.projectClose({ handle });
    } catch (error: unknown) {
      console.warn('project_close failed', errorCode(error));
    }
  }

  /** Closes the project's handle, clears it from the window and shows the start page. */
  private async forget(handle: Handle): Promise<void> {
    await this.closeHandle(handle);
    const { project, actions } = this.ctx.store.getState();
    if (project?.handle === handle) {
      actions.setProject(null);
    }
    actions.setUi({ screen: 'start' });
  }

  // ---- The compiler core -----------------------------------------------------------------------

  /** Runs `action` on the core, once more on a fresh instance when the first one traps. */
  private async withCore<T>(action: (core: CoreWasm) => T): Promise<T> {
    const host = appCoreHost();
    const core = this.ctx.core() ?? (await host.start());
    try {
      return action(core);
    } catch (error: unknown) {
      if (!(error instanceof CoreTrap)) {
        throw error;
      }
      console.error('The compiler core stopped while saving; starting a new one', error);
      return action(await host.restart());
    }
  }
}
