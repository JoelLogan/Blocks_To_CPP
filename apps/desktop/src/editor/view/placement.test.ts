/**
 * Where the canvas controls sit (./placement.ts): at the top right of the visible canvas, clear of
 * its scrollbar, never wider than the canvas (so never over the toolbox or its flyout), placed
 * again when Blockly resizes the workspace and when the flyout changes size.
 */
import { b2cLightTheme } from '@blocks2cpp/blockly-ext';
import * as Blockly from 'blockly/core';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { editorInjectOptions } from '../EditorWorkspace';
import { disposeWorkspaces, renderedWorkspace, setViewSize } from '../sync/testing';
import {
  attachControlsPlacement,
  CONTROLS_COMPONENT_ID,
  CONTROLS_MARGIN,
  CONTROLS_MIN_WIDTH,
  controlsPlace,
} from './placement';

/** A ResizeObserver the test calls back by hand (happy-dom lays nothing out). */
class ManualResizeObserver {
  static instances: ManualResizeObserver[] = [];
  readonly callback: () => void;
  readonly observed: Element[] = [];
  disconnected = false;
  constructor(callback: () => void) {
    this.callback = callback;
    ManualResizeObserver.instances.push(this);
  }
  observe(target: Element): void {
    this.observed.push(target);
  }
  unobserve(): void {
    // Not used.
  }
  disconnect(): void {
    this.disconnected = true;
  }
}

beforeEach(() => {
  ManualResizeObserver.instances = [];
  vi.stubGlobal('ResizeObserver', ManualResizeObserver);
});

afterEach(() => {
  disposeWorkspaces();
});

/** UI metrics with a canvas `view` pixels wide right of a toolbox and flyout `left` pixels wide. */
function metrics(left: number, view: number, top = 0): Blockly.MetricsManager.UiMetrics {
  return {
    viewMetrics: { top: 0, left: 0, width: view, height: 500 },
    absoluteMetrics: { top, left },
    toolboxMetrics: { width: 80, height: 500, position: Blockly.utils.toolbox.Position.LEFT },
  };
}

/** A canvas as the editor injects it, `width` × 700 pixels. */
function editorCanvas(width: number): Blockly.WorkspaceSvg {
  const workspace = renderedWorkspace(editorInjectOptions(b2cLightTheme));
  setViewSize(workspace, width, 700);
  Blockly.svgResize(workspace);
  return workspace;
}

/** The controls' element, in the workspace's container as the editor puts it. */
function controlsOf(workspace: Blockly.WorkspaceSvg): HTMLElement {
  const element = document.createElement('div');
  const container = workspace.getInjectionDiv().parentElement;
  if (container === null) {
    throw new Error('The workspace has no container');
  }
  container.append(element);
  return element;
}

/** The visible canvas's width in pixels. */
function viewWidth(workspace: Blockly.WorkspaceSvg): number {
  return workspace.getMetricsManager().getViewMetrics().width;
}

describe('the place of the canvas controls', () => {
  it('is the top right of the visible canvas, left of its scrollbar', () => {
    expect(controlsPlace(metrics(300, 700), 15)).toEqual({
      top: CONTROLS_MARGIN,
      right: 15 + CONTROLS_MARGIN,
      maxWidth: 700 - 15 - 2 * CONTROLS_MARGIN,
      shown: true,
    });
    // No scrollbar, and a canvas that does not start at the top.
    expect(controlsPlace(metrics(300, 700, 40), 0)).toEqual({
      top: 40 + CONTROLS_MARGIN,
      right: CONTROLS_MARGIN,
      maxWidth: 700 - 2 * CONTROLS_MARGIN,
      shown: true,
    });
  });

  it('is never wider than the canvas, so it never reaches the toolbox or its flyout', () => {
    for (const view of [600, 200, 120]) {
      const place = controlsPlace(metrics(480, view), 15);
      expect(place.shown).toBe(true);
      // The controls' left edge at their widest (from the container's right edge, 480 + view) is
      // right of the flyout's, with the margin.
      expect(480 + view - place.right - place.maxWidth).toBe(480 + CONTROLS_MARGIN);
    }
    // A canvas with no room for even one control hides them.
    for (const view of [15 + 2 * CONTROLS_MARGIN + CONTROLS_MIN_WIDTH - 1, 40, 0]) {
      expect(controlsPlace(metrics(480, view), 15).shown).toBe(false);
    }
  });
});

