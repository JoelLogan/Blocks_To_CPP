/** Export as a C++ project: the document sent, the outcome said, failures explained. */
import { IpcCallError } from '@blocks2cpp/ipc-types';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { resetAppStore, useAppStore } from '../../app/store';
import { createFakeIpc } from '../../app/testing/fixtures';
import { fakeDialogs, featureContext, openRunnableProject } from '../build-run/testing';
import { exportFailureMessage, exportFeature } from './feature';

beforeEach(() => {
  resetAppStore();
});

afterEach(() => {
  vi.restoreAllMocks();
});

function install() {
  const ipc = createFakeIpc();
  const dialogs = fakeDialogs();
  const ctx = featureContext(ipc, {}, dialogs);
  const uninstall = exportFeature(ctx);
  return { ipc, dialogs, ctx, uninstall };
}

describe('Export as C++ project', () => {
  it('sends the project and says where it went', async () => {
    openRunnableProject();
    const { ipc, dialogs, ctx } = install();
    ipc.projectExportDialog.mockResolvedValue({ cancelled: false, folderName: 'main', files: 5 });
    await ctx.commands.runCommand('project.export');
    const project = useAppStore.getState().project;
    expect(ipc.projectExportDialog).toHaveBeenCalledWith({
      handle: project?.handle,
      document: JSON.stringify(project?.document),
    });
    expect(dialogs.alert).toHaveBeenCalledWith(
      expect.objectContaining({
        title: 'Project exported',
        message: expect.stringContaining('“main”') as unknown,
      }),
    );
  });

  it('says nothing when the dialog is cancelled', async () => {
    openRunnableProject();
    const { ipc, dialogs, ctx } = install();
    ipc.projectExportDialog.mockResolvedValue({ cancelled: true, folderName: null, files: 0 });
    await ctx.commands.runCommand('project.export');
    expect(dialogs.alert).not.toHaveBeenCalled();
  });

  it('explains a refused export', async () => {
    openRunnableProject();
    vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    const { ipc, dialogs, ctx } = install();
    ipc.projectExportDialog.mockRejectedValue(
      new IpcCallError('project_export_dialog', { code: 'io', kind: 'alreadyExists' }),
    );
    await ctx.commands.runCommand('project.export');
    expect(dialogs.alert).toHaveBeenCalledWith(exportFailureMessage('io'));
    expect(exportFailureMessage('projectErrors').message).toContain('errors');
  });

  it('does nothing without a project', async () => {
    const { ipc, ctx } = install();
    await ctx.commands.runCommand('project.export');
    expect(ipc.projectExportDialog).not.toHaveBeenCalled();
  });
});
