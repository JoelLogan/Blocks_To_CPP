/**
 * The Tab order through the window with the real block editor (docs/spec/04-user-interface.md
 * §4.7, §4.8): toolbar, then Blockly's toolbox, the toolbox's blocks and the canvas (in that order,
 * which the keyboard plugin sets), then the docks. Inside the editor the arrow keys take over
 * (src/editor/keyboard/); Tab always leaves it.
 */
import { act, render, screen } from '@testing-library/react';
import * as Blockly from 'blockly/core';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { keyboardPlugin } from '../../editor/keyboard';
import { guessDocument } from '../../editor/toolbox/testing';
import { expectNoAxeViolations } from '../../test/axe';
import { App } from '../App';
import { setCore } from '../core';
import { EDITOR_PLUGINS } from '../editorPlugins';
import { resetAppStore, useAppStore } from '../store';
import { projectFixture, settingsFixture, toolchainFixture } from '../testing/fixtures';
import { HintProvider } from '../ui/Hint';
import { pressTab } from './sequentialFocus';

/** A ResizeObserver that never calls back (happy-dom lays nothing out). */
class StillResizeObserver {
  observe(): void {
    // Nothing to observe without layout.
  }
  unobserve(): void {
    // Nothing to stop.
  }
  disconnect(): void {
    // Nothing to stop.
  }
}

const cleanups: (() => void)[] = [];
let plugins: typeof EDITOR_PLUGINS = [];

beforeEach(() => {
  resetAppStore();
  vi.stubGlobal('ResizeObserver', StillResizeObserver);
  // No dark scheme, no reduced motion (xterm.js still listens the old way).
  vi.stubGlobal('matchMedia', () => ({
    matches: false,
    addEventListener: () => undefined,
    removeEventListener: () => undefined,
    addListener: () => undefined,
    removeListener: () => undefined,
  }));
  // The core never starts here: these tests are about the focus, not the preview.
  setCore(null);
  vi.spyOn(console, 'error').mockImplementation(() => undefined);
  plugins = [...EDITOR_PLUGINS];
  // The keyboard plugin as the integrator adds it, after the app's other editor plugins.
  EDITOR_PLUGINS.push(keyboardPlugin);
  const style = document.createElement('style');
  style.textContent = '[hidden] { display: none !important; }';
  document.head.append(style);
  cleanups.push(() => {
    style.remove();
  });
});

afterEach(() => {
  for (const cleanup of cleanups.splice(0)) {
    cleanup();
  }
  EDITOR_PLUGINS.splice(0, EDITOR_PLUGINS.length, ...plugins);
  setCore(null);
  Blockly.keyboardNavigationController.setIsActive(false);
});

function openProject(): void {
  const { actions } = useAppStore.getState();
  actions.setProject(projectFixture({ document: guessDocument() }));
  actions.setToolchains({ list: [toolchainFixture()] });
  actions.setSettings({ value: settingsFixture() });
  actions.setUi({ screen: 'editor' });
}

/** Presses Tab (or Shift+Tab) as a browser does. */
function tab(shift = false): void {
  act(() => {
    pressTab(shift);
  });
}

/** Presses a key on the focused element, as Blockly reads it (by `keyCode`). */
function press(key: string, keyCode: number): void {
  act(() => {
    (document.activeElement ?? document.body).dispatchEvent(
      new KeyboardEvent('keydown', { key, keyCode, bubbles: true, cancelable: true }),
    );
  });
}

function renderApp() {
  openProject();
  return render(
    <HintProvider>
      <App />
    </HintProvider>,
  );
}

/** Where the focus is, as the editor's areas or the element's role and name. */
function where(): string {
  const tree = Blockly.getFocusManager().getFocusedTree();
  const element = document.activeElement;
  if (tree instanceof Blockly.Toolbox && tree.HtmlDiv?.contains(element) === true) {
    return 'toolbox';
  }
  if (tree instanceof Blockly.WorkspaceSvg && tree.getParentSvg().contains(element)) {
    return tree.isFlyout ? 'toolbox blocks' : 'canvas';
  }
  if (element === null || element === document.body) {
    return 'nothing';
  }
  const role = element.getAttribute('role') ?? element.tagName.toLowerCase();
  return `${role}: ${element.getAttribute('aria-label') ?? element.textContent.trim()}`;
}

describe('the Tab order with the block editor', () => {
  it('goes from the toolbar to the toolbox, its blocks and the canvas, then the docks', () => {
    renderApp();
    screen.getByRole('button', { name: /^Build/ }).focus();

    const reached: string[] = [];
    for (let step = 0; step < 5; step++) {
      tab();
      reached.push(where());
    }
    expect(reached).toEqual([
      'toolbox',
      'toolbox blocks',
      'canvas',
      'separator: Resize the C++ panel',
      'button: Hide the C++ panel',
    ]);

    // And back.
    tab(true);
    tab(true);
    expect(where()).toBe('canvas');
    tab(true);
    expect(where()).toBe('toolbox blocks');
    tab(true);
    expect(where()).toBe('toolbox');
  });

  it('turns on the keyboard look and names what it lands on in the canvas', async () => {
    const { container } = renderApp();
    const status = container.querySelector('.b2c-keyboard-a11y [role="status"]');
    screen.getByRole('button', { name: /^Build/ }).focus();
    tab();
    tab();
    tab();
    expect(where()).toBe('canvas');
    expect(Blockly.keyboardNavigationController.getIsActive()).toBe(true);
    // Blockly moves the focus on to the canvas's first block while the toolbox's blocks lose it. A
    // browser then drops the Tab's own focus change; happy-dom carries it out, which leaves the
    // focus on the canvas itself (src/editor/keyboard/plugin.test.ts has the browser's case).
    const node = Blockly.getFocusManager().getFocusedNode();
    expect(status?.textContent).toMatch(
      node instanceof Blockly.BlockSvg ? /^block “main”/ : /^the canvas/,
    );

    // The arrow keys move inside the canvas; Tab still leaves it.
    if (!(node instanceof Blockly.BlockSvg)) {
      press('ArrowDown', 40);
      expect(status?.textContent).toMatch(/^block “main”/);
    }
    press('ArrowDown', 40);
    expect(status?.textContent).toMatch(/^block “create int variable guess”/);
    tab();
    expect(where()).toBe('separator: Resize the C++ panel');
    await expectNoAxeViolations(container);
    // axe goes through the whole window and Blockly's SVG: slow when the suite runs in parallel.
  }, 60_000);
});
