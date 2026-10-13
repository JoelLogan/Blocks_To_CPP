/**
 * The editor's layout (docs/spec/04-user-interface.md §4.1): the keyboard focus when a splitter
 * collapses its dock; the docks' sizes as a share of the window, kept within their limits (the
 * same limits are the splitters' `aria-valuemax`) and leaving the canvas 200 px; the C++ dock
 * hidden while it does not fit; the *Resize the toolbox* splitter at the flyout's edge; and the
 * layout remembered between sessions (05 §5.9 `ui.layout`): taken from the settings once, and each
 * change the user makes handed on.
 */
import type { LayoutPatch, LayoutSettings } from '@blocks2cpp/ipc-types';
import { act, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, type Mock, vi } from 'vitest';

import { flyoutWidth } from '../../editor/toolbox/continuous';
import { expectNoAxeViolations } from '../../test/axe';
import { resetAppStore, useAppStore } from '../store';
import { settingsFixture } from '../testing/fixtures';
import { HintProvider } from '../ui/Hint';
import { EditorLayout } from './EditorLayout';
import { announceFlyoutLayout, type FlyoutLayout } from './flyoutLayout';
import { registerLayoutSaver } from './rememberedLayout';
import { SPLITTER_SIZE } from './sizes';
import { BIG_STEP, STEP } from './Splitter';

/** A ResizeObserver whose callback the test runs with the size it chooses. */
class ManualResizeObserver {
  static instances: ManualResizeObserver[] = [];
  private readonly callback: ResizeObserverCallback;

  constructor(callback: ResizeObserverCallback) {
    this.callback = callback;
    ManualResizeObserver.instances.push(this);
  }

  observe(): void {
    // The test reports sizes itself.
  }

  unobserve(): void {
    // Not needed.
  }

  disconnect(): void {
    ManualResizeObserver.instances = ManualResizeObserver.instances.filter(
      (observer) => observer !== this,
    );
  }

  /** Reports the observed element's content box. */
  report(width: number, height: number): void {
    const entry = { contentRect: { width, height } } as unknown as ResizeObserverEntry;
    this.callback([entry], this);
  }
}

/** The height of the toolbar and the status bar around the editor area, in CSS pixels. */
const CHROME_HEIGHT = 64;

/** The width of the toolbox's category column in these tests, in CSS pixels. */
const COLUMN = 100;

/** The natural width of the toolbox's blocks at their scale (the ask block's), in CSS pixels. */
const NATURAL = 600;

/** The happy-dom window, which can be given a size. */
interface SizedWindow {
  readonly happyDOM: { setViewport(viewport: { width: number; height: number }): void };
}

function setWindowSize(width: number, height: number): void {
  (window as unknown as SizedWindow).happyDOM.setViewport({ width, height });
}

beforeEach(() => {
  resetAppStore();
  ManualResizeObserver.instances = [];
  vi.stubGlobal('ResizeObserver', ManualResizeObserver);
});

afterEach(() => {
  vi.unstubAllGlobals();
  setWindowSize(1024, 768);
});

function renderLayout() {
  return render(
    <HintProvider>
      <EditorLayout
        workspace={<div className="injectionDiv" data-testid="canvas" />}
        hidden={false}
      />
    </HintProvider>,
  );
}

/** Lays the editor area out for a window `width` × `height`, as a browser would. */
function layOut(width: number, height: number): void {
  setWindowSize(width, height);
  act(() => {
    for (const observer of ManualResizeObserver.instances) {
      observer.report(width, height - CHROME_HEIGHT);
    }
  });
}

function separator(name: string): HTMLElement {
  return screen.getByRole('separator', { name });
}

function valueOf(element: HTMLElement, attribute: 'now' | 'min' | 'max'): number {
  return Number(element.getAttribute(`aria-value${attribute}`));
}

/**
 * A stand-in for the toolbox's flyout (src/editor/toolbox/continuous.ts): it announces itself from
 * the canvas with the width rule of the real one, and again whenever the user chooses a width.
 */
