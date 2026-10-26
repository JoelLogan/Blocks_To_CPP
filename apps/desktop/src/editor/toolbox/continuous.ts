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
 * - the toolbox builds its dynamic categories only when what they depend on changed, shows the
 *   flyout again only when its contents changed, never while a block is dragged, a field editor
 *   or a menu is open or the flyout scrolls to a category, and then keeps the category in view
 *   and the selected category (04 §4.2); its delayed refresh is cancelled when the toolbox is
 *   disposed;
 * - the flyout keeps each toolbox block and label whose item is unchanged from one showing to the
 *   next (./inflater.ts) and only moves it where the new contents put it, so showing it again
 *   builds and draws only what changed; the plugin's own recycler (one block per block type,
 *   for blocks without mutators or variables) is never used, and stays empty instead of needing to
 *   replace Blockly's block inflater for every flyout; and it shows its contents with Blockly's
 *   events off (no one listens for the flyout's own block events), without hiding a displayed
 *   flyout around the showing;
 * - showing the flyout again keeps the keyboard focus on the same item when it was in the flyout
 *   (04 §4.8), and the animated scroll to a chosen category jumps at once when the system asks for
 *   reduced motion;
 * - the scroll to a chosen category goes half a unit past its heading, so that the category stays
 *   selected at every zoom level (04 §4.2);
 * - the flyout draws its blocks at one scale whatever the canvas zoom, and is as wide as the user
 *   made it with the *Resize the toolbox* splitter, by default at most
 *   {@link FLYOUT_WIDTH}.preferred wide; it narrows when the canvas would otherwise have less than
 *   {@link FLYOUT_WIDTH}.canvas beside it, and a wider block is cut off at its edge (04 §4.2);
 * - after every layout of the flyout, and whenever the flyout moves or changes size, the canvas's
 *   record of where the flyout is as a place where a drop deletes is marked stale, and recorded
 *   again in the next animation frame or at the start of a drag that comes first (03 §3.3
 *   "Placing blocks", `sync/dragTargets.ts`);
 * - the flyout tells the window's layout where it is and how wide it may be made
 *   (`app/layout/flyoutLayout.ts`), whenever that changes;
 * - the flyout tells the toolbox which canvas block it creates from which of its blocks, so a
 *   block taken from the toolbox is settled where it is dropped (./landing.ts).
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
import { markDragTargetsStale } from '../sync/dragTargets';
import { B2cBlockInflater, B2cLabelInflater, LABEL_KIND } from './inflater';
import { noteCreatedFromToolbox } from './landing';
import { B2C_BLOCK_KIND } from './presets';

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
 * The continuous toolbox, with these changes:
 *
 * - a refresh builds the dynamic categories itself, and only when what they depend on changed
 *   ({@link B2cContinuousToolbox.setContentsKey}: the module's variables and functions, not the
 *   selection or every edit), and shows the flyout again only when what it would show changed
 *   (rebuilding hundreds of flyout blocks is slow); each rebuild is the User Timing span
 *   `b2c:toolbox-refresh` (src/lib/timing.ts);
 * - a refresh waits while a block is dragged, while a field editor or a menu is open, and while
 *   the flyout scrolls to a chosen category (04 §4.2), and then shows the flyout again where it
 *   was: the category at the top of the view stays there, as far into it as before, and the
 *   selected category stays selected;
 * - drawing new categories (a new toolbox definition) keeps the selected category selected, in its
 *   new row, so that the first category is selected at startup whatever the timing;
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
  /** What the dynamic categories depend on ({@link setContentsKey}), or `null`. */
  private contentsKey: (() => string) | null = null;
  /** The key the flyout's contents were last built for, or `null` (build at the next refresh). */
  private builtKey: string | null = null;
  /**
   * The flyout's scroll offset (pixels) right after it was last shown again, while a category was
   * selected; `null` otherwise. Until the flyout scrolls elsewhere, a selection by the scroll
   * position is not the user's: the selected category stays selected.
   */
  private keptAt: number | null = null;

  /**
   * Sets what the dynamic categories' contents depend on, as a key (the toolbox plugin's): a
   * refresh then builds them only when the key differs from the one they were last built for.
   * Blockly asks the toolbox to refresh after every change of a block, most of which change
   * nothing it shows. With `null`, every refresh builds them and compares what it would show.
   */
  setContentsKey(key: (() => string) | null): void {
    this.contentsKey = key;
    this.builtKey = null;
  }

  /** The current contents key, or `null` without one (or when it cannot be computed). */
  private currentKey(): string | null {
    if (this.contentsKey === null) {
      return null;
    }
    try {
      return this.contentsKey();
    } catch (error: unknown) {
      console.error('The toolbox could not tell whether its contents changed', error);
      return null;
    }
  }

  /**
   * Rebuilds the flyout a little after the last call (changes come in bursts), unless its contents
   * key has not changed since it was last built.
   */
  override refreshSelection(): void {
    if (!this.getFlyout().isVisible()) {
      return;
    }
    if (this.builtKey !== null && this.currentKey() === this.builtKey) {
      return;
    }
    this.refreshLater();
  }

  /** Rebuilds the flyout after {@link CONTINUOUS_REFRESH_DELAY_MS}, once nothing holds it back. */
  private refreshLater(): void {
    if (this.refreshTimer !== null) {
      clearTimeout(this.refreshTimer);
    }
    this.refreshTimer = setTimeout(() => {
      this.refreshTimer = null;
      if (this.mustWait()) {
        this.refreshLater();
      } else {
        this.showAllCategories(false);
      }
    }, CONTINUOUS_REFRESH_DELAY_MS);
  }

  /**
   * Whether showing the flyout again must wait (04 §4.2): while a block is dragged (or the pointer
   * is pressed on one), while a field editor or a menu is open (on the canvas or in the flyout,
   * whose blocks a new showing would replace), and while the flyout scrolls to a chosen category.
   */
  private mustWait(): boolean {
    const flyout = this.getFlyout();
    return (
      Blockly.Gesture.inProgress() ||
      this.getWorkspace().isDragging() ||
      Blockly.WidgetDiv.isVisible() ||
      Blockly.DropDownDiv.isVisible() ||
      (flyout instanceof B2cContinuousFlyout && flyout.isScrollingToCategory())
    );
  }

  /**
   * Shows every category in the flyout now. With `force` false, the flyout is left as it is when
   * its contents have not changed. Returns whether the flyout was shown again. The flyout keeps
   * the category at the top of its view where it was ({@link B2cContinuousFlyout.scrollAnchor}),
   * and the selected category stays selected. The whole rebuild (building the dynamic
   * categories, comparing and showing the flyout) is the User Timing span `b2c:toolbox-refresh`.
   */
  showAllCategories(force = true): boolean {
    return timed('b2c:toolbox-refresh', () => {
      const key = this.currentKey();
      const contents = this.flyoutContents();
      const serialised = JSON.stringify(contents);
      const flyout = this.getFlyout();
      this.builtKey = key;
      if (!force && serialised === this.shownContents && flyout.isVisible()) {
        return false;
      }
      const anchor =
        flyout instanceof B2cContinuousFlyout && this.shownContents !== null
          ? flyout.scrollAnchor()
          : null;
      keepFlyoutFocus(flyout, () => {
        flyout.show(contents);
      });
      if (anchor !== null && flyout instanceof B2cContinuousFlyout) {
        flyout.scrollToAnchor(anchor);
      }
      this.keptAt = this.getSelectedItem() === null ? null : -flyout.getWorkspace().scrollY;
      this.shownContents = serialised;
      return true;
    });
  }

  /**
   * Selects the category `name` without scrolling (what the plugin does when the flyout's scroll
   * stops at a category), except right after the flyout was shown again: that scroll is not the
   * user's, so the selected category stays selected (04 §4.2) until the flyout scrolls elsewhere.
   */
  override selectCategoryByName(name: string): void {
    if (this.keptAt !== null) {
      if (Math.abs(-this.getFlyout().getWorkspace().scrollY - this.keptAt) < 1) {
        return;
      }
      this.keptAt = null;
    }
    super.selectCategoryByName(name);
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

  /**
   * Draws new categories; the next refresh then shows the flyout again whatever it held. The
   * category that was selected stays selected, in its new row (04 §4.2); when drawing them moves
   * the flyout, its scroll event then selects the category at the top, as a scroll by hand does.
   * Blockly 12.5.1's `Toolbox.render` disposes the old rows but keeps the old selected row as the
   * selection: the new rows were all drawn unselected, the flyout's next showing selected none by
   * its scroll position since the toolbox still counted one as selected, and whether the first
   * category came out selected at startup depended on whether the flyout's scroll event came
   * before that showing.
   */
  override render(toolboxDef: Blockly.utils.toolbox.ToolboxInfo): void {
    const selected = this.getSelectedItem();
    const name = selected instanceof Blockly.ToolboxCategory ? selected.getName() : null;
    super.render(toolboxDef);
    this.shownContents = null;
    this.builtKey = null;
    this.keptAt = null;
    this.selectedItem_ = null;
    const again = name === null ? null : this.getCategoryByName(name);
    if (again !== null) {
      this.selectItem_(null, again);
    }
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

/** The longest a scroll to a chosen category is taken to run (04 §4.2 refresh rules), in ms. */
export const MAX_CATEGORY_SCROLL_MS = 1_000;

/**
 * How long a scroll to a chosen category may stand still before it is taken to have ended, in ms:
 * a few frames, in each of which a running scroll moves.
 */
export const CATEGORY_SCROLL_STILL_MS = 50;

/** A scroll to a chosen category, as {@link B2cContinuousFlyout} follows it (pixels, ms). */
interface CategoryScroll {
  /** Where it ends. */
  readonly target: number;
  /** When it started (`performance.now()`). */
  readonly since: number;
  /** Where the flyout was when it was last looked at. */
  seen: number;
  /** When that was. */
  seenAt: number;
}

/**
 * Where the flyout is scrolled to: the category whose heading is at or above the top of the view,
 * and how far below its heading the view starts, in workspace units.
 */
export interface FlyoutScrollAnchor {
  readonly category: string;
  readonly offset: number;
}

/**
 * The continuous flyout, keeping its toolbox blocks across showings (./inflater.ts) instead of the
 * plugin's block recycling, without its scroll animation on request, landing on a chosen category
 * at every zoom level, keeping the category in view when its contents are shown again, with a
 * fixed block scale and a width the user can choose, recorded as a drop target wherever it is
 * (without laying the page out for it), telling the window's layout where it is, and telling the
 * toolbox which blocks it creates on the canvas (./landing.ts).
 */
export class B2cContinuousFlyout extends ContinuousFlyout {
  /**
   * The scroll to the category chosen last, or `null`: the toolbox shows no new contents while it
   * runs.
   */
  private categoryScroll: CategoryScroll | null = null;
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

  /** Whether a showing is under way that began while the flyout was displayed ({@link show}). */
  private keepShown = false;

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
   * - the canvas's record of its drag targets is marked stale after every layout, whatever the
   *   width (`sync/dragTargets.ts`): it is recorded again in the next animation frame, or at the
   *   start of a drag that comes first. `Flyout.show` records them only when the flyout was not
   *   displayed before (its `setVisible(true)` records before it displays the flyout again, see
   *   {@link setVisible}), when the flyout, and the toolbox that stands for it, end at the window's
   *   left edge. Blockly's version
   *   recorded again because it placed the flyout every time; without a later record, a drop on
   *   the flyout or the category column deleted nothing after a rebuild until the layout changed
   *   (03 §3.3 "Placing blocks"). A record here, at once, would read the flyout's place on screen
   *   and so make the browser lay the page out, the flyout's new blocks and the canvas included,
   *   in every rebuild: after nearly every edit, since each new preview may rebuild the flyout;
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
    markDragTargetsStale(this.targetWorkspace);
    this.announceLayout();
  }

  /**
   * Places the flyout beside the toolbox. Blockly does this whenever the workspace is resized (the
   * window, or a dock beside the canvas), so the width is fitted to the room left first. A new
   * width resizes the canvas; a new width or place marks the canvas's record of its drag targets
   * stale (`sync/dragTargets.ts`), so that from the next animation frame on, or from the start of
   * a drag that comes first, a drop counts where the flyout now is; and the window's layout hears
   * of it.
   */
  override position(): void {
    const resized = this.fitWidth();
    const moved = this.place();
    if (resized) {
      this.targetWorkspace.resizeContents();
    }
    if (resized || moved) {
      markDragTargetsStale(this.targetWorkspace);
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
    // Where the plugin's animation ends: the target, or the lowest point the flyout scrolls to.
    const workspace = this.getWorkspace();
    const metrics = workspace.getMetrics();
    const target = Math.min(
      (position + CATEGORY_SCROLL_SLACK) * workspace.scale,
      metrics.scrollHeight - metrics.viewHeight,
    );
    const since = performance.now();
    super.scrollTo(position + CATEGORY_SCROLL_SLACK);
    // The plugin takes its first step at once, and a short scroll ends with it.
    const seen = -workspace.scrollY;
    this.categoryScroll =
      Math.abs(target - seen) < 1 ? null : { target, since, seen, seenAt: since };
  }

  /**
   * Whether the flyout is still scrolling to the category chosen last: it has not reached its
   * target yet, has moved in the last {@link CATEGORY_SCROLL_STILL_MS} (a scroll that stands still
   * has ended, or cannot reach its target), and started at most {@link MAX_CATEGORY_SCROLL_MS}
   * ago (a scroll that never ends must not hold the toolbox back for ever).
   */
  isScrollingToCategory(): boolean {
    const scroll = this.categoryScroll;
    if (scroll === null) {
      return false;
    }
    const now = performance.now();
    const current = -this.getWorkspace().scrollY;
    const still = Math.abs(current - scroll.seen) < 1;
    if (
      Math.abs(scroll.target - current) < 1 ||
      now - scroll.since > MAX_CATEGORY_SCROLL_MS ||
      (still && now - scroll.seenAt >= CATEGORY_SCROLL_STILL_MS)
    ) {
      this.categoryScroll = null;
      return false;
    }
    if (!still) {
      scroll.seen = current;
      scroll.seenAt = now;
    }
    return true;
  }

  /**
   * Where the view starts (04 §4.2): the last category whose heading is at or above the top of the
   * view, and how far below its heading the view starts. `null` at the very top, or before the
   * flyout has shown its categories: there is nothing to keep then.
   */
  scrollAnchor(): FlyoutScrollAnchor | null {
    const toolbox = this.targetWorkspace.getToolbox();
    const workspace = this.getWorkspace();
    const scroll = -workspace.scrollY / workspace.scale;
    if (!(toolbox instanceof Blockly.Toolbox) || this.getContents().length === 0 || scroll < 1) {
      return null;
    }
    let anchor: { category: string; position: number } | null = null;
    for (const item of toolbox.getToolboxItems()) {
      if (!(item instanceof Blockly.ToolboxCategory)) {
        continue;
      }
      const category = item.getName();
      const position = this.getCategoryScrollPosition(category);
      if (position !== null && position <= scroll && (anchor?.position ?? -1) <= position) {
        anchor = { category, position };
      }
    }
    return anchor === null ? null : { category: anchor.category, offset: scroll - anchor.position };
  }

  /**
   * Scrolls at once to where `anchor` says the view started, in contents shown since: the heading
   * of its category, as far below it as before (or as far down as the flyout scrolls).
   */
  scrollToAnchor(anchor: FlyoutScrollAnchor): void {
    const position = this.getCategoryScrollPosition(anchor.category);
    if (position === null) {
      return;
    }
    const workspace = this.getWorkspace();
    const metrics = workspace.getMetrics();
    const target = Math.max(
      0,
      Math.min(
        (position + anchor.offset) * workspace.scale,
        metrics.scrollHeight - metrics.viewHeight,
      ),
    );
    workspace.scrollbar?.setY(target);
  }

  /**
   * Shows `flyoutDef` (the plugin's showing) without hiding the flyout first when it is displayed
   * ({@link setVisible}), and with Blockly's events off: nothing listens for the flyout's own
   * block creations, deletions and moves, which cost Blockly two serialisations and an event for
   * each block it builds or disposes of, and a move event for each block it places. Also left
   * out: the viewport events of the flyout's resizing within the showing and, at the first
   * showing only, the plugin's selection of the first category. The toolbox keeps the focus and
   * the scroll position after the showing (`B2cContinuousToolbox.showAllCategories`), with events
   * on.
   */
  override show(flyoutDef: Blockly.utils.toolbox.FlyoutDefinition | string): void {
    this.keepShown = this.isVisible();
    Blockly.Events.disable();
    try {
      super.show(flyoutDef);
    } finally {
      Blockly.Events.enable();
      this.keepShown = false;
    }
  }

  /**
   * Shows or hides the flyout (Blockly's), except that a flyout shown again while it is displayed
   * stays displayed: `Flyout.show` hides it and shows it again around building the new contents,
   * which set `display: none` and back on the flyout's SVG (the browser then lays out and draws
   * every block of the flyout again, kept ones included) and recorded the canvas's drag targets
   * twice. The layout marks the drag targets stale instead ({@link reflowInternal_}). `hide()`
   * still removes Blockly's reflow listener, and the showing adds it again.
   */
  override setVisible(visible: boolean): void {
    if (this.keepShown && !visible) {
      return;
    }
    super.setVisible(visible);
  }

  /**
   * Places the items of the flyout's new contents one below the other: Blockly 12.5.1's
   * `VerticalFlyout.layout_`, with three changes:
   *
   * - first, the toolbox blocks and labels parked by the last showing that no item claimed are
   *   disposed of (`B2cBlockInflater.sweep`, `B2cLabelInflater.sweep`), so the contents' bounds,
   *   measured next, count only what is shown;
   * - an item is placed where it goes, not moved by that much: a block kept from the last showing
   *   is already somewhere, and it is moved only when its place changed;
   * - the scale is left as it is: {@link reflowInternal_} sets the flyout's own scale next
   *   (Blockly's set the canvas's, only for it to be replaced).
   *
   * The inflaters are read from the flyout's own map, so the trash can's flyout, which has no
   * toolbox blocks, gets none.
   */
  protected override layout_(contents: Blockly.FlyoutItem[]): void {
    const blocks = this.inflaters.get(B2C_BLOCK_KIND);
    if (blocks instanceof B2cBlockInflater) {
      blocks.sweep();
    }
    const labels = this.inflaters.get(LABEL_KIND);
    if (labels instanceof B2cLabelInflater) {
      labels.sweep();
    }
    const x = this.RTL ? this.MARGIN : this.MARGIN + this.tabWidth_;
    let y = this.MARGIN;
    for (const item of contents) {
      const element = item.getElement();
      const at = placeOf(element);
      if (at.x !== x || at.y !== y) {
        element.moveBy(x - at.x, y - at.y);
      }
      y += element.getBoundingRectangle().getHeight();
    }
  }

  /**
   * The inflater of a kind of flyout item (Blockly's), with this flyout's own label inflater, which
   * keeps labels across showings (`B2cLabelInflater`), instead of the one registered for every
   * flyout.
   */
  protected override getInflaterForType(type: string): Blockly.IFlyoutInflater | null {
    if (type !== LABEL_KIND) {
      return super.getInflaterForType(type);
    }
    let inflater = this.inflaters.get(type);
    if (!(inflater instanceof B2cLabelInflater)) {
      inflater = new B2cLabelInflater();
      this.inflaters.set(type, inflater);
    }
    return inflater;
  }

  /**
   * Creates a block on the canvas from `original`, a block of this flyout (Blockly's), and tells
   * the toolbox, which settles a block taken from it where it is dropped (./landing.ts).
   */
  override createBlock(original: Blockly.BlockSvg): Blockly.BlockSvg {
    const block = super.createBlock(original);
    noteCreatedFromToolbox(original, block);
    return block;
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

/** Where a flyout item is now (its top-left corner, in flyout workspace units). */
function placeOf(element: Blockly.IBoundedElement): Blockly.utils.Coordinate {
  if (element instanceof Blockly.BlockSvg) {
    return element.getRelativeToSurfaceXY();
  }
  if (element instanceof Blockly.FlyoutButton) {
    return element.getPosition();
  }
  const bounds = element.getBoundingRectangle();
  return new Blockly.utils.Coordinate(bounds.left, bounds.top);
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
