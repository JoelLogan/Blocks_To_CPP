/**
 * What the toolbox's flyout tells the window's layout about itself (docs/spec/04-user-interface.md
 * §4.1, §4.2): where it is, how wide it is and may be made, and how to change its width. The
 * layout puts the *Resize the toolbox* splitter at the flyout's right edge with it, and keeps the
 * canvas at least {@link CANVAS_MIN_WIDTH} wide when it sizes the C++ dock.
 *
 * The flyout (src/editor/toolbox/continuous.ts) sends a {@link FLYOUT_LAYOUT_EVENT} from the
 * canvas's injection div whenever any of these change; it bubbles up to the workspace region,
 * where the layout listens. The module holds no Blockly: the shell never loads it (Blockly is the
 * editor's, loaded with the workspace).
 */

/** The DOM event a toolbox's flyout sends, with a {@link FlyoutLayout} as its `detail`. */
export const FLYOUT_LAYOUT_EVENT = 'b2c-flyout-layout';

/**
 * The narrowest the canvas beside the toolbox's flyout is made, in CSS pixels: the flyout
 * narrows to leave it, and the C++ dock is never made so wide that it takes it (04 §4.1).
 */
export const CANVAS_MIN_WIDTH = 200;

/** Where the toolbox's flyout is and how wide it may be made, in CSS pixels. */
export interface FlyoutLayout {
  /**
   * The flyout's left edge: from the left of the canvas's injection div (the event's target),
   * which is the width of the toolbox's category column.
   */
  readonly left: number;
  /** The flyout's width (0 while it shows nothing). */
  readonly width: number;
  /** The narrowest the flyout can be made. */
  readonly min: number;
  /** The widest the flyout can be made now: what leaves the canvas {@link CANVAS_MIN_WIDTH}. */
  readonly max: number;
  /**
   * Sets the width the user chose, which the flyout keeps within {@link min}–{@link max} as the
   * window changes; `null` goes back to the default (as wide as its blocks, at most 320 px).
   */
  readonly setWidth: (width: number | null) => void;
}

/** Sends `layout` from `target` (the canvas's injection div) up to the layout around it. */
export function announceFlyoutLayout(target: EventTarget, layout: FlyoutLayout): void {
  target.dispatchEvent(new CustomEvent(FLYOUT_LAYOUT_EVENT, { bubbles: true, detail: layout }));
}

/**
 * Calls `listener` with each {@link FlyoutLayout} a flyout inside `region` sends, and the element
 * it came from (the canvas's injection div). Returns the function that stops listening.
 */
export function listenToFlyoutLayout(
  region: EventTarget,
  listener: (layout: FlyoutLayout, from: Element) => void,
): () => void {
  const onLayout = (event: Event): void => {
    if (event instanceof CustomEvent && event.target instanceof Element) {
      listener(event.detail as FlyoutLayout, event.target);
    }
  };
  region.addEventListener(FLYOUT_LAYOUT_EVENT, onLayout);
  return () => {
    region.removeEventListener(FLYOUT_LAYOUT_EVENT, onLayout);
  };
}
