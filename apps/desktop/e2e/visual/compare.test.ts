/** Comparing screenshots with their baselines (compare.ts), on synthetic and committed PNG images. */
import { readdirSync, readFileSync } from 'node:fs';
import path from 'node:path';

import { PNG } from 'pngjs';
import { describe, expect, it } from 'vitest';

import { BASELINES_DIR } from './baselines';
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

  it('counts a recoloured area pixel for pixel', () => {
    // A 20 × 20 red square on light grey: 400 pixels, every one well beyond the threshold.
    const base = png(100, 100);
    const image = PNG.sync.read(base);
    for (let y = 10; y < 30; y += 1) {
      for (let x = 10; x < 30; x += 1) {
        image.data.set([200, 30, 30], (y * 100 + x) * 4);
      }
    }
    const result = compareScreenshots(base, PNG.sync.write(image));
    expect(result.diffPixels).toBe(400);
    expect(result.passed).toBe(false);
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

/** `bytes` (a PNG) with each pixel changed by `change`, which gets the pixel's index. */
function changed(bytes: Buffer, change: (rgb: number[], index: number) => number[]): Buffer {
  const image = PNG.sync.read(bytes);
  for (let index = 0; index < image.width * image.height; index += 1) {
    const at = index * 4;
    const rgb = change(
      [image.data[at] ?? 0, image.data[at + 1] ?? 0, image.data[at + 2] ?? 0],
      index,
    );
    image.data.set(
      rgb.map((value) => Math.min(255, Math.max(0, value))),
      at,
    );
  }
  return PNG.sync.write(image);
}

/** Every committed baseline, as `<system>/<name>.png` and its bytes. */
function committedBaselines(): [string, Buffer][] {
  return ['linux', 'windows'].flatMap((system) =>
    readdirSync(path.join(BASELINES_DIR, system))
      .filter((name) => name.endsWith('.png'))
      .map((name): [string, Buffer] => [
        `${system}/${name}`,
        readFileSync(path.join(BASELINES_DIR, system, name)),
      ]),
  );
}

describe('the threshold on the committed baselines', () => {
  // pixelmatch 8 measures a perceptual (OKLab HyAB) distance where pixelmatch 7 measured YIQ; at
  // 0.1 it must still let a runner's rendering noise through (a real change is counted above).
  const baselines = committedBaselines();

  it('has baselines to check', () => {
    expect(baselines.length).toBeGreaterThan(0);
  });

  it.each(baselines)('ignores rendering noise in %s', (_name, baseline) => {
    // Each channel ±3 (a fixed pattern), and the whole image 10 brighter.
    const noisy = changed(baseline, (rgb, index) =>
      rgb.map((value, channel) => value + (((index * 7 + channel * 3) % 7) - 3)),
    );
    expect(compareScreenshots(baseline, noisy).diffPixels).toBe(0);
    const brighter = changed(baseline, (rgb) => rgb.map((value) => value + 10));
    expect(compareScreenshots(baseline, brighter).diffPixels).toBe(0);
  });
});
