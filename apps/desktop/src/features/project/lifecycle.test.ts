/**
 * The project lifecycle against a fake backend and the real compiler core: new from each
 * template, open (cancelled, ok, refused), recent projects, saving (byte-identical, generator,
 * save-as fallback, changed on disk, edits made while saving, a Save asked for while one runs),
 * the unsaved-changes prompts of open, new, close and the window's close request, and the order
 * of operations.
 *
 * Without a build of the compiler core these tests are skipped, unless B2C_REQUIRE_WASM is set
 * (as in CI), in which case a missing build fails them.
 */
import {
  type BdmDocument,
  CoreError,
  type CoreWasm,
  type JsonValue,
} from '@blocks2cpp/b2c-core-wasm';
import type { Trust } from '@blocks2cpp/ipc-types';
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';

import type { EditorHandle } from '../../app/editor-types';
import { installShortcuts } from '../../app/shortcuts';
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

/**
 * The hash of the hello-world file's text, which is saved canonically: the `textHash` of what the
 * editor shows when it opens the file, and of what a save writes.
 */
function helloHash(): string {
  const canonical = core.canonical(HELLO_TEXT);
  if (!canonical.ok || canonical.text !== HELLO_TEXT) {
    throw new Error('hello world is not saved canonically');
  }
  return canonical.textHash;
}

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

/** An editor that shows `currentDocument` and does nothing else. */
function stubEditor(currentDocument: () => BdmDocument): EditorHandle {
  return {
    currentDocument,
    loadDocument: () => undefined,
    selectBlock: () => undefined,
    updateProjectInfo: () => undefined,
    undo: () => undefined,
    redo: () => undefined,
    workspace: undefined as never,
  };
}

