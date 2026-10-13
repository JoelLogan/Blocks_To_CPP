/**
 * The Scratch-style continuous toolbox (docs/spec/04-user-interface.md §4.2): category bubbles on
 * the left and one flyout, always open, that scrolls through every category. It uses
 * `@blockly/continuous-toolbox` (Blockly team; checked against the dependency policy of 08 §8.9),
 * whose toolbox shows dynamic categories inside the continuous flyout under Blockly 12.5.
 *
 * The plugin's own registration function is not used: it replaces Blockly's defaults for every
 * workspace (the category row, the block inflater) and registers CSS, which Blockly allows only
 * before the first workspace is injected. Instead its classes are registered under names of ours
 * and chosen per workspace through the injection options (`toolboxInjectOptions`, register.ts),
 * with these changes:
 *
 * - the toolbox shows the flyout again only when its contents changed, and its delayed refresh is
 *   cancelled when the toolbox is disposed;
 * - the flyout never recycles blocks (the toolbox's blocks change between showings: default names,
 *   fresh symbol IDs), so it keeps a recycler of its own that stays empty instead of needing the
 *   plugin's recycler to replace Blockly's block inflater for every flyout;
 * - showing the flyout again keeps the keyboard focus on the same item when it was in the flyout
 *   (04 §4.8), and the animated scroll to a chosen category jumps at once when the system asks for
 *   reduced motion;
 * - the scroll to a chosen category goes half a unit past its heading, so that the category stays
 *   selected at every zoom level (04 §4.2);
 * - the flyout draws its blocks at one scale whatever the canvas zoom, and is as wide as the user
 *   made it with the *Resize the toolbox* splitter, by default at most
 *   {@link FLYOUT_WIDTH}.preferred wide; it narrows when the canvas would otherwise have less than
 *   {@link FLYOUT_WIDTH}.canvas beside it, and a wider block is cut off at its edge (04 §4.2);
 * - the canvas records where the flyout is as a place where a drop deletes after every layout of
 *   the flyout, and whenever the flyout moves or changes size (03 §3.3 "Placing blocks");
 * - the flyout tells the window's layout where it is and how wide it may be made
 *   (`app/layout/flyoutLayout.ts`), whenever that changes.
 */
import {
  ContinuousFlyout,
  ContinuousMetrics,
  ContinuousToolbox,
  RecyclableBlockFlyoutInflater,
} from '@blockly/continuous-toolbox';
import * as Blockly from 'blockly/core';

import { announceFlyoutLayout, CANVAS_MIN_WIDTH } from '../../app/layout/flyoutLayout';
import { timed } from '../../lib/timing';
import { keepFlyoutFocus } from '../keyboard/keepFocus';
import { prefersReducedMotion } from '../keyboard/motion';

/** How long the toolbox waits after the last change before it rebuilds the flyout. */
export const CONTINUOUS_REFRESH_DELAY_MS = 100;

/** The registry name of the continuous toolbox. */
export const CONTINUOUS_TOOLBOX = 'b2c_continuous_toolbox';
/** The registry name of the continuous flyout. */
export const CONTINUOUS_FLYOUT = 'b2c_continuous_flyout';
/** The registry name of the metrics manager that leaves room for the open flyout. */
export const CONTINUOUS_METRICS = 'b2c_continuous_metrics';

/** Whether a flyout item stands for a dynamic category (`{custom: key}`). */
function dynamicCategoryKey(item: Blockly.utils.toolbox.FlyoutItemInfo): string | null {
  const custom = (item as { custom?: unknown }).custom;
  return typeof custom === 'string' ? custom : null;
}

/**
 * The continuous toolbox, with four changes:
 *
 * - a refresh builds the dynamic categories itself and shows the flyout again only when what it
 *   would show changed (the analysis and the selection change much more often than the toolbox's
 *   blocks do, and rebuilding hundreds of flyout blocks on every edit is slow); each rebuild is
 *   the User Timing span `b2c:toolbox-refresh` (src/lib/timing.ts);
 * - a dynamic category that cannot be built is left out instead of failing the whole flyout;
 * - showing the flyout again disposes every item in it, so a keyboard focus that was on one goes
 *   to the same item in the new contents instead of being lost (the keyboard plugin's
 *   `keepFlyoutFocus`);
 * - the delayed refresh is cancelled when the toolbox is disposed.
 */
