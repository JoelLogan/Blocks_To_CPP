/**
 * The external-change feature over a fake backend: which question is asked when, what *Reload*,
 * *Keep mine (save as…)* and *Not now* do, how reports are merged, and every failure.
 */
import type { ProjectReloadResponse } from '@blocks2cpp/ipc-types';
import { afterEach, beforeEach, describe, expect, it, type Mock, vi } from 'vitest';

import { installAll } from '../../app/features';
import { useAppStore } from '../../app/store';
import { projectFixture } from '../../app/testing/fixtures';
import {
  answerChoice,
  CHANGED_OUTSIDE,
  choiceIds,
  closeAlert,
  createHarness,
  HANDLE_A,
  HANDLE_B,
  type Harness,
  ipcFailure,
  nextChoice,
  nextDialog,
  type OpenInEditorMock,
  TRUSTED,
} from '../recovery/testing';
import { ExternalChangeController } from './controller';
import { externalChangeFeature, installExternalChangeFeature } from './feature';
import { KEEP_MINE_LABEL, LATER_LABEL, RELOAD_LABEL } from './messages';

let harness: Harness;
let openInEditor: OpenInEditorMock;
let uninstall: () => void;
let saveAs: Mock<() => void | Promise<void>>;

const RELOADED: ProjectReloadResponse = {
  document: '{"reloaded":true}',
  trust: TRUSTED,
  migratedFrom: null,
};

function open(overrides: Parameters<typeof projectFixture>[0] = {}): void {
  useAppStore.getState().actions.setProject(
    projectFixture({
      handle: HANDLE_A,
      fileName: 'game.b2c',
      document: {
        ...projectFixture().document,
        project: { ...projectFixture().document.project, name: 'Game' },
      },
      ...overrides,
    }),
  );
}

/** The backend's watcher reports a change. */
function changedOnDisk(deleted = false, handle = HANDLE_A): void {
  harness.events.emit({ kind: 'projectChangedOnDisk', handle, deleted });
}

/** Lets the controller act on answers given so far (the app's events arrive as tasks). */
async function settled(): Promise<void> {
  await new Promise((resolve) => setTimeout(resolve, 0));
}

/** A save was refused with `changedOnDisk`. */
function saveRefused(handle = HANDLE_A): void {
  harness.events.emit({ kind: 'project:changedOnDisk', handle });
}

beforeEach(() => {
  harness = createHarness();
  openInEditor = vi.fn((_ctx, args) => {
    useAppStore.getState().actions.updateProject({
      canonicalText: args.documentText,
      savedCanonicalText: args.documentText,
      dirty: false,
      trust: args.trust,
    });
    return Promise.resolve({ ok: true as const });
  });
  harness.ipc.projectReload.mockResolvedValue(RELOADED);
  saveAs = vi.fn<() => void | Promise<void>>(() => undefined);
  harness.commands.registerCommand('project.saveAs', saveAs);
  uninstall = installExternalChangeFeature(harness.ctx, { openInEditor }).uninstall;
  open();
});

afterEach(() => {
  uninstall();
  harness.dispose();
});

describe('the question', () => {
  it('offers Reload first when there are no unsaved changes', async () => {
    changedOnDisk();
    const question = await nextChoice(harness.dialogs);
    expect(question.options.title).toBe('“Game” was changed outside Blocks2Cpp');
    expect(question.options.message).toContain('Another program changed game.b2c on disk.');
    expect(question.options.choices).toEqual([
      { id: 'reload', label: RELOAD_LABEL, primary: true },
      { id: 'keepMine', label: KEEP_MINE_LABEL },
      { id: 'later', label: LATER_LABEL },
    ]);
    expect(question.options.cancel).toBe('later');
  });

  it('warns that Reload discards unsaved changes, and suggests keeping them', async () => {
    useAppStore.getState().actions.updateProject({ dirty: true });
    changedOnDisk();
    const question = await nextChoice(harness.dialogs);
    expect(question.options.message).toContain(
      'Reloading discards the changes you have not saved.',
    );
    expect(question.options.choices).toEqual([
      { id: 'reload', label: RELOAD_LABEL, destructive: true },
      { id: 'keepMine', label: KEEP_MINE_LABEL, primary: true },
      { id: 'later', label: LATER_LABEL },
    ]);
  });

  it('offers only Keep mine when the file was deleted or moved', async () => {
    changedOnDisk(true);
    const question = await nextChoice(harness.dialogs);
    expect(question.options.title).toBe('“Game” was deleted or moved');
    expect(question.options.message).toContain('It cannot be reloaded.');
    expect(choiceIds(question)).toEqual(['keepMine', 'later']);
    expect(question.options.choices[0]?.primary).toBe(true);
  });

  it('shows hidden characters in the names it quotes', async () => {
    open({ fileName: 'a‮b.b2c' });
    changedOnDisk();
    expect((await nextChoice(harness.dialogs)).options.message).toContain('a⟨U+202E⟩b.b2c');
  });

  it('asks once for reports that arrive while it is open', async () => {
    changedOnDisk();
    changedOnDisk();
    saveRefused();
    await nextChoice(harness.dialogs);
    expect(harness.dialogs.store.getState().queue).toHaveLength(1);
    await answerChoice(harness.dialogs, 'later');
    await vi.waitFor(() => {
      expect(harness.dialogs.store.getState().queue).toHaveLength(0);
    });
  });

  it('ignores reports about other projects and projects without a file', async () => {
    changedOnDisk(false, HANDLE_B);
    open({ fileName: null });
    changedOnDisk();
    await Promise.resolve();
    expect(harness.dialogs.store.getState().queue).toHaveLength(0);
  });

  it('asks about a save that was refused because the file changed', async () => {
    saveRefused();
    expect(choiceIds(await nextChoice(harness.dialogs))).toEqual(['reload', 'keepMine', 'later']);
  });
});

