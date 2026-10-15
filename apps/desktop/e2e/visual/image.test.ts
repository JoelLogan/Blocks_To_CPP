/** Cutting scenes out of window screenshots and masking them (image.ts), on synthetic PNG images. */
import { PNG } from 'pngjs';
import { describe, expect, it } from 'vitest';

import { VisualCompareError } from './compare';
import {
  cropPng,
  isBox,
  MASK_COLOUR,
  maskPng,
  samePixels,
  sceneImage,
  toPixels,
  unionBox,
} from './image';

/** A `width`×`height` PNG whose pixel (x, y) has the colour (x, y, 7). */
function gradient(width: number, height: number): Buffer {
  const image = new PNG({ width, height });
  for (let y = 0; y < height; y += 1) {
    for (let x = 0; x < width; x += 1) {
      const at = (y * width + x) * 4;
      image.data[at] = x;
      image.data[at + 1] = y;
      image.data[at + 2] = 7;
      image.data[at + 3] = 255;
    }
  }
  return PNG.sync.write(image);
}

/** The colour of pixel (x, y) of a PNG. */
function pixel(png: Buffer, x: number, y: number): [number, number, number] {
  const image = PNG.sync.read(png);
  const at = (y * image.width + x) * 4;
  return [image.data[at] ?? -1, image.data[at + 1] ?? -1, image.data[at + 2] ?? -1];
}

function size(png: Buffer): [number, number] {
  const image = PNG.sync.read(png);
  return [image.width, image.height];
}

describe('isBox', () => {
  it('accepts finite boxes of no negative size only', () => {
    expect(isBox({ left: 0, top: -3.5, width: 0, height: 10 })).toBe(true);
    for (const bad of [
      null,
      'box',
      { left: 0, top: 0, width: 1 },
      { left: 0, top: 0, width: -1, height: 1 },
      { left: Number.NaN, top: 0, width: 1, height: 1 },
      { left: 0, top: Infinity, width: 1, height: 1 },
      { left: '0', top: 0, width: 1, height: 1 },
    ]) {
      expect(isBox(bad), JSON.stringify(bad)).toBe(false);
    }
  });
});

describe('unionBox', () => {
  it('is the smallest box around all', () => {
    expect(
      unionBox([
        { left: 0, top: 40, width: 100, height: 332 },
        { left: 100, top: 40, width: 214, height: 332 },
      ]),
    ).toEqual({ left: 0, top: 40, width: 314, height: 332 });
    expect(unionBox([{ left: 5, top: 6, width: 7, height: 8 }])).toEqual({
      left: 5,
      top: 6,
      width: 7,
      height: 8,
    });
    expect(unionBox([])).toBeNull();
  });
});

describe('toPixels', () => {
  it('rounds the edges outwards, at the device scale', () => {
    expect(toPixels({ left: 10.5, top: 2.2, width: 5, height: 3.1 }, 1, 100, 100)).toEqual({
      left: 10,
      top: 2,
      right: 16,
      bottom: 6,
    });
    expect(toPixels({ left: 10.5, top: 2.25, width: 5, height: 3 }, 2, 100, 100)).toEqual({
      left: 21,
      top: 4,
      right: 31,
      bottom: 11,
    });
  });

  it('does not grow a box by a rounding error', () => {
    expect(
      toPixels({ left: 0.1 + 0.2 - 0.3, top: 99.99999, width: 10, height: 1 }, 1, 200, 200),
    ).toMatchObject({ left: 0, top: 100, right: 10, bottom: 101 });
  });

  it('cuts the box to the image, and gives null for a box outside it', () => {
    expect(toPixels({ left: -5, top: 550, width: 900, height: 20 }, 1, 800, 560)).toEqual({
      left: 0,
      top: 550,
      right: 800,
      bottom: 560,
    });
    expect(toPixels({ left: 900, top: 0, width: 10, height: 10 }, 1, 800, 560)).toBeNull();
    expect(toPixels({ left: 0, top: 0, width: 0, height: 10 }, 1, 800, 560)).toBeNull();
  });

  it('refuses a scale that is not a positive number', () => {
    for (const scale of [0, -1, Number.NaN]) {
      expect(() => toPixels({ left: 0, top: 0, width: 1, height: 1 }, scale, 9, 9)).toThrow(
        VisualCompareError,
      );
    }
  });
});

