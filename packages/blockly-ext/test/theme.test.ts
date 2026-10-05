/** Category colours, icons and the themes (src/theme/). */
import * as Blockly from 'blockly/core';
import { describe, expect, it } from 'vitest';

import {
  BLOCK_DEFS,
  CANVAS_COLOUR,
  CATEGORY_IDS,
  CATEGORY_STYLE,
  EXPRESSION_BLOCK_STYLE,
  NEUTRAL_COLOUR,
  PLACEHOLDER_BLOCK_STYLE,
  TOOLBOX,
  b2cDarkTheme,
  b2cLightTheme,
  b2cTheme,
  blockStyleFor,
  contrastRatio,
  isHexColour,
  mix,
  toolboxCategoryStyle,
  type ThemeMode,
} from '../src';

const MODES: readonly ThemeMode[] = ['light', 'dark'];

/** Every colour a block can have, per theme. */
function blockColours(mode: ThemeMode): [string, string][] {
  return [
    ...CATEGORY_IDS.map((id): [string, string] => [id, CATEGORY_STYLE[id].colour[mode]]),
    ['expression', NEUTRAL_COLOUR.expression[mode]],
    ['placeholder', NEUTRAL_COLOUR.placeholder[mode]],
  ];
}

describe('category styles', () => {
  it('cover every toolbox category, in toolbox order, with the toolbox icons', () => {
    expect(CATEGORY_IDS).toEqual(TOOLBOX.map((category) => category.id));
    for (const category of TOOLBOX) {
      expect(CATEGORY_STYLE[category.id].icon).toBe(category.icon);
    }
    expect(CATEGORY_IDS.map((id) => CATEGORY_STYLE[id].icon)).toEqual([
      '▶',
      '𝑥',
      '∑',
      '◇',
      '“ ”',
      '⑂',
      '↻',
      '⌨',
      'ƒ',
    ]);
  });

  it.each(MODES)('give white block text at least 4.5:1 contrast in the %s theme', (mode) => {
    for (const [name, colour] of blockColours(mode)) {
      expect(isHexColour(colour), name).toBe(true);
      expect(contrastRatio(colour, '#ffffff'), `${name} ${colour}`).toBeGreaterThanOrEqual(4.5);
    }
  });

  it.each(MODES)('stand out from the %s canvas by at least 3:1', (mode) => {
    for (const [name, colour] of blockColours(mode)) {
      expect(
        contrastRatio(colour, CANVAS_COLOUR[mode]),
        `${name} ${colour}`,
      ).toBeGreaterThanOrEqual(3);
    }
  });

  it.each(MODES)('are nine different colours in the %s theme', (mode) => {
    expect(new Set(CATEGORY_IDS.map((id) => CATEGORY_STYLE[id].colour[mode])).size).toBe(9);
  });
});

describe('themes', () => {
  it('keep the M0 names and workspace colours', () => {
    expect(b2cLightTheme.name).toBe('b2c-light');
    expect(b2cDarkTheme.name).toBe('b2c-dark');
    expect(b2cLightTheme.getComponentStyle('workspaceBackgroundColour')).toBe('#f7f8fb');
    expect(b2cDarkTheme.getComponentStyle('workspaceBackgroundColour')).toBe('#191d26');
    expect(b2cTheme('light')).toBe(b2cLightTheme);
    expect(b2cTheme('dark')).toBe(b2cDarkTheme);
  });

  it.each([
    ['light', b2cLightTheme],
    ['dark', b2cDarkTheme],
  ] as const)(
    'define a block style for every catalog block and the internal blocks (%s)',
    (mode, theme) => {
      for (const def of BLOCK_DEFS) {
        const style = theme.blockStyles[blockStyleFor(def.category, def.shape)];
        expect(style, def.id).toBeDefined();
        expect(style?.colourPrimary).toBe(CATEGORY_STYLE[def.category].colour[mode]);
        expect(style?.hat).toBe(def.shape === 'hat' || def.shape === 'definition' ? 'cap' : '');
      }
      expect(theme.blockStyles[EXPRESSION_BLOCK_STYLE]?.colourPrimary).toBe(
        NEUTRAL_COLOUR.expression[mode],
      );
      expect(theme.blockStyles[PLACEHOLDER_BLOCK_STYLE]?.colourPrimary).toBe(
        NEUTRAL_COLOUR.placeholder[mode],
      );
      for (const id of CATEGORY_IDS) {
        expect(theme.categoryStyles[toolboxCategoryStyle(id)]?.colour).toBe(
          CATEGORY_STYLE[id].colour[mode],
        );
      }
    },
  );

  it('use darker secondary and tertiary colours, so shadow text keeps its contrast', () => {
    const ours = Object.entries(b2cLightTheme.blockStyles).filter(([name]) =>
      name.startsWith('b2c_'),
    );
    expect(ours.length).toBe(2 * CATEGORY_IDS.length + 2);
    for (const [, style] of ours) {
      expect(contrastRatio(style.colourSecondary, '#ffffff')).toBeGreaterThanOrEqual(
        contrastRatio(style.colourPrimary, '#ffffff'),
      );
      expect(contrastRatio(style.colourTertiary, '#ffffff')).toBeGreaterThanOrEqual(
        contrastRatio(style.colourSecondary, '#ffffff'),
      );
    }
  });

  it('are registered with Blockly under their names', () => {
    expect(Blockly.registry.getObject(Blockly.registry.Type.THEME, 'b2c-light')).toBe(
      b2cLightTheme,
    );
    expect(Blockly.registry.getObject(Blockly.registry.Type.THEME, 'b2c-dark')).toBe(b2cDarkTheme);
  });
});

describe('colour helpers', () => {
  it('mix and measure colours', () => {
    expect(mix('#000000', '#ffffff', 0.5)).toBe('#808080');
    expect(mix('#123456', '#000000', 0)).toBe('#123456');
    expect(contrastRatio('#000000', '#ffffff')).toBeCloseTo(21, 5);
    expect(contrastRatio('#777777', '#777777')).toBe(1);
    expect(() => mix('red', '#000000', 0.5)).toThrow(RangeError);
  });
});
