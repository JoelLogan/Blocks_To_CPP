/**
 * The start page with Testing Library and axe-core: new project buttons, Open…, the recent list
 * (open, remove, empty, unreadable), the load-failure report (E0108, E0105), the sections other
 * features add, the busy state and hidden characters in names and paths; and the accessibility
 * of the page and of the unsaved-changes dialog.
 *
 * The page itself needs no compiler core; opening a project does, so those tests are skipped
 * without a build (unless B2C_REQUIRE_WASM is set, as in CI).
 */
import type { CoreWasm } from '@blocks2cpp/b2c-core-wasm';
import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';

import { DialogHost } from '../../app/dialogs';
import { useAppStore } from '../../app/store';
import { projectFixture } from '../../app/testing/fixtures';
import { expectNoAxeViolations } from '../../test/axe';
import {
  CORE_BUILT,
  CREATED_HERE,
  EMPTY_TEXT,
  HANDLE_A,
  HELLO_TEXT,
  type Harness,
  installHarness,
  ipcFailure,
  loaderDiagnostic,
  opened,
  realCore,
  recentEntry,
} from './testing';

let core: CoreWasm | null = null;
let harness: Harness;

beforeAll(async () => {
  if (CORE_BUILT) {
    core = await realCore();
  }
});

beforeEach(() => {
  harness = installHarness(core);
  vi.spyOn(console, 'warn').mockImplementation(() => undefined);
});

afterEach(() => {
  harness.dispose();
});

/** Renders the start page the feature registered, with the dialog host. */
function renderStartPage() {
  const StartScreen = harness.ctx.screens.screen('start');
  if (StartScreen === null) {
    throw new Error('the start page is not registered');
  }
  return render(
    <main aria-label="Start page">
      <StartScreen />
      <DialogHost queue={harness.dialogs} />
    </main>,
  );
}

/** Waits until the recent list has been read. */
async function listRead(): Promise<void> {
  await waitFor(() => {
    expect(harness.feature.model.getState().recent.status).not.toBe('loading');
  });
}