export class B2cContinuousToolbox extends ContinuousToolbox {
  private refreshTimer: ReturnType<typeof setTimeout> | null = null;
  /** The serialised flyout contents last shown, or `null` before the first showing. */
  private shownContents: string | null = null;

  /** Rebuilds the flyout a little after the last call (changes come in bursts). */
  override refreshSelection(): void {
    if (!this.getFlyout().isVisible()) {
      return;
    }
    if (this.refreshTimer !== null) {
      clearTimeout(this.refreshTimer);
    }
    this.refreshTimer = setTimeout(() => {
      this.refreshTimer = null;
      this.showAllCategories(false);
    }, CONTINUOUS_REFRESH_DELAY_MS);
  }

  /**
   * Shows every category in the flyout now. With `force` false, the flyout is left as it is when
   * its contents have not changed. Returns whether the flyout was shown again. The whole rebuild
   * (building the dynamic categories, comparing and showing the flyout) is the User Timing span
   * `b2c:toolbox-refresh`.
   */
  showAllCategories(force = true): boolean {
    return timed('b2c:toolbox-refresh', () => {
      const contents = this.flyoutContents();
      const serialised = JSON.stringify(contents);
      const flyout = this.getFlyout();
      if (!force && serialised === this.shownContents && flyout.isVisible()) {
        return false;
      }
      keepFlyoutFocus(flyout, () => {
        flyout.show(contents);
      });
      this.shownContents = serialised;
      return true;
    });
  }

  /** The items of every category, with the dynamic categories built for the current project. */
  flyoutContents(): Blockly.utils.toolbox.FlyoutItemInfoArray {
    const workspace = this.getWorkspace();
    return this.getToolboxItems().flatMap((item) =>
      this.convertToolboxItemToFlyoutItems(item).flatMap((entry) => {
        const key = dynamicCategoryKey(entry);
        if (key === null) {
          return [entry];
        }
        const build = workspace.getToolboxCategoryCallback(key);
        if (build === null) {
          return [];
        }
        try {
          return Blockly.utils.toolbox.convertFlyoutDefToJsonArray(build(workspace));
        } catch (error: unknown) {
          console.error(`The toolbox category ${key} could not be built`, error);
          return [];
        }
      }),
    );
  }

  /** Draws new categories; the next refresh then shows the flyout again whatever it held. */
  override render(toolboxDef: Blockly.utils.toolbox.ToolboxInfo): void {
    super.render(toolboxDef);
    this.shownContents = null;
  }

  override dispose(): void {
    if (this.refreshTimer !== null) {
      clearTimeout(this.refreshTimer);
      this.refreshTimer = null;
    }
    super.dispose();
  }
}

/** Each flyout's private recycler, which recycling is never turned on for. */
const RECYCLERS = new WeakMap<ContinuousFlyout, RecyclableBlockFlyoutInflater>();

/**
 * The share of the remaining distance the flyout scrolls each frame when a category is chosen
 * (the continuous toolbox's own value).
 */
const CATEGORY_SCROLL_FRACTION = 0.3;

/**
 * How far past a category's heading (in workspace units) the flyout scrolls when the category is
 * chosen, so that the plugin selects that category again when the scroll ends (see
 * {@link B2cContinuousFlyout.scrollTo}).
 */
export const CATEGORY_SCROLL_SLACK = 0.5;

/**
 * The scale the flyout draws its blocks at, whatever the canvas zoom (04 §4.2, decision D9 of the
 * M2 polish plan): a block of the toolbox is three quarters of its size on the canvas at 100 %.
 */
export const FLYOUT_SCALE = 0.75;

