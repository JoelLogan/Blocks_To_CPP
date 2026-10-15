/**
 * Cutting a scene out of a window screenshot and masking what changes from run to run
 * (docs/spec/09-quality-and-delivery.md §9.2 "Visual diff"). The page measures its boxes in CSS
 * pixels; a screenshot has device pixels, `scale` of them per CSS pixel. A box becomes the device
 * pixels it touches (its edges rounded outwards), so a mask covers everything it is meant to hide.
 */
import { PNG } from 'pngjs';

import { decodePng, VisualCompareError } from './compare';

/** A box in CSS pixels, as `getBoundingClientRect` gives it (viewport coordinates). */
export interface Box {
  readonly left: number;
  readonly top: number;
  readonly width: number;
  readonly height: number;
}

/** A rectangle of whole device pixels, `right` and `bottom` excluded. */
export interface PixelRect {
  readonly left: number;
  readonly top: number;
  readonly right: number;
  readonly bottom: number;
}

/** The colour of a masked area (opaque magenta: never part of the app's palette). */
export const MASK_COLOUR: readonly [number, number, number] = [255, 0, 255];

/** Whether `value` is a box with finite coordinates and no negative size. */
export function isBox(value: unknown): value is Box {
  if (typeof value !== 'object' || value === null) {
    return false;
  }
  const box = value as Record<string, unknown>;
  const side = (key: string): number | null => {
    const found = box[key];
    return typeof found === 'number' && Number.isFinite(found) ? found : null;
  };
  const [left, top, width, height] = ['left', 'top', 'width', 'height'].map(side);
  return (
    left !== null &&
    top !== null &&
    width !== null &&
    height !== null &&
    width !== undefined &&
    height !== undefined &&
    width >= 0 &&
    height >= 0
  );
}

/** The smallest box around all of `boxes`, or `null` when there are none. */
export function unionBox(boxes: readonly Box[]): Box | null {
  if (boxes.length === 0) {
    return null;
  }
  const left = Math.min(...boxes.map((box) => box.left));
  const top = Math.min(...boxes.map((box) => box.top));
  const right = Math.max(...boxes.map((box) => box.left + box.width));
  const bottom = Math.max(...boxes.map((box) => box.top + box.height));
  return { left, top, width: right - left, height: bottom - top };
}

/**
 * The device pixels box `box` touches, at `scale` device pixels per CSS pixel, cut to an image of
 * `width`×`height`; `null` when nothing of it is in the image. The edges are rounded outwards
 * (after a tolerance of 1/1000 pixel, so 99.9999 counts as 100).
 */
export function toPixels(box: Box, scale: number, width: number, height: number): PixelRect | null {
  if (!Number.isFinite(scale) || scale <= 0) {
    throw new VisualCompareError(
      `The device pixel scale ${String(scale)} is not a positive number`,
    );
  }
  const outwards = (value: number, round: (n: number) => number, toward: number) =>
    round(value * scale + toward);
  const rect = {
    left: Math.max(0, outwards(box.left, Math.floor, 0.001)),
    top: Math.max(0, outwards(box.top, Math.floor, 0.001)),
    right: Math.min(width, outwards(box.left + box.width, Math.ceil, -0.001)),
    bottom: Math.min(height, outwards(box.top + box.height, Math.ceil, -0.001)),
  };
  return rect.right > rect.left && rect.bottom > rect.top ? rect : null;
}

/**
 * The part `clip` of the PNG `screenshot` (device pixels), as a new PNG.
 *
 * @throws VisualCompareError when the screenshot is no PNG, or `clip` is not inside it.
 */
export function cropPng(screenshot: Buffer, clip: PixelRect): Buffer {
  const image = decodePng(screenshot, 'screenshot');
  if (
    clip.left < 0 ||
    clip.top < 0 ||
    clip.right > image.width ||
    clip.bottom > image.height ||
    clip.right <= clip.left ||
    clip.bottom <= clip.top
  ) {
    throw new VisualCompareError(
      `The scene's area (${String(clip.left)},${String(clip.top)})–(${String(clip.right)},${String(clip.bottom)}) ` +
        `is not inside the ${String(image.width)}×${String(image.height)} screenshot`,
    );
  }
  const width = clip.right - clip.left;
  const height = clip.bottom - clip.top;
  const out = new PNG({ width, height });
  PNG.bitblt(image, out, clip.left, clip.top, width, height, 0, 0);
  return PNG.sync.write(out);
}

