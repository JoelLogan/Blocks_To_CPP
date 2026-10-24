/**
 * Where the canvas controls (./CanvasControls.tsx) sit: at the top right of the visible canvas,
 * clear of its vertical scrollbar, and never wider than the canvas, so they never cover the
 * toolbox, its flyout or the separator at the flyout's edge (04 §4.1). A narrow canvas wraps them
 * onto a second row instead; a canvas too narrow for even one of them hides them.
 *
 * The controls are HTML next to Blockly's injection `div` (in the same positioned container), so
 * Blockly's pixel coordinates are theirs. They are placed the way Blockly places its own trash can
 * and zoom controls: as a positionable component of the workspace (Blockly's component manager),
 * which Blockly places again whenever the workspace is resized. The flyout changes its width
 * without resizing the workspace (a new category's contents, the user dragging its edge), so the
 * flyout's size is watched as well.
 */
import * as Blockly from 'blockly/core';

/** The space between the controls and the edges of the visible canvas, in pixels. */
export const CONTROLS_MARGIN = 8;

/**
 * The least room the controls need, in pixels: below it (a canvas squeezed to almost nothing) they
 * are hidden rather than drawn over the toolbox.
 */
export const CONTROLS_MIN_WIDTH = 72;

/** The controls' ID in the workspace's component manager. */
export const CONTROLS_COMPONENT_ID = 'b2cCanvasControls';

/** Where the controls go, in pixels relative to the workspace's container. */
export interface ControlsPlace {
  /** From the container's top edge to the controls' top. */
  readonly top: number;
  /** From the container's right edge to the controls' right. */
  readonly right: number;
  /** The widest the controls may be: the visible canvas less the scrollbar and the margins. */
  readonly maxWidth: number;
  /** Whether they are shown: `maxWidth` is at least {@link CONTROLS_MIN_WIDTH}. */
  readonly shown: boolean;
}

/**
 * Where the controls go for the workspace's `metrics` (Blockly's, in pixels) with a vertical
 * scrollbar `scrollbar` pixels wide at the canvas's right edge (0 when there is none). The editor's
 * toolbox is at the left, so the visible canvas reaches the container's right edge.
 */
export function controlsPlace(
  metrics: Blockly.MetricsManager.UiMetrics,
  scrollbar: number,
): ControlsPlace {
  const { absoluteMetrics, viewMetrics } = metrics;
  const maxWidth = Math.max(0, viewMetrics.width - scrollbar - 2 * CONTROLS_MARGIN);
  return {
    top: absoluteMetrics.top + CONTROLS_MARGIN,
    right: scrollbar + CONTROLS_MARGIN,
    maxWidth,
    shown: maxWidth >= CONTROLS_MIN_WIDTH,
  };
}

/** The width of the canvas's vertical scrollbar, or 0 when it has none. */
function verticalScrollbar(workspace: Blockly.WorkspaceSvg): number {
  return workspace.scrollbar?.canScrollVertically() === true
    ? Blockly.Scrollbar.scrollbarThickness
    : 0;
}

/** The controls as a positionable component of the workspace. */
class ControlsPositioner implements Blockly.IPositionable {
  readonly id = CONTROLS_COMPONENT_ID;
  private readonly workspace: Blockly.WorkspaceSvg;
  private readonly element: HTMLElement;

  constructor(workspace: Blockly.WorkspaceSvg, element: HTMLElement) {
    this.workspace = workspace;
    this.element = element;
  }

  /** Places the controls for `metrics` (Blockly calls this whenever the workspace is resized). */
  position(metrics: Blockly.MetricsManager.UiMetrics): void {
    const place = controlsPlace(metrics, verticalScrollbar(this.workspace));
    const style = this.element.style;
    style.top = `${String(place.top)}px`;
    style.right = `${String(place.right)}px`;
    style.maxWidth = `${String(place.maxWidth)}px`;
    style.display = place.shown ? '' : 'none';
  }

  /** `null`: Blockly's other components (the trash can, at the bottom right) need not avoid them. */
  getBoundingRectangle(): Blockly.utils.Rect | null {
    return null;
  }

  /** Places the controls for the workspace's metrics now. */
  update(): void {
    this.position(this.workspace.getMetricsManager().getUiMetrics());
  }
}

/**
 * Keeps `element` (the controls, absolutely positioned in the container of the workspace's
 * injection `div`) at the top right of the visible canvas: places it now, whenever Blockly resizes
 * the workspace and whenever the toolbox's flyout changes size. Returns the function that stops.
 */
export function attachControlsPlacement(
  workspace: Blockly.WorkspaceSvg,
  element: HTMLElement,
): () => void {
  const positioner = new ControlsPositioner(workspace, element);
  const components = workspace.getComponentManager();
  components.addComponent({
    component: positioner,
    weight: Blockly.ComponentManager.ComponentWeight.ZOOM_CONTROLS_WEIGHT,
    capabilities: [Blockly.ComponentManager.Capability.POSITIONABLE],
  });
  positioner.update();

  const observer = new ResizeObserver(() => {
    positioner.update();
  });
  const flyout = workspace.getFlyout();
  if (flyout !== null) {
    observer.observe(flyout.getWorkspace().getParentSvg());
  }
  return () => {
    observer.disconnect();
    components.removeComponent(CONTROLS_COMPONENT_ID);
  };
}
