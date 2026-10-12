/**
 * The window's chrome as the webview draws it (docs/spec/04-user-interface.md §4.1, §4.8), checked
 * on the screen rather than in the DOM alone:
 *
 * - The main menu `≡` is drawn above the canvas and its toolbox (Blockly's toolbox sets z-index 70,
 *   the menu 50; app.css isolates the canvas): 10 px inside each edge of every item, the item is
 *   what the pointer would hit, and *Close project* clicked 3 px inside its left edge, over the
 *   toolbox, closes the project.
 * - The Debug/Release drop-down reads at 4.5:1 in a crop of its screenshot (WebKitGTK drew the
 *   native control light in GTK's light theme, under the toolbar's white text), and it is no
 *   taller than the toolbar's buttons. CI runs it in GTK's default light theme; the drop-down is
 *   the app's own drawing, so it reads the same in the dark one.
 * - The status bar's toolchain link has no edge of its own.
 * - At the Windows runner's window size (1028 × 749 CSS pixels) with the C++ panel shown, the
 *   toolbox's flyout (blocks at 0.75, at most 320 px wide) leaves at least 200 px of canvas beside
 *   it (04 §4.2). A native driver that cannot resize the window leaves the app's own size, where
 *   the canvas is wider still.
 */
import { By, type WebDriver } from 'selenium-webdriver';
import { PNG } from 'pngjs';
import { describe, expect, it } from 'vitest';

import { byTestId, clickTestId, testId, waitForFlyout } from '../../support/ui';
import { waitFor } from '../../support/wait';
import { textContrast } from './lib/contrast';
import { startFlow } from './lib/launch';
import { newEmptyProject } from './lib/project';
import { answerDialog, dialogText, UI_TIMEOUT_MS, waitForTestId } from './lib/ui';

/** How far inside each edge of a menu item the pointer is checked, in CSS pixels. */
const INSET_PX = 10;

/** One point checked inside a menu item: whether the item (or its text) is what is hit there. */
interface MenuPoint {
  readonly x: number;
  readonly y: number;
  readonly onItem: boolean;
  readonly overToolbox: boolean;
  /** What is hit there instead, for the failure message. */
  readonly hit: string;
}

/** The points checked inside one item of the open main menu. */
interface MenuItemHits {
  readonly label: string;
  readonly points: readonly MenuPoint[];
}

/**
 * Runs in the webview: for every item of the open main menu, the element `elementFromPoint` finds
 * `arguments[0]` px inside each of its edges (at the middle of the other axis), whether that is the
 * item or inside it, and whether the point is over Blockly's toolbox. A plain script, so nothing
 * the test runner adds to compiled functions can end up in the page.
 */
const MENU_HITS_SCRIPT = `
  const inset = arguments[0];
  const toolbox = document.querySelector('.blocklyToolbox');
  const box = toolbox === null ? null : toolbox.getBoundingClientRect();
  const describe = (element) => {
    if (element === null) {
      return 'nothing';
    }
    const id = element.id === '' ? '' : '#' + element.id;
    const classes = typeof element.className === 'string' && element.className !== ''
      ? '.' + element.className.trim().split(/\\s+/).join('.')
      : '';
    return element.tagName.toLowerCase() + id + classes;
  };
  // An item's label without what is hidden from assistive technology (the shortcut "Ctrl+S").
  const labelOf = (item) => Array.from(item.childNodes)
    .filter((node) => !(node instanceof Element && node.getAttribute('aria-hidden') === 'true'))
    .map((node) => node.textContent || '')
    .join(' ');
  const items = Array.from(document.querySelectorAll('[role="menu"] [role="menuitem"]'));
  return items.map((item) => {
    const r = item.getBoundingClientRect();
    const middleX = r.left + r.width / 2;
    const middleY = r.top + r.height / 2;
    const points = [
      [r.left + inset, middleY],
      [r.right - inset, middleY],
      [middleX, r.top + inset],
      [middleX, r.bottom - inset],
    ];
    return {
      label: labelOf(item).replace(/\\s+/g, ' ').trim(),
      points: points.map(([x, y]) => {
        const hit = document.elementFromPoint(x, y);
        return {
          x,
          y,
          onItem: hit !== null && item.contains(hit),
          overToolbox:
            box !== null && x >= box.left && x < box.right && y >= box.top && y < box.bottom,
          hit: describe(hit),
        };
      }),
    };
  });
`;