/**
 * The flyout's width, in CSS pixels (04 §4.2, D9). By default it is as wide as its widest item,
 * but at most `preferred` (a wider block is cut off at the flyout's edge and can still be dragged
 * out). The user can make it anything from `min` to `max` with the *Resize the toolbox* splitter.
 * Either way, when the canvas would have less than `canvas` beside it, the flyout narrows, down to
 * `min`.
 */
export const FLYOUT_WIDTH = Object.freeze({
  preferred: 320,
  min: 160,
  max: 1200,
  canvas: CANVAS_MIN_WIDTH,
});

/**
 * The widest the flyout may be when it and the canvas share `room` pixels (the workspace right of
 * the category column): what leaves the canvas {@link FLYOUT_WIDTH}.canvas, within
 * {@link FLYOUT_WIDTH}.min–max.
 */
export function flyoutWidthLimit(room: number): number {
  return Math.max(FLYOUT_WIDTH.min, Math.min(FLYOUT_WIDTH.max, room - FLYOUT_WIDTH.canvas));
}

/**
 * The flyout's width for contents `natural` pixels wide (its widest item at {@link FLYOUT_SCALE},
 * with the margins and the scrollbar), when the flyout and the canvas share `room` pixels: the
 * width the user `chosen`, or by default the natural width up to {@link FLYOUT_WIDTH}.preferred,
 * and never wider than {@link flyoutWidthLimit}.
 */
export function flyoutWidth(natural: number, room: number, chosen: number | null = null): number {
  const limit = flyoutWidthLimit(room);
  if (chosen !== null) {
    return Math.min(Math.max(chosen, FLYOUT_WIDTH.min), limit);
  }
  return Math.max(0, Math.min(natural, FLYOUT_WIDTH.preferred, limit));
}

/**
 * The continuous flyout, without block recycling, without its scroll animation on request, landing
 * on a chosen category at every zoom level, with a fixed block scale and a width the user can
 * choose, recorded as a drop target wherever it is, and telling the window's layout where it is.
 */
export class B2cContinuousFlyout extends ContinuousFlyout {
  /** The width the flyout's contents need, in pixels, as of the last reflow. */
  private naturalWidth = 0;

  /** The width the user chose with the *Resize the toolbox* splitter, or `null` for the default. */
  private chosenWidth: number | null = null;

  /**
   * The width last applied, in pixels. Blockly's own `width_` cannot tell whether the width
   * changed: `Flyout.show` sets it to 0 before every reflow.
   */
  private appliedWidth = 0;

  /** The widest the flyout may be in the room it had when it was last fitted, in pixels. */
  private widthLimit: number = FLYOUT_WIDTH.min;

  /** Where the flyout was last placed: its x and y, width and height, in pixels. */
  private placement: readonly number[] = [];

  /** What the layout was last told ({@link announceLayout}), or `null` before the first time. */
  private announced: string | null = null;

  /** The flyout draws its blocks at {@link FLYOUT_SCALE}, whatever the canvas zoom. */
  override getFlyoutScale(): number {
    return FLYOUT_SCALE;
  }

  /**
   * Lays the flyout out for its contents: Blockly 12.5.1's `VerticalFlyout.reflowInternal_`
   * re-implemented rather than wrapped (ADR-0014), with five changes:
   *
   * - the scale is {@link getFlyoutScale} (`VerticalFlyout.layout_` set it to the canvas's);
   * - the width is {@link flyoutWidth} of the contents' natural width and of the width the user
   *   chose, not the natural width;
   * - the flyout is placed again and the canvas resized only when the width changes. Blockly's
   *   version compares with `width_`, which `Flyout.show` has just set to 0, so it did that work
   *   on every showing, that is on every rebuild of a dynamic category;
   * - the canvas records its drag targets after every layout, whatever the width. `Flyout.show`
   *   records them only while the flyout is not displayed (its `setVisible(true)` records before
   *   it displays the flyout again), when the flyout, and the toolbox that stands for it, end at
   *   the window's left edge. Blockly's version recorded again because it placed the flyout every
   *   time; without a record here, a drop on the flyout or the category column deleted nothing
   *   after a rebuild until the layout changed (03 §3.3 "Placing blocks");
   * - the window's layout hears of a new width ({@link announceLayout}).
   *
   * The editor's canvas is left to right and has scrollbars, so Blockly's right alignment for
   * right-to-left workspaces and its translation for a workspace without scrollbars are left out.
   */
  protected override reflowInternal_(): void {
    const workspace = this.getWorkspace();
    workspace.scale = this.getFlyoutScale();
    const widest = this.getContents().reduce(
      (max, item) => Math.max(max, item.getElement().getBoundingRectangle().getWidth()),
      0,
    );
    this.naturalWidth =
      (widest + this.MARGIN * 1.5 + this.tabWidth_) * workspace.scale +
      Blockly.Scrollbar.scrollbarThickness;
    if (this.fitWidth()) {
      this.place();
      this.targetWorkspace.resizeContents();
    }
    this.targetWorkspace.recordDragTargets();
    this.announceLayout();
  }

