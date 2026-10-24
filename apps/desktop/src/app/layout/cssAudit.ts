/**
 * The app's style sheets as data, for the accessibility checks that happy-dom cannot make by
 * rendering (it computes no layout and applies no style sheets): the colour tokens of each theme
 * for contrast (WCAG 2.2 AA 1.4.3, 1.4.11), the sizes rules give controls (2.5.8), and the
 * reduced-motion rules (§4.8). Only the tests use this module.
 *
 * The parser is small and made for the app's own CSS: rules, comments, and the `@media` blocks it
 * uses. It is not a general CSS parser.
 */
import { contrastRatio } from '@blocks2cpp/blockly-ext';

/** One style rule: its selectors and declarations, and the `@media` condition around it. */
export interface CssRule {
  readonly selectors: readonly string[];
  readonly declarations: ReadonlyMap<string, string>;
  /** The `@media` condition the rule is in (`prefers-color-scheme: dark`), or `null`. */
  readonly media: string | null;
}

/** A theme: the light one, or the dark one (`prefers-color-scheme: dark`). */
export type Theme = 'light' | 'dark';

/** Removes comments. */
function stripComments(css: string): string {
  return css.replace(/\/\*[\s\S]*?\*\//g, '');
}

/** Parses `name: value; …` into a map (later declarations win, `!important` is dropped). */
function parseDeclarations(body: string): Map<string, string> {
  const declarations = new Map<string, string>();
  for (const part of body.split(';')) {
    const colon = part.indexOf(':');
    if (colon < 0) {
      continue;
    }
    const name = part.slice(0, colon).trim();
    const value = part
      .slice(colon + 1)
      .replace(/!important/g, '')
      .trim();
    if (name !== '' && value !== '') {
      declarations.set(name, value);
    }
  }
  return declarations;
}

/** The rules of a style sheet, with nested `@media` blocks flattened. */
export function parseRules(css: string): CssRule[] {
  const text = stripComments(css);
  const rules: CssRule[] = [];
  const media: (string | null)[] = [null];
  let position = 0;
  let prelude = '';
  while (position < text.length) {
    const character = text[position] ?? '';
    if (character === '{') {
      const head = prelude.trim();
      prelude = '';
      if (head.startsWith('@media')) {
        media.push(head.slice('@media'.length).trim());
        position += 1;
        continue;
      }
      const end = text.indexOf('}', position);
      if (end < 0) {
        break;
      }
      const body = text.slice(position + 1, end);
      if (!head.startsWith('@')) {
        rules.push({
          selectors: head
            .split(',')
            .map((selector) => selector.trim().replace(/\s+/g, ' '))
            .filter((selector) => selector !== ''),
          declarations: parseDeclarations(body),
          media: media.at(-1) ?? null,
        });
      }
      position = end + 1;
      continue;
    }
    if (character === '}') {
      if (media.length > 1) {
        media.pop();
      }
      prelude = '';
      position += 1;
      continue;
    }
    prelude += character;
    position += 1;
  }
  return rules;
}

/** The custom properties (`--name`) set on `:root`, per theme (dark falls back to light). */
export function themeTokens(rules: readonly CssRule[]): Record<Theme, Map<string, string>> {
  const light = new Map<string, string>();
  const dark = new Map<string, string>();
  for (const rule of rules) {
    if (!rule.selectors.includes(':root')) {
      continue;
    }
    const isDark = rule.media?.includes('prefers-color-scheme: dark') === true;
    if (rule.media !== null && !isDark) {
      continue;
    }
    for (const [name, value] of rule.declarations) {
      if (!name.startsWith('--')) {
        continue;
      }
      if (isDark) {
        dark.set(name, value);
      } else {
        light.set(name, value);
      }
    }
  }
  return { light, dark: new Map([...light, ...dark]) };
}

/** A token's value with `var(--x, fallback)` resolved, or `null` when it cannot be. */
export function resolveValue(
  tokens: ReadonlyMap<string, string>,
  value: string,
  depth = 0,
): string | null {
  if (depth > 16) {
    return null;
  }
  const match = /^var\(\s*(--[\w-]+)\s*(?:,\s*(.+))?\)$/.exec(value.trim());
  if (match === null) {
    return value.trim();
  }
  const [, name = '', fallback] = match;
  const found = tokens.get(name);
  if (found !== undefined) {
    return resolveValue(tokens, found, depth + 1);
  }
  return fallback === undefined ? null : resolveValue(tokens, fallback, depth + 1);
}

/** An sRGB colour with alpha (0–1). */
interface Rgba {
  readonly r: number;
  readonly g: number;
  readonly b: number;
  readonly a: number;
}

/** Parses `#rgb`, `#rrggbb` and `rgb(r g b / a%)`; `null` for anything else. */
export function parseColour(value: string): Rgba | null {
  const text = value.trim().toLowerCase();
  const short = /^#([0-9a-f])([0-9a-f])([0-9a-f])$/.exec(text);
  if (short !== null) {
    const [, r = '0', g = '0', b = '0'] = short;
    return {
      r: Number.parseInt(r + r, 16),
      g: Number.parseInt(g + g, 16),
      b: Number.parseInt(b + b, 16),
      a: 1,
    };
  }
  const long = /^#([0-9a-f]{2})([0-9a-f]{2})([0-9a-f]{2})$/.exec(text);
  if (long !== null) {
    const [, r = '0', g = '0', b = '0'] = long;
    return {
      r: Number.parseInt(r, 16),
      g: Number.parseInt(g, 16),
      b: Number.parseInt(b, 16),
      a: 1,
    };
  }
  const rgb = /^rgba?\(\s*(\d+)[\s,]+(\d+)[\s,]+(\d+)\s*(?:[/,]\s*([\d.]+)(%?))?\s*\)$/.exec(text);
  if (rgb !== null) {
    const [, r = '0', g = '0', b = '0', alpha, percent] = rgb;
    const a = alpha === undefined ? 1 : Number(alpha) / (percent === '%' ? 100 : 1);
    return { r: Number(r), g: Number(g), b: Number(b), a };
  }
  return null;
}

function hex(colour: Rgba): string {
  const part = (value: number) =>
    Math.round(Math.min(255, Math.max(0, value)))
      .toString(16)
      .padStart(2, '0');
  return `#${part(colour.r)}${part(colour.g)}${part(colour.b)}`;
}

/** `colour` painted over the opaque `background` (alpha compositing), as `#rrggbb`. */
export function composite(colour: Rgba, background: Rgba): string {
  const a = colour.a;
  return hex({
    r: colour.r * a + background.r * (1 - a),
    g: colour.g * a + background.g * (1 - a),
    b: colour.b * a + background.b * (1 - a),
    a: 1,
  });
}

/**
 * The WCAG 2.2 contrast ratio of a foreground colour (possibly translucent) on an opaque
 * background, both given as CSS values (tokens resolved). Throws for a value it cannot read, so a
 * renamed token fails the check instead of passing it.
 */
export function contrastOf(
  tokens: ReadonlyMap<string, string>,
  foreground: string,
  background: string,
): number {
  const fg = resolveValue(tokens, foreground);
  const bg = resolveValue(tokens, background);
  const fgColour = fg === null ? null : parseColour(fg);
  const bgColour = bg === null ? null : parseColour(bg);
  if (fgColour === null || bgColour?.a !== 1) {
    throw new Error(`Cannot read the colours ${foreground} on ${background}`);
  }
  return contrastRatio(composite(fgColour, bgColour), hex(bgColour));
}

/** The declarations of every rule whose selector list contains exactly `selector`. */
export function declarationsOf(rules: readonly CssRule[], selector: string): Map<string, string> {
  const merged = new Map<string, string>();
  for (const rule of rules) {
    if (rule.media === null && rule.selectors.includes(selector)) {
      for (const [name, value] of rule.declarations) {
        merged.set(name, value);
      }
    }
  }
  return merged;
}

/** The root font size (the browser's 16 px; the app sets 14 px on the body only). */
const ROOT_FONT_PX = 16;

/** The body's font size (app.css), which `em` lengths in the chrome use. */
const BODY_FONT_PX = 14;

/** A length in CSS pixels (`px`, `rem`, or `em` at the body's font size), or `null`. */
export function pixels(value: string | undefined, fontSize = BODY_FONT_PX): number | null {
  if (value === undefined) {
    return null;
  }
  const match = /^(-?[\d.]+)(px|rem|em)$/.exec(value.trim());
  if (match === null) {
    return null;
  }
  const [, amount = '0', unit] = match;
  const number = Number(amount);
  switch (unit) {
    case 'px':
      return number;
    case 'rem':
      return number * ROOT_FONT_PX;
    default:
      return number * fontSize;
  }
}
