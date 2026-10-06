/**
 * The recovery offer's controller over a fake backend: listing, restoring (with the compiler core
 * started first, the document shown through `openDocumentInEditor`, and one project per window)
 * and discarding snapshots, and every failure the user is told about.
 */
import type { CoreWasm } from '@blocks2cpp/b2c-core-wasm';
import type { RecoveryRestoreResponse } from '@blocks2cpp/ipc-types';
import { afterEach, beforeEach, describe, expect, it, type Mock, vi } from 'vitest';

import { useAppStore } from '../../app/store';
import { documentFixture, projectFixture } from '../../app/testing/fixtures';
import { RecoveryController } from './controller';
import { createRecoveryModel, MAX_OFFERED_SNAPSHOTS, offeredSnapshots } from './model';
import type { ProjectLifecycle } from '../project/lifecycle';
import { createProjectModel, type ProjectModel } from '../project/model';
import {
  answerChoice,
  answerConfirm,
  CREATED_HERE,
  closeAlert,
  createHarness,
  HANDLE_A,
  HANDLE_B,
  type Harness,
  ipcFailure,
  nextDialog,
  type OpenInEditorMock,
  projectQueue,
  snapshotId,
  snapshotInfo,
  TRUSTED,
} from './testing';

let harness: Harness;
let openInEditor: OpenInEditorMock;
let startCore: Mock<() => Promise<CoreWasm>>;
let queue: ProjectLifecycle;
let projectModel: ProjectModel;

function restored(overrides: Partial<RecoveryRestoreResponse> = {}): RecoveryRestoreResponse {
  return {
    handle: HANDLE_B,
    document: '{"restored":true}',
    trust: TRUSTED,
    fileName: 'game.b2c',
    ...overrides,
  };
}

/** A controller whose offer lists `count` snapshots. */
async function controllerWith(count = 2): Promise<{
  controller: RecoveryController;
  model: ReturnType<typeof createRecoveryModel>;
}> {
  const model = createRecoveryModel();
  harness.ipc.recoveryList.mockResolvedValue({
    snapshots: Array.from({ length: count }, (_, index) => snapshotInfo(index + 1)),
  });
  const controller = new RecoveryController(harness.ctx, model, {
    openInEditor,
    startCore,
    project: queue,
  });
  await controller.refresh();
  return { controller, model };
}

/** What `openInEditor` does when it succeeds: the restored project becomes the open one. */
function showsProject(): void {
  openInEditor.mockImplementation((_ctx, args) => {
    useAppStore
      .getState()
      .actions.setProject(
        projectFixture({ handle: args.handle, fileName: args.fileName, dirty: true }),
      );
    return Promise.resolve({ ok: true });
  });
}

beforeEach(() => {
  harness = createHarness();
  projectModel = createProjectModel();
  queue = projectQueue(harness, projectModel);
  openInEditor = vi.fn();
  startCore = vi.fn(() => Promise.resolve({} as CoreWasm));
  showsProject();
  harness.ipc.recoveryRestore.mockResolvedValue(restored());
  harness.ipc.recoveryDiscard.mockResolvedValue({});
});

afterEach(() => {
  harness.dispose();
});

describe('the offered snapshots', () => {
  it('lists what recovery_list answers', async () => {
    const { model } = await controllerWith(2);
    expect(model.getState()).toEqual({
      status: 'ready',
      snapshots: [snapshotInfo(1), snapshotInfo(2)],
      busy: null,
    });
  });

  it('shows that the list could not be read', async () => {
    vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    const model = createRecoveryModel();
    harness.ipc.recoveryList.mockRejectedValue(ipcFailure({ code: 'io', kind: 'other' }));
    const controller = new RecoveryController(harness.ctx, model, {
      openInEditor,
      startCore,
      project: queue,
    });
    await controller.refresh();
    expect(model.getState().status).toBe('failed');
  });

  it('keeps only well-formed entries, each once, at most 100', () => {
    const good = snapshotInfo(1);
    expect(
      offeredSnapshots([
        good,
        good,
        { ...good, snapshotId: 'sn_XYZ' },
        { ...snapshotInfo(2), hasPath: 'yes' },
        { ...snapshotInfo(3), savedAt: 7 },
        { ...snapshotInfo(4), savedAt: 'x'.repeat(65) },
        { ...snapshotInfo(5), projectName: null },
        null,
        [],
        'sn_00000000000000000000000000000006',
      ]),
    ).toEqual([good]);
    expect(offeredSnapshots({ snapshots: [] })).toEqual([]);
    const many = Array.from({ length: 150 }, (_, index) => snapshotInfo(index + 1));
    expect(offeredSnapshots(many)).toHaveLength(MAX_OFFERED_SNAPSHOTS);
    expect(
      offeredSnapshots([snapshotInfo(1, { projectName: 'n'.repeat(5000) })])[0]?.projectName,
    ).toHaveLength(1024);
  });

  it('drops a late answer to an older request', async () => {
    const model = createRecoveryModel();
    const controller = new RecoveryController(harness.ctx, model, {
      openInEditor,
      startCore,
      project: queue,
    });
    let answerFirst: (value: { snapshots: [] }) => void = () => undefined;
    harness.ipc.recoveryList.mockReturnValueOnce(
      new Promise((resolve) => {
        answerFirst = resolve;
      }),
    );
    harness.ipc.recoveryList.mockResolvedValueOnce({ snapshots: [snapshotInfo(9)] });
    const first = controller.refresh();
    await controller.refresh();
    answerFirst({ snapshots: [] });
    await first;
    expect(model.getState().snapshots).toEqual([snapshotInfo(9)]);
  });
});

