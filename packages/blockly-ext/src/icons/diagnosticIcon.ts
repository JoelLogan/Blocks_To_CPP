/**
 * The diagnostics badge on a block (docs/spec/04-user-interface.md §4.4): a registered Blockly icon,
 * `b2c_diagnostic`, at the block's top-right corner. It shows ✖ for an error, ⚠ for a warning and
 * ℹ for an info (the most serious one wins), each with its own shape, and a plain-text tooltip and
 * accessible name with the friendly messages. The block's outline is tinted by the same severity,
 * and the part a diagnostic points at is marked (./parts.ts).
 *
 * The icon is never saved: it has no `saveState`, so Blockly's serialisation (copy, paste,
 * insertion markers) leaves it out, and the project file never sees it.
 *
 * Layout: Blockly lays icons out at the start of the first row. The badge must not move the block's
 * content when it appears, so it reports a negative width that cancels the padding the renderer
 * puts after an icon, and draws itself at the top-right corner once the block has its final size
 * (`onLocationChange`, which Blockly calls after every render and move).
 */
import * as Blockly from 'blockly/core';

import { BADGE_FILL, DIAGNOSTIC_CSS_CLASS, registerDiagnosticCss } from './css';
import { syncPartMarks } from './parts';
import {
  type BlockDiagnosticItem,
  type DiagnosticBadgeSummary,
  sameSummary,
  type Severity,
  summariseDiagnostics,
} from './summary';

/** The badge's registry name. */
export const DIAGNOSTIC_ICON_NAME = 'b2c_diagnostic';

/** The badge's size in workspace units (a square). */
export const BADGE_SIZE = 18;

/** How far the badge sticks out of the block's top-right corner, in workspace units. */
const BADGE_OVERHANG = 6;

/** Icons with a larger weight go later; Blockly's own (mutator, warning, comment) come first. */
const BADGE_WEIGHT = 100;

const SVG_NS = 'http://www.w3.org/2000/svg';

/** The badge's shapes, drawn in an 18 × 18 box. Static data: nothing here comes from a project. */
const BADGE_DRAWING: Readonly<Record<Severity, { readonly shape: Shape; readonly glyph: string }>> =
  Object.freeze({
    // A circle with a cross (✖).
    error: { shape: { circle: { cx: 9, cy: 9, r: 8 } }, glyph: 'M6 6 L12 12 M12 6 L6 12' },
    // A triangle with an exclamation mark (⚠).
    warning: {
      shape: { path: 'M9 1.2 L17.2 16 L0.8 16 Z' },
      glyph: 'M9 6.4 L9 10.6 M9 13.4 L9 13.5',
    },
    // A circle with an "i" (ℹ).
    info: { shape: { circle: { cx: 9, cy: 9, r: 8 } }, glyph: 'M9 8.2 L9 13 M9 5.2 L9 5.3' },
  });

type Shape =
  | { readonly circle: { readonly cx: number; readonly cy: number; readonly r: number } }
  | { readonly path: string };

/** The badge's icon type, for `block.getIcon(DIAGNOSTIC_ICON_TYPE)`. */
export const DIAGNOSTIC_ICON_TYPE = new Blockly.icons.IconType<DiagnosticIcon>(
  DIAGNOSTIC_ICON_NAME,
);

/** The diagnostics badge of one block. Create it through {@link setBlockDiagnostics}. */
export class DiagnosticIcon extends Blockly.icons.Icon {
  #summary: DiagnosticBadgeSummary;
  /** The severity the current drawing shows, or null before the view exists. */
  #drawn: Severity | null = null;

  constructor(block: Blockly.Block, summary: DiagnosticBadgeSummary) {
    super(block);
    this.#summary = summary;
    this.setTooltip(summary.tooltip);
  }

  override getType(): Blockly.icons.IconType<DiagnosticIcon> {
    return DIAGNOSTIC_ICON_TYPE;
  }

  /** What the badge shows now. */
  get summary(): DiagnosticBadgeSummary {
    return this.#summary;
  }

