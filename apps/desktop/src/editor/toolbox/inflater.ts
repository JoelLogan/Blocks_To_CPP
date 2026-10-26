/**
 * Creates the toolbox's blocks in a flyout (flyout item kind `b2c_block`, see presets.ts).
 *
 * It is Blockly's block inflater with these differences:
 *
 * - each declaration the item names gets a new symbol ID, so no two blocks created from one item
 *   declare the same symbol;
 * - a call block from *Your functions* gets its argument labels (`n:`) right away. On the canvas the
 *   call mutator labels arguments from the analysis, which does not know flyout blocks;
 * - a block is kept from one showing of the flyout to the next when the next showing has an equal
 *   item, so showing the flyout again builds and draws only the blocks whose items changed
 *   (drawing a block is most of the cost of a showing). Everything a block shows comes from its
 *   item, so a kept block is the block its item would build. These are built again instead:
 *   a block that declares something ({@link declaresSymbols}: each showing declares new symbols),
 *   and a block the user changed in the flyout (a number typed in, a variable chosen), which is told by comparing
 *   its serialised state with the state it was built with when the flyout is shown again;
 * - each block remembers the item it was built from, so a block dragged out of it can be settled
 *   where it lands (./landing.ts);
 * - Blockly's listener that disables flyout blocks when the canvas reaches a block limit is added
 *   only when the canvas has limits (the app sets none).
 *
 * Its items have their own type, so the flyout gives them back to this inflater to dispose of: a
 * block that may be kept is parked, without its pointer listeners and its hover highlight, until
 * the flyout lays out its new contents ({@link B2cBlockInflater.sweep}, called from
 * `B2cContinuousFlyout.layout_`); a parked block no item claimed by then is disposed of.
 */
import { B2cSymbolDeclField } from '@blocks2cpp/blockly-ext';
import * as Blockly from 'blockly/core';

import { rememberToolboxItem } from './landing';
import { B2C_BLOCK_KIND, blockState, type B2cBlockInfo } from './presets';

/** The field name of the flyout-only label before argument `index`. */
export function argLabelField(index: number): string {
  return `B2C_TOOLBOX_LABEL_ARG${String(index)}`;
}

/** Whether `state` is one of our block items. */
function isB2cBlockInfo(state: object): state is B2cBlockInfo {
  return (state as { kind?: unknown }).kind === B2C_BLOCK_KIND;
}

/**
 * Blockly's block item for one of ours: the same block, with a new symbol ID for each declaration
 * and without our own keys.
 */
export function plainBlockInfo(info: B2cBlockInfo): Blockly.utils.toolbox.BlockInfo {
  return { kind: 'block', ...blockState(info) };
}

/** Shows `labels[i]` before the call argument `ARG{i}` (empty labels are skipped). */
export function applyArgLabels(block: Blockly.Block, labels: readonly string[]): void {
  labels.forEach((text, index) => {
    const input = block.getInput(`ARG${String(index)}`);
    if (text === '' || input === null || block.getField(argLabelField(index)) !== null) {
      return;
    }
    input.appendField(new Blockly.FieldLabel(text), argLabelField(index));
  });
}

/**
 * Whether `block` or a block in it declares a symbol: an item's declaration (`declares`), or one a
 * block makes itself when it is built (the catch variable of *try*). Such a block is never kept:
 * every showing gives its declarations new symbol IDs, as building it does.
 */
export function declaresSymbols(block: Blockly.Block): boolean {
  return block
    .getDescendants(false)
    .some((part) =>
      part.inputList.some((input) =>
        input.fieldRow.some(
          (field) => field instanceof B2cSymbolDeclField && field.getSymbolId() !== null,
        ),
      ),
    );
}

/**
 * The state a block of the flyout is built with, without IDs, to tell whether the user changed it
 * there; `null` when it cannot be read (the block is then never kept).
 */
function fingerprint(block: Blockly.BlockSvg): string | null {
  try {
    return JSON.stringify(
      Blockly.serialization.blocks.save(block, { addCoordinates: false, saveIds: false }),
    );
  } catch {
    return null;
  }
}

/** How many flyout blocks an inflater built, kept from a showing before and disposed of. */
export interface FlyoutBlockStats {
  readonly created: number;
  readonly reused: number;
  readonly disposed: number;
}