/** Opens the main menu and waits for its items. */
async function openMainMenu(driver: WebDriver): Promise<void> {
  await clickTestId(driver, 'main-menu');
  await waitFor(async () => (await driver.findElements(By.css('[role="menuitem"]'))).length > 0, {
    timeout: UI_TIMEOUT_MS,
    message: 'the main menu’s items',
  });
}

/**
 * Runs in the webview: the heights of the Debug/Release drop-down, the ► Run button and the
 * toolbar, the left edge of the status bar's toolchain link, and whether the system prefers dark.
 */
const TOOLBAR_SCRIPT = `
  const height = (selector) => {
    const element = document.querySelector(selector);
    return element === null ? null : element.getBoundingClientRect().height;
  };
  const link = document.querySelector('[data-testid="status-toolchain"]');
  const style = link === null ? null : getComputedStyle(link);
  return {
    select: height('[data-testid="toolbar-config"]'),
    run: height('[data-testid="toolbar-run"]'),
    toolbar: height('.toolbar'),
    linkIsButton: link !== null && link.tagName === 'BUTTON',
    linkBorders: style === null
      ? null
      : [style.borderTopWidth, style.borderRightWidth, style.borderBottomWidth, style.borderLeftWidth],
    dark: window.matchMedia('(prefers-color-scheme: dark)').matches,
    devicePixelRatio: window.devicePixelRatio,
  };
`;

interface ToolbarFacts {
  readonly select: number | null;
  readonly run: number | null;
  readonly toolbar: number | null;
  readonly linkIsButton: boolean;
  readonly linkBorders: readonly string[] | null;
  readonly dark: boolean;
  readonly devicePixelRatio: number;
}

/**
 * The drop-down's text area in its screenshot, in CSS pixels from each edge: past its border and
 * its corners, and left of the chevron (28 px of right padding hold it).
 */
const TEXT_AREA_INSET = { left: 4, top: 4, right: 28, bottom: 4 } as const;

/** The window size of the Windows CI runner, in CSS pixels (its screen is 1024 × 768). */
const RUNNER_WINDOW = { width: 1028, height: 749 } as const;

/** How the workspace is shared out, from {@link LAYOUT_SCRIPT}. */
interface WorkspaceLayout {
  /** The page's size. */
  readonly viewport: readonly [number, number];
  /** The right edge of the toolbox's flyout and of the canvas. */
  readonly flyoutRight: number;
  readonly canvasRight: number;
  /** The flyout's width, and the scale its blocks are drawn at. */
  readonly flyoutWidth: number;
  readonly flyoutScale: number | null;
  /** Whether the C++ panel is shown. */
  readonly codePanelShown: boolean;
}

/** Runs in the webview: the {@link WorkspaceLayout}. */
const LAYOUT_SCRIPT = `
  const workspace = document.querySelector('[data-testid="workspace"]');
  const canvas = workspace.querySelector('svg.blocklySvg').getBoundingClientRect();
  const flyout = workspace.querySelector('svg.blocklyToolboxFlyout');
  const blocks = flyout.querySelector('.blocklyBlockCanvas').getAttribute('transform') || '';
  const scale = /scale\\(([0-9.]+)\\)/.exec(blocks);
  const dock = document.querySelector('aside.right-dock');
  return {
    viewport: [window.innerWidth, window.innerHeight],
    flyoutRight: flyout.getBoundingClientRect().right,
    canvasRight: canvas.right,
    flyoutWidth: flyout.getBoundingClientRect().width,
    flyoutScale: scale === null ? null : Number(scale[1]),
    codePanelShown: dock !== null && dock.getAttribute('data-collapsed') !== 'true',
  };
`;

