/**
 * The project lifecycle against a fake backend and the real compiler core: new from each
 * template, open (cancelled, ok, refused), recent projects, saving (byte-identical, generator,
 * save-as fallback, changed on disk, edits made while saving), the unsaved-changes prompts of
 * open, new, close and the window's close request, and the order of operations.
 *
 * Without a build of the compiler core these tests are skipped, unless B2C_REQUIRE_WASM is set
 * (as in CI), in which case a missing build fails them.
 */
import { CoreError, type CoreWasm } from '@blocks2cpp/b2c-core-wasm';
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';

import { useAppStore } from '../../app/store';
import { projectFixture } from '../../app/testing/fixtures';
import { NEWER_FORMAT_CODE } from './errors';
import { ProjectLifecycle } from './lifecycle';
import { createProjectModel } from './model';
import {
  answerChoice,
  answerConfirm,
  closeAlert,
  CORE_BUILT,
  CREATED_HERE,
  EMPTY_TEXT,
  HANDLE_A,
  HANDLE_B,
  HANDLE_C,
  HELLO_TEXT,
  type Harness,
  installHarness,
  ipcFailure,
  loaderDiagnostic,
  nextDialog,
  opened,
  realCore,
  recentEntry,
  TRUSTED,
} from './testing';

let core: CoreWasm;
let harness: Harness;

beforeAll(async () => {
  if (CORE_BUILT) {
    core = await realCore();
  }
});

beforeEach(() => {
  harness = installHarness(CORE_BUILT ? core : null);
  vi.spyOn(console, 'warn').mockImplementation(() => undefined);
});

afterEach(() => {
  harness.dispose();
});

/** The project in the store, which must exist. */
function project() {
  const current = useAppStore.getState().project;
  if (current === null) {
    throw new Error('no project is open');
  }
  return current;
}

/**
 * Opens `text` as a saved file under `handle` through the open dialog (the test then sees only its
 * own `project_open_dialog` calls).
 */
async function openFile(text: string, handle = HANDLE_B, fileName = 'game.b2c'): Promise<void> {
  harness.ipc.projectOpenDialog.mockResolvedValueOnce({
    status: 'ok',
    ...opened(text, { handle, fileName }),
  });
  expect(await harness.feature.lifecycle.open()).toBe(true);
  harness.ipc.projectOpenDialog.mockClear();
}

/** Marks the open project as changed (as the live preview would after an edit). */
function makeDirty(): void {
  useAppStore.getState().actions.updateProject({ dirty: true });
}

/** `text` with one key written twice, which the loader refuses (B2C-E0105). */
function withDuplicateKey(text: string): string {
  return text.replace('"formatVersion": 1,', '"formatVersion": 1,\n  "formatVersion": 1,');
}

