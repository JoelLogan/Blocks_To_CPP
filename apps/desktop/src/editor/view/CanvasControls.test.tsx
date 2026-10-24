/**
 * The canvas controls (./CanvasControls.tsx) on a canvas injected with the editor's options: named
 * buttons with hints and keys, what each does to the zoom and the view, what is announced, the
 * zoom shown following every zoom, the pointer leaving the focus where it was, axe, and their
 * style sheet (target sizes, the edge's contrast in both themes).
 */
import { b2cLightTheme } from '@blocks2cpp/blockly-ext';
import { act, fireEvent, render, screen, within } from '@testing-library/react';
import * as Blockly from 'blockly/core';
import { createPortal } from 'react-dom';
import { afterEach, describe, expect, it } from 'vitest';

import appCss from '../../app/app.css?raw';
import {
  contrastOf,
  declarationsOf,
  parseRules,
  pixels,
  themeTokens,
} from '../../app/layout/cssAudit';
import { expectNoAxeViolations } from '../../test/axe';
import { editorInjectOptions } from '../EditorWorkspace';
import { disposeWorkspaces, renderedWorkspace, setViewSize } from '../sync/testing';
import { CanvasControls, SHOW_ALL_ANNOUNCEMENTS, ZOOM_KEYS, zoomKeyOf } from './CanvasControls';
import controlsCss from './CanvasControls.css?raw';
import { CONTROLS_COMPONENT_ID } from './placement';

afterEach(() => {
  disposeWorkspaces();
});

/** A canvas as the editor injects it, 1000 × 700 pixels, with one block at (300, 200). */
function editorCanvas(): Blockly.WorkspaceSvg {
  const workspace = renderedWorkspace(editorInjectOptions(b2cLightTheme));
  setViewSize(workspace, 1000, 700);
  Blockly.svgResize(workspace);
  const block = workspace.newBlock('control.forever');
  block.initSvg();
  block.render();
  block.moveTo(new Blockly.utils.Coordinate(300, 200));
  return workspace;
}

/**
 * The controls of `workspace`, rendered into its container after Blockly's injection `div` as the
 * editor renders them (a portal). `container` is that container.
 */
function renderControls(workspace: Blockly.WorkspaceSvg) {
  const container = workspace.getInjectionDiv().parentElement;
  if (!(container instanceof HTMLElement)) {
    throw new Error('The workspace has no container');
  }
  return {
    ...render(createPortal(<CanvasControls workspace={workspace} />, container)),
    container,
  };
}

/** An element's accessible description: the text of the elements `aria-describedby` names. */
function descriptionOf(element: HTMLElement): string {
  return (element.getAttribute('aria-describedby') ?? '')
    .split(/\s+/)
    .map((id) => document.getElementById(id)?.textContent ?? '')
    .join(' ')
    .trim();
}

/** The live region's text, without the no-break space that makes a repeated text a change. */
function announced(): string {
  return screen.getByRole('status').textContent.replace(/\u00a0$/, '');
}

/** Lets Blockly deliver its queued events (a frame and a task later). */
async function eventsDelivered(): Promise<void> {
  await act(async () => {
    await new Promise((resolve) => requestAnimationFrame(resolve));
    await new Promise((resolve) => setTimeout(resolve, 0));
  });
}