/** The flyout inflater of the toolbox's blocks. */
export class B2cBlockInflater extends Blockly.BlockFlyoutInflater {
  /**
   * Whether blocks are kept between showings (see the module comment). Off, every showing builds
   * every block again, as Blockly does; for comparisons only.
   */
  reuse = true;
  /** Blocks parked by the last disposal, by the key of their item, until the next layout. */
  private readonly parked = new Map<string, Blockly.BlockSvg[]>();
  /** The key of the item each block that may be kept was built from. */
  private readonly keys = new WeakMap<Blockly.BlockSvg, string>();
  /** The state each block that may be kept was built with ({@link fingerprint}). */
  private readonly fingerprints = new WeakMap<Blockly.BlockSvg, string>();
  /** Whether Blockly's capacity listener is on the flyout's canvas ({@link setFlyout}). */
  private capacityListened = false;
  /** The key of the item `load` is loading, for `createBlock`, or `null`. */
  private pendingKey: string | null = null;
  private created = 0;
  private reused = 0;
  private disposed = 0;

  override load(state: object, flyout: Blockly.IFlyout): Blockly.FlyoutItem {
    const info = isB2cBlockInfo(state) ? state : null;
    const key =
      this.reuse && info !== null && info.declares === undefined ? JSON.stringify(info) : null;
    this.pendingKey = key;
    let block: Blockly.IBoundedElement & Blockly.IFocusableNode;
    try {
      block = super.load(info === null ? state : plainBlockInfo(info), flyout).getElement();
    } finally {
      this.pendingKey = null;
    }
    if (info !== null && block instanceof Blockly.BlockSvg) {
      rememberToolboxItem(block, info);
      if (info.argLabels !== undefined) {
        applyArgLabels(block, info.argLabels);
      }
      // A kept block has its key and state from when it was built.
      if (key !== null && !this.keys.has(block) && !declaresSymbols(block)) {
        const built = fingerprint(block);
        if (built !== null) {
          this.keys.set(block, key);
          this.fingerprints.set(block, built);
        }
      }
    }
    return new Blockly.FlyoutItem(block, B2C_BLOCK_KIND);
  }

  /** A parked block for the item being loaded, when there is one, else a new block. */
  override createBlock(
    blockDefinition: Blockly.utils.toolbox.BlockInfo,
    workspace: Blockly.WorkspaceSvg,
  ): Blockly.BlockSvg {
    const key = this.pendingKey;
    const blocks = key === null ? undefined : this.parked.get(key);
    while (blocks !== undefined && blocks.length > 0) {
      const block = blocks.pop();
      if (block !== undefined && !block.isDeadOrDying() && block.workspace === workspace) {
        this.reused += 1;
        return block;
      }
    }
    this.created += 1;
    return super.createBlock(blockDefinition, workspace);
  }

  /**
   * Parks a block that may be kept and that the user did not change in the flyout (it keeps its
   * place until the next layout); disposes of any other. A disabled block is never kept: Blockly's
   * `load` would take it for one disabled by its item, and never enable it again by the canvas's
   * block limits.
   */
  override disposeItem(item: Blockly.FlyoutItem): void {
    const block = item.getElement();
    if (
      block instanceof Blockly.BlockSvg &&
      this.reuse &&
      !block.isDeadOrDying() &&
      block.isEnabled()
    ) {
      const key = this.keys.get(block);
      const built = this.fingerprints.get(block);
      if (key !== undefined && built !== undefined && fingerprint(block) === built) {
        this.removeListeners(block.id);
        block.removeSelect();
        const blocks = this.parked.get(key);
        if (blocks === undefined) {
          this.parked.set(key, [block]);
        } else {
          blocks.push(block);
        }
        return;
      }
    }
    this.disposed += 1;
    super.disposeItem(item);
  }

  /** Disposes of the parked blocks no item of the new contents claimed. */
  sweep(): void {
    for (const blocks of this.parked.values()) {
      for (const block of blocks) {
        if (!block.isDeadOrDying()) {
          this.disposed += 1;
          super.disposeItem(new Blockly.FlyoutItem(block, B2C_BLOCK_KIND));
        }
      }
    }
    this.parked.clear();
  }

  /**
   * Remembers the flyout (Blockly's), without Blockly's capacity listener while the canvas has no
   * block limits: on every block created or deleted on the canvas, that listener walks every
   * block of the flyout to enable or disable it by the canvas's remaining capacity. The app sets
   * no block limits; Blockly's listener is kept if limits are ever set, also once they are set
   * after the flyout's first showing.
   */
  protected override setFlyout(flyout: Blockly.IFlyout): void {
    if (flyout.targetWorkspace?.hasBlockLimits() !== true) {
      this.flyout = flyout;
      return;
    }
    if (this.capacityListened && this.flyout === flyout) {
      return;
    }
    if (this.flyout === flyout) {
      // Blockly adds its listener only for a flyout it has not seen.
      delete this.flyout;
    }
    super.setFlyout(flyout);
    this.capacityListened = true;
  }

  /** How many blocks this inflater built, kept and disposed of so far. */
  stats(): FlyoutBlockStats {
    return { created: this.created, reused: this.reused, disposed: this.disposed };
  }

  override getType(): string {
    return B2C_BLOCK_KIND;
  }
}
