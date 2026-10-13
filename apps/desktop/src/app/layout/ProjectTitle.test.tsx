/**
 * Renaming the project in the top bar (docs/spec/04-user-interface.md §4.1, D12): the name is a
 * button that turns into a field; `Enter` or moving the focus away keeps a valid name, `Escape`
 * cancels; a refused name says why; the focus returns to the name; the ≡ menu's *Rename project…*
 * opens the same field; axe passes in each state. The compiler core's own check of the name runs
 * against the real core when it is built.
 */
import type { CoreWasm } from '@blocks2cpp/b2c-core-wasm';
import type { Handle } from '@blocks2cpp/ipc-types';
import { act, fireEvent, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import { CORE_BUILT, HELLO_TEXT, realCore } from '../../features/project/testing';
import { expectNoAxeViolations } from '../../test/axe';
import { type CommandRegistry, createCommandRegistry } from '../commands';
import type { EditorHandle, ProjectInfo } from '../editor-types';
import { resetAppStore, useAppStore } from '../store';
import { projectFixture } from '../testing/fixtures';
import { HintProvider } from '../ui/Hint';
import { MainMenu } from './MainMenu';
import { checkProjectName, MAX_PROJECT_NAME_CHARS, ProjectTitle } from './ProjectTitle';

/** A right-to-left override, which the loader refuses in project text. */
const RLO = String.fromCodePoint(0x202e);

/** The Unicode line separator. */
const LINE_SEPARATOR = String.fromCodePoint(0x2028);

let registry: CommandRegistry;
let renamed: string[];
let editorShown: boolean;

/** An editor that applies a rename to the store, as the editing session does. */
const editor = {
  updateProjectInfo(info: ProjectInfo) {
    renamed.push(info.name);
    const project = useAppStore.getState().project;
    if (project !== null) {
      useAppStore.getState().actions.updateProject({
        document: {
          ...project.document,
          project: { ...project.document.project, name: info.name },
        },
        dirty: true,
      });
    }
  },
} as unknown as EditorHandle;

/** A core whose loader refuses bidirectional controls (B2C-E0126) and accepts anything else. */
const core = {
  canonical: (json: string) =>
    json.includes(RLO)
      ? { ok: false, diagnostics: [{ code: 'B2C-E0126' }] }
      : { ok: true, text: json, hash: 'a'.repeat(64), diagnostics: [] },
} as unknown as CoreWasm;

function renderTitle() {
  return render(
    <HintProvider>
      <header className="toolbar">
        <MainMenu registry={registry} editor={() => null} />
        <ProjectTitle
          registry={registry}
          editor={() => (editorShown ? editor : null)}
          core={() => core}
        />
        <button type="button">Elsewhere</button>
      </header>
    </HintProvider>,
  );
}

function openProject(
  name = 'Guessing Game',
  handle: Handle = 'ph_0123456789abcdef0123456789abcdef',
): void {
  act(() => {
    const project = projectFixture({ handle });
    useAppStore.getState().actions.setProject({
      ...project,
      document: { ...project.document, project: { ...project.document.project, name } },
    });
  });
}

function nameButton(): HTMLElement {
  return screen.getByTestId('project-name-button');
}

function field(): HTMLInputElement {
  const element = screen.getByRole('textbox', { name: 'Project name' });
  if (!(element instanceof HTMLInputElement)) {
    throw new Error('the name field is not an input');
  }
  return element;
}

function shownName(): string | null {
  return useAppStore.getState().project?.document.project.name ?? null;
}

beforeEach(() => {
  resetAppStore();
  registry = createCommandRegistry();
  renamed = [];
  editorShown = true;
});

describe('the project name', () => {
  it('is a button that turns into a field holding the name, all of it selected', async () => {
    const { container } = renderTitle();
    openProject();
    const button = nameButton();
    expect(button.closest('h1')).not.toBeNull();
    expect(screen.getByRole('button', { name: 'Guessing Game' })).toBe(button);
    expect(button.getAttribute('aria-describedby')).not.toBeNull();
    expect(
      document.getElementById(button.getAttribute('aria-describedby') ?? '')?.textContent,
    ).toBe('Rename the project');
    await expectNoAxeViolations(container);

    fireEvent.click(button);
    const input = field();
    expect(document.activeElement).toBe(input);
    expect(input.value).toBe('Guessing Game');
    expect(input.selectionStart).toBe(0);
    expect(input.selectionEnd).toBe('Guessing Game'.length);
    expect(input.getAttribute('aria-invalid')).toBe('false');
    await expectNoAxeViolations(container);
  });

  it('keeps a new name with Enter, trimmed, and gives the focus back to the name', async () => {
    const user = userEvent.setup();
    renderTitle();
    openProject();
    await user.click(nameButton());
    await user.clear(field());
    await user.type(field(), '  Space Invaders  {Enter}');

    expect(renamed).toEqual(['Space Invaders']);
    expect(shownName()).toBe('Space Invaders');
    expect(screen.queryByRole('textbox')).toBeNull();
    expect(document.activeElement).toBe(nameButton());
    expect(nameButton().textContent).toBe('Space Invaders');
    expect(useAppStore.getState().project?.dirty).toBe(true);
  });

  it('keeps a new name when the focus moves away, and leaves the focus there', async () => {
    const user = userEvent.setup();
    renderTitle();
    openProject();
    await user.click(nameButton());
    await user.clear(field());
    await user.type(field(), 'Snake');
    await user.click(screen.getByRole('button', { name: 'Elsewhere' }));

    expect(renamed).toEqual(['Snake']);
    expect(screen.queryByRole('textbox')).toBeNull();
    expect(document.activeElement).toBe(screen.getByRole('button', { name: 'Elsewhere' }));
  });

  it('cancels with Escape, which goes no further, and gives the focus back to the name', async () => {
    const user = userEvent.setup();
    const outside = vi.fn();
    document.addEventListener('keydown', outside);
    renderTitle();
    openProject();
    await user.click(nameButton());
    await user.clear(field());
    await user.type(field(), 'Not this');
    outside.mockClear();
    await user.keyboard('{Escape}');
    document.removeEventListener('keydown', outside);

    expect(renamed).toEqual([]);
    expect(shownName()).toBe('Guessing Game');
    expect(screen.queryByRole('textbox')).toBeNull();
    expect(document.activeElement).toBe(nameButton());
    expect(outside).not.toHaveBeenCalled();
  });

  it('changes nothing when the name stays the same', async () => {
    const user = userEvent.setup();
    renderTitle();
    openProject();
    await user.click(nameButton());
    await user.type(field(), '{End}  {Enter}');
    expect(renamed).toEqual([]);
    expect(useAppStore.getState().project?.dirty).toBe(false);
  });

  it.each([
    ['an empty name', '   ', 'Type a name for the project.'],
    [
      'a name of 101 characters',
      'n'.repeat(MAX_PROJECT_NAME_CHARS + 1),
      'A project name can have at most 100 characters; this one has 101.',
    ],
    [
      'a name with a tab',
      'Tab\there',
      'A project name is one line of text, without tabs or other control characters.',
    ],
    [
      // A text field drops pasted line breaks itself, but not the Unicode line separator.
      'a name of two lines',
      `Two${LINE_SEPARATOR}lines`,
      'A project name is one line of text, without tabs or other control characters.',
    ],
    [
      'a name the loader refuses',
      `Game ${RLO}evil`,
      'A project name cannot contain invisible characters that change the direction of text.',
    ],
  ])('refuses %s with Enter and says why, keeping the field', async (_what, name, message) => {
    const { container } = renderTitle();
    openProject();
    fireEvent.click(nameButton());
    fireEvent.change(field(), { target: { value: name } });
    fireEvent.keyDown(field(), { key: 'Enter' });

    expect(renamed).toEqual([]);
    const input = field();
    expect(document.activeElement).toBe(input);
    expect(input.getAttribute('aria-invalid')).toBe('true');
    const error = screen.getByRole('alert');
    expect(error.textContent).toBe(message);
    expect(input.getAttribute('aria-describedby')).toBe(error.id);
    await expectNoAxeViolations(container);

    // Typing again takes the message away.
    fireEvent.change(input, { target: { value: 'Fine' } });
    expect(screen.queryByRole('alert')).toBeNull();
    expect(input.getAttribute('aria-invalid')).toBe('false');
  });

  it('accepts 100 characters, counted as characters rather than UTF-16 units', () => {
    renderTitle();
    openProject();
    const name = '😀'.repeat(MAX_PROJECT_NAME_CHARS);
    fireEvent.click(nameButton());
    fireEvent.change(field(), { target: { value: name } });
    fireEvent.keyDown(field(), { key: 'Enter' });
    expect(renamed).toEqual([name]);
  });

  it('keeps the old name when the focus leaves a refused name', async () => {
    const user = userEvent.setup();
    renderTitle();
    openProject();
    await user.click(nameButton());
    await user.clear(field());
    await user.keyboard('{Enter}');
    expect(screen.getByRole('alert')).toBeTruthy();
    await user.click(screen.getByRole('button', { name: 'Elsewhere' }));

    expect(renamed).toEqual([]);
    expect(nameButton().textContent).toBe('Guessing Game');
    expect(screen.queryByRole('alert')).toBeNull();
  });

  it('says so when the block editor is not there to take the name', () => {
    editorShown = false;
    renderTitle();
    openProject();
    fireEvent.click(nameButton());
    fireEvent.change(field(), { target: { value: 'Later' } });
    fireEvent.keyDown(field(), { key: 'Enter' });
    expect(screen.getByRole('alert').textContent).toContain('until the block editor has started');
    expect(shownName()).toBe('Guessing Game');
  });

  it('opens from the main menu’s Rename project…, which it provides with a project', () => {
    renderTitle();
    expect(registry.hasCommand('project.rename')).toBe(false);
    openProject();
    expect(registry.hasCommand('project.rename')).toBe(true);
    registry.registerCommand('project.open', () => undefined);

    fireEvent.click(screen.getByRole('button', { name: 'Main menu' }));
    fireEvent.click(screen.getByRole('menuitem', { name: 'Rename project…' }));
    expect(screen.queryByRole('menu')).toBeNull();
    expect(document.activeElement).toBe(field());
    expect(field().value).toBe('Guessing Game');

    fireEvent.keyDown(field(), { key: 'Escape' });
    expect(document.activeElement).toBe(nameButton());
  });

  it('shows the next project’s name, not a field left open for the previous one', () => {
    renderTitle();
    openProject();
    fireEvent.click(nameButton());
    fireEvent.change(field(), { target: { value: 'Half typed' } });
    openProject('Hello World', 'ph_fedcba9876543210fedcba9876543210');
    expect(screen.queryByRole('textbox')).toBeNull();
    expect(nameButton().textContent).toBe('Hello World');
    expect(renamed).toEqual([]);
  });

  it('shows the app’s name, which renames nothing, without a project', () => {
    renderTitle();
    expect(screen.getByRole('heading', { level: 1 }).textContent).toBe('Blocks2Cpp');
    expect(screen.queryByTestId('project-name-button')).toBeNull();
  });
});

describe('checking a name', () => {
  it('refuses line breaks and every control character, wherever the text comes from', () => {
    const doc = projectFixture().document;
    for (const name of [
      'Two\nlines',
      'Two\r\nlines',
      'Bell\u0007',
      'Next\u0085line',
      'DEL\u007f',
    ]) {
      expect(checkProjectName(name, doc, core)).toEqual({
        ok: false,
        message: 'A project name is one line of text, without tabs or other control characters.',
      });
    }
    expect(checkProjectName('\n Snake \t', doc, core)).toEqual({ ok: true, name: 'Snake' });
  });
});

describe.skipIf(!CORE_BUILT)('checking a name with the real compiler core', () => {
  it('accepts names in any script and refuses bidirectional controls', async () => {
    const wasm = await realCore();
    const loaded = wasm.load(new TextEncoder().encode(HELLO_TEXT));
    if (!loaded.ok) {
      throw new Error('Hello World does not load');
    }
    const doc = loaded.document;
    expect(checkProjectName('  Space Invaders ', doc, wasm)).toEqual({
      ok: true,
      name: 'Space Invaders',
    });
    expect(checkProjectName('لعبة التخمين', doc, wasm)).toEqual({ ok: true, name: 'لعبة التخمين' });
    expect(checkProjectName(`Game ${RLO}evil`, doc, wasm)).toEqual({
      ok: false,
      message:
        'A project name cannot contain invisible characters that change the direction of text.',
    });
    // Without a core, a name cannot be checked yet.
    expect(checkProjectName('Space Invaders', doc, null).ok).toBe(false);
  });
});
