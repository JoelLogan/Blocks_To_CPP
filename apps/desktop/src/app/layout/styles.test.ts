/**
 * Accessibility checks of the style sheets (docs/spec/04-user-interface.md §4.8, WCAG 2.2 AA),
 * for what happy-dom cannot render: colour contrast in both themes (1.4.3 text, 1.4.11 controls,
 * focus rings and state marks), a visible focus on every control (2.4.7), targets of at least
 * 24 px (2.5.8) and no motion when the system asks for reduced motion. The end-to-end tests and the
 * manual pass (docs/manual-tests/m2-accessibility.md) check the same in the real webviews.
 */
import { CANVAS_COLOUR, CATEGORY_STYLE, NEUTRAL_COLOUR } from '@blocks2cpp/blockly-ext';
import { describe, expect, it } from 'vitest';

import {
  contrastOf,
  type CssRule,
  declarationsOf,
  parseColour,
  parseRules,
  pixels,
  resolveValue,
  type Theme,
  themeTokens,
} from './cssAudit';

/** Every style sheet of the app, by path relative to src/. */
const SHEETS: Readonly<Record<string, string>> = Object.fromEntries(
  Object.entries(
    import.meta.glob<string>('/src/**/*.css', { query: '?raw', import: 'default', eager: true }),
  ).map(([path, text]) => [path.replace(/^\/src\//, ''), text]),
);

function sheet(path: string): string {
  const text = SHEETS[path];
  if (text === undefined) {
    throw new Error(`No style sheet ${path}`);
  }
  return text;
}

const ALL_RULES: readonly CssRule[] = Object.values(SHEETS).flatMap((text) => parseRules(text));
const TOKENS = themeTokens(ALL_RULES);
const THEMES: readonly Theme[] = ['light', 'dark'];

/** Text on its background: 4.5:1 (WCAG 1.4.3). */
const TEXT_PAIRS: readonly [string, string][] = [
  ['--b2c-text', '--b2c-bg'],
  ['--b2c-text', '--b2c-surface'],
  ['--b2c-text', '--b2c-surface-raised'],
  ['--b2c-text-muted', '--b2c-bg'],
  ['--b2c-text-muted', '--b2c-surface'],
  ['--b2c-text-muted', '--b2c-surface-raised'],
  ['--b2c-primary-text', '--b2c-primary'],
  ['--b2c-danger-text', '--b2c-danger'],
  ['--b2c-primary', '--b2c-surface-raised'],
  ['--b2c-ok', '--b2c-surface'],
  ['--b2c-ok', '--b2c-surface-raised'],
  ['--b2c-error', '--b2c-bg'],
  ['--b2c-error', '--b2c-surface'],
  ['--b2c-error', '--b2c-surface-raised'],
  ['--b2c-warning-text', '--b2c-warning-bg'],
  ['--b2c-toolbar-text', '--b2c-toolbar-bg'],
  ['--b2c-toolbar-text', '--b2c-toolbar-hover'],
  // Hints are drawn inverted.
  ['--b2c-bg', '--b2c-text'],
  ['--b2c-panel-error', '--b2c-bg'],
  ['--b2c-panel-error', '--b2c-surface'],
  ['--b2c-panel-warning', '--b2c-bg'],
  ['--b2c-panel-warning', '--b2c-surface'],
  ['--b2c-panel-info', '--b2c-bg'],
  ['--b2c-panel-info', '--b2c-surface'],
  ['--b2c-panel-ok', '--b2c-bg'],
  ['--b2c-panel-ok', '--b2c-surface'],
  ['--b2c-panel-warning-text', '--b2c-panel-warning-bg'],
  ['--b2c-code-text', '--b2c-code-bg'],
  ['--b2c-code-gutter-text', '--b2c-code-gutter-bg'],
  ['--b2c-code-keyword', '--b2c-code-bg'],
  ['--b2c-code-type', '--b2c-code-bg'],
  ['--b2c-code-string', '--b2c-code-bg'],
  ['--b2c-code-escape', '--b2c-code-bg'],
  ['--b2c-code-number', '--b2c-code-bg'],
  ['--b2c-code-comment', '--b2c-code-bg'],
  ['--b2c-code-preprocessor', '--b2c-code-bg'],
  ['--b2c-code-function', '--b2c-code-bg'],
  ['--b2c-code-hidden-char', '--b2c-code-bg'],
  ['--b2c-console-fg', '--b2c-console-bg'],
];

/** Focus rings, control edges and state marks on their background: 3:1 (WCAG 1.4.11). */
const UI_PAIRS: readonly [string, string][] = [
  ['--b2c-focus', '--b2c-bg'],
  ['--b2c-focus', '--b2c-surface'],
  ['--b2c-focus', '--b2c-surface-raised'],
  ['--b2c-focus', '--b2c-canvas'],
  ['--b2c-focus', '--b2c-warning-bg'],
  ['--b2c-panel-focus', '--b2c-code-bg'],
  ['--b2c-toolbar-focus', '--b2c-toolbar-bg'],
  ['--b2c-toolbar-focus', '--b2c-toolbar-hover'],
  ['--b2c-control-border', '--b2c-bg'],
  ['--b2c-control-border', '--b2c-surface'],
  ['--b2c-control-border', '--b2c-surface-raised'],
  ['--b2c-selected', '--b2c-bg'],
  ['--b2c-selected', '--b2c-surface'],
  ['--b2c-accent', '--b2c-toolbar-bg'],
  ['--b2c-console-cursor', '--b2c-console-bg'],
];

function token(name: string): string {
  return `var(${name})`;
}

describe('the style sheets', () => {
  it('are all found and read', () => {
    expect(Object.keys(SHEETS)).toEqual(
      expect.arrayContaining([
        'app/app.css',
        'panels/panels.css',
        'editor/keyboard/keyboard.css',
        'features/settings/page/page.css',
      ]),
    );
    for (const [path, text] of Object.entries(SHEETS)) {
      expect(parseRules(text).length, path).toBeGreaterThan(0);
    }
  });
});

describe('the themes', () => {
  it('are chosen by the Appearance setting, never by a media query of the system', () => {
    for (const [path, text] of Object.entries(SHEETS)) {
      expect(text, path).not.toMatch(/prefers-color-scheme/);
    }
  });

  it('give the webview the scheme of the theme for what it draws itself', () => {
    expect(TOKENS.light.get('--b2c-color-scheme')).toBe('light');
    expect(TOKENS.dark.get('--b2c-color-scheme')).toBe('dark');
    expect(declarationsOf(ALL_RULES, ':root').get('color-scheme')).toBe('var(--b2c-color-scheme)');
  });

  it('set every dark token over a light one', () => {
    for (const name of TOKENS.dark.keys()) {
      expect(TOKENS.light.has(name), name).toBe(true);
    }
  });
});

describe.each(THEMES)('colour contrast in the %s theme', (theme) => {
  const tokens = TOKENS[theme];

  it.each(TEXT_PAIRS)('%s on %s reads at 4.5:1', (foreground, background) => {
    expect(contrastOf(tokens, token(foreground), token(background))).toBeGreaterThanOrEqual(4.5);
  });

  it.each(UI_PAIRS)('%s on %s shows at 3:1', (foreground, background) => {
    expect(contrastOf(tokens, token(foreground), token(background))).toBeGreaterThanOrEqual(3);
  });

  it('shows the toolbar’s drop-down edge at 3:1', () => {
    const border = declarationsOf(ALL_RULES, '.toolbar-select').get('border') ?? '';
    const colour = /rgba?\([^)]*\)|#[0-9a-f]{3,6}/i.exec(border)?.[0];
    expect(colour).toBeDefined();
    for (const background of ['--b2c-toolbar-bg', '--b2c-toolbar-hover']) {
      expect(contrastOf(tokens, colour ?? '', token(background))).toBeGreaterThanOrEqual(3);
    }
  });

  it('reads the Debug/Release drop-down at 4.5:1, also under the pointer', () => {
    const select = declarationsOf(ALL_RULES, '.toolbar-select');
    const hover = declarationsOf(ALL_RULES, '.toolbar-select:hover');
    const text = select.get('color') ?? '';
    for (const background of [select.get('background-color'), hover.get('background-color')]) {
      expect(contrastOf(tokens, text, background ?? '')).toBeGreaterThanOrEqual(4.5);
    }
  });

  it('draws the keyboard ring on the canvas and on every block colour', () => {
    const ring = resolveValue(tokens, token('--b2c-kb-ring')) ?? '';
    const edge = resolveValue(tokens, token('--b2c-kb-ring-edge')) ?? '';
    // The ring is two-tone: one of its colours stands out on any background, and they stand out
    // from each other.
    expect(contrastOf(tokens, ring, edge)).toBeGreaterThanOrEqual(3);
    const backgrounds = [
      CANVAS_COLOUR[theme],
      ...Object.values(CATEGORY_STYLE).map((style) => style.colour[theme]),
      NEUTRAL_COLOUR.expression[theme],
    ];
    for (const background of backgrounds) {
      const best = Math.max(
        contrastOf(tokens, ring, background),
        contrastOf(tokens, edge, background),
      );
      expect(best, `on ${background}`).toBeGreaterThanOrEqual(3);
    }
    // On a block, the yellow alone is enough.
    for (const style of Object.values(CATEGORY_STYLE)) {
      expect(contrastOf(tokens, ring, style.colour[theme])).toBeGreaterThanOrEqual(3);
    }
  });
});

describe('the focus', () => {
  it('is drawn on every control', () => {
    const focus = declarationsOf(ALL_RULES, ':focus-visible');
    expect(focus.get('outline')).toBe('2px solid var(--b2c-focus)');
  });

  it('is only hidden where something else shows it', () => {
    // Each place that turns the outline off, and what shows the focus there instead.
    const replaced: Readonly<Record<string, string>> = {
      '.main-menu-list:focus-visible': 'the focused menu item has its own ring',
      '.feature-page-title:focus': 'the :focus-visible rule draws the ring for keyboard users',
      '.start-title:focus': 'the :focus-visible rule draws the ring for keyboard users',
      ":root:not([data-input='keyboard']) h1[tabindex='-1']:focus-visible":
        'a heading focused from code shows its ring once the keyboard is in use',
      ":root:not([data-input='keyboard']) h2[tabindex='-1']:focus-visible":
        'a heading focused from code shows its ring once the keyboard is in use',
    };
    const hiding = ALL_RULES.filter((rule) => {
      const outline = rule.declarations.get('outline');
      return (
        (outline === 'none' || outline === '0') &&
        rule.selectors.some((selector) => selector.includes(':focus'))
      );
    }).flatMap((rule) => rule.selectors.filter((selector) => selector.includes(':focus')));
    for (const selector of hiding) {
      expect(replaced[selector], selector).toBeDefined();
    }
    expect(declarationsOf(ALL_RULES, '.feature-page-title:focus-visible').get('outline')).toMatch(
      /^2px solid /,
    );
    expect(declarationsOf(ALL_RULES, '.start-title:focus-visible').get('outline')).toMatch(
      /^2px solid /,
    );
  });

  it('is drawn on a heading focused from code only once the keyboard is in use', () => {
    const hidden = ALL_RULES.filter((rule) =>
      rule.selectors.some((selector) => selector.includes('[data-input=')),
    );
    expect(hidden.flatMap((rule) => rule.selectors)).toEqual([
      ":root:not([data-input='keyboard']) h1[tabindex='-1']:focus-visible",
      ":root:not([data-input='keyboard']) h2[tabindex='-1']:focus-visible",
    ]);
    expect(hidden[0]?.declarations.get('outline')).toBe('none');
    // The pages' headings take the focus from code: these are the headings the rule covers.
    expect(declarationsOf(ALL_RULES, '.start-title:focus-visible').get('outline')).toMatch(
      /^2px solid /,
    );
  });

  it('is drawn on the canvas by the keyboard plugin', () => {
    const keyboard = parseRules(sheet('editor/keyboard/keyboard.css'));
    const ring = keyboard.find((rule) =>
      rule.selectors.includes('.blocklyKeyboardNavigation .blocklyActiveFocus.blocklyPath'),
    );
    expect(ring?.declarations.get('stroke')).toBe('var(--b2c-kb-ring)');
    expect(pixels(ring?.declarations.get('stroke-width'))).toBeGreaterThanOrEqual(2);
  });
});

/** The z-index each rule sets, by selector. */
function zIndexes(): { selector: string; value: number }[] {
  return ALL_RULES.flatMap((rule) => {
    const value = rule.declarations.get('z-index');
    return value === undefined
      ? []
      : rule.selectors.map((selector) => ({ selector, value: Number(value) }));
  });
}

/** The stacking scale documented at the top of app.css. */
function stackingScale(): string {
  const text = sheet('app/app.css');
  const start = text.indexOf('Stacking (z-index)');
  expect(start, 'the stacking scale in app.css').toBeGreaterThan(0);
  return text.slice(start, text.indexOf('*/', start));
}

describe('the stacking (04 §4.1: menus, hints and dialogs above the canvas)', () => {
  it('keeps Blockly’s layers, such as its toolbox at 70, inside the canvas', () => {
    expect(declarationsOf(ALL_RULES, '.workspace').get('isolation')).toBe('isolate');
    expect(declarationsOf(ALL_RULES, '.workspace').get('position')).toBe('relative');
  });

  it('draws the main menu and hints above the canvas, and dialogs above them', () => {
    const z = (selector: string) => Number(declarationsOf(ALL_RULES, selector).get('z-index'));
    expect(z('.main-menu-list')).toBeGreaterThan(0);
    expect(z('.hint')).toBeGreaterThan(0);
    expect(z('.dialog-overlay')).toBeGreaterThan(Math.max(z('.main-menu-list'), z('.hint')));
    expect(z('.dialog')).toBeGreaterThan(z('.dialog-overlay'));
  });

  it('uses only z-indexes the scale in app.css documents', () => {
    const scale = stackingScale();
    const found = zIndexes();
    expect(found.length).toBeGreaterThan(0);
    for (const { selector, value } of found) {
      expect(Number.isInteger(value), selector).toBe(true);
      expect(scale, `${selector} z-index ${String(value)}`).toMatch(
        new RegExp(`(^|\\s)${String(value)}(\\s|$)`, 'm'),
      );
    }
  });
});

describe('the toolbar', () => {
  const select = declarationsOf(ALL_RULES, '.toolbar-select');

  it('draws the Debug/Release drop-down itself, in the toolbar’s colours', () => {
    expect(select.get('appearance')).toBe('none');
    expect(select.get('background-color')).toBe('var(--b2c-toolbar-bg)');
    expect(select.get('color')).toBe('var(--b2c-toolbar-text)');
    // What the webview still draws itself in the toolbar is drawn dark, like the toolbar.
    expect(declarationsOf(ALL_RULES, '.toolbar').get('color-scheme')).toBe('dark');
  });

  it('gives the drop-down a chevron in its text colour, kept under the pointer', () => {
    const chevron = select.get('background-image') ?? '';
    expect(chevron).toContain('linear-gradient');
    expect(chevron).toContain('currentColor');
    expect(select.get('background-repeat')).toBe('no-repeat');
    // Room for the chevron after the text.
    expect(pixels(select.get('padding-right'))).toBeGreaterThanOrEqual(24);
    // The `background` shorthand would remove the chevron under the pointer.
    const hover = declarationsOf(ALL_RULES, '.toolbar-select:hover');
    expect(hover.has('background')).toBe(false);
    expect(hover.get('background-color')).toBe('var(--b2c-toolbar-hover)');
  });

  it('keeps the drop-down as tall as the buttons, so the toolbar keeps its height', () => {
    const button = declarationsOf(ALL_RULES, '.toolbar-button');
    for (const property of ['min-height', 'padding', 'border', 'font']) {
      expect(select.get(property), property).toBe(button.get(property));
    }
    for (const property of ['height', 'max-height', 'padding-top', 'padding-bottom']) {
      expect(select.has(property), property).toBe(false);
    }
  });

  it('cuts off only the project’s name, so the unsaved-changes marker stays in view', () => {
    const text = declarationsOf(ALL_RULES, '.project-name-text');
    expect(text.get('text-overflow')).toBe('ellipsis');
    expect(text.get('overflow')).toBe('hidden');
    expect(text.get('white-space')).toBe('nowrap');
    expect(text.get('min-width')).toBe('0');
    const heading = declarationsOf(ALL_RULES, '.project-name');
    expect(heading.get('display')).toBe('flex');
    expect(heading.get('min-width')).toBe('0');
    expect(heading.has('text-overflow')).toBe(false);
    expect(heading.has('overflow')).toBe(false);
    expect(declarationsOf(ALL_RULES, '.dirty-marker').get('flex')).toBe('none');
  });
});

describe('the status bar', () => {
  it('draws no edge around the toolchain link', () => {
    expect(declarationsOf(ALL_RULES, '.status-link').get('border')).toBe('0');
  });
});

describe('the target sizes', () => {
  /** Controls and the smallest height (and width, for icon buttons) their rules give them. */
  const CONTROLS: readonly [string, 'min-height' | 'min-width'][] = [
    ['.toolbar-button', 'min-height'],
    ['.toolbar-select', 'min-height'],
    ['.button', 'min-height'],
    ['.icon-button', 'min-height'],
    ['.icon-button', 'min-width'],
    ['.dock-tab', 'min-height'],
    ['.status-item', 'min-height'],
    ['.dialog-input', 'min-height'],
    ['.main-menu-item', 'min-height'],
    ['.main-menu-button', 'min-width'],
    ['.b2c-panel button', 'min-height'],
    ['.b2c-panel select', 'min-height'],
    ['.b2c-dialog button', 'min-height'],
    ['.b2c-console-notice summary', 'min-height'],
    ['.feature-input', 'min-height'],
    ['.feature-option', 'min-height'],
    ['.recent-remove', 'min-width'],
    ['.start-card', 'min-height'],
  ];

  it.each(CONTROLS)('%s has a %s of at least 24 px', (selector, property) => {
    const size = pixels(declarationsOf(ALL_RULES, selector).get(property));
    expect(size, `${selector} ${property}`).not.toBeNull();
    expect(size ?? 0).toBeGreaterThanOrEqual(24);
  });
});

describe('reduced motion', () => {
  const app = parseRules(sheet('app/app.css'));

  it('turns every transition and animation off', () => {
    const reduced = app.filter((rule) => rule.media?.includes('prefers-reduced-motion: reduce'));
    const everything = reduced.find(
      (rule) =>
        rule.selectors.includes('*') &&
        rule.selectors.includes('*::before') &&
        rule.selectors.includes('*::after'),
    );
    expect(everything?.declarations.get('animation-duration')).toBe('0s');
    expect(everything?.declarations.get('transition-duration')).toBe('0s');
  });

  it('hides the effects Blockly draws itself', () => {
    const keyboard = parseRules(sheet('editor/keyboard/keyboard.css')).filter((rule) =>
      rule.media?.includes('prefers-reduced-motion: reduce'),
    );
    const hidden = keyboard
      .filter((rule) => rule.declarations.get('display') === 'none')
      .flatMap((rule) => rule.selectors);
    expect(hidden).toEqual(
      expect.arrayContaining(['.blocklyAnimationLayer', '.blocklySvg > circle']),
    );
  });

  it('never scrolls smoothly', () => {
    for (const rule of ALL_RULES) {
      expect(rule.declarations.get('scroll-behavior'), rule.selectors.join(', ')).not.toBe(
        'smooth',
      );
    }
  });
});

describe('the CSS reader', () => {
  it('reads colours, lengths and tokens', () => {
    expect(parseColour('#abc')).toEqual({ r: 0xaa, g: 0xbb, b: 0xcc, a: 1 });
    expect(parseColour('rgb(255 255 255 / 45%)')).toEqual({ r: 255, g: 255, b: 255, a: 0.45 });
    expect(parseColour('rgba(0, 0, 0, 0.5)')).toEqual({ r: 0, g: 0, b: 0, a: 0.5 });
    expect(parseColour('red')).toBeNull();
    expect(pixels('24px')).toBe(24);
    expect(pixels('2rem')).toBe(32);
    expect(pixels('2em')).toBe(28);
    expect(pixels('auto')).toBeNull();
    const tokens = new Map([
      ['--a', 'var(--b)'],
      ['--b', '#000000'],
    ]);
    expect(resolveValue(tokens, 'var(--a)')).toBe('#000000');
    expect(resolveValue(tokens, 'var(--missing, #ffffff)')).toBe('#ffffff');
    expect(resolveValue(tokens, 'var(--missing)')).toBeNull();
    expect(() => contrastOf(tokens, 'var(--missing)', '#ffffff')).toThrow(/Cannot read/);
    expect(contrastOf(tokens, 'var(--a)', '#ffffff')).toBeCloseTo(21, 0);
  });

  it('reads rules inside media blocks and skips comments', () => {
    const rules = parseRules(
      '/* x { } */ .a { color: red; } @media (prefers-reduced-motion: reduce) { .b { --x: #000; } }',
    );
    expect(rules).toHaveLength(2);
    expect(rules[1]?.media).toBe('(prefers-reduced-motion: reduce)');
  });

  it("reads the dark theme's tokens from :root[data-theme='dark'], with either quote", () => {
    const rules = parseRules(
      ':root { --x: #fff; --y: #eee; } :root[data-theme="dark"] { --x: #000; } ' +
        ":root[data-theme='dark'] { --z: #111; } @media (prefers-color-scheme: dark) { :root { --y: #222; } }",
    );
    const tokens = themeTokens(rules);
    expect(tokens.light.get('--x')).toBe('#fff');
    expect(tokens.light.has('--z')).toBe(false);
    expect(tokens.dark.get('--x')).toBe('#000');
    expect(tokens.dark.get('--z')).toBe('#111');
    // A media query never decides a token: the Appearance setting does.
    expect(tokens.dark.get('--y')).toBe('#eee');
  });
});
