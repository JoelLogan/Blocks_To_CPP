/**
 * Keeping the keyboard focus somewhere sensible when the window changes what it shows
 * (docs/spec/04-user-interface.md §4.8, WCAG 2.4.3): when a page or the editor replaces what had
 * the focus, the focus would otherwise fall back to the document's body, and the next Tab would
 * start again from the top of the window.
 */

/**
 * Whether the keyboard focus is lost: on nothing, on the body, on an element that is no longer in
 * the document, or inside something hidden.
 */
export function focusIsLost(doc: Document = document): boolean {
  const active = doc.activeElement;
  return (
    active === null ||
    active === doc.body ||
    active === doc.documentElement ||
    !active.isConnected ||
    active.closest('[hidden]') !== null
  );
}

/**
 * Moves the focus to `target` (without scrolling) when it is lost; leaves a focus that is still
 * somewhere visible alone. The target can be an SVG element, such as a block of the canvas.
 * Returns whether it moved the focus.
 */
export function focusIfLost(
  target: HTMLElement | SVGElement | null,
  doc: Document = document,
): boolean {
  if (target === null || !focusIsLost(doc)) {
    return false;
  }
  target.focus({ preventScroll: true });
  return doc.activeElement === target;
}