describe.skipIf(!CORE_BUILT)('the project lifecycle', () => {
  describe('new projects', () => {
    it.each([
      ['empty', EMPTY_TEXT, 'My Project'],
      ['helloWorld', HELLO_TEXT, 'Hello World'],
    ] as const)('creates one from the %s template', async (template, text, name) => {
      harness.ipc.projectNew.mockResolvedValueOnce({
        handle: HANDLE_A,
        document: text,
        trust: CREATED_HERE,
      });

      expect(await harness.feature.lifecycle.newProject(template)).toBe(true);

      expect(harness.ipc.projectNew).toHaveBeenCalledWith({ template });
      const shown = project();
      expect(shown.handle).toBe(HANDLE_A);
      expect(shown.document.project.name).toBe(name);
      expect(shown.fileName).toBeNull();
      expect(shown.trust).toEqual(CREATED_HERE);
      // An untouched new project has nothing to save.
      expect(shown.dirty).toBe(false);
      expect(shown.savedAt).toBeNull();
      expect(useAppStore.getState().ui.screen).toBe('editor');
      expect(harness.ipc.projectClose).not.toHaveBeenCalled();
    });

    it('asks for the template from the project.new command', async () => {
      harness.ipc.projectNew.mockResolvedValueOnce({
        handle: HANDLE_A,
        document: HELLO_TEXT,
        trust: CREATED_HERE,
      });
      const created = harness.ctx.commands.runCommand('project.new');
      const request = await answerChoice(harness.dialogs, 'helloWorld');
      await created;
      expect(request.kind === 'choice' && request.options.choices.map((c) => c.label)).toEqual([
        'Empty project',
        'Hello World',
        'Cancel',
      ]);
      expect(harness.ipc.projectNew).toHaveBeenCalledWith({ template: 'helloWorld' });
      expect(project().handle).toBe(HANDLE_A);
    });

    it('does nothing when the template choice is cancelled', async () => {
      const created = harness.feature.lifecycle.chooseAndCreate();
      await answerChoice(harness.dialogs, 'cancel');
      expect(await created).toBe(false);
      expect(harness.ipc.projectNew).not.toHaveBeenCalled();
    });

    it('closes the previous project once the new one is shown', async () => {
      await openFile(HELLO_TEXT, HANDLE_A);
      harness.ipc.projectNew.mockResolvedValueOnce({
        handle: HANDLE_C,
        document: EMPTY_TEXT,
        trust: CREATED_HERE,
      });
      expect(await harness.feature.lifecycle.newProject('empty')).toBe(true);
      expect(harness.ipc.projectClose).toHaveBeenCalledWith({ handle: HANDLE_A });
      expect(project().handle).toBe(HANDLE_C);
    });

    it('reports a backend failure and keeps the window as it was', async () => {
      harness.ipc.projectNew.mockRejectedValueOnce(ipcFailure({ code: 'tooManyHandles' }));
      const created = harness.feature.lifecycle.newProject('empty');
      const alert = await closeAlert(harness.dialogs);
      expect(await created).toBe(false);
      expect(alert.options.message).toContain('Too many projects are open');
      expect(useAppStore.getState().project).toBeNull();
    });
  });

  describe('opening', () => {
    it('does nothing when the open dialog is cancelled', async () => {
      await openFile(HELLO_TEXT, HANDLE_A);
      harness.ipc.projectOpenDialog.mockResolvedValueOnce({ status: 'cancelled' });
      expect(await harness.feature.lifecycle.open()).toBe(false);
      expect(project().handle).toBe(HANDLE_A);
      expect(harness.ipc.projectClose).not.toHaveBeenCalled();
      expect(harness.dialogs.store.getState().queue).toHaveLength(0);
    });

    it('shows an opened file, unchanged and saved, and closes the previous project', async () => {
      await openFile(EMPTY_TEXT, HANDLE_A, 'first.b2c');
      await openFile(HELLO_TEXT, HANDLE_B, 'hello.b2c');
      const shown = project();
      expect(shown.handle).toBe(HANDLE_B);
      expect(shown.fileName).toBe('hello.b2c');
      expect(shown.canonicalText).toBe(HELLO_TEXT);
      expect(shown.savedCanonicalText).toBe(HELLO_TEXT);
      expect(shown.dirty).toBe(false);
      expect(harness.ipc.projectClose).toHaveBeenCalledExactlyOnceWith({ handle: HANDLE_A });
      expect(useAppStore.getState().ui.screen).toBe('editor');
      await vi.waitFor(() => {
        expect(harness.ipc.recentList).toHaveBeenCalled();
      });
    });

    it('keeps the migrated-from version of an older file', async () => {
      harness.ipc.projectOpenDialog.mockResolvedValueOnce({
        status: 'ok',
        ...opened(HELLO_TEXT, { migratedFrom: 0 }),
      });
      await harness.feature.lifecycle.open();
      expect(project().migratedFrom).toBe(0);
      expect(project().dirty).toBe(false);
    });

    it('shows E0108 for a project made with a newer version, and opens nothing', async () => {
      harness.ipc.projectOpenDialog.mockRejectedValueOnce(
        ipcFailure({ code: 'newerFormat', needs: '0.3.0' }),
      );
      expect(await harness.feature.lifecycle.open()).toBe(false);
      const failure = harness.feature.model.getState().loadFailure;
      expect(failure?.problems).toEqual([
        {
          code: NEWER_FORMAT_CODE,
          message:
            'This project was made with a newer version of Blocks2Cpp (needs ≥ 0.3.0). Update Blocks2Cpp to open it.',
        },
      ]);
      expect(useAppStore.getState().project).toBeNull();
      expect(useAppStore.getState().ui.screen).toBe('start');
    });

    it("shows the loader's problems (E0105) the backend found", async () => {
      harness.ipc.projectOpenDialog.mockRejectedValueOnce(
        ipcFailure({
          code: 'invalidDocument',
          diagnostics: [
            loaderDiagnostic(
              'B2C-E0105',
              'The key "format" appears twice in the same object (line 4, column 3).',
            ),
          ],
        }),
      );
      expect(await harness.feature.lifecycle.open()).toBe(false);
      const failure = harness.feature.model.getState().loadFailure;
      expect(failure?.problems.map((problem) => problem.code)).toEqual(['B2C-E0105']);
      expect(failure?.name).toBeNull();
    });

    it('refuses a document the compiler core does not load, closing its new handle', async () => {
      await openFile(EMPTY_TEXT, HANDLE_A);
      harness.ipc.projectOpenDialog.mockResolvedValueOnce({
        status: 'ok',
        ...opened(withDuplicateKey(HELLO_TEXT), { handle: HANDLE_B, fileName: 'bad.b2c' }),
      });
      const opening = harness.feature.lifecycle.open();
      // A project is open, so the problems are shown in a dialog over it.
      const alert = await closeAlert(harness.dialogs);
      expect(await opening).toBe(false);
      expect(alert.options.title).toBe('“bad.b2c” could not be opened');
      expect(alert.options.message).toMatch(/^B2C-E0105: /);
      expect(harness.ipc.projectClose).toHaveBeenCalledExactlyOnceWith({ handle: HANDLE_B });
      expect(project().handle).toBe(HANDLE_A);
    });

    it('lists at most 20 problems and counts the rest', async () => {
      const diagnostics = Array.from({ length: 25 }, (_, index) =>
        loaderDiagnostic('B2C-E0110', `Unknown key number ${String(index)}.`),
      );
      harness.ipc.projectOpenDialog.mockRejectedValueOnce(
        ipcFailure({ code: 'invalidDocument', diagnostics }),
      );
      await harness.feature.lifecycle.open();
      const failure = harness.feature.model.getState().loadFailure;
      expect(failure?.problems).toHaveLength(20);
      expect(failure?.omitted).toBe(5);
    });

    it('asks about unsaved changes first: Cancel keeps everything as it was', async () => {
      await openFile(EMPTY_TEXT, HANDLE_A);
      makeDirty();
      const opening = harness.feature.lifecycle.open();
      const prompt = await answerChoice(harness.dialogs, 'cancel');
      expect(await opening).toBe(false);
      expect(prompt.options.title).toBe('Save changes to “My Project”?');
      expect(harness.ipc.projectOpenDialog).not.toHaveBeenCalled();
      expect(project().dirty).toBe(true);
    });

    it("asks about unsaved changes first: Don't save opens the other project", async () => {
      await openFile(EMPTY_TEXT, HANDLE_A);
      makeDirty();
      harness.ipc.projectOpenDialog.mockResolvedValueOnce({
        status: 'ok',
        ...opened(HELLO_TEXT, { handle: HANDLE_B }),
      });
      const opening = harness.feature.lifecycle.open();
      await answerChoice(harness.dialogs, 'discard');
      expect(await opening).toBe(true);
      expect(harness.ipc.projectSave).not.toHaveBeenCalled();
      expect(harness.ipc.projectClose).toHaveBeenCalledWith({ handle: HANDLE_A });
      expect(project().handle).toBe(HANDLE_B);
    });

    it('asks about unsaved changes first: Save saves, then opens', async () => {
      await openFile(EMPTY_TEXT, HANDLE_A);
      makeDirty();
      harness.ipc.projectSave.mockResolvedValueOnce({
        savedAt: '2026-10-05T10:42:00Z',
        hash: 'f'.repeat(64),
      });
      harness.ipc.projectOpenDialog.mockResolvedValueOnce({ status: 'cancelled' });
      const opening = harness.feature.lifecycle.open();
      await answerChoice(harness.dialogs, 'save');
      expect(await opening).toBe(false);
      expect(harness.ipc.projectSave).toHaveBeenCalledOnce();
      expect(harness.ipc.projectOpenDialog).toHaveBeenCalledOnce();
      expect(project().dirty).toBe(false);
    });

    it('does not open when saving first fails', async () => {
      await openFile(EMPTY_TEXT, HANDLE_A);
      makeDirty();
      harness.ipc.projectSave.mockRejectedValueOnce(
        ipcFailure({ code: 'io', kind: 'permissionDenied' }),
      );
      const opening = harness.feature.lifecycle.open();
      await answerChoice(harness.dialogs, 'save');
      await closeAlert(harness.dialogs);
      expect(await opening).toBe(false);
      expect(harness.ipc.projectOpenDialog).not.toHaveBeenCalled();
    });

    it('reports other open failures in a dialog', async () => {
      harness.ipc.projectOpenDialog.mockRejectedValueOnce(ipcFailure({ code: 'busy' }));
      const opening = harness.feature.lifecycle.open();
      const alert = await closeAlert(harness.dialogs);
      expect(await opening).toBe(false);
      expect(alert.options.message).toBe('Another dialog is already open. Close it first.');
    });
  });

  describe('recent projects', () => {
    it('reads the list, newest first, keeping only well-formed entries', async () => {
      harness.ipc.recentList.mockResolvedValueOnce({
        entries: [
          recentEntry(1),
          { ...recentEntry(2), recentId: 'rc_bad' as const },
          recentEntry(3),
        ],
      });
      await harness.feature.lifecycle.refreshRecent();
      const recent = harness.feature.model.getState().recent;
      expect(recent.status).toBe('ready');
      expect(recent.entries.map((entry) => entry.projectName)).toEqual(['Project 1', 'Project 3']);
    });

    it('opens an entry', async () => {
      const entry = recentEntry(7);
      harness.ipc.projectOpenRecent.mockResolvedValueOnce(opened(HELLO_TEXT));
      expect(await harness.feature.lifecycle.openRecent(entry)).toBe(true);
      expect(harness.ipc.projectOpenRecent).toHaveBeenCalledWith({ recentId: entry.recentId });
      expect(project().handle).toBe(HANDLE_B);
    });

    it('offers to remove an entry whose file is gone', async () => {
      const entry = recentEntry(7, { projectName: 'Lost' });
      harness.ipc.projectOpenRecent.mockRejectedValueOnce(ipcFailure({ code: 'notFound' }));
      const opening = harness.feature.lifecycle.openRecent(entry);
      const confirm = await answerConfirm(harness.dialogs, true);
      expect(await opening).toBe(false);
      expect(confirm.options.title).toBe('“Lost” was not found');
      expect(confirm.options.message).toContain(entry.displayPath);
      await vi.waitFor(() => {
        expect(harness.ipc.recentRemove).toHaveBeenCalledWith({ recentId: entry.recentId });
      });
    });

    it('keeps an entry whose file is gone when the user says so', async () => {
      harness.ipc.projectOpenRecent.mockRejectedValueOnce(ipcFailure({ code: 'notFound' }));
      const opening = harness.feature.lifecycle.openRecent(recentEntry(7));
      await answerConfirm(harness.dialogs, false);
      expect(await opening).toBe(false);
      expect(harness.ipc.recentRemove).not.toHaveBeenCalled();
    });

    it('reads the list again when an entry is no longer known', async () => {
      harness.ipc.projectOpenRecent.mockRejectedValueOnce(ipcFailure({ code: 'unknownRecent' }));
      expect(await harness.feature.lifecycle.openRecent(recentEntry(7))).toBe(false);
      expect(harness.ipc.recentList).toHaveBeenCalled();
      expect(harness.dialogs.store.getState().queue).toHaveLength(0);
    });

    it('shows load problems under the entry name', async () => {
      harness.ipc.projectOpenRecent.mockRejectedValueOnce(
        ipcFailure({ code: 'newerFormat', needs: null }),
      );
      await harness.feature.lifecycle.openRecent(recentEntry(7, { projectName: 'Future' }));
      const failure = harness.feature.model.getState().loadFailure;
      expect(failure?.name).toBe('Future');
      expect(failure?.problems[0]?.message).toBe(
        'This project was made with a newer version of Blocks2Cpp. Update Blocks2Cpp to open it.',
      );
    });

    it('removes an entry and reads the list again', async () => {
      harness.ipc.recentList.mockResolvedValue({ entries: [] });
      await harness.feature.lifecycle.removeRecent(recentEntry(1).recentId);
      expect(harness.ipc.recentRemove).toHaveBeenCalledWith({ recentId: recentEntry(1).recentId });
      expect(harness.ipc.recentList).toHaveBeenCalled();
    });

    it('reports a list that cannot be read', async () => {
      harness.ipc.recentList.mockRejectedValueOnce(ipcFailure({ code: 'internal' }));
      await harness.feature.lifecycle.refreshRecent();
      expect(harness.feature.model.getState().recent.status).toBe('failed');
    });
  });

  describe('saving', () => {
    it('saves an unchanged project byte for byte as it was read', async () => {
      await openFile(HELLO_TEXT, HANDLE_A);
      harness.ipc.projectSave.mockResolvedValue({
        savedAt: '2026-10-05T10:42:00Z',
        hash: 'f'.repeat(64),
      });

      expect(await harness.feature.lifecycle.save()).toBe(true);
      expect(await harness.feature.lifecycle.save()).toBe(true);

      expect(harness.ipc.projectSave).toHaveBeenCalledTimes(2);
      for (const [request] of harness.ipc.projectSave.mock.calls) {
        expect(request).toEqual({ handle: HANDLE_A, document: HELLO_TEXT });
      }
      const saved = project();
      expect(saved.savedCanonicalText).toBe(HELLO_TEXT);
      expect(saved.savedAt).toBe('2026-10-05T10:42:00Z');
      expect(saved.dirty).toBe(false);
    });

    it('writes this app and catalog as the generator', async () => {
      const older = HELLO_TEXT.replace('"app": "0.1.0"', '"app": "0.0.9"');
      await openFile(older, HANDLE_A);
      harness.ipc.projectSave.mockResolvedValueOnce({
        savedAt: '2026-10-05T10:42:00Z',
        hash: 'f'.repeat(64),
      });
      await harness.feature.lifecycle.save();
      const sent = harness.ipc.projectSave.mock.calls[0]?.[0].document ?? '';
      expect(sent).toBe(HELLO_TEXT);
      expect(project().document.generator).toEqual({ app: '0.1.0', catalog: '1.0.0' });
      expect(project().savedCanonicalText).toBe(HELLO_TEXT);
    });

    it('falls back to Save as when the backend has no file for the project', async () => {
      await openFile(HELLO_TEXT, HANDLE_A);
      makeDirty();
      harness.ipc.projectSave.mockRejectedValueOnce(ipcFailure({ code: 'noPath' }));
      harness.ipc.projectSaveAsDialog.mockResolvedValueOnce({
        status: 'ok',
        handle: HANDLE_A,
        savedAt: '2026-10-05T11:00:00Z',
        hash: 'e'.repeat(64),
        fileName: 'renamed.b2c',
      });

      expect(await harness.feature.lifecycle.save()).toBe(true);

      expect(harness.ipc.projectSaveAsDialog).toHaveBeenCalledWith({
        handle: HANDLE_A,
        document: HELLO_TEXT,
      });
      expect(project().fileName).toBe('renamed.b2c');
      expect(project().dirty).toBe(false);
      expect(harness.ipc.trustGet).toHaveBeenCalledWith({ handle: HANDLE_A });
    });

    it('saves a never-saved project with Save as, taking the trust the save recorded', async () => {
      harness.ipc.projectNew.mockResolvedValueOnce({
        handle: HANDLE_A,
        document: EMPTY_TEXT,
        trust: CREATED_HERE,
      });
      await harness.feature.lifecycle.newProject('empty');
      harness.ipc.projectSaveAsDialog.mockResolvedValueOnce({
        status: 'ok',
        handle: HANDLE_A,
        savedAt: '2026-10-05T11:00:00Z',
        hash: 'e'.repeat(64),
        fileName: 'mine.b2c',
      });

      await harness.ctx.commands.runCommand('project.save');

      expect(harness.ipc.projectSave).not.toHaveBeenCalled();
      expect(project().fileName).toBe('mine.b2c');
      expect(project().trust).toEqual(TRUSTED);
      expect(project().savedAt).toBe('2026-10-05T11:00:00Z');
    });

    it('keeps the changes when Save as is cancelled', async () => {
      harness.ipc.projectNew.mockResolvedValueOnce({
        handle: HANDLE_A,
        document: EMPTY_TEXT,
        trust: CREATED_HERE,
      });
      await harness.feature.lifecycle.newProject('empty');
      makeDirty();
      harness.ipc.projectSaveAsDialog.mockResolvedValueOnce({ status: 'cancelled' });
      expect(await harness.feature.lifecycle.saveAs()).toBe(false);
      expect(project().dirty).toBe(true);
      expect(project().fileName).toBeNull();
    });

    it('hands a file changed on disk to the external-change feature', async () => {
      await openFile(HELLO_TEXT, HANDLE_A);
      const changed = vi.fn();
      harness.ctx.events.on('project:changedOnDisk', changed);
      harness.ipc.projectSave.mockRejectedValueOnce(ipcFailure({ code: 'changedOnDisk' }));

      expect(await harness.feature.lifecycle.save()).toBe(false);

      expect(changed).toHaveBeenCalledWith({ kind: 'project:changedOnDisk', handle: HANDLE_A });
      expect(harness.dialogs.store.getState().queue).toHaveLength(0);
    });

    it('says why a save failed', async () => {
      await openFile(HELLO_TEXT, HANDLE_A);
      harness.ipc.projectSave.mockRejectedValueOnce(
        ipcFailure({ code: 'payloadTooLarge', limit: 32 * 1024 * 1024 }),
      );
      const saving = harness.feature.lifecycle.save();
      const alert = await closeAlert(harness.dialogs);
      expect(await saving).toBe(false);
      expect(alert.options.message).toBe(
        'The project was not saved: a project file can be at most 32 MiB.',
      );
    });

    it('keeps edits made while the save was on its way as unsaved', async () => {
      await openFile(HELLO_TEXT, HANDLE_A);
      let finish: (value: { savedAt: string; hash: string }) => void = () => undefined;
      harness.ipc.projectSave.mockImplementationOnce(
        () =>
          new Promise((resolve) => {
            finish = resolve;
          }),
      );
      const saving = harness.feature.lifecycle.save();
      await vi.waitFor(() => {
        expect(harness.ipc.projectSave).toHaveBeenCalled();
      });
      // The live preview commits an edit while the backend writes.
      const edited = structuredClone(project().document);
      edited.project.name = 'Edited meanwhile';
      const canonical = core.canonical(JSON.stringify(edited));
      if (!canonical.ok) {
        throw new Error('the edited document does not load');
      }
      useAppStore.getState().actions.updateProject({
        document: edited,
        canonicalText: canonical.text,
        contentHash: canonical.hash,
        dirty: true,
      });
      finish({ savedAt: '2026-10-05T10:42:00Z', hash: 'f'.repeat(64) });

      expect(await saving).toBe(true);
      const after = project();
      expect(after.savedCanonicalText).toBe(HELLO_TEXT);
      expect(after.document.project.name).toBe('Edited meanwhile');
      expect(after.dirty).toBe(true);
      expect(after.savedAt).toBe('2026-10-05T10:42:00Z');
    });

    it('refuses to save a document the core does not load, saying it is a bug', async () => {
      await openFile(HELLO_TEXT, HANDLE_A);
      const broken = structuredClone(project().document);
      broken.modules = [];
      useAppStore.getState().actions.updateProject({ document: broken });
      const saving = harness.feature.lifecycle.save();
      const alert = await closeAlert(harness.dialogs);
      expect(await saving).toBe(false);
      expect(alert.options.message).toContain('This is a bug in Blocks2Cpp');
      expect(harness.ipc.projectSave).not.toHaveBeenCalled();
    });

    it('saves what the editor shows', async () => {
      await openFile(HELLO_TEXT, HANDLE_A);
      const shown = structuredClone(project().document);
      shown.project.name = 'From the canvas';
      harness.setEditor({
        currentDocument: () => shown,
        loadDocument: () => undefined,
        selectBlock: () => undefined,
        workspace: undefined as never,
      });
      harness.ipc.projectSave.mockResolvedValueOnce({
        savedAt: '2026-10-05T10:42:00Z',
        hash: 'f'.repeat(64),
      });
      await harness.feature.lifecycle.save();
      expect(harness.ipc.projectSave.mock.calls[0]?.[0].document).toContain('"From the canvas"');
    });

    it('does not save when the editor cannot read its blocks', async () => {
      await openFile(HELLO_TEXT, HANDLE_A);
      vi.spyOn(console, 'error').mockImplementation(() => undefined);
      harness.setEditor({
        currentDocument: () => {
          throw new Error('broken canvas');
        },
        loadDocument: () => undefined,
        selectBlock: () => undefined,
        workspace: undefined as never,
      });
      const saving = harness.feature.lifecycle.save();
      const alert = await closeAlert(harness.dialogs);
      expect(await saving).toBe(false);
      expect(alert.options.message).toContain('could not be read from the editor');
    });
  });

  describe('closing', () => {
    it('closes a saved project and shows the start page', async () => {
      await openFile(HELLO_TEXT, HANDLE_A);
      await harness.ctx.commands.runCommand('project.close');
      expect(harness.ipc.projectClose).toHaveBeenCalledWith({ handle: HANDLE_A });
      expect(useAppStore.getState().project).toBeNull();
      expect(useAppStore.getState().ui.screen).toBe('start');
    });

    it('asks first when there are unsaved changes, and Cancel keeps the project', async () => {
      await openFile(HELLO_TEXT, HANDLE_A);
      makeDirty();
      const closing = harness.feature.lifecycle.close();
      await answerChoice(harness.dialogs, 'cancel');
      expect(await closing).toBe(false);
      expect(harness.ipc.projectClose).not.toHaveBeenCalled();
      expect(project().handle).toBe(HANDLE_A);
    });

    it('shows the start page when no project is open', async () => {
      useAppStore.getState().actions.setUi({ screen: 'settings' });
      expect(await harness.feature.lifecycle.close()).toBe(true);
      expect(useAppStore.getState().ui.screen).toBe('start');
    });
  });

  describe('closing the window (closeRequested)', () => {
    it('quits at once when nothing is unsaved', async () => {
      await openFile(HELLO_TEXT, HANDLE_A);
      expect(await harness.feature.lifecycle.quit()).toBe(true);
      expect(harness.ipc.appQuit).toHaveBeenCalledOnce();
      expect(harness.dialogs.store.getState().queue).toHaveLength(0);
    });

    it('Save: saves, then quits', async () => {
      await openFile(HELLO_TEXT, HANDLE_A);
      makeDirty();
      harness.ipc.projectSave.mockResolvedValueOnce({
        savedAt: '2026-10-05T10:42:00Z',
        hash: 'f'.repeat(64),
      });
      harness.ctx.events.emit({ kind: 'closeRequested' });
      const prompt = await answerChoice(harness.dialogs, 'save');
      expect(prompt.kind === 'choice' && prompt.options.choices.map((c) => c.label)).toEqual([
        'Save',
        "Don't save",
        'Cancel',
      ]);
      await vi.waitFor(() => {
        expect(harness.ipc.appQuit).toHaveBeenCalledOnce();
      });
      expect(harness.ipc.projectSave).toHaveBeenCalledOnce();
      expect(harness.ipc.projectSave.mock.invocationCallOrder[0]).toBeLessThan(
        harness.ipc.appQuit.mock.invocationCallOrder[0] ?? 0,
      );
    });

    it("Don't save: closes the project (dropping its snapshot), then quits", async () => {
      await openFile(HELLO_TEXT, HANDLE_A);
      makeDirty();
      harness.ctx.events.emit({ kind: 'closeRequested' });
      await answerChoice(harness.dialogs, 'discard');
      await vi.waitFor(() => {
        expect(harness.ipc.appQuit).toHaveBeenCalledOnce();
      });
      expect(harness.ipc.projectSave).not.toHaveBeenCalled();
      expect(harness.ipc.projectClose).toHaveBeenCalledWith({ handle: HANDLE_A });
      expect(harness.ipc.projectClose.mock.invocationCallOrder[0]).toBeLessThan(
        harness.ipc.appQuit.mock.invocationCallOrder[0] ?? 0,
      );
    });

    it('Cancel: keeps the window open', async () => {
      await openFile(HELLO_TEXT, HANDLE_A);
      makeDirty();
      const quitting = harness.feature.lifecycle.quit();
      await answerChoice(harness.dialogs, 'cancel');
      expect(await quitting).toBe(false);
      expect(harness.ipc.appQuit).not.toHaveBeenCalled();
      expect(project().dirty).toBe(true);
    });

    it('does not quit when the save fails or is cancelled', async () => {
      harness.ipc.projectNew.mockResolvedValueOnce({
        handle: HANDLE_A,
        document: EMPTY_TEXT,
        trust: CREATED_HERE,
      });
      await harness.feature.lifecycle.newProject('empty');
      makeDirty();
      harness.ipc.projectSaveAsDialog.mockResolvedValueOnce({ status: 'cancelled' });
      const quitting = harness.feature.lifecycle.quit();
      await answerChoice(harness.dialogs, 'save');
      expect(await quitting).toBe(false);
      expect(harness.ipc.appQuit).not.toHaveBeenCalled();
    });

    it('asks once when the window asks twice', async () => {
      await openFile(HELLO_TEXT, HANDLE_A);
      makeDirty();
      const first = harness.feature.lifecycle.quit();
      const second = harness.feature.lifecycle.quit();
      expect(second).toBe(first);
      await nextDialog(harness.dialogs);
      expect(harness.dialogs.store.getState().queue).toHaveLength(1);
      await answerChoice(harness.dialogs, 'cancel');
      expect(await first).toBe(false);
    });

    it('says so when the app cannot quit', async () => {
      vi.spyOn(console, 'error').mockImplementation(() => undefined);
      harness.ipc.appQuit.mockRejectedValueOnce(ipcFailure({ code: 'internal' }));
      const quitting = harness.feature.lifecycle.quit();
      const alert = await closeAlert(harness.dialogs);
      expect(await quitting).toBe(false);
      expect(alert.options.title).toBe('Blocks2Cpp could not quit');
    });
  });

  describe('failures on the way', () => {
    it('reports a compiler core that cannot start, closing the new handle', async () => {
      vi.spyOn(console, 'error').mockImplementation(() => undefined);
      const lifecycle = new ProjectLifecycle(harness.ctx, createProjectModel(), {
        openInEditor: () => Promise.reject(new CoreError('init', 'no module')),
      });
      harness.ipc.projectOpenDialog.mockResolvedValueOnce({
        status: 'ok',
        ...opened(HELLO_TEXT, { handle: HANDLE_B }),
      });
      const opening = lifecycle.open();
      const alert = await closeAlert(harness.dialogs);
      expect(await opening).toBe(false);
      expect(alert.options.message).toContain('compiler core could not start');
      expect(harness.ipc.projectClose).toHaveBeenCalledWith({ handle: HANDLE_B });
      expect(useAppStore.getState().project).toBeNull();
    });

    it('says when the chosen file no longer exists', async () => {
      harness.ipc.projectOpenDialog.mockRejectedValueOnce(ipcFailure({ code: 'notFound' }));
      const opening = harness.feature.lifecycle.open();
      const alert = await closeAlert(harness.dialogs);
      expect(await opening).toBe(false);
      expect(alert.options.message).toBe('The file no longer exists.');
    });

    it('says when a recent entry cannot be removed', async () => {
      harness.ipc.recentRemove.mockRejectedValueOnce(ipcFailure({ code: 'io', kind: 'other' }));
      const removing = harness.feature.lifecycle.removeRecent(recentEntry(1).recentId);
      const alert = await closeAlert(harness.dialogs);
      await removing;
      expect(alert.options.title).toBe('The entry was not removed');
      expect(harness.ipc.recentList).toHaveBeenCalled();
    });

    it('treats an entry removed meanwhile as removed', async () => {
      harness.ipc.recentRemove.mockRejectedValueOnce(ipcFailure({ code: 'unknownRecent' }));
      await harness.feature.lifecycle.removeRecent(recentEntry(1).recentId);
      expect(harness.dialogs.store.getState().queue).toHaveLength(0);
    });

    it('says why Save as failed', async () => {
      await openFile(HELLO_TEXT, HANDLE_A);
      harness.ipc.projectSaveAsDialog.mockRejectedValueOnce(ipcFailure({ code: 'busy' }));
      const saving = harness.feature.lifecycle.saveAs();
      const alert = await closeAlert(harness.dialogs);
      expect(await saving).toBe(false);
      expect(alert.options.message).toBe('Another dialog is already open. Close it first.');
    });

    it('keeps the saved state when the trust cannot be read after Save as', async () => {
      await openFile(HELLO_TEXT, HANDLE_A);
      harness.ipc.trustGet.mockRejectedValueOnce(ipcFailure({ code: 'internal' }));
      harness.ipc.projectSaveAsDialog.mockResolvedValueOnce({
        status: 'ok',
        handle: HANDLE_A,
        savedAt: '2026-10-05T11:00:00Z',
        hash: 'e'.repeat(64),
        fileName: 'copy.b2c',
      });
      expect(await harness.feature.lifecycle.saveAs()).toBe(true);
      expect(project().fileName).toBe('copy.b2c');
      expect(project().trust).toEqual(TRUSTED);
    });

    it('closes the project in the window even when the backend refuses project_close', async () => {
      await openFile(HELLO_TEXT, HANDLE_A);
      harness.ipc.projectClose.mockRejectedValueOnce(ipcFailure({ code: 'unknownHandle' }));
      expect(await harness.feature.lifecycle.close()).toBe(true);
      expect(useAppStore.getState().project).toBeNull();
    });

    it('leaves another project alone when the saved one was replaced meanwhile', async () => {
      await openFile(HELLO_TEXT, HANDLE_A);
      let finish: (value: { savedAt: string; hash: string }) => void = () => undefined;
      harness.ipc.projectSave.mockImplementationOnce(
        () =>
          new Promise((resolve) => {
            finish = resolve;
          }),
      );
      const saving = harness.feature.lifecycle.save();
      await vi.waitFor(() => {
        expect(harness.ipc.projectSave).toHaveBeenCalled();
      });
      const other = projectFixture({ handle: HANDLE_C, dirty: true });
      useAppStore.getState().actions.setProject(other);
      finish({ savedAt: '2026-10-05T10:42:00Z', hash: 'f'.repeat(64) });
      expect(await saving).toBe(true);
      expect(project()).toEqual(other);
    });
  });

  describe('order and state', () => {
    it('runs operations one at a time, in order', async () => {
      await openFile(HELLO_TEXT, HANDLE_A);
      let finish: (value: { savedAt: string; hash: string }) => void = () => undefined;
      harness.ipc.projectSave.mockImplementationOnce(
        () =>
          new Promise((resolve) => {
            finish = resolve;
          }),
      );
      const saving = harness.feature.lifecycle.save();
      const closing = harness.feature.lifecycle.close();
      await vi.waitFor(() => {
        expect(harness.ipc.projectSave).toHaveBeenCalled();
      });
      expect(harness.feature.model.getState().busy).toBe('save');
      expect(harness.ipc.projectClose).not.toHaveBeenCalled();
      finish({ savedAt: '2026-10-05T10:42:00Z', hash: 'f'.repeat(64) });
      expect(await saving).toBe(true);
      expect(await closing).toBe(true);
      expect(harness.ipc.projectClose).toHaveBeenCalledWith({ handle: HANDLE_A });
      expect(harness.feature.model.getState().busy).toBeNull();
    });

    it('does nothing once uninstalled', async () => {
      harness.feature.uninstall();
      expect(await harness.feature.lifecycle.open()).toBe(false);
      expect(harness.ipc.projectOpenDialog).not.toHaveBeenCalled();
    });

    it('registers the start page and the project commands, and removes them again', () => {
      expect(harness.ctx.screens.screen('start')).not.toBeNull();
      for (const id of [
        'project.new',
        'project.open',
        'project.save',
        'project.saveAs',
        'project.close',
      ] as const) {
        expect(harness.ctx.commands.hasCommand(id)).toBe(true);
      }
      harness.feature.uninstall();
      expect(harness.ctx.screens.screen('start')).toBeNull();
      expect(harness.ctx.commands.hasCommand('project.save')).toBe(false);
    });
  });
});
