/**
 * Reduced motion in the block editor (docs/spec/04-user-interface.md §4.8). The app's style sheet
 * already turns CSS transitions and animations off when the system asks for reduced motion
 * (src/app/app.css), which covers Blockly's scrolling and flyout transitions, and ./keyboard.css
 * hides the two effects Blockly draws itself (the shrinking copy of a deleted block and the ripple
 * of a new connection). What is left is the wiggle Blockly starts with a timer when a block is
 * dragged out of a stack: it is stopped as soon as the drag is reported.
 */
import * as Blockly from 'blockly/core';

/** The media query of the system's reduced-motion setting. */
export const REDUCED_MOTION_QUERY = '(prefers-reduced-motion: reduce)';

/** Looks a media query up (`window.matchMedia`, or a stand-in in tests). */
export type MatchMedia = (query: string) => Pick<MediaQueryList, 'matches'> | null;

/** The window's `matchMedia`, or `null` where there is none. */
export function windowMatchMedia(): MatchMedia | null {
  return typeof window.matchMedia === 'function' ? (query) => window.matchMedia(query) : null;
}

/** Whether the system asks for reduced motion now. Never throws. */
export function prefersReducedMotion(matchMedia: MatchMedia | null = windowMatchMedia()): boolean {
  try {
    return matchMedia?.(REDUCED_MOTION_QUERY)?.matches === true;
  } catch {
    return false;
  }
}

/**
 * Stops Blockly's wiggle of a block dragged out of a stack on `workspace` while the system asks for
 * reduced motion (read at each drag, so a change of the setting applies at once). Returns the
 * function that stops listening.
 */
export function attachReducedMotion(
  workspace: Blockly.WorkspaceSvg,
  matchMedia: MatchMedia | null = windowMatchMedia(),
): () => void {
  const onChange = (event: Blockly.Events.Abstract) => {
    if (
      event instanceof Blockly.Events.BlockDrag &&
      event.isStart === true &&
      prefersReducedMotion(matchMedia)
    ) {
      Blockly.blockAnimations.disconnectUiStop();
    }
  };
  workspace.addChangeListener(onChange);
  return () => {
    workspace.removeChangeListener(onChange);
  };
}