describe('restoring a snapshot', () => {
  it('starts the core first, then opens the snapshot as a project with unsaved changes', async () => {
    const { controller, model } = await controllerWith(2);
    const calls: string[] = [];
    startCore.mockImplementation(() => {
      calls.push('core');
      return Promise.resolve({} as CoreWasm);
    });
    harness.ipc.recoveryRestore.mockImplementation(() => {
      calls.push('restore');
      return Promise.resolve(restored({ trust: CREATED_HERE, fileName: null }));
    });

    await expect(controller.restore(snapshotId(1))).resolves.toBe(true);
    expect(calls).toEqual(['core', 'restore']);
    expect(harness.ipc.recoveryRestore).toHaveBeenCalledWith({ snapshotId: snapshotId(1) });
    expect(openInEditor).toHaveBeenCalledWith(harness.ctx, {
      handle: HANDLE_B,
      documentText: '{"restored":true}',
      trust: CREATED_HERE,
      fileName: null,
      migratedFrom: null,
      savedText: null,
    });
    expect(model.getState().snapshots).toEqual([snapshotInfo(2)]);
    expect(model.getState().busy).toBeNull();
    // No project was open, so none is closed.
    expect(harness.ipc.projectClose).not.toHaveBeenCalled();
  });

  it('closes the project it replaces once the restored one is shown', async () => {
    useAppStore.getState().actions.setProject(projectFixture({ handle: HANDLE_A }));
    const { controller } = await controllerWith(1);
    await expect(controller.restore(snapshotId(1))).resolves.toBe(true);
    expect(harness.ipc.projectClose).toHaveBeenCalledWith({ handle: HANDLE_A });
    expect(harness.ipc.projectClose).toHaveBeenCalledTimes(1);
  });

  it("asks about the open project's unsaved changes first", async () => {
    useAppStore.getState().actions.setProject(projectFixture({ handle: HANDLE_A, dirty: true }));
    const { controller } = await controllerWith(1);

    // Cancel: nothing happens.
    let restoring = controller.restore(snapshotId(1));
    const question = await answerChoice(harness.dialogs, 'cancel');
    expect(question.options.choices.map((choice) => choice.id)).toEqual([
      'save',
      'discard',
      'cancel',
    ]);
    await expect(restoring).resolves.toBe(false);
    expect(harness.ipc.recoveryRestore).not.toHaveBeenCalled();

    // Don't save: restored, and the old project closed (which drops its changes).
    restoring = controller.restore(snapshotId(1));
    await answerChoice(harness.dialogs, 'discard');
    await expect(restoring).resolves.toBe(true);
    expect(harness.ipc.projectClose).toHaveBeenCalledWith({ handle: HANDLE_A });
  });

  it('runs in the project lifecycle, which is busy restoring meanwhile', async () => {
    const { controller } = await controllerWith(1);
    const busy: unknown[] = [];
    startCore.mockImplementation(() => {
      busy.push(projectModel.getState().busy);
      return Promise.resolve({} as CoreWasm);
    });
    await expect(controller.restore(snapshotId(1))).resolves.toBe(true);
    expect(busy).toEqual(['restore']);
  });

  it('restores nothing without the project lifecycle, and says so', async () => {
    const error = vi.spyOn(console, 'error').mockImplementation(() => undefined);
    const model = createRecoveryModel();
    harness.ipc.recoveryList.mockResolvedValue({ snapshots: [snapshotInfo(1)] });
    const controller = new RecoveryController(harness.ctx, model, { openInEditor, startCore });
    await controller.refresh();
    const restoring = controller.restore(snapshotId(1));
    expect((await closeAlert(harness.dialogs)).options.message).toContain('noProjectLifecycle');
    await expect(restoring).resolves.toBe(false);
    expect(error).toHaveBeenCalled();
    expect(startCore).not.toHaveBeenCalled();
    expect(model.getState().snapshots).toHaveLength(1);
  });

  it('leaves the snapshot alone when the compiler core cannot start', async () => {
    vi.spyOn(console, 'error').mockImplementation(() => undefined);
    const { controller, model } = await controllerWith(1);
    startCore.mockRejectedValue(new Error('no core'));
    const restoring = controller.restore(snapshotId(1));
    expect((await closeAlert(harness.dialogs)).options.message).toContain('could not start');
    await expect(restoring).resolves.toBe(false);
    expect(harness.ipc.recoveryRestore).not.toHaveBeenCalled();
    expect(model.getState().snapshots).toHaveLength(1);
  });

  it('drops a snapshot that is no longer there and reads the list again', async () => {
    vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    const { controller, model } = await controllerWith(2);
    harness.ipc.recoveryRestore.mockRejectedValue(ipcFailure({ code: 'unknownSnapshot' }));
    harness.ipc.recoveryList.mockResolvedValue({ snapshots: [snapshotInfo(2)] });
    const restoring = controller.restore(snapshotId(1));
    expect((await closeAlert(harness.dialogs)).options.message).toContain('no longer available');
    await expect(restoring).resolves.toBe(false);
    expect(model.getState().snapshots).toEqual([snapshotInfo(2)]);
    expect(harness.ipc.recoveryList).toHaveBeenCalledTimes(2);
  });

  it.each([
    [
      'a document the loader refuses',
      {
        code: 'invalidDocument' as const,
        diagnostics: [
          {
            code: 'B2C-E0105',
            severity: 'error' as const,
            message: 'Unknown key ‮here',
            primary: { part: { kind: 'whole' as const } },
            source: 'loader' as const,
          },
        ],
      },
      'B2C-E0105: Unknown key ⟨U+202E⟩here',
    ],
    ['a newer format', { code: 'newerFormat' as const, needs: '9.0.0' }, 'needs ≥ 9.0.0'],
    ['too many projects', { code: 'tooManyHandles' as const }, 'Too many projects'],
    ['no permission', { code: 'io' as const, kind: 'permissionDenied' as const }, 'not allowed'],
    ['a read failure', { code: 'io' as const, kind: 'other' as const }, 'could not be read'],
    ['a bug', { code: 'internal' as const }, '(Error: internal)'],
  ])('explains %s and keeps the snapshot', async (_what, error, expected) => {
    vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    const { controller, model } = await controllerWith(1);
    harness.ipc.recoveryRestore.mockRejectedValue(ipcFailure(error));
    const restoring = controller.restore(snapshotId(1));
    expect((await closeAlert(harness.dialogs)).options.message).toContain(expected);
    await expect(restoring).resolves.toBe(false);
    expect(model.getState().snapshots).toHaveLength(1);
    expect(openInEditor).not.toHaveBeenCalled();
  });

  it('keeps the restored handle open when the editor cannot show it, so nothing is lost', async () => {
    vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    vi.spyOn(console, 'error').mockImplementation(() => undefined);
    useAppStore.getState().actions.setProject(projectFixture({ handle: HANDLE_A }));
    const { controller, model } = await controllerWith(2);

    openInEditor.mockResolvedValueOnce({
      ok: false,
      diagnostics: [
        {
          code: 'B2C-E0110',
          severity: 'error',
          message: 'Bad block',
          primary: { part: { kind: 'whole' } },
          source: 'loader',
        },
      ],
    });
    let restoring = controller.restore(snapshotId(1));
    const alert = await closeAlert(harness.dialogs);
    expect(alert.options.message).toContain('B2C-E0110: Bad block');
    expect(alert.options.message).toContain('offered again');
    await expect(restoring).resolves.toBe(false);

    openInEditor.mockRejectedValueOnce(new Error('trap'));
    restoring = controller.restore(snapshotId(2));
    expect((await closeAlert(harness.dialogs)).options.message).toContain('stopped');
    await expect(restoring).resolves.toBe(false);

    // Neither the restored handle nor the open project was closed; the backend took both snapshots.
    expect(harness.ipc.projectClose).not.toHaveBeenCalled();
    expect(useAppStore.getState().project?.handle).toBe(HANDLE_A);
    expect(model.getState().snapshots).toEqual([]);
  });

  it('runs one operation at a time and ignores snapshots it does not offer', async () => {
    const { controller, model } = await controllerWith(2);
    let finish: (value: RecoveryRestoreResponse) => void = () => undefined;
    harness.ipc.recoveryRestore.mockReturnValueOnce(
      new Promise((resolve) => {
        finish = resolve;
      }),
    );
    const first = controller.restore(snapshotId(1));
    await vi.waitFor(() => {
      expect(harness.ipc.recoveryRestore).toHaveBeenCalled();
    });
    expect(model.getState().busy).toEqual({ kind: 'restore', snapshotId: snapshotId(1) });
    await expect(controller.restore(snapshotId(2))).resolves.toBe(false);
    await expect(controller.discard(snapshotId(2))).resolves.toBe(false);
    finish(restored());
    await expect(first).resolves.toBe(true);
    await expect(controller.restore(snapshotId(77))).resolves.toBe(false);
    expect(harness.ipc.recoveryRestore).toHaveBeenCalledTimes(1);
  });

  it('does nothing once disposed', async () => {
    const { controller } = await controllerWith(1);
    controller.dispose();
    await expect(controller.restore(snapshotId(1))).resolves.toBe(false);
    await controller.refresh();
    expect(harness.ipc.recoveryList).toHaveBeenCalledTimes(1);
  });
});