describe('Reload', () => {
  it('reads the file again and shows it through the core, with its trust state', async () => {
    useAppStore.getState().actions.updateProject({ dirty: true, migratedFrom: null });
    harness.ipc.projectReload.mockResolvedValue({
      document: '{"outside":true}',
      trust: CHANGED_OUTSIDE,
      migratedFrom: 0,
    });
    changedOnDisk();
    await answerChoice(harness.dialogs, 'reload');
    await vi.waitFor(() => {
      expect(openInEditor).toHaveBeenCalledTimes(1);
    });
    expect(harness.ipc.projectReload).toHaveBeenCalledWith({ handle: HANDLE_A });
    expect(openInEditor).toHaveBeenCalledWith(harness.ctx, {
      handle: HANDLE_A,
      documentText: '{"outside":true}',
      trust: CHANGED_OUTSIDE,
      fileName: 'game.b2c',
      migratedFrom: 0,
      savedText: '{"outside":true}',
    });
    const project = useAppStore.getState().project;
    expect(project?.trust).toEqual(CHANGED_OUTSIDE);
    expect(project?.dirty).toBe(false);
    expect(harness.dialogs.store.getState().queue).toHaveLength(0);
  });

  it('asks again without Reload when the file turns out to be gone', async () => {
    vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    harness.ipc.projectReload.mockRejectedValueOnce(ipcFailure({ code: 'notFound' }));
    saveRefused();
    await answerChoice(harness.dialogs, 'reload');
    const again = await vi.waitFor(async () => {
      const question = await nextChoice(harness.dialogs);
      if ((question.options.title ?? '').includes('changed')) {
        throw new Error('still the first question');
      }
      return question;
    });
    expect(choiceIds(again)).toEqual(['keepMine', 'later']);
    again.settle('keepMine');
    await vi.waitFor(() => {
      expect(saveAs).toHaveBeenCalledTimes(1);
    });
    expect(openInEditor).not.toHaveBeenCalled();
  });

  it('treats an io notFound like a file that is gone', async () => {
    vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    harness.ipc.projectReload.mockRejectedValueOnce(ipcFailure({ code: 'io', kind: 'notFound' }));
    changedOnDisk();
    await answerChoice(harness.dialogs, 'reload');
    await vi.waitFor(() => {
      expect(harness.dialogs.store.getState().queue[0]?.options.title).toBe(
        '“Game” was deleted or moved',
      );
    });
  });

  it('reloads when asked even after a report said the file is gone', async () => {
    changedOnDisk();
    const question = await nextChoice(harness.dialogs);
    changedOnDisk(true);
    question.settle('reload');
    await vi.waitFor(() => {
      expect(openInEditor).toHaveBeenCalledTimes(1);
    });
  });

  it('asks again when the file changed once more while it was being read', async () => {
    let answer: (value: ProjectReloadResponse) => void = () => undefined;
    harness.ipc.projectReload.mockReturnValueOnce(
      new Promise((resolve) => {
        answer = resolve;
      }),
    );
    changedOnDisk();
    await answerChoice(harness.dialogs, 'reload');
    await vi.waitFor(() => {
      expect(harness.ipc.projectReload).toHaveBeenCalledTimes(1);
    });
    changedOnDisk();
    answer(RELOADED);
    const again = await nextChoice(harness.dialogs);
    expect(choiceIds(again)).toEqual(['reload', 'keepMine', 'later']);
    again.settle('later');
  });

  it.each([
    [
      'a file the loader refuses',
      {
        code: 'invalidDocument' as const,
        diagnostics: [
          {
            code: 'B2C-E0104',
            severity: 'error' as const,
            message: 'Not JSON',
            primary: { part: { kind: 'whole' as const } },
            source: 'loader' as const,
          },
        ],
      },
      'B2C-E0104: Not JSON',
    ],
    ['a newer format', { code: 'newerFormat' as const, needs: null }, 'newer version'],
    ['a huge file', { code: 'payloadTooLarge' as const, limit: 33554432 }, '32 MiB'],
    ['no permission', { code: 'io' as const, kind: 'permissionDenied' as const }, 'not allowed'],
    ['a read failure', { code: 'io' as const, kind: 'other' as const }, 'could not be read'],
    ['a bug', { code: 'internal' as const }, '(Error: internal)'],
  ])('explains %s and keeps the blocks', async (_what, error, expected) => {
    vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    harness.ipc.projectReload.mockRejectedValueOnce(ipcFailure(error));
    changedOnDisk();
    await answerChoice(harness.dialogs, 'reload');
    const alert = await closeAlert(harness.dialogs);
    expect(alert.options.title).toBe('“Game” could not be reloaded');
    expect(alert.options.message).toContain(expected);
    expect(openInEditor).not.toHaveBeenCalled();
  });

  it('asks again when the file changes while a failed reload is explained', async () => {
    vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    harness.ipc.projectReload.mockRejectedValueOnce(ipcFailure({ code: 'io', kind: 'other' }));
    changedOnDisk();
    await answerChoice(harness.dialogs, 'reload');
    const alert = await nextDialog(harness.dialogs);
    changedOnDisk();
    if (alert.kind !== 'alert') {
      throw new Error(`expected an alert, got ${alert.kind}`);
    }
    alert.settle();
    await settled();
    expect(choiceIds(await nextChoice(harness.dialogs))).toEqual(['reload', 'keepMine', 'later']);
  });

  it('says nothing when the project was closed meanwhile', async () => {
    vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    harness.ipc.projectReload.mockRejectedValueOnce(ipcFailure({ code: 'unknownHandle' }));
    changedOnDisk();
    await answerChoice(harness.dialogs, 'reload');
    await vi.waitFor(() => {
      expect(harness.ipc.projectReload).toHaveBeenCalled();
    });
    await Promise.resolve();
    expect(harness.dialogs.store.getState().queue).toHaveLength(0);
  });

  it('keeps the blocks as unsaved changes when the editor cannot show the file', async () => {
    vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    vi.spyOn(console, 'error').mockImplementation(() => undefined);
    harness.ipc.projectReload.mockResolvedValue({ ...RELOADED, trust: CHANGED_OUTSIDE });
    openInEditor.mockResolvedValueOnce({
      ok: false,
      diagnostics: [
        {
          code: 'B2C-E0110',
          severity: 'error',
          message: 'Bad',
          primary: { part: { kind: 'whole' } },
          source: 'loader',
        },
      ],
    });
    changedOnDisk();
    await answerChoice(harness.dialogs, 'reload');
    const alert = await closeAlert(harness.dialogs);
    expect(alert.options.message).toContain('B2C-E0110: Bad');
    const project = useAppStore.getState().project;
    // The backend now enforces the reloaded file's trust, and the blocks differ from the file.
    expect(project?.trust).toEqual(CHANGED_OUTSIDE);
    expect(project?.dirty).toBe(true);
    expect(project?.savedCanonicalText).toBeNull();
    expect(harness.ipc.projectSetDirty).toHaveBeenCalledWith({ handle: HANDLE_A, dirty: true });
    await settled();

    openInEditor.mockRejectedValueOnce(new Error('trap'));
    harness.ipc.projectSetDirty.mockRejectedValueOnce(ipcFailure({ code: 'unknownHandle' }));
    changedOnDisk();
    await answerChoice(harness.dialogs, 'reload');
    expect((await closeAlert(harness.dialogs)).options.message).toContain('stopped');
  });

  it('does nothing when the project was closed while the question was open', async () => {
    changedOnDisk();
    const question = await nextChoice(harness.dialogs);
    useAppStore.getState().actions.setProject(null);
    question.settle('reload');
    await Promise.resolve();
    await Promise.resolve();
    expect(harness.ipc.projectReload).not.toHaveBeenCalled();
  });
});