  /**
   * Places the flyout beside the toolbox. Blockly does this whenever the workspace is resized (the
   * window, or a dock beside the canvas), so the width is fitted to the room left first. A new
   * width resizes the canvas; a new width or place makes the canvas record its drag targets, so
   * that a drop counts where the flyout now is, and the window's layout hears of it.
   */
  override position(): void {
    const resized = this.fitWidth();
    const moved = this.place();
    if (resized) {
      this.targetWorkspace.resizeContents();
    }
    if (resized || moved) {
      this.targetWorkspace.recordDragTargets();
    }
    this.announceLayout();
  }

  /** Tells the window's layout that the flyout is gone, so it stops offering to resize it. */
  override dispose(): void {
    const toolboxFlyout = this.isToolboxFlyout();
    const target = this.targetWorkspace.getInjectionDiv();
    super.dispose();
    if (!toolboxFlyout) {
      return;
    }
    announceFlyoutLayout(target, {
      left: 0,
      width: 0,
      min: FLYOUT_WIDTH.min,
      max: FLYOUT_WIDTH.min,
      setWidth: () => undefined,
    });
  }

  /**
   * Sets the width the user chose (the *Resize the toolbox* splitter), kept within
   * {@link FLYOUT_WIDTH}.min–max, or with `null` goes back to the default. The flyout is fitted
   * to it at once, and again whenever the room beside the toolbox changes.
   */
  private readonly setChosenWidth = (width: number | null): void => {
    this.chosenWidth =
      width === null || !Number.isFinite(width)
        ? null
        : Math.round(Math.min(Math.max(width, FLYOUT_WIDTH.min), FLYOUT_WIDTH.max));
    this.position();
  };

  /**
   * Whether this is the toolbox's flyout. Blockly's trash can builds its own flyout from the same
   * registered class (`FLYOUTS_VERTICAL_TOOLBOX`); that one is never resized by the user, and the
   * window's layout must not hear of it.
   */
  private isToolboxFlyout(): boolean {
    return this.targetWorkspace.getToolbox()?.getFlyout() === this;
  }

  /**
   * Places the flyout (Blockly's `VerticalFlyout.position`); returns whether it moved or changed
   * size since it was last placed. Blockly leaves a flyout where it is while it or the canvas is
   * hidden.
   */
  private place(): boolean {
    super.position();
    if (!this.isVisible() || !this.targetWorkspace.isVisible()) {
      return false;
    }
    const placement = [this.getX(), this.getY(), this.getWidth(), this.getHeight()];
    const changed = placement.some((value, index) => value !== this.placement[index]);
    this.placement = placement;
    return changed;
  }

  /**
   * Sets the flyout's width for its contents, the width the user chose and the room the workspace
   * has for it and the canvas; returns whether that differs from the width last applied.
   */
  private fitWidth(): boolean {
    const metrics = this.targetWorkspace.getMetricsManager();
    const room = metrics.getSvgMetrics().width - metrics.getToolboxMetrics().width;
    this.widthLimit = flyoutWidthLimit(room);
    const width = flyoutWidth(this.naturalWidth, room, this.chosenWidth);
    this.width_ = width;
    if (width === this.appliedWidth) {
      return false;
    }
    this.appliedWidth = width;
    return true;
  }