  /** Shows `summary` instead (redrawing only what changed). */
  setSummary(summary: DiagnosticBadgeSummary): void {
    if (sameSummary(summary, this.#summary)) {
      return;
    }
    this.#summary = summary;
    this.setTooltip(summary.tooltip);
    this.#updateView();
  }

  override getWeight(): number {
    return BADGE_WEIGHT;
  }

  /**
   * A negative width that cancels the padding the renderer puts after an icon, so the block's
   * content stays where it is when the badge comes and goes (the badge draws itself outside the
   * row, see {@link onLocationChange}).
   */
  override getSize(): Blockly.utils.Size {
    const block = this.sourceBlock;
    const padding =
      block instanceof Blockly.BlockSvg
        ? block.workspace.getRenderer().getConstants().MEDIUM_PADDING
        : 0;
    return new Blockly.utils.Size(-padding, 0);
  }

  override initView(pointerdownListener: (e: PointerEvent) => void): void {
    if (this.svgRoot !== null) {
      return;
    }
    super.initView(pointerdownListener);
    // Read again through a method: the base class has just created it.
    const root = this.#root();
    if (root === null) {
      return;
    }
    Blockly.utils.dom.addClass(root, DIAGNOSTIC_CSS_CLASS.badge);
    root.setAttribute('role', 'img');
    this.#updateView();
    this.#place();
  }

  /** The badge stays visible on a collapsed block: it then also stands for the blocks inside. */
  override isShownWhenCollapsed(): boolean {
    return true;
  }

  override updateCollapsed(): void {
    // Blockly hides icons of collapsed blocks; this one keeps showing.
  }

  override setOffsetInBlock(): void {
    // The renderer's place is the start of the first row; the badge goes to the top-right corner.
    this.#place();
  }

  override onLocationChange(blockOrigin: Blockly.utils.Coordinate): void {
    // Called after every render, when the block has its final width.
    this.#place();
    super.onLocationChange(blockOrigin);
  }

  override onNodeFocus(): void {
    // Keyboard navigation: show the badge (the base class measures the negative layout size).
    const block = this.sourceBlock;
    if (block instanceof Blockly.BlockSvg) {
      const origin = block.getRelativeToSurfaceXY();
      const left = origin.x + this.offsetInBlock.x;
      const top = origin.y + this.offsetInBlock.y;
      block.workspace.scrollBoundsIntoView(
        new Blockly.utils.Rect(top, top + BADGE_SIZE, left, left + BADGE_SIZE),
      );
    }
  }

  override onClick(): void {
    // Nothing more to show: the messages are in the tooltip and in Problems.
  }

  /** The badge's SVG group, or null before the view exists. */
  #root(): SVGGElement | null {
    return this.svgRoot;
  }

  /** Draws the badge for the current severity and sets its classes and accessible name. */
  #updateView(): void {
    const root = this.svgRoot;
    if (root === null) {
      return;
    }
    const { severity, dimmed, label } = this.#summary;
    root.setAttribute('aria-label', label);
    for (const [name, className] of Object.entries(DIAGNOSTIC_CSS_CLASS.badgeSeverity)) {
      if (name === severity) {
        Blockly.utils.dom.addClass(root, className);
      } else {
        Blockly.utils.dom.removeClass(root, className);
      }
    }
    if (dimmed) {
      Blockly.utils.dom.addClass(root, DIAGNOSTIC_CSS_CLASS.badgeStale);
    } else {
      Blockly.utils.dom.removeClass(root, DIAGNOSTIC_CSS_CLASS.badgeStale);
    }
    if (this.#drawn !== severity) {
      this.#drawn = severity;
      drawBadge(root, severity);
    }
  }

  /** Moves the badge to the block's top-right corner (top-left on a right-to-left workspace). */
  #place(): void {
    const root = this.svgRoot;
    const block = this.sourceBlock;
    if (root === null || !(block instanceof Blockly.BlockSvg)) {
      return;
    }
    const width = Number.isFinite(block.width) ? block.width : 0;
    const x = block.RTL ? -width - BADGE_OVERHANG : width - BADGE_SIZE + BADGE_OVERHANG;
    const y = -BADGE_OVERHANG;
    this.offsetInBlock = new Blockly.utils.Coordinate(x, y);
    root.setAttribute('transform', `translate(${String(x)}, ${String(y)})`);
    // Above the blocks nested in this one (their groups are children of the block's group too).
    const parent = root.parentNode;
    if (parent !== null && parent.lastChild !== root) {
      parent.appendChild(root);
    }
  }
}