describe('cropPng', () => {
  it('cuts out the rectangle', () => {
    const cut = cropPng(gradient(20, 10), { left: 3, top: 2, right: 8, bottom: 6 });
    expect(size(cut)).toEqual([5, 4]);
    expect(pixel(cut, 0, 0)).toEqual([3, 2, 7]);
    expect(pixel(cut, 4, 3)).toEqual([7, 5, 7]);
  });

  it('refuses a rectangle outside the image or empty', () => {
    const png = gradient(20, 10);
    for (const rect of [
      { left: -1, top: 0, right: 5, bottom: 5 },
      { left: 0, top: 0, right: 21, bottom: 5 },
      { left: 0, top: 0, right: 5, bottom: 11 },
      { left: 5, top: 0, right: 5, bottom: 5 },
    ]) {
      expect(() => cropPng(png, rect), JSON.stringify(rect)).toThrow(/not inside/);
    }
  });
});

describe('maskPng', () => {
  it('paints the rectangles in the mask colour and leaves the rest', () => {
    const masked = maskPng(gradient(20, 10), [
      { left: 2, top: 1, right: 4, bottom: 3 },
      { left: 18, top: 8, right: 30, bottom: 30 },
    ]);
    expect(pixel(masked, 2, 1)).toEqual([...MASK_COLOUR]);
    expect(pixel(masked, 3, 2)).toEqual([...MASK_COLOUR]);
    expect(pixel(masked, 4, 2)).toEqual([4, 2, 7]);
    expect(pixel(masked, 19, 9)).toEqual([...MASK_COLOUR]);
    expect(pixel(masked, 17, 9)).toEqual([17, 9, 7]);
  });

  it('returns the image as it is without masks', () => {
    const png = gradient(4, 4);
    expect(maskPng(png, [])).toBe(png);
  });
});

describe('sceneImage', () => {
  it('cuts the area out of a 1× screenshot and moves the masks to its corner', () => {
    const shot = gradient(100, 60);
    const image = sceneImage(shot, {
      viewport: { width: 100, height: 60 },
      clip: { left: 10, top: 20, width: 30, height: 15 },
      masks: [
        { left: 12, top: 22, width: 2, height: 2 },
        // Outside the area: it changes nothing there.
        { left: 80, top: 50, width: 5, height: 5 },
      ],
    });
    expect(size(image)).toEqual([30, 15]);
    expect(pixel(image, 0, 0)).toEqual([10, 20, 7]);
    expect(pixel(image, 2, 2)).toEqual([...MASK_COLOUR]);
    expect(pixel(image, 3, 3)).toEqual([...MASK_COLOUR]);
    expect(pixel(image, 4, 4)).toEqual([14, 24, 7]);
  });

  it('scales CSS pixels to the screenshot’s device pixels', () => {
    // A 50×30 page drawn at 2 device pixels per CSS pixel.
    const image = sceneImage(gradient(100, 60), {
      viewport: { width: 50, height: 30 },
      clip: { left: 5, top: 10, width: 15, height: 7.5 },
      masks: [],
    });
    expect(size(image)).toEqual([30, 15]);
    expect(pixel(image, 0, 0)).toEqual([10, 20, 7]);
  });

  it('takes the whole window', () => {
    const shot = gradient(40, 30);
    const image = sceneImage(shot, {
      viewport: { width: 40, height: 30 },
      clip: { left: 0, top: 0, width: 40, height: 30 },
      masks: [],
    });
    expect(samePixels(image, shot)).toBe(true);
  });

  it('refuses an area outside the window', () => {
    expect(() =>
      sceneImage(gradient(40, 30), {
        viewport: { width: 40, height: 30 },
        clip: { left: 50, top: 0, width: 10, height: 10 },
        masks: [],
      }),
    ).toThrow(/no area inside the window/);
  });
});

describe('samePixels', () => {
  it('compares the size and every pixel', () => {
    expect(samePixels(gradient(8, 8), gradient(8, 8))).toBe(true);
    expect(samePixels(gradient(8, 8), gradient(8, 9))).toBe(false);
    expect(
      samePixels(
        gradient(8, 8),
        maskPng(gradient(8, 8), [{ left: 7, top: 7, right: 8, bottom: 8 }]),
      ),
    ).toBe(false);
  });
});