function fakeFlyout(room: () => number) {
  let chosen: number | null = null;
  const canvas = screen.getByTestId('canvas');
  // Called from the layout's handlers and effects, which React already runs inside `act`.
  const setWidth = vi.fn((width: number | null) => {
    chosen = width;
    announceFlyoutLayout(canvas, layout());
  });
  const layout = (): FlyoutLayout => ({
    left: COLUMN,
    width: flyoutWidth(NATURAL, room(), chosen),
    min: 160,
    max: Math.max(160, room() - 200),
    setWidth,
  });
  const announce = (): void => {
    act(() => {
      announceFlyoutLayout(canvas, layout());
    });
  };
  announce();
  return { setWidth, announce, width: () => layout().width };
}

/** The canvas left beside the toolbox in a window `width` wide, with the C++ dock shown. */
function canvasWidth(width: number): number {
  const dock = valueOf(separator('Resize the C++ panel'), 'now');
  const room = width - dock - SPLITTER_SIZE - COLUMN;
  return room - flyoutWidth(NATURAL, room);
}

describe('EditorLayout keyboard focus', () => {
  it.each([
    {
      dock: 'bottom',
      splitter: 'Resize the bottom panel',
      show: 'Show the bottom panel',
      collapsed: () => useAppStore.getState().ui.bottomCollapsed,
    },
    {
      dock: 'right',
      splitter: 'Resize the C++ panel',
      show: 'Show the C++ panel',
      collapsed: () => useAppStore.getState().ui.rightCollapsed,
    },
  ])(
    'moves the focus to the $dock dock’s toggle when Enter on its splitter collapses it',
    ({ splitter, show, collapsed }) => {
      renderLayout();
      const bar = separator(splitter);
      act(() => {
        bar.focus();
      });
      expect(document.activeElement).toBe(bar);

      fireEvent.keyDown(bar, { key: 'Enter' });
      expect(collapsed()).toBe(true);
      expect(screen.queryByRole('separator', { name: splitter })).toBeNull();
      // The splitter is gone; the focus is on the button that brings the dock back, never on body.
      const toggle = screen.getByRole('button', { name: show });
      expect(document.activeElement).toBe(toggle);
      expect(toggle.getAttribute('aria-expanded')).toBe('false');

      fireEvent.click(toggle);
      expect(collapsed()).toBe(false);
      expect(separator(splitter)).toBeTruthy();
    },
  );
});

