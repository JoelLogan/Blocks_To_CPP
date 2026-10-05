/**
 * The external-change dialog as the user sees it, through the app's dialog host: its buttons for
 * a changed and for a deleted file, answering by click and by Escape, and axe-core on both.
 */
import { fireEvent, render, screen, within } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, type Mock, vi } from 'vitest';

import { DialogHost } from '../../app/dialogs';
import { useAppStore } from '../../app/store';
import { projectFixture } from '../../app/testing/fixtures';
import { expectNoAxeViolations } from '../../test/axe';
import {
  createHarness,
  HANDLE_A,
  type Harness,
  type OpenInEditorMock,
  TRUSTED,
} from '../recovery/testing';
import { installExternalChangeFeature } from './feature';

let harness: Harness;
let openInEditor: OpenInEditorMock;
let uninstall: () => void;
let saveAs: Mock<() => void | Promise<void>>;

beforeEach(() => {
  harness = createHarness();
  openInEditor = vi.fn(() => Promise.resolve({ ok: true as const }));
  harness.ipc.projectReload.mockResolvedValue({
    document: '{}',
    trust: TRUSTED,
    migratedFrom: null,
  });
  saveAs = vi.fn<() => void | Promise<void>>(() => undefined);
  harness.commands.registerCommand('project.saveAs', saveAs);
  uninstall = installExternalChangeFeature(harness.ctx, { openInEditor }).uninstall;
  useAppStore
    .getState()
    .actions.setProject(projectFixture({ handle: HANDLE_A, fileName: 'guessing-game.b2c' }));
  render(<DialogHost queue={harness.dialogs} />);
});

afterEach(() => {
  uninstall();
  harness.dispose();
});

describe('the external-change dialog', () => {
  it('offers Reload, Keep mine and Not now for a changed file, and reloads on click', async () => {
    harness.events.emit({ kind: 'projectChangedOnDisk', handle: HANDLE_A, deleted: false });
    const dialog = await screen.findByRole('dialog', {
      name: '“Guessing Game” was changed outside Blocks2Cpp',
    });
    await expectNoAxeViolations(dialog);
    const buttons = within(dialog)
      .getAllByRole('button')
      .map((button) => button.textContent);
    expect(buttons).toEqual(['Reload', 'Keep mine (save as…)', 'Not now']);
    // Nothing unsaved would be lost: Reload is the suggested answer.
    expect(document.activeElement).toBe(within(dialog).getByRole('button', { name: 'Reload' }));

    fireEvent.click(within(dialog).getByRole('button', { name: 'Reload' }));
    await vi.waitFor(() => {
      expect(openInEditor).toHaveBeenCalledTimes(1);
    });
    expect(screen.queryByRole('dialog')).toBeNull();
  });

  it('has no Reload for a deleted file, and keeps mine on click', async () => {
    harness.events.emit({ kind: 'projectChangedOnDisk', handle: HANDLE_A, deleted: true });
    const dialog = await screen.findByRole('dialog', {
      name: '“Guessing Game” was deleted or moved',
    });
    await expectNoAxeViolations(dialog);
    expect(within(dialog).queryByRole('button', { name: 'Reload' })).toBeNull();
    const keepMine = within(dialog).getByRole('button', { name: 'Keep mine (save as…)' });
    expect(document.activeElement).toBe(keepMine);
    fireEvent.click(keepMine);
    await vi.waitFor(() => {
      expect(saveAs).toHaveBeenCalledTimes(1);
    });
    expect(harness.ipc.projectReload).not.toHaveBeenCalled();
  });

  it('suggests keeping unsaved changes, and Escape decides later', async () => {
    useAppStore.getState().actions.updateProject({ dirty: true });
    harness.events.emit({ kind: 'project:changedOnDisk', handle: HANDLE_A });
    const dialog = await screen.findByRole('dialog');
    expect(document.activeElement).toBe(
      within(dialog).getByRole('button', { name: 'Keep mine (save as…)' }),
    );
    expect(within(dialog).getByRole('button', { name: 'Reload' }).className).toContain(
      'button-danger',
    );
    fireEvent.keyDown(dialog, { key: 'Escape' });
    await vi.waitFor(() => {
      expect(screen.queryByRole('dialog')).toBeNull();
    });
    expect(harness.ipc.projectReload).not.toHaveBeenCalled();
    expect(saveAs).not.toHaveBeenCalled();
  });
});