/** Replaces the badge's drawing with the one for `severity`. */
function drawBadge(root: SVGGElement, severity: Severity): void {
  root.replaceChildren();
  const { shape, glyph } = BADGE_DRAWING[severity];
  const fill = BADGE_FILL[severity];
  const shapeElement =
    'circle' in shape
      ? svgElement('circle', {
          cx: String(shape.circle.cx),
          cy: String(shape.circle.cy),
          r: String(shape.circle.r),
        })
      : svgElement('path', { d: shape.path, 'stroke-linejoin': 'round' });
  // Presentation attributes as well as the CSS classes: the badge reads right even without the CSS.
  shapeElement.setAttribute('class', DIAGNOSTIC_CSS_CLASS.badgeShape);
  shapeElement.setAttribute('fill', fill);
  shapeElement.setAttribute('stroke', '#ffffff');
  const glyphElement = svgElement('path', {
    d: glyph,
    class: DIAGNOSTIC_CSS_CLASS.badgeGlyph,
    fill: 'none',
    stroke: '#ffffff',
    'stroke-width': '2',
    'stroke-linecap': 'round',
  });
  root.append(shapeElement, glyphElement);
}

function svgElement(name: string, attributes: Readonly<Record<string, string>>): SVGElement {
  const element = document.createElementNS(SVG_NS, name);
  for (const [key, value] of Object.entries(attributes)) {
    element.setAttribute(key, value);
  }
  return element;
}

let iconRegistered = false;

/**
 * Registers the badge's icon type with Blockly and adds its CSS. Idempotent; call it before the
 * first {@link setBlockDiagnostics} (that calls it too).
 */
export function registerDiagnosticIcon(): void {
  registerDiagnosticCss();
  if (iconRegistered) {
    return;
  }
  iconRegistered = true;
  try {
    Blockly.icons.registry.register(DIAGNOSTIC_ICON_TYPE, DiagnosticIconFromRegistry);
  } catch {
    // Already registered (a second copy of this module, or a hot reload): keep that one.
  }
}

/**
 * The constructor the icon registry needs (`new (block) => IIcon`). The badge is never saved, so
 * Blockly never builds one from saved state; if it did, the badge would start empty.
 */
class DiagnosticIconFromRegistry extends DiagnosticIcon {
  constructor(block: Blockly.Block) {
    super(block, {
      severity: 'info',
      dimmed: true,
      count: 0,
      tooltip: '',
      label: 'No problems',
    });
  }
}

/** Runs `action` with Blockly's events off: marks and badges are display, never edits. */
function withoutEvents(action: () => void): void {
  Blockly.Events.disable();
  try {
    action();
  } finally {
    Blockly.Events.enable();
  }
}

/** The block's outline classes for a summary (none when there is nothing to show). */
function syncOutline(block: Blockly.Block, summary: DiagnosticBadgeSummary | null): void {
  if (!(block instanceof Blockly.BlockSvg)) {
    return;
  }
  for (const [severity, className] of Object.entries(DIAGNOSTIC_CSS_CLASS.block)) {
    if (summary?.severity === severity) {
      block.addClass(className);
    } else {
      block.removeClass(className);
    }
  }
  if (summary?.dimmed === true) {
    block.addClass(DIAGNOSTIC_CSS_CLASS.blockStale);
  } else {
    block.removeClass(DIAGNOSTIC_CSS_CLASS.blockStale);
  }
}

/**
 * Shows `items` on `block`: the badge (added, updated or removed), the severity outline, and the
 * marks on the parts the items point at. An empty list removes everything. Items with an unknown
 * severity are ignored. Fires no Blockly events, so it never touches undo or the document.
 */
export function setBlockDiagnostics(
  block: Blockly.Block,
  items: readonly BlockDiagnosticItem[],
): void {
  registerDiagnosticIcon();
  if (block.isDeadOrDying()) {
    return;
  }
  const summary = summariseDiagnostics(items);
  withoutEvents(() => {
    const icon = block.getIcon(DIAGNOSTIC_ICON_TYPE);
    if (summary === null) {
      if (icon !== undefined) {
        block.removeIcon(DIAGNOSTIC_ICON_TYPE);
      }
    } else if (icon instanceof DiagnosticIcon) {
      icon.setSummary(summary);
    } else {
      block.addIcon(new DiagnosticIcon(block, summary));
    }
    syncOutline(block, summary);
    syncPartMarks(block, summary === null ? [] : items);
  });
}

/** What the badge of `block` shows, or null when it has none. */
export function getBlockDiagnostics(block: Blockly.Block): DiagnosticBadgeSummary | null {
  const icon = block.getIcon(DIAGNOSTIC_ICON_TYPE);
  return icon instanceof DiagnosticIcon ? icon.summary : null;
}