describe('the start page', () => {
  it('offers the templates, Open… and the recent projects, accessibly', async () => {
    harness.ipc.recentList.mockResolvedValue({ entries: [recentEntry(1), recentEntry(2)] });
    const { container } = renderStartPage();
    await listRead();

    expect(screen.getByRole('heading', { level: 2, name: 'Start' })).toBeTruthy();
    expect(screen.getByRole('button', { name: 'Empty project' })).toBeTruthy();
    expect(screen.getByRole('button', { name: 'Hello World' })).toBeTruthy();
    expect(screen.getByRole('button', { name: 'Open…' })).toBeTruthy();

    const recent = screen.getByRole('region', { name: 'Recent projects' });
    const items = within(recent).getAllByTestId('recent-item');
    expect(items).toHaveLength(2);
    const first = within(recent).getByRole('button', { name: 'Project 1' });
    expect(first.textContent).toContain('/home/ada/projects/project-1.b2c');
    expect(first.querySelector('time')?.getAttribute('datetime')).toBe('2026-10-05T10:42:00Z');
    expect(
      within(recent).getByRole('button', { name: 'Remove “Project 2” from the list' }),
    ).toBeTruthy();

    await expectNoAxeViolations(container);
  });

  it('says when there are no recent projects yet', async () => {
    renderStartPage();
    await listRead();
    expect(
      screen.getByText('No recent projects yet. Projects you open or save appear here.'),
    ).toBeTruthy();
  });

  it('offers to read the list again when it could not be read', async () => {
    harness.ipc.recentList.mockRejectedValueOnce(ipcFailure({ code: 'internal' }));
    renderStartPage();
    const retry = await screen.findByRole('button', { name: 'Try again' });
    harness.ipc.recentList.mockResolvedValueOnce({ entries: [recentEntry(3)] });
    fireEvent.click(retry);
    expect(await screen.findByRole('button', { name: 'Project 3' })).toBeTruthy();
  });

  it('removes a recent project from the list', async () => {
    harness.ipc.recentList.mockResolvedValueOnce({ entries: [recentEntry(1)] });
    renderStartPage();
    const remove = await screen.findByRole('button', { name: 'Remove “Project 1” from the list' });
    harness.ipc.recentList.mockResolvedValueOnce({ entries: [] });
    fireEvent.click(remove);
    await waitFor(() => {
      expect(harness.ipc.recentRemove).toHaveBeenCalledWith({ recentId: recentEntry(1).recentId });
    });
    expect(await screen.findByText(/No recent projects yet/)).toBeTruthy();
  });

  it('shows hidden characters in names and paths as placeholders', async () => {
    harness.ipc.recentList.mockResolvedValueOnce({
      entries: [
        recentEntry(1, {
          projectName: 'Game‮exe.b2c',
          displayPath: '/home/ada/​hidden/game.b2c',
        }),
      ],
    });
    renderStartPage();
    const open = await screen.findByRole('button', { name: 'Game⟨U+202E⟩exe.b2c' });
    expect(open.textContent).toContain('/home/ada/⟨U+200B⟩hidden/game.b2c');
    expect(open.textContent).not.toContain('‮');
  });

  it('shows the sections other features add, in order', async () => {
    function Second() {
      return <p>second section</p>;
    }
    function First() {
      return <p>first section</p>;
    }
    harness.sections.register('second', Second, { order: 2 });
    harness.sections.register('first', First, { order: 1 });
    renderStartPage();
    await listRead();
    const texts = screen.getAllByText(/section$/).map((element) => element.textContent);
    expect(texts).toEqual(['first section', 'second section']);
  });

  it('reports a file made with a newer version (E0108), until dismissed', async () => {
    harness.ipc.projectOpenDialog.mockRejectedValueOnce(
      ipcFailure({ code: 'newerFormat', needs: '0.3.0' }),
    );
    const { container } = renderStartPage();
    await listRead();
    fireEvent.click(screen.getByRole('button', { name: 'Open…' }));

    const report = await screen.findByTestId('project-load-failure');
    expect(within(report).getByRole('alert').textContent).toContain(
      'The project could not be opened',
    );
    expect(report.textContent).toContain(
      'B2C-E0108 This project was made with a newer version of Blocks2Cpp (needs ≥ 0.3.0).',
    );
    await expectNoAxeViolations(container);

    fireEvent.click(within(report).getByRole('button', { name: 'Dismiss' }));
    expect(screen.queryByTestId('project-load-failure')).toBeNull();
  });

  it("reports the loader's problems (E0105) with the count of the rest", async () => {
    const diagnostics = [
      loaderDiagnostic('B2C-E0105', 'The key "format" appears twice in the same object.'),
      ...Array.from({ length: 21 }, () => loaderDiagnostic('B2C-E0110', 'An unknown key.')),
    ];
    harness.ipc.recentList.mockResolvedValue({ entries: [recentEntry(1, { projectName: 'Bad' })] });
    harness.ipc.projectOpenRecent.mockRejectedValueOnce(
      ipcFailure({ code: 'invalidDocument', diagnostics }),
    );
    renderStartPage();
    fireEvent.click(await screen.findByRole('button', { name: 'Bad' }));

    const report = await screen.findByTestId('project-load-failure');
    expect(within(report).getByRole('heading').textContent).toBe('“Bad” could not be opened');
    const problems = within(report).getAllByRole('listitem');
    expect(problems).toHaveLength(20);
    expect(problems[0]?.textContent).toBe(
      'B2C-E0105 The key "format" appears twice in the same object.',
    );
    expect(report.textContent).toContain('…and 2 more problems.');
  });

  it('marks the page busy while an operation runs', async () => {
    let finish: (value: { status: 'cancelled' }) => void = () => undefined;
    harness.ipc.projectOpenDialog.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          finish = resolve;
        }),
    );
    renderStartPage();
    await listRead();
    const openButton = screen.getByRole('button', { name: 'Open…' });
    fireEvent.click(openButton);
    await waitFor(() => {
      expect(openButton.getAttribute('aria-disabled')).toBe('true');
    });
    expect(screen.getByRole('status').textContent).toBe('Opening the project…');
    // A second press while busy does nothing.
    fireEvent.click(openButton);
    fireEvent.click(screen.getByRole('button', { name: 'Empty project' }));
    act(() => {
      finish({ status: 'cancelled' });
    });
    await waitFor(() => {
      expect(openButton.getAttribute('aria-disabled')).toBe('false');
    });
    expect(harness.ipc.projectOpenDialog).toHaveBeenCalledOnce();
    expect(harness.ipc.projectNew).not.toHaveBeenCalled();
    expect(screen.getByRole('status').textContent).toBe('');
  });

  it('leads back to the open project', async () => {
    useAppStore.getState().actions.setProject(projectFixture());
    useAppStore.getState().actions.setUi({ screen: 'start' });
    renderStartPage();
    await listRead();
    fireEvent.click(screen.getByRole('button', { name: 'Back to “Guessing Game”' }));
    expect(useAppStore.getState().ui.screen).toBe('editor');
  });

  describe.skipIf(!CORE_BUILT)('with the compiler core', () => {
    it.each([
      ['Empty project', 'empty', EMPTY_TEXT],
      ['Hello World', 'helloWorld', HELLO_TEXT],
    ] as const)('creates a project from “%s”', async (label, template, text) => {
      harness.ipc.projectNew.mockResolvedValueOnce({
        handle: HANDLE_A,
        document: text,
        trust: CREATED_HERE,
      });
      renderStartPage();
      await listRead();
      fireEvent.click(screen.getByRole('button', { name: label }));
      await waitFor(() => {
        expect(useAppStore.getState().project?.handle).toBe(HANDLE_A);
      });
      expect(harness.ipc.projectNew).toHaveBeenCalledWith({ template });
      expect(useAppStore.getState().ui.screen).toBe('editor');
    });

    it('opens a recent project', async () => {
      harness.ipc.recentList.mockResolvedValue({ entries: [recentEntry(1)] });
      harness.ipc.projectOpenRecent.mockResolvedValueOnce(opened(HELLO_TEXT));
      renderStartPage();
      fireEvent.click(await screen.findByRole('button', { name: 'Project 1' }));
      await waitFor(() => {
        expect(useAppStore.getState().project?.fileName).toBe('game.b2c');
      });
    });

    it('asks about unsaved changes in an accessible dialog', async () => {
      useAppStore.getState().actions.setProject(projectFixture({ dirty: true }));
      renderStartPage();
      await listRead();
      fireEvent.click(screen.getByRole('button', { name: 'Open…' }));

      const dialog = await screen.findByRole('dialog', {
        name: 'Save changes to “Guessing Game”?',
      });
      expect(
        within(dialog).getByText("If you don't save, your changes will be lost."),
      ).toBeTruthy();
      const buttons = within(dialog)
        .getAllByRole('button')
        .map((button) => button.textContent);
      expect(buttons).toEqual(['Save', "Don't save", 'Cancel']);
      expect(document.activeElement?.textContent).toBe('Save');
      await expectNoAxeViolations(dialog);

      fireEvent.click(within(dialog).getByRole('button', { name: 'Cancel' }));
      await waitFor(() => {
        expect(screen.queryByRole('dialog')).toBeNull();
      });
      expect(harness.ipc.projectOpenDialog).not.toHaveBeenCalled();
    });
  });
});
