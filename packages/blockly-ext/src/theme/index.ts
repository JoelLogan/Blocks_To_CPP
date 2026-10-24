/** Block colours, category icons and the Blockly themes (docs/spec/03-block-language.md §3.7). */
export { CATEGORY_IDS, CATEGORY_STYLE, NEUTRAL_COLOUR, type CategoryStyle } from './categories';
export { contrastRatio, isHexColour, mix, relativeLuminance } from './colour';
export {
  CANVAS_COLOUR,
  EXPRESSION_BLOCK_STYLE,
  PLACEHOLDER_BLOCK_STYLE,
  b2cDarkTheme,
  b2cLightTheme,
  b2cTheme,
  blockStyleFor,
  blockStyleFromColour,
  categoryBlockStyle,
  categoryHatStyle,
  toolboxCategoryStyle,
  type ThemeMode,
} from './themes';
