/**
 * Category colours and icons (docs/spec/03-block-language.md §3.7). Every category has an icon as
 * well as a colour, so colour is never the only cue. The toolbox metadata (catalog/toolbox.toml)
 * names a colour token per category; the values live here.
 *
 * The colours are chosen so that, in both themes, white block text has a contrast of at least
 * 4.5:1 (WCAG 2.2 AA for normal text) and blocks stand out from the canvas by at least 3:1 (non-text
 * contrast). The nine categories of milestone M2 have hues spread evenly (OKLCH hue 10° to 330° in
 * 40° steps) at one lightness per theme (0.50 light, 0.55 dark); the four that milestone M3's wave
 * W2 adds (docs/plans/m3-contracts/w2.md §2.1) sit about halfway between them (Errors 30°, Types
 * 122°, Files 180°, Collections 270°), so M2's colours stay as they were. Errors and Collections
 * have the same lightness and the largest chroma up to 0.2 that sRGB holds. Types and Files sit
 * where sRGB holds little chroma, between two M2 hues 40° apart: at that lightness they would be
 * barely distinguishable from their neighbours (about 3 apart in OKLab), so they are darker and
 * less saturated (OKLCH 0.47 and 0.465 light, 0.53 and 0.515 dark). Every two categories are at
 * least 6 apart in OKLab (×100) in both themes, M2's own closest pair (Logic and Control) being
 * 6.3. test/theme.test.ts checks the contrasts, that the colours differ, and that distance.
 * Checking them under simulated colour-blindness is planned with the colour-blind-safe palette
 * (milestone M5, docs/spec/04-user-interface.md).
 */
import type { CategoryId } from '../generated/catalog';

/** A category's colours (light and dark theme) and its icon. */
export interface CategoryStyle {
  readonly colour: { readonly light: string; readonly dark: string };
  /** A short symbol shown next to the category name and on its blocks' help. */
  readonly icon: string;
}

/** The style of every toolbox category, in toolbox order. */
export const CATEGORY_STYLE: Readonly<Record<CategoryId, CategoryStyle>> = Object.freeze({
  program: { colour: { light: '#7a6001', dark: '#8b6e01' }, icon: '▶' },
  variables: { colour: { light: '#9c4700', dark: '#b15204' }, icon: 'x' },
  math: { colour: { light: '#497101', dark: '#548100' }, icon: '∑' },
  logic: { colour: { light: '#02707e', dark: '#068090' }, icon: '◇' },
  text: { colour: { light: '#9d2398', dark: '#ad36a7' }, icon: '“ ”' },
  control: { colour: { light: '#04755b', dark: '#048568' }, icon: '⋔' },
  loops: { colour: { light: '#0465af', dark: '#0574c7' }, icon: '↻' },
  io: { colour: { light: '#6741ca', dark: '#7552db' }, icon: '⌨' },
  functions: { colour: { light: '#b80049', dark: '#ca2356' }, icon: 'ƒ' },
  collections: { colour: { light: '#3b51d3', dark: '#4761e4' }, icon: '☰' },
  types: { colour: { light: '#53622c', dark: '#637435' }, icon: '◆' },
  files: { colour: { light: '#3c625a', dark: '#427269' }, icon: '▤' },
  errors: { colour: { light: '#ba0d01', dark: '#cc2a1b' }, icon: '⚠' },
});

/** The categories in toolbox order (docs/spec/03-block-language.md §3.7). */
export const CATEGORY_IDS: readonly CategoryId[] = Object.freeze([
  'program',
  'variables',
  'math',
  'logic',
  'text',
  'control',
  'loops',
  'io',
  'functions',
  'collections',
  'types',
  'files',
  'errors',
]);

/**
 * Colours of the blocks that belong to no category: the internal expression shadows and the
 * placeholders for blocks from a missing library pack.
 */
export const NEUTRAL_COLOUR = Object.freeze({
  /** Expression slots shown as shadows (docs/spec/03-block-language.md §3.4, M2). */
  expression: { light: '#5a6274', dark: '#646d80' },
  /** "Missing pack" placeholders: grey. */
  placeholder: { light: '#696969', dark: '#737373' },
});
