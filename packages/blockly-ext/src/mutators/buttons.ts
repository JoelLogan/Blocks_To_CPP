/**
 * The inline ⊕ and ⊖ buttons (03 §3.2): image fields with a text alternative.
 *
 * The images are constant SVG documents in `data:` URLs, which the app's CSP allows for images
 * (`img-src 'self' data:`). No project content ever goes into them. The text alternative is set as
 * plain text: an SVG `<title>` element's text content, the image's `aria-label` and the field's
 * tooltip (docs/security/custom-field-review-checklist.md: text-only rendering, no HTML).
 */
import * as Blockly from 'blockly/core';

/** What a button does. */
export type ButtonAction = 'add' | 'remove';

/** The size of a button in workspace units. */
const BUTTON_SIZE = 20;

function iconUrl(path: string): string {
  const svg =
    '<svg xmlns="http://www.w3.org/2000/svg" width="20" height="20" viewBox="0 0 20 20">' +
    '<circle cx="10" cy="10" r="8.5" fill="#000000" fill-opacity="0.18" stroke="#ffffff" stroke-width="1.5"/>' +
    `<path d="${path}" stroke="#ffffff" stroke-width="2" stroke-linecap="round" fill="none"/>` +
    '</svg>';
  return `data:image/svg+xml,${encodeURIComponent(svg)}`;
}

/** ⊕ */
const ADD_ICON = iconUrl('M10 5.5v9M5.5 10h9');
/** ⊖ */
const REMOVE_ICON = iconUrl('M5.5 10h9');

/**
 * An inline ⊕ or ⊖ button. Clicking it, or activating it from the keyboard, calls `onActivate`.
 * It is not serialised and holds no project data.
 */
export class MutatorButton extends Blockly.FieldImage {
  /** What the button does. */
  readonly action: ButtonAction;
  /** The text alternative, also the tooltip. */
  readonly label: string;

  constructor(action: ButtonAction, label: string, onActivate: (button: MutatorButton) => void) {
    super(action === 'add' ? ADD_ICON : REMOVE_ICON, BUTTON_SIZE, BUTTON_SIZE, label, (field) => {
      if (field instanceof MutatorButton) {
        onActivate(field);
      }
    });
    this.action = action;
    this.label = label;
    this.setTooltip(label);
  }

  override initView(): void {
    super.initView();
    const image = this.imageElement;
    if (image === null) {
      return;
    }
    image.setAttribute('role', 'button');
    image.setAttribute('aria-label', this.label);
    const title = Blockly.utils.dom.createSvgElement('title', {}, image);
    title.textContent = this.label;
  }
}