describe('Keep mine and Not now', () => {
  it('Keep mine runs project.saveAs', async () => {
    changedOnDisk();
    await answerChoice(harness.dialogs, 'keepMine');
    await vi.waitFor(() => {
      expect(saveAs).toHaveBeenCalledTimes(1);
    });
    expect(harness.ipc.projectReload).not.toHaveBeenCalled();
  });

  it('Keep mine says so when there is no save-as command', async () => {
    vi.spyOn(console, 'error').mockImplementation(() => undefined);
    const fresh = createHarness();
    fresh.commands.registerCommand('project.save', () => undefined);
    const controller = new ExternalChangeController(fresh.ctx, { openInEditor });
    open();
    const done = controller.notify(HANDLE_A, false);
    await answerChoice(fresh.dialogs, 'keepMine');
    expect((await closeAlert(fresh.dialogs)).options.message).toContain('noSaveAsCommand');
    await done;
    fresh.dispose();
  });

  it('Keep mine logs a failing save-as command', async () => {
    const error = vi.spyOn(console, 'error').mockImplementation(() => undefined);
    saveAs.mockImplementation(() => Promise.reject(new Error('broken')));
    changedOnDisk();
    await answerChoice(harness.dialogs, 'keepMine');
    await vi.waitFor(() => {
      expect(error).toHaveBeenCalledWith(
        'Save as… failed after an outside change',
        expect.any(Error),
      );
    });
  });

  it('Not now changes nothing, and the next report asks again', async () => {
    changedOnDisk();
    await answerChoice(harness.dialogs, 'later');
    await settled();
    expect(harness.dialogs.store.getState().queue).toHaveLength(0);
    expect(harness.ipc.projectReload).not.toHaveBeenCalled();
    expect(saveAs).not.toHaveBeenCalled();
    saveRefused();
    expect(choiceIds(await nextChoice(harness.dialogs))).toEqual(['reload', 'keepMine', 'later']);
  });
});