describe('the canvas controls', () => {
  it('are named buttons with hints and keys, in a group after the canvas', async () => {
    const workspace = editorCanvas();
    const { container } = renderControls(workspace);
    const group = screen.getByRole('group', { name: 'Canvas zoom' });
    const names = within(group)
      .getAllByRole('button')
      .map((button) => button.getAttribute('aria-label'));
    expect(names).toEqual(['Zoom out', 'Zoom in', 'Reset zoom (now 90%)', 'Show all blocks']);
    expect(
      within(group)
        .getAllByRole('button')
        .map((button) => button.dataset['testid']),
    ).toEqual(['canvas-zoom-out', 'canvas-zoom-in', 'canvas-zoom-reset', 'canvas-show-all']);

    const zoomIn = screen.getByRole('button', { name: 'Zoom in' });
    expect(zoomIn.getAttribute('aria-keyshortcuts')).toBe(ZOOM_KEYS.in.aria);
    expect(descriptionOf(zoomIn)).toBe(`Zoom in (${ZOOM_KEYS.in.label})`);
    expect(descriptionOf(screen.getByRole('button', { name: /^Reset zoom/ }))).toBe(
      `Reset the zoom to 100% (${ZOOM_KEYS.reset.label})`,
    );
    // The zoom now is the reset button's text.
    expect(screen.getByTestId('canvas-zoom-reset').textContent).toBe('90%');

    // Right after Blockly's injection `div` (the canvas), placed as a workspace component.
    expect(group.previousElementSibling).toBe(workspace.getInjectionDiv());
    expect(workspace.getComponentManager().getComponent(CONTROLS_COMPONENT_ID)).toBeDefined();
    await expectNoAxeViolations(container);
  });

  it('zoom in and out by one step and announce the zoom', () => {
    const workspace = editorCanvas();
    renderControls(workspace);
    fireEvent.click(screen.getByRole('button', { name: 'Zoom in' }));
    expect(workspace.scale).toBeCloseTo(1.08, 10);
    expect(announced()).toBe('Zoom 108%');
    expect(screen.getByTestId('canvas-zoom-reset').textContent).toBe('108%');
    expect(screen.getByTestId('canvas-zoom-reset').getAttribute('aria-label')).toBe(
      'Reset zoom (now 108%)',
    );

    fireEvent.click(screen.getByRole('button', { name: 'Zoom out' }));
    fireEvent.click(screen.getByRole('button', { name: 'Zoom out' }));
    expect(workspace.scale).toBeCloseTo(0.75, 10);
    expect(announced()).toBe('Zoom 75%');
  });

  it('reset the zoom to 100 %, announcing it again when it is pressed again', () => {
    const workspace = editorCanvas();
    renderControls(workspace);
    const reset = screen.getByTestId('canvas-zoom-reset');
    fireEvent.click(reset);
    expect(workspace.scale).toBe(1);
    expect(announced()).toBe('Zoom 100%');
    const first = screen.getByRole('status').textContent;
    fireEvent.click(reset);
    // The same words, as a change the screen reader reads again.
    expect(announced()).toBe('Zoom 100%');
    expect(screen.getByRole('status').textContent).not.toBe(first);
  });

  it('show all blocks without changing the zoom, and say so', () => {
    const workspace = editorCanvas();
    renderControls(workspace);
    fireEvent.click(screen.getByRole('button', { name: 'Zoom in' }));
    fireEvent.click(screen.getByRole('button', { name: 'Zoom in' }));
    const scale = workspace.scale;
    workspace.scroll(4000, 4000);
    const before = { x: workspace.scrollX, y: workspace.scrollY };

    fireEvent.click(screen.getByRole('button', { name: 'Show all blocks' }));
    expect(workspace.scale).toBe(scale);
    expect({ x: workspace.scrollX, y: workspace.scrollY }).not.toEqual(before);
    expect(announced()).toBe(SHOW_ALL_ANNOUNCEMENTS.shown);

    workspace.clear();
    fireEvent.click(screen.getByRole('button', { name: 'Show all blocks' }));
    expect(announced()).toBe(SHOW_ALL_ANNOUNCEMENTS.empty);
  });

  // Blockly hears its zoom keys only inside its injection `div`, which the controls are beside: the
  // keys that the hints and `aria-keyshortcuts` name did nothing while a control had the focus.
  it('zoom with the keys they name while one of them has the focus', () => {
    const workspace = editorCanvas();
    renderControls(workspace);
    const zoomIn = screen.getByRole('button', { name: 'Zoom in' });
    zoomIn.focus();
    expect(fireEvent.keyDown(zoomIn, { key: '=', code: 'Equal', ctrlKey: true })).toBe(false);
    expect(workspace.scale).toBeCloseTo(1.08, 10);
    expect(announced()).toBe('Zoom 108%');
    expect(document.activeElement).toBe(zoomIn);

    // Two steps out, as the buttons go: 108 % → 90 % → 75 %.
    const zoomOut = screen.getByRole('button', { name: 'Zoom out' });
    fireEvent.keyDown(zoomOut, { key: '-', code: 'Minus', ctrlKey: true });
    fireEvent.keyDown(zoomOut, { key: '-', code: 'NumpadSubtract', ctrlKey: true });
    expect(workspace.scale).toBeCloseTo(0.75, 10);
    expect(announced()).toBe('Zoom 75%');
    fireEvent.keyDown(screen.getByTestId('canvas-zoom-reset'), {
      key: '0',
      code: 'Digit0',
      ctrlKey: true,
    });
    expect(workspace.scale).toBe(1);
    expect(announced()).toBe('Zoom 100%');
    // On the other controls too, and with Shift (`Ctrl++`).
    fireEvent.keyDown(screen.getByRole('button', { name: 'Show all blocks' }), {
      key: '+',
      code: 'Equal',
      ctrlKey: true,
      shiftKey: true,
    });
    expect(workspace.scale).toBeCloseTo(1.2, 10);
    expect(screen.getByTestId('canvas-zoom-reset').textContent).toBe('120%');

    // Without Ctrl, or with Alt, a key does nothing here.
    expect(fireEvent.keyDown(zoomIn, { key: '=', code: 'Equal' })).toBe(true);
    expect(
      fireEvent.keyDown(zoomIn, { key: '=', code: 'Equal', ctrlKey: true, altKey: true }),
    ).toBe(true);
    expect(workspace.scale).toBeCloseTo(1.2, 10);
  });

  it('know the zoom keys of the canvas', () => {
    const key = (init: Partial<KeyboardEventInit>) =>
      zoomKeyOf({ key: '', code: '', ctrlKey: true, altKey: false, metaKey: false, ...init });
    expect(key({ key: '=' })).toBe('in');
    expect(key({ key: '+' })).toBe('in');
    expect(key({ key: '+', code: 'NumpadAdd' })).toBe('in');
    expect(key({ key: '-' })).toBe('out');
    expect(key({ key: '-', code: 'NumpadSubtract' })).toBe('out');
    expect(key({ key: '0' })).toBe('reset');
    expect(key({ key: 'Insert', code: 'Numpad0' })).toBe('reset');
    expect(key({ key: '1' })).toBeNull();
    expect(key({ key: '=', ctrlKey: false })).toBeNull();
    expect(key({ key: '=', metaKey: true })).toBeNull();
    expect(key({ key: '0', altKey: true })).toBeNull();
  });

  it('show the zoom however it changed (Ctrl + wheel, a pinch, a saved view)', async () => {
    const workspace = editorCanvas();
    renderControls(workspace);
    workspace.setScale(2);
    await eventsDelivered();
    expect(screen.getByTestId('canvas-zoom-reset').textContent).toBe('200%');
  });

  it('leave the keyboard focus where it was when pressed with the pointer', () => {
    const workspace = editorCanvas();
    renderControls(workspace);
    const group = screen.getByRole('group', { name: 'Canvas zoom' });
    for (const button of within(group).getAllByRole('button')) {
      // `fireEvent` returns false when the press's default (taking the focus) was prevented.
      expect(fireEvent.mouseDown(button)).toBe(false);
    }
  });

  it('stop following the workspace and leave its components when they go', async () => {
    const workspace = editorCanvas();
    const { unmount } = renderControls(workspace);
    unmount();
    expect(workspace.getComponentManager().getComponent(CONTROLS_COMPONENT_ID)).toBeUndefined();
    // A zoom after they are gone reaches no state of theirs (React would warn).
    workspace.setScale(2);
    await eventsDelivered();
  });
});