describe('the docks’ sizes', () => {
  it.each([
    // [window, C++ dock (30 %, 280–480), its largest, bottom dock (28 %, 120–280), its largest]
    { width: 1366, height: 768, right: 410, rightMax: 887, bottom: 215, bottomMax: 457 },
    { width: 1024, height: 700, right: 307, rightMax: 558, bottom: 196, bottomMax: 413 },
    { width: 800, height: 560, right: 280, rightMax: 334, bottom: 157, bottomMax: 322 },
  ])(
    'start at a share of a $width × $height window and leave the canvas 200 px',
    ({ width, height, right, rightMax, bottom, bottomMax }) => {
      setWindowSize(width, height);
      renderLayout();
      layOut(width, height);
      fakeFlyout(() => width - valueOf(separator('Resize the C++ panel'), 'now') - 6 - COLUMN);

      const rightBar = separator('Resize the C++ panel');
      const bottomBar = separator('Resize the bottom panel');
      expect([
        valueOf(rightBar, 'now'),
        valueOf(rightBar, 'min'),
        valueOf(rightBar, 'max'),
      ]).toEqual([right, 200, rightMax]);
      expect([
        valueOf(bottomBar, 'now'),
        valueOf(bottomBar, 'min'),
        valueOf(bottomBar, 'max'),
      ]).toEqual([bottom, 96, bottomMax]);
      expect(canvasWidth(width)).toBeGreaterThanOrEqual(200);
      expect(useAppStore.getState().ui.rightCollapsed).toBe(false);

      // The widest C++ dock still leaves 200 px of canvas beside the narrowest flyout.
      fireEvent.keyDown(rightBar, { key: 'End' });
      expect(valueOf(rightBar, 'now')).toBe(rightMax);
      expect(canvasWidth(width)).toBeGreaterThanOrEqual(200);
      fireEvent.keyDown(bottomBar, { key: 'End' });
      expect(valueOf(bottomBar, 'now')).toBe(bottomMax);
    },
  );

  it('follow the window until the user resizes them', () => {
    setWindowSize(1024, 700);
    renderLayout();
    layOut(1024, 700);
    const rightBar = separator('Resize the C++ panel');
    expect(valueOf(rightBar, 'now')).toBe(307);
    layOut(1366, 768);
    expect(valueOf(rightBar, 'now')).toBe(410);

    fireEvent.keyDown(rightBar, { key: 'ArrowLeft' });
    expect(valueOf(rightBar, 'now')).toBe(410 + STEP);
    layOut(1280, 800);
    expect(valueOf(rightBar, 'now')).toBe(410 + STEP);
  });

  it('keep a dock within its limits as the window shrinks, so its splitter moves at once', () => {
    setWindowSize(1366, 768);
    renderLayout();
    layOut(1366, 768);
    const rightBar = separator('Resize the C++ panel');
    fireEvent.keyDown(rightBar, { key: 'End' });
    expect(valueOf(rightBar, 'now')).toBe(887);

    layOut(1024, 700);
    // 65 % of 1024 px: no dead zone between the size chosen and the size shown.
    expect(valueOf(rightBar, 'now')).toBe(665);
    expect(valueOf(rightBar, 'max')).toBe(665);
    fireEvent.keyDown(rightBar, { key: 'ArrowRight' });
    expect(valueOf(rightBar, 'now')).toBe(665 - STEP);
    const layout = document.querySelector<HTMLElement>('.editor-layout');
    expect(layout?.style.gridTemplateColumns).toBe(
      `minmax(0, 1fr) auto min(${String(665 - STEP)}px, 65%)`,
    );
  });

  it('hide the C++ dock while it does not fit, and show it again when it does', () => {
    setWindowSize(1024, 700);
    renderLayout();
    layOut(1024, 700);
    fakeFlyout(() => 400);
    const collapsed = () => useAppStore.getState().ui.rightCollapsed;
    expect(collapsed()).toBe(false);

    // 640 px: even a 200 px dock would leave the canvas less than 200 px beside a 160 px flyout.
    layOut(640, 560);
    expect(collapsed()).toBe(true);
    layOut(1024, 700);
    expect(collapsed()).toBe(false);

    // Shown or hidden by the user meanwhile: the user's choice stays.
    layOut(640, 560);
    expect(collapsed()).toBe(true);
    fireEvent.click(screen.getByRole('button', { name: 'Show the C++ panel' }));
    fireEvent.click(screen.getByRole('button', { name: 'Hide the C++ panel' }));
    layOut(1024, 700);
    expect(collapsed()).toBe(true);
  });
});

