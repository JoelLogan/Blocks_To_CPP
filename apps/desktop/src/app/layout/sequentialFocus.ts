/**
 * Tab as a browser does it, for the keyboard tests (only tests use this module): the next element
 * in the sequential focus order from where the focus is, even when the focused element is not
 * itself a tab stop (Blockly takes the tab stop off a tree while the focus is inside it, and
 * Testing Library's `user.tab()` then starts over from the top of the page).
 */

/** Elements that take the focus by Tab unless they say otherwise. */
const NATURALLY_FOCUSABLE =
  'a[href], button, input:not([type="hidden"]), select, textarea, summary, [contenteditable]:not([contenteditable="false"])';

/** Whether an element is displayed (no `hidden`, no `display: none` on it or above it). */
function isDisplayed(element: Element): boolean {
  for (let current: Element | null = element; current !== null; current = current.parentElement) {
    if (current.hasAttribute('hidden')) {
      return false;
    }
    const view = current.ownerDocument.defaultView;
    if (view !== null && view.getComputedStyle(current).display === 'none') {
      return false;
    }
  }
  return true;
}

/**
 * Whether a radio button is its group's tab stop: the checked one, or the first when none is
 * checked (the arrow keys move within the group).
 */
function isGroupStop(radio: HTMLInputElement): boolean {
  if (radio.name === '' || radio.checked) {
    return true;
  }
  const scope = radio.form ?? radio.ownerDocument;
  const group = [...scope.querySelectorAll<HTMLInputElement>('input[type="radio"]')].filter(
    (other) => other.name === radio.name && !other.disabled,
  );
  return !group.some((other) => other.checked) && group[0] === radio;
}

/** Whether Tab can stop at `element`. */
export function isTabStop(element: Element): boolean {
  const tabIndex = element.getAttribute('tabindex');
  if (tabIndex !== null) {
    if (Number(tabIndex) < 0) {
      return false;
    }
  } else if (!element.matches(NATURALLY_FOCUSABLE)) {
    return false;
  }
  if (element.matches(':disabled') || element.closest('[inert]') !== null) {
    return false;
  }
  if (element instanceof HTMLInputElement && element.type === 'radio' && !isGroupStop(element)) {
    return false;
  }
  return isDisplayed(element);
}

/** Every tab stop of the document, in document order (the app uses no positive tabindex). */
export function tabStops(doc: Document = document): Element[] {
  return [...doc.body.querySelectorAll('*')].filter(isTabStop);
}

/**
 * Presses Tab (or Shift+Tab): a `keydown` on the focused element that the page may cancel (Radix
 * dialogs keep the focus inside that way), then, unless cancelled, the focus goes to the next (or
 * previous) tab stop after (or before) the focused element in document order, or nowhere at the
 * end. Returns the element that has the focus afterwards, or `null`.
 */
export function pressTab(shift = false, doc: Document = document): Element | null {
  const active = doc.activeElement ?? doc.body;
  const event = new KeyboardEvent('keydown', {
    key: 'Tab',
    code: 'Tab',
    keyCode: 9,
    shiftKey: shift,
    bubbles: true,
    cancelable: true,
  });
  active.dispatchEvent(event);
  if (!event.defaultPrevented) {
    const stops = tabStops(doc);
    const following = (stop: Element) =>
      (active.compareDocumentPosition(stop) & Node.DOCUMENT_POSITION_FOLLOWING) !== 0 &&
      !active.contains(stop);
    const preceding = (stop: Element) =>
      (active.compareDocumentPosition(stop) & Node.DOCUMENT_POSITION_PRECEDING) !== 0 &&
      !stop.contains(active);
    const target = shift ? stops.filter(preceding).at(-1) : stops.find(following);
    if (target instanceof HTMLElement || target instanceof SVGElement) {
      target.focus();
    } else if (active instanceof HTMLElement || active instanceof SVGElement) {
      active.blur();
    }
  }
  active.dispatchEvent(
    new KeyboardEvent('keyup', {
      key: 'Tab',
      code: 'Tab',
      keyCode: 9,
      shiftKey: shift,
      bubbles: true,
    }),
  );
  const now = doc.activeElement;
  return now === doc.body ? null : now;
}

/** An element's text as a screen reader reads it: without what is hidden from it. */
function spokenText(element: Element): string {
  const copy = element.cloneNode(true);
  if (!(copy instanceof Element)) {
    return '';
  }
  for (const hidden of copy.querySelectorAll('[aria-hidden="true"]')) {
    hidden.remove();
  }
  return copy.textContent.replace(/\s+/g, ' ').trim();
}

/**
 * The accessible name of a control, close enough for the app's controls: `aria-label`, then
 * `aria-labelledby`, then a `<label for>` or wrapping `<label>`, then its own text.
 */
export function accessibleName(element: Element): string {
  const label = element.getAttribute('aria-label');
  if (label !== null) {
    return label.trim();
  }
  const doc = element.ownerDocument;
  const labelledBy = element.getAttribute('aria-labelledby');
  if (labelledBy !== null) {
    return labelledBy
      .split(/\s+/)
      .map((id) => doc.getElementById(id))
      .map((labelElement) => (labelElement === null ? '' : spokenText(labelElement)))
      .join(' ')
      .trim();
  }
  if (element.id !== '') {
    const forLabel = [...doc.querySelectorAll('label')].find(
      (candidate) => candidate.htmlFor === element.id,
    );
    if (forLabel !== undefined) {
      return spokenText(forLabel);
    }
  }
  const wrapping = element.closest('label');
  if (wrapping !== null) {
    return spokenText(wrapping);
  }
  return spokenText(element);
}

/** The role of an element: its `role`, an input's implicit role, or else its tag name. */
function roleOf(element: Element): string {
  const role = element.getAttribute('role');
  if (role !== null) {
    return role;
  }
  if (element instanceof HTMLInputElement) {
    return element.type === 'radio' || element.type === 'checkbox' ? element.type : 'textbox';
  }
  return element.tagName.toLowerCase();
}

/** A focusable element as a person would name it: its role (or tag) and accessible name. */
export function describeStop(element: Element): string {
  return `${roleOf(element)}: ${accessibleName(element)}`;
}

/**
 * Tabs through `container` from just before it to just after it and returns where each Tab
 * landed ({@link describeStop}). Two buttons put around it mark the way in and out; at most
 * `limit` Tabs are pressed.
 */
export function tabThrough(container: Element, limit = 100): string[] {
  const doc = container.ownerDocument;
  const before = doc.createElement('button');
  before.textContent = 'before';
  const after = doc.createElement('button');
  after.textContent = 'after';
  container.before(before);
  container.after(after);
  try {
    before.focus();
    const reached: string[] = [];
    for (let step = 0; step < limit; step++) {
      const landed = pressTab(false, doc);
      if (landed === null || landed === after) {
        break;
      }
      reached.push(describeStop(landed));
    }
    return reached;
  } finally {
    before.remove();
    after.remove();
  }
}