describe('the canvas controls’ style sheet', () => {
  const rules = parseRules(controlsCss);

  it('gives each control a target of at least 24 × 24 px', () => {
    const control = declarationsOf(rules, '.canvas-control');
    expect(pixels(control.get('min-width'))).toBeGreaterThanOrEqual(24);
    expect(pixels(control.get('min-height'))).toBeGreaterThanOrEqual(24);
  });

  it('draws the group’s edge at 3:1 on the canvas and its text at 4.5:1, in both themes', () => {
    const group = declarationsOf(rules, '.canvas-controls');
    const control = declarationsOf(rules, '.canvas-control');
    const tokens = themeTokens(parseRules(appCss));
    for (const theme of ['light', 'dark'] as const) {
      const border = group.get('border')?.split(' ').at(-1) ?? '';
      expect(contrastOf(tokens[theme], border, 'var(--b2c-canvas)')).toBeGreaterThanOrEqual(3);
      const text = control.get('color') ?? '';
      const background = group.get('background') ?? '';
      expect(contrastOf(tokens[theme], text, background)).toBeGreaterThanOrEqual(4.5);
      const hover = declarationsOf(rules, '.canvas-control:hover').get('background') ?? '';
      expect(contrastOf(tokens[theme], text, hover)).toBeGreaterThanOrEqual(4.5);
    }
  });

  it('sets no z-index: Blockly’s toolbox, scroll bars and dragged blocks stay above them', () => {
    for (const rule of rules) {
      expect(rule.declarations.has('z-index'), rule.selectors.join(', ')).toBe(false);
    }
  });
});
