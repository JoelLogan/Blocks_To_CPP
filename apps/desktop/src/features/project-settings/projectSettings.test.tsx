/**
 * The project settings page: the draft follows the project, problems are shown and block saving,
 * saving is one edit of the project, Cancel changes nothing, and the run options of this computer
 * apply at once.
 */
import { act, cleanup, fireEvent, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { resetAppStore, useAppStore } from '../../app/store';
import { documentFixture, projectFixture, settingsFixture } from '../../app/testing/fixtures';
import { featureContext, renderScreen, type TestFeatureContext } from '../settings/page/testing';
import { projectSettingsFeature } from './feature';
import {
  defineNameProblem,
  defineValueProblem,
  draftFrom,
  draftProblems,
  settingsChanged,
  settingsFrom,
} from './model';

let test: TestFeatureContext;

async function settle(): Promise<void> {
  await act(async () => {
    for (let round = 0; round < 5; round++) {
      await Promise.resolve();
    }
  });
}

function openPage() {
  projectSettingsFeature(test.ctx);
  test.ipc.runInputFile.mockResolvedValue({ fileName: null, cancelled: false });
  act(() => {
    void test.ctx.commands.runCommand('project.settings');
  });
  expect(useAppStore.getState().ui.screen).toBe('projectSettings');
  return renderScreen(test, 'projectSettings');
}

beforeEach(() => {
  resetAppStore();
  test = featureContext();
  const document = documentFixture();
  document.project.run = { workingDirectory: 'project' };
  act(() => {
    useAppStore.getState().actions.setProject(projectFixture({ document }));
    useAppStore.getState().actions.setSettings({ value: settingsFixture() });
  });
});

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

describe('the model', () => {
  it('checks define names and values as the backend does', () => {
    expect(defineNameProblem('LEVEL')).toBeNull();
    expect(defineNameProblem('_level2')).toBeNull();
    for (const bad of ['', '2x', 'a-b', '__X', '_Big', 'NDEBUG', 'int', 'x'.repeat(65)]) {
      expect(defineNameProblem(bad), bad).not.toBeNull();
    }
    expect(defineValueProblem('int', '-42')).toBeNull();
    expect(defineValueProblem('int', '9007199254740992')).not.toBeNull();
    expect(defineValueProblem('int', '1.5')).not.toBeNull();
    expect(defineValueProblem('bool', 'true')).toBeNull();
    expect(defineValueProblem('string', 'it’s "quoted"')).toBeNull();
    expect(defineValueProblem('string', 'tab\there')).not.toBeNull();
    expect(defineValueProblem('string', 'evil‮')).not.toBeNull();
  });

  it('round-trips the project and notices duplicates', () => {
    const project = documentFixture().project;
    project.build.defines = [
      { name: 'LEVEL', value: { int: 3 } },
      { name: 'NAME', value: { string: 'Ada' } },
      { name: 'ON', value: { bool: false } },
    ];
    project.run = { args: ['--easy', 'a b'], workingDirectory: 'sandbox' };
    const draft = draftFrom(project);
    expect(draftProblems(draft)).toEqual([]);
    const settings = settingsFrom(draft, project);
    expect(settingsChanged(settings, project)).toBe(false);
    expect(settings.build.defines).toEqual(project.build.defines);
    expect(settings.run).toEqual(project.run);
    const first = draft.defines[0];
    expect(first).toBeDefined();
    const twice = {
      ...draft,
      defines: first === undefined ? draft.defines : [...draft.defines, { ...first, key: -1 }],
    };
    expect(draftProblems(twice).map((problem) => problem.message)).toContain(
      'LEVEL is defined twice.',
    );
  });
});

describe('the page', () => {
  it('saves the changes as one edit of the project', async () => {
    openPage();
    await settle();
    fireEvent.change(screen.getByTestId('project-settings-standard'), {
      target: { value: 'c++17' },
    });
    fireEvent.click(screen.getByTestId('project-settings-release-werror'));
    fireEvent.click(screen.getByTestId('project-settings-add-arg'));
    fireEvent.change(screen.getByTestId('project-settings-arg-text'), {
      target: { value: '--level 2' },
    });
    fireEvent.click(screen.getByLabelText('A sandbox folder for this project'));
    fireEvent.click(screen.getByTestId('project-settings-save'));
    const { project, ui } = useAppStore.getState();
    expect(ui.screen).toBe('editor');
    expect(project?.dirty).toBe(true);
    expect(project?.document.project.language).toEqual({ standard: 'c++17' });
    expect(project?.document.project.build.configurations.release.warningsAsErrors).toBe(true);
    expect(project?.document.project.run).toEqual({
      args: ['--level 2'],
      workingDirectory: 'sandbox',
    });
  });

  it('shows problems and saves nothing while there are any', async () => {
    openPage();
    await settle();
    fireEvent.click(screen.getByTestId('project-settings-add-define'));
    const name = screen.getByTestId('project-settings-define-name');
    fireEvent.change(name, { target: { value: '9lives' } });
    expect(name.getAttribute('aria-invalid')).toBe('true');
    expect(screen.getByRole('alert').textContent).toContain('does not start with a digit');
    fireEvent.click(screen.getByTestId('project-settings-save'));
    expect(useAppStore.getState().ui.screen).toBe('projectSettings');
    expect(useAppStore.getState().project?.dirty).toBe(false);
    expect(document.activeElement).toBe(name);
    fireEvent.change(name, { target: { value: 'LIVES' } });
    fireEvent.change(screen.getByTestId('project-settings-define-value'), {
      target: { value: '9' },
    });
    fireEvent.click(screen.getByTestId('project-settings-save'));
    expect(useAppStore.getState().project?.document.project.build.defines).toEqual([
      { name: 'LIVES', value: { int: 9 } },
    ]);
  });

  it('changes nothing on Cancel, or when nothing changed', async () => {
    openPage();
    await settle();
    const before = useAppStore.getState().project?.document;
    fireEvent.change(screen.getByTestId('project-settings-standard'), {
      target: { value: 'c++23' },
    });
    fireEvent.click(screen.getByTestId('project-settings-cancel'));
    expect(useAppStore.getState().project?.document).toBe(before);
    cleanup();
    openPage();
    await settle();
    fireEvent.click(screen.getByTestId('project-settings-save'));
    expect(useAppStore.getState().project?.document).toBe(before);
    expect(useAppStore.getState().project?.dirty).toBe(false);
  });

  it('chooses the input file and the separate window at once, on this computer', async () => {
    openPage();
    await settle();
    expect(test.ipc.runInputFile).toHaveBeenCalledWith({
      handle: useAppStore.getState().project?.handle,
      action: 'get',
    });
    test.ipc.runInputFile.mockResolvedValue({ fileName: 'numbers.txt', cancelled: false });
    fireEvent.click(screen.getByTestId('project-settings-choose-input'));
    await settle();
    expect(screen.getByTestId('project-settings-input-file').textContent).toContain(
      '“numbers.txt”',
    );
    const updated = settingsFixture({ run: { onErrors: 'disableRun', externalTerminal: true } });
    test.ipc.settingsUpdate.mockResolvedValue({ settings: updated });
    fireEvent.click(screen.getByTestId('project-settings-external'));
    await settle();
    expect(test.ipc.settingsUpdate).toHaveBeenCalledWith({ run: { externalTerminal: true } });
    expect(useAppStore.getState().settings.value?.run.externalTerminal).toBe(true);
    // Neither is an edit of the project.
    expect(useAppStore.getState().project?.dirty).toBe(false);
  });

  it('says so without a project', () => {
    projectSettingsFeature(test.ctx);
    act(() => {
      useAppStore.getState().actions.setProject(null);
    });
    renderScreen(test, 'projectSettings');
    expect(screen.getByText('No project is open.')).toBeDefined();
  });
});
