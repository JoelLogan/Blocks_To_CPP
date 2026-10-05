/**
 * Registration of the toolbox's Blockly classes, and the injection options that choose them.
 *
 * Everything is registered under names of ours, so Blockly's defaults stay as they are for any
 * other workspace or flyout. Registering is idempotent.
 */
import * as Blockly from 'blockly/core';

import { B2cToolboxCategory } from './category';
import { B2C_CATEGORY_KIND, initialToolboxDefinition } from './contents';
import {
  CONTINUOUS_FLYOUT,
  CONTINUOUS_METRICS,
  CONTINUOUS_TOOLBOX,
  registerClass,
  registerContinuousToolbox,
} from './continuous';
import { B2cBlockInflater } from './inflater';
import { B2C_BLOCK_KIND } from './presets';

/**
 * Registers the category row (`b2c_category`), the flyout inflater of the toolbox's blocks
 * (`b2c_block`) and the continuous toolbox's classes.
 */
export function registerToolboxComponents(): void {
  registerClass(Blockly.registry.Type.TOOLBOX_ITEM, B2C_CATEGORY_KIND, B2cToolboxCategory);
  registerClass(Blockly.registry.Type.FLYOUT_INFLATER, B2C_BLOCK_KIND, B2cBlockInflater);
  registerContinuousToolbox();
}

/** The injection options {@link toolboxInjectOptions} gives. */
export interface ToolboxInjectOptions {
  /** The starting toolbox; the toolbox plugin replaces it when it is attached. */
  readonly toolbox: Blockly.utils.toolbox.ToolboxInfo;
  /** The continuous toolbox, its flyout and its metrics manager. */
  readonly plugins: Readonly<Record<string, string>>;
}

/**
 * What `Blockly.inject` needs for the Blocks2Cpp toolbox: the starting toolbox and the continuous
 * toolbox's classes. Spread it into the injection options:
 *
 * ```ts
 * const toolbox = toolboxInjectOptions();
 * Blockly.inject(host, { ...options, toolbox: toolbox.toolbox,
 *   plugins: { ...options.plugins, ...toolbox.plugins } });
 * ```
 *
 * It registers the classes first. A workspace injected with any other category toolbox still gets
 * the full toolbox from the plugin, in Blockly's category style (one category at a time).
 */
export function toolboxInjectOptions(): ToolboxInjectOptions {
  registerToolboxComponents();
  return {
    toolbox: initialToolboxDefinition(),
    plugins: {
      toolbox: CONTINUOUS_TOOLBOX,
      flyoutsVerticalToolbox: CONTINUOUS_FLYOUT,
      metricsManager: CONTINUOUS_METRICS,
    },
  };
}
