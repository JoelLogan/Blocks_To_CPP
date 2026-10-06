/** Comparing screenshots with their baselines (compare.ts), on synthetic PNG images. */
import { PNG } from 'pngjs';
import { describe, expect, it } from 'vitest';

import {
  compareScreenshots,
  decodePng,
  MAX_DIFF_RATIO,
  MAX_SIDE,
  pngSize,
  VisualCompareError,
} from './compare';

/** A `width`×`height` PNG of one colour, with `marked` pixels (index order) in another. */
function png(
  width: number,
  height: number,
  marked = 0,
  colour: [number, number, number] = [240, 240, 240],
): Buffer {
  const image = new PNG({ width, height });
  for (let index = 0; index < width * height; index += 1) {
    const [r, g, b] = index < marked ? [200, 30, 30] : colour;
    image.data[index * 4] = r;
    image.data[index * 4 + 1] = g;
    image.data[index * 4 + 2] = b;
    image.data[index * 4 + 3] = 255;
  }
  return PNG.sync.write(image);
}

describe('compareScreenshots', () => {
  it('passes identical images', () => {
    const result = compareScreenshots(png(100, 50), png(100, 50));
    expect(result).toMatchObject({ width: 100, height: 50, diffPixels: 0, ratio: 0, passed: true });
    expect(decodePng(result.diffPng, 'diff').width).toBe(100);
  });

  it('passes up to 0.5% of differing pixels and fails above', () => {
    // 100 × 100 = 10,000 pixels: 50 is 0.5%, 51 is over.
    const at = compareScreenshots(png(100, 100), png(100, 100, 50));
    expect(at.diffPixels).toBe(50);
    expect(at.ratio).toBe(MAX_DIFF_RATIO);
    expect(at.passed).toBe(true);
    const over = compareScreenshots(png(100, 100), png(100, 100, 51));
    expect(over.diffPixels).toBe(51);
    expect(over.passed).toBe(false);
  });

  it('ignores colour changes below the threshold', () => {
    const result = compareScreenshots(png(40, 40), png(40, 40, 0, [238, 239, 240]));
    expect(result.diffPixels).toBe(0);
  });

  it('refuses images of different sizes', () => {
    expect(() => compareScreenshots(png(100, 50), png(100, 51))).toThrow(VisualCompareError);
    expect(() => compareScreenshots(png(100, 50), png(99, 50))).toThrow(
      /99×50 pixels but the baseline is 100×50/,
    );
  });

  it('refuses what is not a PNG, or too large', () => {
    expect(() => compareScreenshots(Buffer.from('not a png'), png(1, 1))).toThrow(
      /The baseline is not a PNG image/,
    );
    expect(() => compareScreenshots(png(1, 1), Buffer.alloc(0))).toThrow(
      /The screenshot is not a PNG image/,
    );
    expect(() => decodePng(png(MAX_SIDE + 1, 1), 'baseline')).toThrow(/each side must be/);
  });
});

describe('pngSize', () => {
  it('names the size of a PNG', () => {
    expect(pngSize(png(963, 510))).toBe('963×510');
  });
});