describe('the Resize the toolbox splitter', () => {
  it('appears at the flyout’s right edge once the flyout is laid out', () => {
    renderLayout();
    expect(screen.queryByRole('separator', { name: 'Resize the toolbox' })).toBeNull();
    const flyout = fakeFlyout(() => 794);
    const bar = separator('Resize the toolbox');
    expect([valueOf(bar, 'now'), valueOf(bar, 'min'), valueOf(bar, 'max')]).toEqual([
      320, 160, 594,
    ]);
    expect(bar.getAttribute('aria-orientation')).toBe('vertical');
    expect(bar.style.left).toBe(`${String(COLUMN + 320)}px`);
    expect(bar.classList.contains('splitter-toolbox')).toBe(true);

    // The flyout goes (the editor is disposed): so does its splitter.
    act(() => {
      announceFlyoutLayout(screen.getByTestId('canvas'), {
        left: 0,
        width: 0,
        min: 160,
        max: 160,
        setWidth: () => undefined,
      });
    });
    expect(screen.queryByRole('separator', { name: 'Resize the toolbox' })).toBeNull();
    expect(flyout.setWidth).not.toHaveBeenCalled();
  });

  it('sets the flyout’s width by keyboard, within 160 px and its largest width', () => {
    renderLayout();
    const flyout = fakeFlyout(() => 794);
    const bar = () => separator('Resize the toolbox');
    // The flyout is left of the bar: right makes it wider.
    fireEvent.keyDown(bar(), { key: 'ArrowRight' });
    expect(flyout.setWidth).toHaveBeenLastCalledWith(320 + STEP);
    expect(valueOf(bar(), 'now')).toBe(320 + STEP);
    expect(bar().style.left).toBe(`${String(COLUMN + 320 + STEP)}px`);
    fireEvent.keyDown(bar(), { key: 'ArrowLeft', shiftKey: true });
    expect(valueOf(bar(), 'now')).toBe(320 + STEP - BIG_STEP);
    fireEvent.keyDown(bar(), { key: 'End' });
    expect(valueOf(bar(), 'now')).toBe(594);
    fireEvent.keyDown(bar(), { key: 'ArrowRight' });
    expect(flyout.setWidth).toHaveBeenLastCalledWith(594);
    fireEvent.keyDown(bar(), { key: 'Home' });
    expect(valueOf(bar(), 'now')).toBe(160);
    // The toolbox cannot be collapsed: Enter does nothing.
    const calls = flyout.setWidth.mock.calls.length;
    fireEvent.keyDown(bar(), { key: 'Enter' });
    expect(flyout.setWidth).toHaveBeenCalledTimes(calls);
    expect(bar()).toBeTruthy();
  });

  it('sets the flyout’s width by pointer', () => {
    renderLayout();
    const flyout = fakeFlyout(() => 794);
    const bar = separator('Resize the toolbox');
    bar.setPointerCapture = vi.fn();
    bar.hasPointerCapture = vi.fn(() => true);
    const release = vi.fn();
    bar.releasePointerCapture = release;
    fireEvent.pointerDown(bar, { button: 0, clientX: 420, pointerId: 1 });
    fireEvent.pointerMove(bar, { clientX: 470, pointerId: 1 });
    expect(flyout.setWidth).toHaveBeenLastCalledWith(370);
    fireEvent.pointerMove(bar, { clientX: 2000, pointerId: 1 });
    expect(flyout.setWidth).toHaveBeenLastCalledWith(594);
    fireEvent.pointerMove(bar, { clientX: 0, pointerId: 1 });
    expect(flyout.setWidth).toHaveBeenLastCalledWith(160);
    fireEvent.pointerUp(bar, { pointerId: 1 });
    expect(release).toHaveBeenCalledWith(1);
  });

  it('passes axe with every splitter shown', async () => {
    const { container } = renderLayout();
    fakeFlyout(() => 794);
    await expectNoAxeViolations(container);
  });
});

/** Puts settings with the remembered `layout` into the store, as the start of the window does. */
function readSettings(layout: LayoutSettings): void {
  act(() => {
    useAppStore.getState().actions.setSettings({
      value: settingsFixture({ ui: { layout } }),
      notices: [],
    });
  });
}

