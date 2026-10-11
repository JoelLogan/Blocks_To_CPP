/**
 * The top bar's project name and unsaved-changes marker (docs/spec/04-user-interface.md §4.1), and
 * the input modality the toolbar tracks for the pages' headings (§4.8). happy-dom lays nothing
 * out, so the ellipsis itself is checked through the style sheet (./styles.test.ts) and in the
 * real webviews (e2e/specs/flows/chrome.e2e.ts); here the marker must not be inside the element
 * that is cut off.
 */
import { act, render, screen } from '@testing-library/react';
import { beforeEach, describe, expect, it } from 'vitest';

import { resetAppStore, useAppStore } from '../store';
import { documentFixture, projectFixture } from '../testing/fixtures';
import { HintProvider } from '../ui/Hint';
import { INPUT_ATTRIBUTE, KEYBOARD_INPUT } from './inputModality';
import { Toolbar } from './Toolbar';

/** A name far wider than the toolbar: 120 characters. */
const LONG_NAME = 'A very long project name that goes on and on '.repeat(3).slice(0, 120);

function renderToolbar() {
  return render(
    <HintProvider>
      <Toolbar />
    </HintProvider>,
  );
}

/** Opens a project named `name`, with or without unsaved changes. */
function openProject(name: string, dirty: boolean): void {
  const document = documentFixture();
  act(() => {
    useAppStore.getState().actions.setProject(
      projectFixture({
        document: { ...document, project: { ...document.project, name } },
        dirty,
      }),
    );
  });
}

beforeEach(() => {
  resetAppStore();
});

describe('the project name', () => {
  it('keeps the unsaved-changes marker outside the part a long name is cut off in', () => {
    expect(LONG_NAME).toHaveLength(120);
    renderToolbar();
    openProject(LONG_NAME, true);

    const heading = screen.getByRole('heading', { level: 1 });
    const name = screen.getByTestId('project-name-text');
    expect(name.className).toBe('project-name-text');
    expect(name.textContent).toBe(LONG_NAME);

    const marker = heading.querySelector('.dirty-marker');
    expect(marker).not.toBeNull();
    expect(name.contains(marker)).toBe(false);
    expect(marker?.parentElement).toBe(heading);
    expect(marker?.getAttribute('aria-hidden')).toBe('true');
    // Read as words: the name, then what the marker means.
    expect(heading.textContent).toBe(`${LONG_NAME} • (unsaved changes)`);
  });

  it('shows no marker without unsaved changes', () => {
    renderToolbar();
    openProject(LONG_NAME, false);
    const heading = screen.getByRole('heading', { level: 1 });
    expect(heading.querySelector('.dirty-marker')).toBeNull();
    expect(heading.textContent).toBe(LONG_NAME);
  });

  it('shows the app name in the same element without a project', () => {
    renderToolbar();
    const heading = screen.getByRole('heading', { level: 1 });
    expect(heading.querySelector('.project-name-text')?.textContent).toBe('Blocks2Cpp');
  });
});

describe('the input modality', () => {
  it('is tracked while the toolbar is shown', () => {
    const { unmount } = renderToolbar();
    document.body.dispatchEvent(new KeyboardEvent('keydown', { key: 'Tab', bubbles: true }));
    expect(document.documentElement.getAttribute(INPUT_ATTRIBUTE)).toBe(KEYBOARD_INPUT);

    unmount();
    expect(document.documentElement.hasAttribute(INPUT_ATTRIBUTE)).toBe(false);
  });
});
