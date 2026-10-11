/**
 * Whether the keyboard is in use (docs/spec/04-user-interface.md §4.8): `data-input="keyboard"` on
 * the document's root element from a Tab key press until the next pointer press. app.css shows
 * the focus ring of a page's heading, which takes the focus from code when the page opens, only
 * then: a keyboard user sees where the focus is, and someone who clicked does not get a ring
 * around a heading they never chose.
 *
 * The browser's own `:focus-visible` cannot tell: WebKitGTK matches it for focus moved by a script
 * when the window starts and after a mouse click.
 */
import { useEffect } from 'react';

/** The attribute on the root element, and its value while the keyboard is in use. */
export const INPUT_ATTRIBUTE = 'data-input';
export const KEYBOARD_INPUT = 'keyboard';

/**
 * Tracks the input modality on `doc`: sets {@link INPUT_ATTRIBUTE} on a Tab key press and removes
 * it on a pointer press. Both listeners are on the capture phase, so a handler that stops an
 * event cannot hide it. Returns the function that stops tracking and removes the attribute.
 */
export function trackInputModality(doc: Document = document): () => void {
  const root = doc.documentElement;
  const onKeyDown = (event: KeyboardEvent) => {
    if (event.key === 'Tab') {
      root.setAttribute(INPUT_ATTRIBUTE, KEYBOARD_INPUT);
    }
  };
  const onPointerDown = () => {
    root.removeAttribute(INPUT_ATTRIBUTE);
  };
  doc.addEventListener('keydown', onKeyDown, true);
  doc.addEventListener('pointerdown', onPointerDown, true);
  return () => {
    doc.removeEventListener('keydown', onKeyDown, true);
    doc.removeEventListener('pointerdown', onPointerDown, true);
    root.removeAttribute(INPUT_ATTRIBUTE);
  };
}

/** Tracks the input modality on the window's document while the calling component is mounted. */
export function useInputModality(): void {
  useEffect(() => trackInputModality(document), []);
}