describe('the remembered layout', () => {
  /** The saver the settings feature would register; removed after each test. */
  let saved: Mock<(change: LayoutPatch, settled: boolean) => void>;
  let unregister: () => void;

  beforeEach(() => {
    saved = vi.fn();
    unregister = registerLayoutSaver(saved);
  });

  afterEach(() => {
    unregister();
  });

  it('is taken from the settings before the first paint, and not handed back', () => {
    readSettings({
      toolboxWidth: 250,
      rightDockWidth: 420,
      bottomDockHeight: 150,
      bottomCollapsed: true,
    });
    setWindowSize(1280, 800);
    renderLayout();
    layOut(1280, 800);
    const flyout = fakeFlyout(() => 794);

    expect(valueOf(separator('Resize the C++ panel'), 'now')).toBe(420);
    expect(flyout.setWidth).toHaveBeenCalledTimes(1);
    expect(flyout.setWidth).toHaveBeenCalledWith(250);
    expect(valueOf(separator('Resize the toolbox'), 'now')).toBe(250);
    expect(useAppStore.getState().ui).toMatchObject({
      rightCollapsed: false,
      bottomCollapsed: true,
    });
    expect(screen.queryByRole('separator', { name: 'Resize the bottom panel' })).toBeNull();
    // A remembered size no longer follows the window.
    layOut(1366, 768);
    expect(valueOf(separator('Resize the C++ panel'), 'now')).toBe(420);
    expect(saved).not.toHaveBeenCalled();

    // Shown again: the remembered height, and the change is handed on.
    fireEvent.click(screen.getByRole('button', { name: 'Show the bottom panel' }));
    expect(valueOf(separator('Resize the bottom panel'), 'now')).toBe(150);
    expect(saved.mock.calls).toEqual([[{ bottomCollapsed: false }, true]]);
  });

  it('is taken from the settings when they arrive after the layout, and only once', () => {
    setWindowSize(1024, 700);
    renderLayout();
    layOut(1024, 700);
    const flyout = fakeFlyout(() => 517);
    expect(valueOf(separator('Resize the C++ panel'), 'now')).toBe(307);
    // Before the settings were read, nothing is handed on: the defaults would replace them.
    fireEvent.click(screen.getByRole('button', { name: 'Hide the bottom panel' }));
    fireEvent.click(screen.getByRole('button', { name: 'Show the bottom panel' }));
    expect(saved).not.toHaveBeenCalled();

    readSettings({ toolboxWidth: 200, rightDockWidth: 300, rightCollapsed: true });
    expect(useAppStore.getState().ui.rightCollapsed).toBe(true);
    expect(flyout.setWidth).toHaveBeenLastCalledWith(200);
    fireEvent.click(screen.getByRole('button', { name: 'Show the C++ panel' }));
    expect(valueOf(separator('Resize the C++ panel'), 'now')).toBe(300);
    expect(saved.mock.calls).toEqual([[{ rightCollapsed: false }, true]]);

    // Later answers (such as the one to a saved change) change nothing.
    readSettings({ rightDockWidth: 500, rightCollapsed: true });
    expect(valueOf(separator('Resize the C++ panel'), 'now')).toBe(300);
    expect(useAppStore.getState().ui.rightCollapsed).toBe(false);
  });

  it('hands on each change the user makes, but not the C++ dock hidden to fit', () => {
    readSettings({});
    setWindowSize(1024, 700);
    renderLayout();
    layOut(1024, 700);
    fakeFlyout(() => 794);

    fireEvent.keyDown(separator('Resize the C++ panel'), { key: 'ArrowLeft' });
    fireEvent.keyDown(separator('Resize the bottom panel'), { key: 'End' });
    fireEvent.keyDown(separator('Resize the toolbox'), { key: 'ArrowRight' });
    // Arrow keys are steps of a series; End ends one (it is written at once).
    expect(saved.mock.calls).toEqual([
      [{ rightDockWidth: 307 + STEP }, false],
      [{ bottomDockHeight: 413 }, true],
      [{ toolboxWidth: 320 + STEP }, false],
    ]);
    saved.mockClear();

    // Hidden because the window is too small, and shown again: not the user's choice.
    layOut(640, 560);
    expect(useAppStore.getState().ui.rightCollapsed).toBe(true);
    layOut(1024, 700);
    expect(useAppStore.getState().ui.rightCollapsed).toBe(false);
    expect(saved).not.toHaveBeenCalled();

    // Hidden by the user, and the bottom dock shown by a command (as Run does).
    fireEvent.click(screen.getByRole('button', { name: 'Hide the C++ panel' }));
    fireEvent.click(screen.getByRole('button', { name: 'Hide the bottom panel' }));
    act(() => {
      useAppStore.getState().actions.setUi({ bottomTab: 'console', bottomCollapsed: false });
    });
    // Showing or hiding a dock is settled: written at once, so closing the window right after it
    // keeps it.
    expect(saved.mock.calls).toEqual([
      [{ rightCollapsed: true }, true],
      [{ bottomCollapsed: true }, true],
      [{ bottomCollapsed: false }, true],
    ]);
  });
});