/** Paints each of `rects` (device pixels; parts outside the image are left out) in {@link MASK_COLOUR}. */
export function maskPng(png: Buffer, rects: readonly PixelRect[]): Buffer {
  if (rects.length === 0) {
    return png;
  }
  const image = decodePng(png, 'screenshot');
  const [r, g, b] = MASK_COLOUR;
  for (const rect of rects) {
    const top = Math.max(0, rect.top);
    const bottom = Math.min(image.height, rect.bottom);
    const left = Math.max(0, rect.left);
    const right = Math.min(image.width, rect.right);
    for (let y = top; y < bottom; y += 1) {
      for (let x = left; x < right; x += 1) {
        const at = (y * image.width + x) * 4;
        image.data[at] = r;
        image.data[at + 1] = g;
        image.data[at + 2] = b;
        image.data[at + 3] = 255;
      }
    }
  }
  return PNG.sync.write(image);
}

/** What the page measured for a scene, in CSS pixels. */
export interface SceneLayout {
  /** The page's size. */
  readonly viewport: { readonly width: number; readonly height: number };
  /** The scene's area: the window, or the union of its elements. */
  readonly clip: Box;
  /** The boxes to mask. */
  readonly masks: readonly Box[];
}

/**
 * Cuts the scene out of a screenshot of the whole window and paints its masks: the screenshot's
 * device pixels per CSS pixel come from its width and the page's, and the masks are moved to the
 * cut-out's corner.
 *
 * @throws VisualCompareError when the screenshot is no PNG, or the scene is not inside it.
 */
export function sceneImage(windowShot: Buffer, layout: SceneLayout): Buffer {
  const image = decodePng(windowShot, 'screenshot');
  const scale = image.width / layout.viewport.width;
  const clip = toPixels(layout.clip, scale, image.width, image.height);
  if (clip === null) {
    throw new VisualCompareError('The scene has no area inside the window');
  }
  const cut = cropPng(windowShot, clip);
  const masks = layout.masks
    .map((box) => toPixels(box, scale, image.width, image.height))
    .filter((rect): rect is PixelRect => rect !== null)
    .map((rect) => ({
      left: rect.left - clip.left,
      top: rect.top - clip.top,
      right: rect.right - clip.left,
      bottom: rect.bottom - clip.top,
    }));
  return maskPng(cut, masks);
}

/** Whether two PNG images have the same size and the same pixels. */
export function samePixels(a: Buffer, b: Buffer): boolean {
  const first = decodePng(a, 'screenshot');
  const second = decodePng(b, 'screenshot');
  return (
    first.width === second.width && first.height === second.height && first.data.equals(second.data)
  );
}

/**
 * The smallest rectangle holding every pixel that differs between two images of the same size, or
 * `null` when none does (or the sizes differ).
 */
export function changedArea(a: Buffer, b: Buffer): PixelRect | null {
  const first = decodePng(a, 'screenshot');
  const second = decodePng(b, 'screenshot');
  if (first.width !== second.width || first.height !== second.height) {
    return null;
  }
  let left = Infinity;
  let top = Infinity;
  let right = -Infinity;
  let bottom = -Infinity;
  for (let y = 0; y < first.height; y += 1) {
    for (let x = 0; x < first.width; x += 1) {
      const at = (y * first.width + x) * 4;
      if (
        first.data[at] !== second.data[at] ||
        first.data[at + 1] !== second.data[at + 1] ||
        first.data[at + 2] !== second.data[at + 2] ||
        first.data[at + 3] !== second.data[at + 3]
      ) {
        left = Math.min(left, x);
        top = Math.min(top, y);
        right = Math.max(right, x + 1);
        bottom = Math.max(bottom, y + 1);
      }
    }
  }
  return Number.isFinite(left) ? { left, top, right, bottom } : null;
}