describe('the placement of the canvas controls', () => {
  it('places them at once, and again whenever Blockly resizes the workspace', () => {
    const workspace = editorCanvas(1000);
    const element = controlsOf(workspace);
    const detach = attachControlsPlacement(workspace, element);
    const scrollbar = Blockly.Scrollbar.scrollbarThickness;
    expect(element.style.top).toBe(`${String(CONTROLS_MARGIN)}px`);
    expect(element.style.right).toBe(`${String(scrollbar + CONTROLS_MARGIN)}px`);
    expect(element.style.maxWidth).toBe(
      `${String(viewWidth(workspace) - scrollbar - 2 * CONTROLS_MARGIN)}px`,
    );

    expect(element.style.display).toBe('');

    // A narrower window: Blockly resizes the workspace and places its components again.
    setViewSize(workspace, 640, 700);
    Blockly.svgResize(workspace);
    expect(viewWidth(workspace)).toBeLessThan(640);
    expect(element.style.maxWidth).toBe(
      `${String(viewWidth(workspace) - scrollbar - 2 * CONTROLS_MARGIN)}px`,
    );

    // No room left for the canvas: hidden, and shown again with room.
    setViewSize(workspace, 200, 700);
    Blockly.svgResize(workspace);
    expect(element.style.display).toBe('none');
    setViewSize(workspace, 1000, 700);
    Blockly.svgResize(workspace);
    expect(element.style.display).toBe('');
    detach();
  });

  it('places them again when the flyout changes size, which does not resize the workspace', () => {
    const workspace = editorCanvas(1000);
    const element = controlsOf(workspace);
    const detach = attachControlsPlacement(workspace, element);
    const observer = ManualResizeObserver.instances.at(-1);
    const flyout = workspace.getFlyout()?.getWorkspace().getParentSvg();
    expect(observer?.observed).toEqual([flyout]);

    // The user drags the flyout's edge: 100 pixels less canvas.
    const before = workspace.getMetricsManager().getUiMetrics();
    vi.spyOn(workspace.getMetricsManager(), 'getUiMetrics').mockReturnValue({
      ...before,
      absoluteMetrics: { ...before.absoluteMetrics, left: before.absoluteMetrics.left + 100 },
      viewMetrics: { ...before.viewMetrics, width: before.viewMetrics.width - 100 },
    });
    observer?.callback();
    const scrollbar = Blockly.Scrollbar.scrollbarThickness;
    expect(element.style.maxWidth).toBe(
      `${String(before.viewMetrics.width - 100 - scrollbar - 2 * CONTROLS_MARGIN)}px`,
    );
    detach();
  });

  it('stops: the component is removed and the flyout no longer watched', () => {
    const workspace = editorCanvas(1000);
    const element = controlsOf(workspace);
    const detach = attachControlsPlacement(workspace, element);
    const components = workspace.getComponentManager();
    expect(
      components.hasCapability(
        CONTROLS_COMPONENT_ID,
        Blockly.ComponentManager.Capability.POSITIONABLE,
      ),
    ).toBe(true);

    detach();
    expect(components.getComponent(CONTROLS_COMPONENT_ID)).toBeUndefined();
    expect(ManualResizeObserver.instances.at(-1)?.disconnected).toBe(true);
    // Resizing no longer moves the controls.
    element.style.maxWidth = '';
    setViewSize(workspace, 700, 700);
    Blockly.svgResize(workspace);
    expect(element.style.maxWidth).toBe('');
  });
});