  /**
   * Tells the window's layout (`app/layout/flyoutLayout.ts`) where the toolbox's flyout is, how
   * wide it is and how wide it may be made, when any of that changed since it was last told. Its
   * top and height count too: the layout measures where the canvas is when they change (a module
   * switcher shown above it).
   */
  private announceLayout(): void {
    if (!this.isToolboxFlyout()) {
      return;
    }
    const left = this.getX();
    const width = this.getWidth();
    const max = this.widthLimit;
    const key = [left, this.getY(), width, this.getHeight(), max].join(' ');
    if (key === this.announced) {
      return;
    }
    this.announced = key;
    announceFlyoutLayout(this.targetWorkspace.getInjectionDiv(), {
      left,
      width,
      min: FLYOUT_WIDTH.min,
      max,
      setWidth: this.setChosenWidth,
    });
  }

  /**
   * Scrolls to `position` (a category chosen in the toolbox): animated, or at once when the system
   * asks for reduced motion (04 §4.8; read at each scroll, so a change of the setting applies at
   * once). The animation runs frame by frame in script, which the style sheet's reduced-motion
   * rule cannot stop.
   *
   * The target is {@link CATEGORY_SCROLL_SLACK} units past the heading. When the scroll ends,
   * `@blockly/continuous-toolbox` 7.0.9 selects the last category whose heading position is at or
   * above `Math.round(scroll / scale)` (`ContinuousFlyout.selectCategoryByScrollPosition`). The
   * heading positions are fractional (the label's top less half a gap), so a heading at 312.4
   * scrolled to exactly gives 312, which is above it: the category before the chosen one was
   * selected, and the selection seemed to jump back. Half a unit more rounds to at least the
   * heading's position, and stays far below the next heading. It also keeps the target of the
   * first category above 0, which the plugin's `if (this.scrollTarget)` takes for "not
   * scrolling", so it would select categories on the way up. Both defects are still to be
   * reported upstream: the owner files the report drafted in `docs/plans/upstream/` and records
   * its link here; the slack goes once the plugin is fixed.
   */
  override scrollTo(position: number): void {
    this.scrollAnimationFraction = prefersReducedMotion() ? 1 : CATEGORY_SCROLL_FRACTION;
    super.scrollTo(position + CATEGORY_SCROLL_SLACK);
  }

  /**
   * A recycler of this flyout's own (the base class insists on one): it is never used to create
   * blocks, so nothing is ever recycled, and Blockly's block inflater stays the default for every
   * other flyout.
   */
  protected override getRecyclableInflater(): RecyclableBlockFlyoutInflater {
    // Called from the base constructor, before this class's own fields exist.
    let recycler = RECYCLERS.get(this);
    if (recycler === undefined) {
      recycler = new RecyclableBlockFlyoutInflater();
      RECYCLERS.set(this, recycler);
    }
    recycler.recyclingEnabled = false;
    return recycler;
  }
}

/** Registers `cls` as `name` of `type`, replacing a different class of that name. */
export function registerClass<T>(
  type: Blockly.registry.Type<T>,
  name: string,
  cls: new (...args: never[]) => T,
): void {
  if (Blockly.registry.getClass(type, name, false) !== cls) {
    Blockly.registry.register(type, name, cls, true);
  }
}

/** Registers the continuous toolbox's classes under our names (idempotent). */
export function registerContinuousToolbox(): void {
  registerClass(Blockly.registry.Type.TOOLBOX, CONTINUOUS_TOOLBOX, B2cContinuousToolbox);
  registerClass(
    Blockly.registry.Type.FLYOUTS_VERTICAL_TOOLBOX,
    CONTINUOUS_FLYOUT,
    B2cContinuousFlyout,
  );
  registerClass(Blockly.registry.Type.METRICS_MANAGER, CONTINUOUS_METRICS, ContinuousMetrics);
}
