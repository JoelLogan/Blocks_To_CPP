/**
 * The Blockly themes: M0's `b2c-light` and `b2c-dark` workspace colours (apps/desktop/src/app/app.css
 * `--b2c-canvas*`), extended with a block style per category and per category's hat blocks, a
 * category style for the toolbox, and the styles of the internal blocks. Both build on Blockly's
 * Zelos theme (docs/adr/0002-block-editor-blockly.md).
 */
import * as Blockly from 'blockly/core';

import type { CategoryId, Shape } from '../generated/catalog';
import { CATEGORY_IDS, CATEGORY_STYLE, NEUTRAL_COLOUR } from './categories';
import { mix } from './colour';

/** Which theme a colour is for. */
export type ThemeMode = 'light' | 'dark';

/** Workspace colours per theme; they match the `--b2c-canvas` tokens of the app. */
export const CANVAS_COLOUR: Readonly<Record<ThemeMode, string>> = Object.freeze({
  light: '#f7f8fb',
  dark: '#191d26',
});

/** The block style for blocks of a category. */
export function categoryBlockStyle(category: CategoryId): string {
  return `b2c_${category}`;
}

/** The block style for a category's hat and definition blocks (a rounded "cap" top). */
export function categoryHatStyle(category: CategoryId): string {
  return `b2c_${category}_hat`;
}

/** The toolbox category style of a category (`categorystyle` in a toolbox definition). */
export function toolboxCategoryStyle(category: CategoryId): string {
  return `b2c_${category}`;
}

/** The block style of the internal expression shadows. */
export const EXPRESSION_BLOCK_STYLE = 'b2c_expression';

/** The block style of "missing pack" placeholders. */
export const PLACEHOLDER_BLOCK_STYLE = 'b2c_placeholder';

/** The block style a catalog block uses, from its category and shape. */
export function blockStyleFor(category: CategoryId, shape: Shape): string {
  return shape === 'hat' || shape === 'definition'
    ? categoryHatStyle(category)
    : categoryBlockStyle(category);
}

/**
 * A full block style from a primary colour. Zelos fills shadow blocks with the secondary colour and
 * draws borders with the tertiary one; both are darker, so white text on them has at least the
 * primary colour's contrast.
 */
export function blockStyleFromColour(
  primary: string,
  hat: '' | 'cap' = '',
): Blockly.Theme.BlockStyle {
  return {
    colourPrimary: primary,
    colourSecondary: mix(primary, '#000000', 0.15),
    colourTertiary: mix(primary, '#000000', 0.3),
    hat,
  };
}

function blockStyles(mode: ThemeMode): Record<string, Blockly.Theme.BlockStyle> {
  const styles: Record<string, Blockly.Theme.BlockStyle> = {};
  for (const category of CATEGORY_IDS) {
    const colour = CATEGORY_STYLE[category].colour[mode];
    styles[categoryBlockStyle(category)] = blockStyleFromColour(colour);
    styles[categoryHatStyle(category)] = blockStyleFromColour(colour, 'cap');
  }
  styles[EXPRESSION_BLOCK_STYLE] = blockStyleFromColour(NEUTRAL_COLOUR.expression[mode]);
  styles[PLACEHOLDER_BLOCK_STYLE] = blockStyleFromColour(NEUTRAL_COLOUR.placeholder[mode]);
  return styles;
}

function categoryStyles(mode: ThemeMode): Record<string, Blockly.Theme.CategoryStyle> {
  const styles: Record<string, Blockly.Theme.CategoryStyle> = {};
  for (const category of CATEGORY_IDS) {
    styles[toolboxCategoryStyle(category)] = { colour: CATEGORY_STYLE[category].colour[mode] };
  }
  return styles;
}

/** The light theme (`b2c-light`). */
export const b2cLightTheme: Blockly.Theme = Blockly.Theme.defineTheme('b2c-light', {
  name: 'b2c-light',
  base: Blockly.Themes.Zelos,
  blockStyles: blockStyles('light'),
  categoryStyles: categoryStyles('light'),
  componentStyles: {
    workspaceBackgroundColour: CANVAS_COLOUR.light,
    scrollbarColour: '#c5cbd8',
  },
});

/** The dark theme (`b2c-dark`). */
export const b2cDarkTheme: Blockly.Theme = Blockly.Theme.defineTheme('b2c-dark', {
  name: 'b2c-dark',
  base: Blockly.Themes.Zelos,
  blockStyles: blockStyles('dark'),
  categoryStyles: categoryStyles('dark'),
  componentStyles: {
    workspaceBackgroundColour: CANVAS_COLOUR.dark,
    scrollbarColour: '#4a5263',
    flyoutBackgroundColour: '#232836',
    flyoutForegroundColour: '#e6e9f0',
    toolboxBackgroundColour: '#1b1f29',
    toolboxForegroundColour: '#e6e9f0',
  },
});

/** The theme for a colour scheme. */
export function b2cTheme(mode: ThemeMode): Blockly.Theme {
  return mode === 'dark' ? b2cDarkTheme : b2cLightTheme;
}