describe('The window’s chrome', () => {
  it('draws the main menu above the toolbox, where Close project works at its left edge', async (context) => {
    const flow = startFlow(context);
    const app = await flow.launch();
    const { driver } = app;
    await newEmptyProject(app);
    await waitForTestId(driver, 'workspace', true);

    await openMainMenu(driver);
    const items = await driver.executeScript<MenuItemHits[]>(MENU_HITS_SCRIPT, INSET_PX);
    expect(items.map((item) => item.label)).toEqual(
      expect.arrayContaining(['Save', 'Close project']),
    );
    const missed = items.flatMap((item) =>
      item.points
        .filter((point) => !point.onItem)
        .map(
          (point) =>
            `"${item.label}" at ${point.x.toFixed(0)},${point.y.toFixed(0)} hits ${point.hit}`,
        ),
    );
    expect(missed).toEqual([]);
    // The check means something only where the menu lies over the toolbox.
    expect(items.some((item) => item.points.some((point) => point.overToolbox))).toBe(true);

    // Close project, clicked 3 px inside its left edge.
    const close = await waitFor(
      async () => {
        for (const item of await driver.findElements(By.css('[role="menuitem"]'))) {
          if ((await item.getText()).startsWith('Close project')) {
            return item;
          }
        }
        return null;
      },
      { timeout: UI_TIMEOUT_MS, message: 'the main menu item "Close project"' },
    );
    const { width } = await close.getRect();
    await driver
      .actions({ async: true })
      .move({ origin: close, x: -Math.floor(width / 2) + 3, y: 0 })
      .click()
      .perform();
    await waitFor(
      async () => {
        if ((await driver.findElements(byTestId('start-page'))).length > 0) {
          return true;
        }
        // A project with unsaved changes asks first.
        if ((await dialogText(driver)).includes('Save')) {
          await answerDialog(driver, "Don't save");
        }
        return false;
      },
      { timeout: UI_TIMEOUT_MS, message: 'the start page after Close project' },
    );
  }, 180_000);

  it('leaves the canvas 200 px beside the toolbox at the Windows runner’s size', async (context) => {
    const flow = startFlow(context);
    const app = await flow.launch();
    const { driver } = app;
    // The Windows runner's window has this size by itself (the screen is smaller than the app's
    // 1280 × 800); WebKitWebDriver resizes the window to it, and WebView2's driver may not.
    await driver
      .manage()
      .window()
      .setRect({ ...RUNNER_WINDOW })
      .catch(() => undefined);
    await newEmptyProject(app);
    await waitForTestId(driver, 'workspace', true);
    await waitForFlyout(driver);

    const layout = await driver.executeScript<WorkspaceLayout>(LAYOUT_SCRIPT);
    const facts = JSON.stringify(layout);
    expect(layout.codePanelShown, facts).toBe(true);
    expect(layout.flyoutScale, facts).toBe(0.75);
    expect(layout.flyoutWidth, facts).toBeLessThanOrEqual(320.5);
    expect(layout.canvasRight - layout.flyoutRight, facts).toBeGreaterThanOrEqual(200);
  }, 120_000);

  it('draws the Debug/Release drop-down readable and no taller than the buttons', async (context) => {
    const flow = startFlow(context);
    const app = await flow.launch();
    const { driver } = app;
    const select = await testId(driver, 'toolbar-config');
    // Nothing hovered or focused changes how it is drawn.
    await driver.actions({ async: true }).move({ x: 1, y: 1 }).perform();

    const facts = await driver.executeScript<ToolbarFacts>(TOOLBAR_SCRIPT);
    expect(facts.select).not.toBeNull();
    expect(facts.run).not.toBeNull();
    // The toolbar keeps its height: the drop-down is no taller than ► Run.
    expect(facts.select ?? Infinity).toBeLessThanOrEqual((facts.run ?? 0) + 0.5);
    expect(facts.toolbar ?? Infinity).toBeLessThanOrEqual((facts.run ?? 0) + 12 + 0.5);

    const shot = PNG.sync.read(Buffer.from(await select.takeScreenshot(), 'base64'));
    const { width, height } = await select.getRect();
    const scale = shot.width / width;
    const region = {
      left: Math.ceil(TEXT_AREA_INSET.left * scale),
      top: Math.ceil(TEXT_AREA_INSET.top * scale),
      right: Math.floor((width - TEXT_AREA_INSET.right) * scale),
      bottom: Math.min(shot.height, Math.floor((height - TEXT_AREA_INSET.bottom) * scale)),
    };
    const found = textContrast(shot, region);
    expect(
      found.ratio,
      `"Debug" is ${found.text} on ${found.background} (${found.ratio.toFixed(2)}:1) in the ${facts.dark ? 'dark' : 'light'} theme, device pixel ratio ${String(facts.devicePixelRatio)}`,
    ).toBeGreaterThanOrEqual(4.5);

    // The status bar's toolchain link (a button once the toolchain page exists) has no edge.
    if (facts.linkIsButton) {
      expect(facts.linkBorders).toEqual(['0px', '0px', '0px', '0px']);
    }
  }, 120_000);
});