describe('the controller', () => {
  it('waits for an open question about a replaced project before asking about the new one', async () => {
    const controller = new ExternalChangeController(harness.ctx, { openInEditor });
    const first = controller.notify(HANDLE_A, false);
    // The question about HANDLE_A is shown; the project is replaced meanwhile.
    const firstQuestion = await nextChoice(harness.dialogs);
    open({ handle: HANDLE_B });
    const second = controller.notify(HANDLE_B, true);
    expect(harness.dialogs.store.getState().queue).toHaveLength(1);
    firstQuestion.settle('reload');
    await first;
    const question = await nextChoice(harness.dialogs);
    expect(choiceIds(question)).toEqual(['keepMine', 'later']);
    question.settle('later');
    await second;
    // The answer about HANDLE_A came after its project was replaced: nothing was reloaded.
    expect(harness.ipc.projectReload).not.toHaveBeenCalled();
  });

  it('installs as a Feature that listens until it is uninstalled', async () => {
    uninstall();
    const remove = installAll([externalChangeFeature], harness.ctx);
    changedOnDisk();
    await answerChoice(harness.dialogs, 'later');
    remove();
    await settled();
    changedOnDisk();
    await settled();
    expect(harness.dialogs.store.getState().queue).toHaveLength(0);
  });

  it('ignores reports after it is uninstalled', async () => {
    uninstall();
    changedOnDisk();
    saveRefused();
    await Promise.resolve();
    expect(harness.dialogs.store.getState().queue).toHaveLength(0);
  });

  it('ends a question that is open when it is disposed', async () => {
    const controller = new ExternalChangeController(harness.ctx, { openInEditor });
    const done = controller.notify(HANDLE_A, false);
    const question = await nextChoice(harness.dialogs);
    controller.dispose();
    question.settle('reload');
    await done;
    expect(harness.ipc.projectReload).not.toHaveBeenCalled();
    await controller.notify(HANDLE_A, false);
  });

  it('logs a bug in a step and still settles', async () => {
    const error = vi.spyOn(console, 'error').mockImplementation(() => undefined);
    const controller = new ExternalChangeController(harness.ctx, { openInEditor });
    vi.spyOn(harness.dialogs, 'choose').mockRejectedValueOnce(new Error('bug'));
    await controller.notify(HANDLE_A, false);
    expect(error).toHaveBeenCalledWith(
      'Handling an outside change of the project file failed',
      expect.any(Error),
    );
  });
});
