/**
 * Taking a chrome scene's screenshot (scenes.ts): the page fixes the size of what is masked and
 * measures the scene's area and its masks; a screenshot of the whole window is cut to the area
 * and the masks are painted over (image.ts), until two screenshots in a row are the same.
 */
import type { WebDriver } from 'selenium-webdriver';

import { sleep } from '../support/wait';
import {
  type Box,
  changedArea,
  isBox,
  type PixelRect,
  samePixels,
  sceneImage,
  type SceneLayout,
  unionBox,
} from './image';
import { type Mask, WINDOW_AREA } from './scenes';

/**
 * Runs in the webview (`executeScript(FIX_WIDTHS_SCRIPT, masks)`): gives every element of each mask
 * that has a `width` that fixed width (and keeps its text from wrapping or spilling out), so that
 * a longer or shorter text there does not move what follows it. Inline style properties set from
 * script are allowed by the app's style-src. Returns how many elements it changed.
 */
export const FIX_WIDTHS_SCRIPT = `
  const masks = arguments[0];
  let changed = 0;
  for (const mask of masks) {
    if (typeof mask.width !== 'number') {
      continue;
    }
    for (const element of document.querySelectorAll(mask.selector)) {
      const width = String(mask.width) + 'px';
      element.style.setProperty('box-sizing', 'border-box');
      element.style.setProperty('width', width);
      element.style.setProperty('min-width', width);
      element.style.setProperty('max-width', width);
      element.style.setProperty('flex', 'none');
      element.style.setProperty('overflow', 'hidden');
      element.style.setProperty('white-space', 'nowrap');
      changed += 1;
    }
  }
  return changed;
`;

/**
 * Runs in the webview (`executeScript(LAYOUT_SCRIPT, area, maskSelectors)`): the page's size, the
 * box of the first element of each of `area`'s selectors (or `null` when there is none) unless
 * `area` is `'window'`, and the boxes of every element of the masks, in CSS pixels.
 */
export const LAYOUT_SCRIPT = `
  const [area, maskSelectors] = arguments;
  const box = (element) => {
    const r = element.getBoundingClientRect();
    return { left: r.left, top: r.top, width: r.width, height: r.height };
  };
  const areaBoxes = area === 'window'
    ? null
    : area.map((selector) => {
        const element = document.querySelector(selector);
        return element === null ? null : box(element);
      });
  const masks = [];
  for (const selector of maskSelectors) {
    for (const element of document.querySelectorAll(selector)) {
      masks.push(box(element));
    }
  }
  return { viewport: [window.innerWidth, window.innerHeight], areaBoxes, masks };
`;

/** A scene's area: the whole window, or the box around the first element of each selector. */
export type SceneArea = typeof WINDOW_AREA | readonly string[];

/**
 * The scene's layout from what {@link LAYOUT_SCRIPT} returned for `area`.
 *
 * @throws Error when the page did not answer as expected, an element of the area is missing or has
 *   no size, or the area is empty.
 */
export function parseLayout(value: unknown, area: SceneArea): SceneLayout {
  if (typeof value !== 'object' || value === null) {
    throw new Error('The page did not measure the scene');
  }
  const found = value as { viewport?: unknown; areaBoxes?: unknown; masks?: unknown };
  const viewport = found.viewport;
  if (
    !Array.isArray(viewport) ||
    viewport.length !== 2 ||
    !viewport.every((side) => typeof side === 'number' && Number.isFinite(side) && side > 0)
  ) {
    throw new Error('The page did not report its size');
  }
  const [width, height] = viewport as [number, number];
  const masks = Array.isArray(found.masks) ? found.masks.filter(isBox) : [];
  if (area === WINDOW_AREA) {
    return { viewport: { width, height }, clip: { left: 0, top: 0, width, height }, masks };
  }
  const boxes = found.areaBoxes;
  if (!Array.isArray(boxes) || boxes.length !== area.length) {
    throw new Error('The page did not measure the scene’s elements');
  }
  const shown: Box[] = [];
  boxes.forEach((box: unknown, index) => {
    if (!isBox(box) || box.width <= 0 || box.height <= 0) {
      throw new Error(`The scene's element ${String(area[index])} is not on the page or not shown`);
    }
    shown.push(box);
  });
  const clip = unionBox(shown);
  if (clip === null) {
    throw new Error('The scene has no elements');
  }
  return { viewport: { width, height }, clip, masks };
}

/** How many screenshots a scene may take to come out the same twice in a row. */
export const CAPTURE_ATTEMPTS = 6;

/** How long to wait between two screenshots of a scene, in milliseconds. */
export const CAPTURE_GAP_MS = 150;

/** A scene that kept changing: its last two images, and where they differ. */
export class UnstableSceneError extends Error {
  override readonly name = 'UnstableSceneError';
  /** The image before the last. */
  readonly previous: Buffer;
  /** The last image. */
  readonly last: Buffer;
  /** Where they differ (scene pixels), or `null` when their sizes differ. */
  readonly changed: PixelRect | null;

  constructor(previous: Buffer, last: Buffer, changed: PixelRect | null) {
    super(
      `The scene still changed after ${String(CAPTURE_ATTEMPTS)} screenshots ${String(CAPTURE_GAP_MS)} ms apart` +
        (changed === null
          ? ' (its size changed)'
          : ` (pixels changed within (${String(changed.left)},${String(changed.top)})–(${String(changed.right)},${String(changed.bottom)}) of the scene)`),
    );
    this.previous = previous;
    this.last = last;
    this.changed = changed;
  }
}

/**
 * Fixes the masked elements' widths ({@link FIX_WIDTHS_SCRIPT}); call it before the page is
 * brought to rest, since it may change the layout.
 */
export async function fixMaskWidths(driver: WebDriver, masks: readonly Mask[]): Promise<void> {
  await driver.executeScript(
    FIX_WIDTHS_SCRIPT,
    masks.map((mask) => ({ selector: mask.selector, width: mask.width ?? null })),
  );
}

/**
 * The scene's image: its area cut out of a screenshot of the window, with its masks painted over,
 * once two screenshots in a row give the same pixels (the page has stopped changing).
 *
 * @throws UnstableSceneError when it still changes after {@link CAPTURE_ATTEMPTS} screenshots.
 * @throws Error when the scene cannot be measured.
 */
export async function captureScene(
  driver: WebDriver,
  area: SceneArea,
  masks: readonly Mask[],
): Promise<Buffer> {
  const selectors = masks.map((mask) => mask.selector);
  let previous: Buffer | null = null;
  let last: Buffer | null = null;
  for (let attempt = 0; attempt < CAPTURE_ATTEMPTS; attempt += 1) {
    if (attempt > 0) {
      await sleep(CAPTURE_GAP_MS);
    }
    const layout = parseLayout(await driver.executeScript(LAYOUT_SCRIPT, area, selectors), area);
    const image = sceneImage(Buffer.from(await driver.takeScreenshot(), 'base64'), layout);
    if (last !== null && samePixels(last, image)) {
      return image;
    }
    previous = last;
    last = image;
  }
  if (previous === null || last === null) {
    throw new Error('A scene needs at least two screenshots');
  }
  throw new UnstableSceneError(previous, last, changedArea(previous, last));
}