/** `doc` with tooling data nested `depth` levels deep, past the loader's limit (B2C-E0104). */
function nestedTooDeeply(doc: BdmDocument, depth = 200): BdmDocument {
  let value: JsonValue = 'deep';
  for (let level = 0; level < depth; level++) {
    value = [value];
  }
  return { ...doc, 'x-ext': { nested: value } };
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
      expect(shown.textHash).toBe(helloHash());
      expect(shown.savedTextHash).toBe(helloHash());
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

    it('opens a file whose blocks were upgraded with unsaved changes, and says so', async () => {
      harness.ipc.projectOpenDialog.mockResolvedValueOnce({
        status: 'ok',
        ...opened(HELLO_TEXT, { blocksUpgraded: true, fileName: 'old.b2c' }),
      });
      const opening = harness.feature.lifecycle.open();
      const alert = await closeAlert(harness.dialogs);
      expect(await opening).toBe(true);
      expect(alert.options.title).toBe('Blocks were upgraded');
      expect(alert.options.message).toBe(
        'Some blocks in “old.b2c” were made by an older version of Blocks2Cpp and have been upgraded, so the project now has unsaved changes. When you save it, the file as it was is kept as “old.b2c.bak”.',
      );
      // The file still holds the old blocks: nothing shown counts as saved.
      expect(project().savedTextHash).toBeNull();
      expect(project().dirty).toBe(true);
      await vi.waitFor(() => {
        expect(harness.ipc.projectSetDirty).toHaveBeenCalledWith({ handle: HANDLE_B, dirty: true });
      });
    });

    it('opens a recent project whose blocks were upgraded with unsaved changes', async () => {
      harness.ipc.projectOpenRecent.mockResolvedValueOnce(
        opened(HELLO_TEXT, { blocksUpgraded: true }),
      );
      const opening = harness.feature.lifecycle.openRecent(recentEntry(1));
      const alert = await closeAlert(harness.dialogs);
      expect(await opening).toBe(true);
      expect(alert.options.message).toContain('“game.b2c.bak”');
      expect(project().dirty).toBe(true);
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
      expect(saved.savedTextHash).toBe(helloHash());
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
      expect(project().document.generator).toEqual({ app: '0.1.0', catalog: '1.3.0' });
      expect(project().savedTextHash).toBe(helloHash());
    });

    it('keeps a newer catalog the file names, so its newer blocks stay made by a newer version', async () => {
      // A print with a field this catalog does not know, from a file of catalog 99.0.0 (ADR-0015).
      const doc = JSON.parse(HELLO_TEXT) as BdmDocument;
      const newer = JSON.parse(
        JSON.stringify(doc).replace('"fields":{', '"fields":{"VOLUME":"3",'),
      ) as BdmDocument;
      const newerText = core.canonical(
        JSON.stringify({ ...newer, generator: { ...newer.generator, catalog: '99.0.0' } }),
      );
      if (!newerText.ok) {
        throw new Error('the newer document does not load');
      }
      expect(newerText.text).toContain('"VOLUME": "3"');
      await openFile(newerText.text, HANDLE_A);
      harness.ipc.projectSave.mockResolvedValueOnce({
        savedAt: '2026-10-05T10:42:00Z',
        hash: 'f'.repeat(64),
      });
      await harness.feature.lifecycle.save();
      const sent = harness.ipc.projectSave.mock.calls[0]?.[0].document ?? '';
      expect(sent).toContain('"catalog": "99.0.0"');
      expect(sent).toContain('"VOLUME": "3"');
      expect(project().document.generator).toEqual({ app: '0.1.0', catalog: '99.0.0' });
      // The preview of what the project now holds still says "made by a newer version".
      const preview = core.update(JSON.stringify(project().document), { indentWidth: 4 });
      const codes = preview.diagnostics.map((diagnostic) => diagnostic.code);
      expect(codes).toContain('B2C-E0602');
      expect(codes).not.toContain('B2C-E0605');
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
        textHash: canonical.textHash,
        contentHash: canonical.hash,
        dirty: true,
      });
      finish({ savedAt: '2026-10-05T10:42:00Z', hash: 'f'.repeat(64) });

      expect(await saving).toBe(true);
      const after = project();
      expect(after.savedTextHash).toBe(helloHash());
      expect(after.document.project.name).toBe('Edited meanwhile');
      expect(after.dirty).toBe(true);
      expect(after.savedAt).toBe('2026-10-05T10:42:00Z');
    });

    it("leaves the backend's flag set after a save that overlapped an edit, and clears it after a clean save", async () => {
      // The backend's flag is what project_set_dirty last reported; a save does not clear it.
      let backendDirty = false;
      harness.ipc.projectSetDirty.mockImplementation((request) => {
        backendDirty = request.dirty;
        return Promise.resolve({});
      });
      await openFile(HELLO_TEXT, HANDLE_A);
      const editName = (name: string) => {
        const edited = structuredClone(project().document);
        edited.project.name = name;
        const canonical = core.canonical(JSON.stringify(edited));
        if (!canonical.ok) {
          throw new Error('the edited document does not load');
        }
        useAppStore.getState().actions.updateProject({
          document: edited,
          textHash: canonical.textHash,
          contentHash: canonical.hash,
          dirty: canonical.textHash !== project().savedTextHash,
        });
      };
      editName('First edit');
      await vi.waitFor(() => {
        expect(backendDirty).toBe(true);
      });

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
      editName('Typed while the save was on its way');
      finish({ savedAt: '2026-10-05T10:42:00Z', hash: 'f'.repeat(64) });
      expect(await saving).toBe(true);
      await new Promise((resolve) => setTimeout(resolve, 10));

      // Closing the window must still ask: the last value sent is true.
      expect(project().dirty).toBe(true);
      expect(harness.ipc.projectSetDirty).toHaveBeenLastCalledWith({
        handle: HANDLE_A,
        dirty: true,
      });
      expect(backendDirty).toBe(true);

      // A clean save turns the store's flag false, which is sent.
      harness.ipc.projectSave.mockResolvedValueOnce({
        savedAt: '2026-10-05T10:43:00Z',
        hash: 'e'.repeat(64),
      });
      expect(await harness.feature.lifecycle.save()).toBe(true);
      await vi.waitFor(() => {
        expect(backendDirty).toBe(false);
      });
      expect(project().dirty).toBe(false);
      expect(harness.ipc.projectSetDirty).toHaveBeenLastCalledWith({
        handle: HANDLE_A,
        dirty: false,
      });
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

    it('refuses to save a canvas that goes past a limit, naming it rather than a bug', async () => {
      await openFile(HELLO_TEXT, HANDLE_A);
      const deep = nestedTooDeeply(project().document);
      harness.setEditor(stubEditor(() => deep));
      const saving = harness.feature.lifecycle.save();
      const alert = await closeAlert(harness.dialogs);
      expect(await saving).toBe(false);
      expect(alert.options.message).toBe(
        'The project was not saved. Blocks are nested more deeply than a project allows (B2C-E0104). Undo the last change (Ctrl+Z), or move some of the innermost blocks into a function.',
      );
      expect(alert.options.message).not.toContain('bug');
      expect(harness.ipc.projectSave).not.toHaveBeenCalled();
    });

    it('saves what the editor shows', async () => {
      await openFile(HELLO_TEXT, HANDLE_A);
      const shown = structuredClone(project().document);
      shown.project.name = 'From the canvas';
      harness.setEditor(stubEditor(() => shown));
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
      harness.setEditor(
        stubEditor(() => {
          throw new Error('broken canvas');
        }),
      );
      const saving = harness.feature.lifecycle.save();
      const alert = await closeAlert(harness.dialogs);
      expect(await saving).toBe(false);
      expect(alert.options.message).toContain('could not be read from the editor');
    });
  });

  // Nightly 17 (Windows): a Ctrl+S pressed right after an edit, while the first Save as was still
  // finishing, joined that running command and was never saved.
  describe('a Save asked for while a save runs', () => {
    const SAVED = { savedAt: '2026-10-05T10:42:00Z', hash: 'f'.repeat(64) };

    /** An answer the test gives when it wants to. */
    function held<T>(): { readonly promise: Promise<T>; readonly answer: (value: T) => void } {
      let answer: (value: T) => void = () => undefined;
      const promise = new Promise<T>((resolve) => {
        answer = resolve;
      });
      return { promise, answer };
    }

    /** Renames the project in the store, as the top bar's name field does; returns the text. */
    function rename(name: string): string {
      const edited = structuredClone(project().document);
      edited.project.name = name;
      const canonical = core.canonical(JSON.stringify(edited));
      if (!canonical.ok) {
        throw new Error('the renamed document does not load');
      }
      useAppStore.getState().actions.updateProject({
        document: edited,
        textHash: canonical.textHash,
        contentHash: canonical.hash,
        dirty: true,
      });
      return canonical.text;
    }

    /** A new project whose first Save (a Save as) waits for its `trust_get` until `answer`. */
    async function newProjectWithSlowSaveAs(): Promise<(value: { trust: Trust }) => void> {
      harness.ipc.projectNew.mockResolvedValueOnce({
        handle: HANDLE_A,
        document: EMPTY_TEXT,
        trust: CREATED_HERE,
      });
      expect(await harness.feature.lifecycle.newProject('empty')).toBe(true);
      harness.ipc.projectSaveAsDialog.mockResolvedValueOnce({
        status: 'ok',
        handle: HANDLE_A,
        savedAt: '2026-10-05T10:41:00Z',
        hash: 'e'.repeat(64),
        fileName: 'mine.b2c',
      });
      const trust = held<{ trust: Trust }>();
      harness.ipc.trustGet.mockReturnValueOnce(trust.promise);
      return trust.answer;
    }

    it('saves an edit made while the first Save as was finishing, after it', async () => {
      const answerTrust = await newProjectWithSlowSaveAs();
      const first = harness.ctx.commands.runCommand('project.save');
      // The file is written and named; the save still waits for the trust it recorded.
      await vi.waitFor(() => {
        expect(project().fileName).toBe('mine.b2c');
      });
      const edited = rename('Space Invaders');
      harness.ipc.projectSave.mockResolvedValueOnce(SAVED);

      const second = harness.ctx.commands.runCommand('project.save');
      answerTrust({ trust: TRUSTED });
      await first;
      await second;

      expect(harness.ipc.projectSaveAsDialog).toHaveBeenCalledTimes(1);
      expect(harness.ipc.projectSave).toHaveBeenCalledTimes(1);
      expect(harness.ipc.projectSave).toHaveBeenCalledWith({ handle: HANDLE_A, document: edited });
      expect(project().dirty).toBe(false);
      expect(project().savedAt).toBe(SAVED.savedAt);
      expect(harness.feature.model.getState().busy).toBeNull();
      expect(harness.feature.lifecycle.isSaving()).toBe(false);
    });

    it('makes one save after the running one of any number of requests, with the latest edits', async () => {
      await openFile(HELLO_TEXT, HANDLE_A);
      const writing = held<typeof SAVED>();
      harness.ipc.projectSave.mockReturnValueOnce(writing.promise).mockResolvedValue(SAVED);
      const first = harness.ctx.commands.runCommand('project.save');
      await vi.waitFor(() => {
        expect(harness.ipc.projectSave).toHaveBeenCalledTimes(1);
      });

      rename('Typed while saving');
      const later = [
        harness.ctx.commands.runCommand('project.save'),
        harness.ctx.commands.runCommand('project.save'),
      ];
      const edited = rename('Typed again');
      later.push(harness.ctx.commands.runCommand('project.save'));
      const asked = [harness.feature.lifecycle.save(), harness.feature.lifecycle.save()];
      expect(harness.feature.lifecycle.isSaving()).toBe(true);
      writing.answer(SAVED);
      await Promise.all([first, ...later, ...asked]);

      expect(harness.ipc.projectSave).toHaveBeenCalledTimes(2);
      expect(harness.ipc.projectSave).toHaveBeenLastCalledWith({
        handle: HANDLE_A,
        document: edited,
      });
      expect(project().dirty).toBe(false);
      expect(harness.feature.model.getState().busy).toBeNull();
      // Until it started, every request was that one save.
      expect(asked[0]).toBe(asked[1]);
    });

    it('asks for a file once when the first Save as is cancelled', async () => {
      harness.ipc.projectNew.mockResolvedValueOnce({
        handle: HANDLE_A,
        document: EMPTY_TEXT,
        trust: CREATED_HERE,
      });
      expect(await harness.feature.lifecycle.newProject('empty')).toBe(true);
      const choosing = held<{ status: 'cancelled' }>();
      harness.ipc.projectSaveAsDialog
        .mockReturnValueOnce(choosing.promise)
        .mockResolvedValue({ status: 'cancelled' });
      const first = harness.ctx.commands.runCommand('project.save');
      await vi.waitFor(() => {
        expect(harness.ipc.projectSaveAsDialog).toHaveBeenCalledTimes(1);
      });

      // Pressed again before the dialog took the keyboard.
      const second = harness.ctx.commands.runCommand('project.save');
      choosing.answer({ status: 'cancelled' });
      await first;
      await second;

      expect(harness.ipc.projectSaveAsDialog).toHaveBeenCalledTimes(1);
      expect(harness.ipc.projectSave).not.toHaveBeenCalled();
      expect(project().fileName).toBeNull();
      expect(harness.feature.model.getState().busy).toBeNull();
    });

    it('reports a failed save once', async () => {
      await openFile(HELLO_TEXT, HANDLE_A);
      makeDirty();
      let fail: (error: unknown) => void = () => undefined;
      harness.ipc.projectSave
        .mockReturnValueOnce(
          new Promise<typeof SAVED>((_, reject) => {
            fail = reject;
          }),
        )
        .mockRejectedValue(ipcFailure({ code: 'busy' }));
      const first = harness.ctx.commands.runCommand('project.save');
      await vi.waitFor(() => {
        expect(harness.ipc.projectSave).toHaveBeenCalledTimes(1);
      });

      const second = harness.ctx.commands.runCommand('project.save');
      fail(ipcFailure({ code: 'busy' }));
      await closeAlert(harness.dialogs);
      await first;
      // A second failed save would wait for its own alert to be closed.
      await vi.waitFor(() => {
        expect(harness.dialogs.store.getState().queue).toHaveLength(0);
        expect(harness.feature.model.getState().busy).toBeNull();
      });
      await second;

      expect(harness.ipc.projectSave).toHaveBeenCalledTimes(1);
      expect(project().dirty).toBe(true);
      expect(harness.feature.model.getState().busy).toBeNull();
    });

    it('saves only the project it was asked for', async () => {
      await openFile(HELLO_TEXT, HANDLE_A);
      makeDirty();
      const writing = held<typeof SAVED>();
      harness.ipc.projectSave.mockReturnValueOnce(writing.promise).mockResolvedValue(SAVED);
      const first = harness.ctx.commands.runCommand('project.save');
      await vi.waitFor(() => {
        expect(harness.ipc.projectSave).toHaveBeenCalledTimes(1);
      });

      harness.ipc.projectOpenDialog.mockResolvedValueOnce({
        status: 'ok',
        ...opened(HELLO_TEXT, { handle: HANDLE_B, fileName: 'other.b2c' }),
      });
      const opening = harness.ctx.commands.runCommand('project.open');
      // Asked for A, which the Open queued before it replaces.
      const second = harness.ctx.commands.runCommand('project.save');
      writing.answer(SAVED);
      await Promise.all([first, opening, second]);

      expect(project().handle).toBe(HANDLE_B);
      expect(harness.ipc.projectSave).toHaveBeenCalledTimes(1);
      expect(harness.ipc.projectSave).toHaveBeenCalledWith(
        expect.objectContaining({ handle: HANDLE_A }),
      );
      expect(harness.feature.model.getState().busy).toBeNull();
    });

    it('keeps the order of operations: a Save asked for after another operation follows it', async () => {
      await openFile(HELLO_TEXT, HANDLE_A);
      const order: string[] = [];
      const writing = held<typeof SAVED>();
      harness.ipc.projectSave
        .mockImplementationOnce(() => {
          order.push('save');
          return writing.promise;
        })
        .mockImplementation(() => {
          order.push('save');
          return Promise.resolve(SAVED);
        });
      const running = harness.feature.lifecycle.save();
      await vi.waitFor(() => {
        expect(order).toEqual(['save']);
      });

      const waiting = harness.feature.lifecycle.save();
      const reloading = harness.feature.lifecycle.reloadWith(() => {
        order.push('reload');
        return Promise.resolve(true);
      });
      const after = harness.feature.lifecycle.save();
      expect(after).not.toBe(waiting);
      writing.answer(SAVED);

      expect(await Promise.all([running, waiting, reloading, after])).toEqual([
        true,
        true,
        true,
        true,
      ]);
      expect(order).toEqual(['save', 'save', 'reload', 'save']);
    });

    it('still does nothing more for a held Ctrl+S', async () => {
      await openFile(HELLO_TEXT, HANDLE_A);
      const writing = held<typeof SAVED>();
      harness.ipc.projectSave.mockReturnValueOnce(writing.promise).mockResolvedValue(SAVED);
      const uninstall = installShortcuts(window, harness.ctx);
      try {
        const press = (repeat: boolean) =>
          document.body.dispatchEvent(
            new KeyboardEvent('keydown', {
              key: 's',
              ctrlKey: true,
              repeat,
              bubbles: true,
              cancelable: true,
            }),
          );
        press(false);
        await vi.waitFor(() => {
          expect(harness.ipc.projectSave).toHaveBeenCalledTimes(1);
        });
        rename('Typed while saving');
        press(true);
        press(true);
        writing.answer(SAVED);
        await vi.waitFor(() => {
          expect(harness.feature.model.getState().busy).toBeNull();
        });

        expect(harness.ipc.projectSave).toHaveBeenCalledTimes(1);
        expect(project().dirty).toBe(true);
      } finally {
        uninstall();
      }
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

  describe("other features' operations", () => {
    it('replaces the open project in the queue, after asking about its unsaved changes', async () => {
      await openFile(HELLO_TEXT, HANDLE_A);
      makeDirty();
      const show = vi.fn(() => {
        expect(harness.feature.model.getState().busy).toBe('restore');
        useAppStore
          .getState()
          .actions.setProject(projectFixture({ handle: HANDLE_C, dirty: true }));
        return Promise.resolve(HANDLE_C);
      });

      // Cancel: nothing is shown.
      let replacing = harness.feature.lifecycle.replaceWith(show);
      await answerChoice(harness.dialogs, 'cancel');
      expect(await replacing).toBe(false);
      expect(show).not.toHaveBeenCalled();

      // Don't save: shown, and the replaced project closed afterwards.
      replacing = harness.feature.lifecycle.replaceWith(show);
      await answerChoice(harness.dialogs, 'discard');
      expect(await replacing).toBe(true);
      expect(harness.ipc.projectClose).toHaveBeenCalledWith({ handle: HANDLE_A });
      expect(harness.feature.model.getState().busy).toBeNull();
    });

    it('keeps the open project when nothing was shown', async () => {
      await openFile(HELLO_TEXT, HANDLE_A);
      expect(await harness.feature.lifecycle.replaceWith(() => Promise.resolve(null))).toBe(false);
      expect(harness.ipc.projectClose).not.toHaveBeenCalled();
      expect(project().handle).toBe(HANDLE_A);
    });

    it('runs a reload after a save that was asked for first', async () => {
      await openFile(HELLO_TEXT, HANDLE_A);
      const order: string[] = [];
      let finishSave: (value: { savedAt: string; hash: string }) => void = () => undefined;
      harness.ipc.projectSave.mockImplementationOnce(() => {
        order.push('save');
        return new Promise((resolve) => {
          finishSave = resolve;
        });
      });
      const saving = harness.feature.lifecycle.save();
      const reloading = harness.feature.lifecycle.reloadWith(() => {
        order.push('reload');
        expect(harness.feature.model.getState().busy).toBe('reload');
        return Promise.resolve(true);
      });
      await vi.waitFor(() => {
        expect(order).toEqual(['save']);
      });
      finishSave({ savedAt: '2026-10-05T10:42:00Z', hash: 'f'.repeat(64) });
      expect(await saving).toBe(true);
      expect(await reloading).toBe(true);
      expect(order).toEqual(['save', 'reload']);
    });

    it('makes every save wait for its barriers, and says when it is saving', async () => {
      await openFile(HELLO_TEXT, HANDLE_A);
      let release: () => void = () => undefined;
      const barrier = vi.fn(
        () =>
          new Promise<void>((resolve) => {
            release = resolve;
          }),
      );
      const remove = harness.feature.lifecycle.addSaveBarrier(barrier);
      harness.ipc.projectSave.mockResolvedValue({
        savedAt: '2026-10-05T10:42:00Z',
        hash: 'f'.repeat(64),
      });
      expect(harness.feature.lifecycle.isSaving()).toBe(false);
      const saving = harness.feature.lifecycle.save();
      await vi.waitFor(() => {
        expect(barrier).toHaveBeenCalledOnce();
      });
      expect(harness.feature.lifecycle.isSaving()).toBe(true);
      expect(harness.ipc.projectSave).not.toHaveBeenCalled();
      release();
      expect(await saving).toBe(true);
      expect(harness.ipc.projectSave).toHaveBeenCalledOnce();
      expect(harness.feature.lifecycle.isSaving()).toBe(false);

      // A barrier that fails is only logged; a removed one is not waited for.
      remove();
      harness.feature.lifecycle.addSaveBarrier(() => Promise.reject(new Error('lost')));
      expect(await harness.feature.lifecycle.save()).toBe(true);
      expect(barrier).toHaveBeenCalledOnce();
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
