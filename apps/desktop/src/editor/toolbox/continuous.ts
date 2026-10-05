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
 *   plugin's recycler to replace Blockly's block inflater for every flyout.
 */
import {
  ContinuousFlyout,
  ContinuousMetrics,
  ContinuousToolbox,
  RecyclableBlockFlyoutInflater,
} from '@blockly/continuous-toolbox';
import * as Blockly from 'blockly/core';

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
 * The continuous toolbox, with three changes:
 *
 * - a refresh builds the dynamic categories itself and shows the flyout again only when what it
 *   would show changed (the analysis and the selection change much more often than the toolbox's
 *   blocks do, and rebuilding hundreds of flyout blocks on every edit is slow);
 * - a dynamic category that cannot be built is left out instead of failing the whole flyout;
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
   * its contents have not changed. Returns whether the flyout was shown again.
   */
  showAllCategories(force = true): boolean {
    const contents = this.flyoutContents();
    const serialised = JSON.stringify(contents);
    if (!force && serialised === this.shownContents && this.getFlyout().isVisible()) {
      return false;
    }
    this.getFlyout().show(contents);
    this.shownContents = serialised;
    return true;
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

/** The continuous flyout, without block recycling. */
export class B2cContinuousFlyout extends ContinuousFlyout {
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