describe('discarding a snapshot', () => {
  it('asks first, as it cannot be undone', async () => {
    const { controller, model } = await controllerWith(2);
    let discarding = controller.discard(snapshotId(1));
    const question = await answerConfirm(harness.dialogs, false);
    expect(question.options.destructive).toBe(true);
    expect(question.options.title).toBe('Discard the unsaved work on “Project 1”?');
    await expect(discarding).resolves.toBe(false);
    expect(harness.ipc.recoveryDiscard).not.toHaveBeenCalled();

    discarding = controller.discard(snapshotId(1));
    await answerConfirm(harness.dialogs, true);
    await expect(discarding).resolves.toBe(true);
    expect(harness.ipc.recoveryDiscard).toHaveBeenCalledWith({ snapshotId: snapshotId(1) });
    expect(model.getState().snapshots).toEqual([snapshotInfo(2)]);
  });

  it('treats a snapshot that is gone already as discarded', async () => {
    const { controller, model } = await controllerWith(1);
    harness.ipc.recoveryDiscard.mockRejectedValue(ipcFailure({ code: 'unknownSnapshot' }));
    const discarding = controller.discard(snapshotId(1));
    await answerConfirm(harness.dialogs, true);
    await expect(discarding).resolves.toBe(true);
    expect(model.getState().snapshots).toEqual([]);
  });

  it.each([
    [{ code: 'io' as const, kind: 'permissionDenied' as const }, 'not allowed to delete'],
    [{ code: 'io' as const, kind: 'other' as const }, 'could not be deleted'],
    [{ code: 'internal' as const }, '(Error: internal)'],
  ])('explains a failure (%o) and keeps the snapshot', async (error, expected) => {
    vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    const { controller, model } = await controllerWith(1);
    harness.ipc.recoveryDiscard.mockRejectedValue(ipcFailure(error));
    const discarding = controller.discard(snapshotId(1));
    await answerConfirm(harness.dialogs, true);
    expect((await closeAlert(harness.dialogs)).options.message).toContain(expected);
    await expect(discarding).resolves.toBe(false);
    expect(model.getState().snapshots).toHaveLength(1);
  });

  it('names a snapshot whose time does not parse without a date', async () => {
    const model = createRecoveryModel();
    harness.ipc.recoveryList.mockResolvedValue({
      snapshots: [snapshotInfo(1, { savedAt: 'yesterday', projectName: '  ' })],
    });
    const controller = new RecoveryController(harness.ctx, model, {
      openInEditor,
      startCore,
      project: queue,
    });
    await controller.refresh();
    const discarding = controller.discard(snapshotId(1));
    const question = await nextDialog(harness.dialogs);
    expect(question.options.title).toBe('Discard the unsaved work on “Untitled project”?');
    expect(question.options.message).toBe(
      'It was saved automatically. Discarded work cannot be recovered.',
    );
    harness.dialogs.cancelAll();
    await expect(discarding).resolves.toBe(false);
  });
});

describe('the default core starter', () => {
  it('uses the published core when there is one', async () => {
    const core = {} as CoreWasm;
    harness = createHarness(core);
    harness.ipc.recoveryRestore.mockResolvedValue(restored());
    harness.ipc.recoveryList.mockResolvedValue({ snapshots: [snapshotInfo(1)] });
    const model = createRecoveryModel();
    const controller = new RecoveryController(harness.ctx, model, {
      openInEditor,
      project: projectQueue(harness),
    });
    await controller.refresh();
    await expect(controller.restore(snapshotId(1))).resolves.toBe(true);
    expect(harness.ipc.recoveryRestore).toHaveBeenCalledTimes(1);
    expect(useAppStore.getState().project?.document).toEqual(documentFixture());
  });
});
