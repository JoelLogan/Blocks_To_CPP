/**
 * The toolbox's category rows (docs/spec/04-user-interface.md §4.2, 03 §3.7, 04 §4.8): a round
 * bubble in the category's colour holding its icon, with the category name below it, so colour is
 * never the only cue. The colour comes from the Blockly theme's category style (light or dark), and
 * follows a theme change.
 *
 * The icon and the name are set as DOM text (`textContent`), never as HTML.
 */
import { TOOLBOX } from '@blocks2cpp/blockly-ext';
import * as Blockly from 'blockly/core';

import { categoryOfDefinition } from './contents';

/** The class of every category row container of ours. */
export const CATEGORY_CLASS = 'b2cToolboxCategory';
/** The class of the icon bubble. */
export const ICON_CLASS = 'b2cToolboxIcon';

/** A toolbox category that shows its catalog icon in a coloured bubble. */
export class B2cToolboxCategory extends Blockly.ToolboxCategory {
  protected override createContainer_(): HTMLDivElement {
    const container = super.createContainer_();
    container.classList.add(CATEGORY_CLASS);
    return container;
  }

  protected override createIconDom_(): Element {
    const icon = document.createElement('span');
    icon.classList.add(ICON_CLASS);
    // The name next to it says the same; screen readers read the name only.
    icon.setAttribute('aria-hidden', 'true');
    const id = categoryOfDefinition(this.toolboxItemDef_);
    icon.textContent = TOOLBOX.find((category) => category.id === id)?.icon ?? '';
    return icon;
  }

  /** Colours the bubble instead of drawing Blockly's coloured left border. */
  protected override addColourBorder_(colour: string): void {
    if (this.iconDom_ instanceof HTMLElement) {
      this.iconDom_.style.backgroundColor = colour;
    }
  }
}
