/**
 * The start page's recovery offer, rendered: the listed snapshots, *Restore* and *Discard* through
 * the real dialog host, the failure note, and axe-core on the offer and its dialog.
 */
import type { CoreWasm } from '@blocks2cpp/b2c-core-wasm';
import { act, fireEvent, render, screen, within } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { DialogHost } from '../../app/dialogs';
import { useAppStore } from '../../app/store';
import { projectFixture } from '../../app/testing/fixtures';
import { expectNoAxeViolations } from '../../test/axe';
import { RecoveryController } from './controller';
import { createRecoveryModel, type RecoveryModel } from './model';
import { RecoveryOffer } from './RecoveryOffer';
import {
  createHarness,
  HANDLE_B,
  type Harness,
  ipcFailure,
  type OpenInEditorMock,
  snapshotId,
  snapshotInfo,
  TRUSTED,
} from './testing';

let harness: Harness;
let openInEditor: OpenInEditorMock;
let model: RecoveryModel;
let controller: RecoveryController;

async function renderOffer(snapshots = [snapshotInfo(1), snapshotInfo(2, { hasPath: false })]) {
  harness.ipc.recoveryList.mockResolvedValue({ snapshots });
  model = createRecoveryModel();
  controller = new RecoveryController(harness.ctx, model, {
    openInEditor,
    startCore: () => Promise.resolve({} as CoreWasm),
  });
  const view = render(
    <>
      <RecoveryOffer model={model} controller={controller} />
      <DialogHost queue={harness.dialogs} />
    </>,
  );
  await act(() => controller.refresh());
  return view;
}

beforeEach(() => {
  harness = createHarness();
  openInEditor = vi.fn((_ctx, args) => {
    useAppStore.getState().actions.setProject(projectFixture({ handle: args.handle, dirty: true }));
    return Promise.resolve({ ok: true as const });
  });
  harness.ipc.recoveryRestore.mockResolvedValue({
    handle: HANDLE_B,
    document: '{}',
    trust: TRUSTED,
    fileName: 'game.b2c',
  });
  harness.ipc.recoveryDiscard.mockResolvedValue({});
});

afterEach(() => {
  harness.dispose();
});

describe('the recovery offer', () => {
  it('lists each snapshot with Restore and Discard', async () => {
    const { container } = await renderOffer();
    const offer = screen.getByRole('region', { name: 'Recover unsaved work' });
    const items = within(offer).getAllByTestId('recovery-item');
    expect(items).toHaveLength(2);
    expect(items[0]?.textContent).toContain('Project 1');
    expect(items[0]?.textContent).toContain('Saved automatically on');
    expect(items[0]?.textContent).not.toContain('never saved to a file');
    expect(items[1]?.textContent).toContain('never saved to a file');
    expect(within(offer).getByRole('button', { name: 'Restore “Project 1”' })).toBeTruthy();
    expect(within(offer).getByRole('button', { name: 'Discard “Project 2”' })).toBeTruthy();
    await expectNoAxeViolations(container);
  });

  it('renders nothing while loading and when there is nothing to offer', async () => {
    model = createRecoveryModel();
    const { container, unmount } = render(
      <RecoveryOffer model={model} controller={new RecoveryController(harness.ctx, model)} />,
    );
    expect(container.childElementCount).toBe(0);
    unmount();
    const view = await renderOffer([]);
    expect(view.container.querySelector('[data-testid="recovery-offer"]')).toBeNull();
  });

  it('shows hidden characters in names and keeps long names short', async () => {
    await renderOffer([
      snapshotInfo(1, { projectName: 'evil‮gnp.exe' }),
      snapshotInfo(2, { projectName: 'x'.repeat(500) }),
    ]);
    expect(screen.getByRole('button', { name: 'Restore “evil⟨U+202E⟩gnp.exe”' })).toBeTruthy();
    const items = screen.getAllByTestId('recovery-item');
    expect(items[1]?.textContent.length).toBeLessThan(300);
  });

  it('restores a snapshot from its button', async () => {
    await renderOffer();
    fireEvent.click(screen.getByRole('button', { name: 'Restore “Project 1”' }));
    await vi.waitFor(() => {
      expect(openInEditor).toHaveBeenCalled();
    });
    await vi.waitFor(() => {
      expect(screen.getAllByTestId('recovery-item')).toHaveLength(1);
    });
    expect(harness.ipc.recoveryRestore).toHaveBeenCalledWith({ snapshotId: snapshotId(1) });
    expect(useAppStore.getState().project?.handle).toBe(HANDLE_B);
  });

  it('discards a snapshot after the confirmation dialog, which passes axe', async () => {
    await renderOffer();
    fireEvent.click(screen.getByRole('button', { name: 'Discard “Project 1”' }));
    const dialog = await screen.findByRole('dialog', {
      name: 'Discard the unsaved work on “Project 1”?',
    });
    await expectNoAxeViolations(dialog);
    // The safe answer has the focus, because discarding loses work.
    expect(document.activeElement).toBe(within(dialog).getByRole('button', { name: 'Keep it' }));
    fireEvent.click(within(dialog).getByRole('button', { name: 'Discard' }));
    await vi.waitFor(() => {
      expect(screen.getAllByTestId('recovery-item')).toHaveLength(1);
    });
    expect(harness.ipc.recoveryDiscard).toHaveBeenCalledWith({ snapshotId: snapshotId(1) });
    expect(screen.queryByRole('button', { name: 'Restore “Project 1”' })).toBeNull();
  });

  it('holds back the buttons and says what runs while an operation is running', async () => {
    await renderOffer();
    let finish: () => void = () => undefined;
    harness.ipc.recoveryRestore.mockReturnValueOnce(
      new Promise((resolve) => {
        finish = () => {
          resolve({ handle: HANDLE_B, document: '{}', trust: TRUSTED, fileName: null });
        };
      }),
    );
    fireEvent.click(screen.getByRole('button', { name: 'Restore “Project 1”' }));
    await vi.waitFor(() => {
      expect(screen.getByRole('status').textContent).toContain('Restoring “Project 1”…');
    });
    const discard = screen.getByRole('button', { name: 'Discard “Project 2”' });
    expect(discard.getAttribute('aria-disabled')).toBe('true');
    fireEvent.click(discard);
    expect(harness.dialogs.store.getState().queue).toHaveLength(0);
    await act(async () => {
      finish();
      await Promise.resolve();
    });
    await vi.waitFor(() => {
      expect(
        screen.getByRole('button', { name: 'Discard “Project 2”' }).getAttribute('aria-disabled'),
      ).toBe('false');
    });
  });

  it('says when the list could not be read, and reads it again on request', async () => {
    vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    harness.ipc.recoveryList.mockRejectedValueOnce(ipcFailure({ code: 'io', kind: 'other' }));
    model = createRecoveryModel();
    controller = new RecoveryController(harness.ctx, model, { openInEditor });
    const { container } = render(<RecoveryOffer model={model} controller={controller} />);
    await act(() => controller.refresh());
    expect(screen.getByRole('region', { name: 'Unsaved work' }).textContent).toContain(
      'could not check for unsaved work',
    );
    await expectNoAxeViolations(container);

    harness.ipc.recoveryList.mockResolvedValueOnce({ snapshots: [snapshotInfo(3)] });
    fireEvent.click(screen.getByRole('button', { name: 'Try again' }));
    expect(await screen.findByRole('button', { name: 'Restore “Project 3”' })).toBeTruthy();
  });
});
