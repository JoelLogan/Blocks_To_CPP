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
  MAX_TILE_DISTANCE,
  pngSize,
  TILE_SIDE,
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

/** `bytes` (a PNG) with the rectangle `[x, y, width, height]` in `colour`. */
function withRect(
  bytes: Buffer,
  [left, top, width, height]: [number, number, number, number],
  colour: [number, number, number],
): Buffer {
  const image = PNG.sync.read(bytes);
  for (let y = top; y < top + height; y += 1) {
    for (let x = left; x < left + width; x += 1) {
      image.data.set(colour, (y * image.width + x) * 4);
    }
  }
  return PNG.sync.write(image);
}

/** `bytes` (a PNG) with the pixels of colour `from` in colour `to`; how many changed. */
function recoloured(bytes: Buffer, from: string, to: string): { png: Buffer; pixels: number } {
  const rgb = (hex: string) => [1, 3, 5].map((at) => Number.parseInt(hex.slice(at, at + 2), 16));
  const [before, after] = [rgb(from), rgb(to)];
  const image = PNG.sync.read(bytes);
  let pixels = 0;
  for (let at = 0; at < image.data.length; at += 4) {
    if (before.every((value, channel) => image.data[at + channel] === value)) {
      image.data.set(after, at);
      pixels += 1;
    }
  }
  return { png: PNG.sync.write(image), pixels };
}

/** The RGB of pixel (`x`, `y`) of a PNG. */
function pixel(bytes: Buffer, x: number, y: number): number[] {
  const image = decodePng(bytes, 'image');
  const at = (y * image.width + x) * 4;
  return [...image.data.subarray(at, at + 3)];
}

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

/**
 * `bytes` (a PNG) with every channel of every pixel 3 up or down: bit `channel` of the pixel's
 * index picks the sign, so each run of 8 pixels has all 8 combinations and neighbours differ. Near
 * 0 and 255 the step is smaller (as far as the value can go either way), so the mean stays the
 * same: 0 or 255 does not move.
 */
function noisy(bytes: Buffer): Buffer {
  return changed(bytes, (rgb, index) =>
    rgb.map(
      (value, channel) =>
        value + Math.min(3, value, 255 - value) * ((index >> channel) % 2 === 1 ? 1 : -1),
    ),
  );
}

/**
 * `bytes` (a PNG) with its anti-aliased pixels `factor` further from their background (negative:
 * nearer), as text drawn with stronger or weaker contrast: in each tile, the commonest colour is
 * the background, and a colour on fewer than 4 of the tile's pixels is taken for anti-aliasing.
 */
function contrasted(bytes: Buffer, factor: number): Buffer {
  const image = PNG.sync.read(bytes);
  const source = Buffer.from(image.data);
  const { width, height } = image;
  for (let top = 0; top < height; top += TILE_SIDE) {
    for (let left = 0; left < width; left += TILE_SIDE) {
      const tile: number[] = [];
      for (let y = top; y < Math.min(top + TILE_SIDE, height); y += 1) {
        for (let x = left; x < Math.min(left + TILE_SIDE, width); x += 1)
          tile.push((y * width + x) * 4);
      }
      const counts = new Map<number, number>();
      for (const at of tile) {
        const key = source.readUInt32BE(at);
        counts.set(key, (counts.get(key) ?? 0) + 1);
      }
      const [background] = [...counts].reduce((best, entry) => (entry[1] > best[1] ? entry : best));
      const back = [background >>> 24, (background >>> 16) & 255, (background >>> 8) & 255];
      for (const at of tile) {
        const key = source.readUInt32BE(at);
        if (key === background || (counts.get(key) ?? 0) >= 4) continue;
        for (let channel = 0; channel < 3; channel += 1) {
          const value = source[at + channel] ?? 0;
          const moved = Math.round(value + factor * (value - (back[channel] ?? 0)));
          image.data[at + channel] = Math.min(255, Math.max(0, moved));
        }
      }
    }
  }
  return PNG.sync.write(image);
}

/** The W2 review's light Types colour before (2f747cf) and after. */
const TYPES_BEFORE: [number, number, number] = [0x68, 0x68, 0x00];
const TYPES_AFTER: [number, number, number] = [0x53, 0x62, 0x2c];

describe('compareScreenshots', () => {
  it('passes identical images', () => {
    const result = compareScreenshots(png(100, 50), png(100, 50));
    expect(result).toMatchObject({
      width: 100,
      height: 50,
      diffPixels: 0,
      ratio: 0,
      changedTiles: 0,
      maxTileDistance: 0,
      passed: true,
    });
    expect(decodePng(result.diffPng, 'diff').width).toBe(100);
  });

  it('passes up to 0.5% of differing pixels and fails above', () => {
    // 100 × 100 = 10,000 pixels: 50 is 0.5%, 51 is over.
    const at = compareScreenshots(png(100, 100), png(100, 100, 50));
    expect(at.diffPixels).toBe(50);
    expect(at.ratio).toBe(MAX_DIFF_RATIO);
    expect(at.changedTiles).toBe(0);
    expect(at.passed).toBe(true);
    const over = compareScreenshots(png(100, 100), png(100, 100, 51));
    expect(over.diffPixels).toBe(51);
    expect(over.passed).toBe(false);
  });

  it('ignores colour changes below the threshold', () => {
    const result = compareScreenshots(png(40, 40), png(40, 40, 0, [238, 239, 240]));
    expect(result.diffPixels).toBe(0);
    expect(result.changedTiles).toBe(0);
    expect(result.passed).toBe(true);
  });

  it('fails a recolouring that no pixel is counted for (the W2 review’s Types colour)', () => {
    // 2f747cf: Types #686800 → #53622c, 0.070 on every pixel of the block, under 0.1. A 48 × 48
    // block on white covers 9 of the 36 tiles.
    const before = withRect(png(96, 96, 0, [255, 255, 255]), [16, 16, 48, 48], TYPES_BEFORE);
    const after = recoloured(before, '#686800', '#53622c');
    expect(after.pixels).toBe(48 * 48);
    const result = compareScreenshots(before, after.png);
    expect(result.diffPixels).toBe(0);
    expect(result.changedTiles).toBe(9);
    expect(result.maxTileDistance).toBeCloseTo(0.0703, 4);
    expect(result.passed).toBe(false);
    // The diff image outlines the changed tiles in blue.
    expect(pixel(result.diffPng, 16, 16)).toEqual([0, 64, 255]);
  });

  it('leaves a tile with a counted pixel to the pixel rule, and keeps its red marks', () => {
    // The recolouring above with a black 2 × 2 dot in its first tile: pixelmatch counts the dot,
    // so that tile is not measured (and not outlined); the other 8 are.
    const before = withRect(png(96, 96, 0, [255, 255, 255]), [16, 16, 48, 48], TYPES_BEFORE);
    const after = withRect(recoloured(before, '#686800', '#53622c').png, [16, 16, 2, 2], [0, 0, 0]);
    const result = compareScreenshots(before, after);
    expect(result.diffPixels).toBe(4);
    expect(result.changedTiles).toBe(8);
    expect(pixel(result.diffPng, 16, 16)).toEqual([255, 0, 0]);
    expect(pixel(result.diffPng, 17, 17)).toEqual([255, 0, 0]);
    expect(pixel(result.diffPng, 32, 16)).toEqual([0, 64, 255]);
  });

  it('passes a small strong change: a 3 × 3 dot is the pixel rule’s, under 0.5%', () => {
    // On the toolbar's 800 × 40 (150 tiles), 9 black pixels on white would move their tile's mean
    // colour 9/256 = 0.035, 2.3 times the tile limit, but pixelmatch counts them, so only the
    // pixel rule applies.
    const before = png(800, 40, 0, [255, 255, 255]);
    const result = compareScreenshots(before, withRect(before, [100, 10, 3, 3], [0, 0, 0]));
    expect(result).toMatchObject({ diffPixels: 9, changedTiles: 0, passed: true });
  });

  it('leaves pixels that pixelmatch takes for anti-aliasing to the pixel rule', () => {
    // A 2-pixel black stroke on the toolbar's 800 × 40, its left column half lightened, as a stroke
    // drawn half a pixel off looks: pixelmatch sees that column change beyond the threshold but
    // takes it for anti-aliasing and counts none of it. Its pixels are flat in the baseline, and
    // counted they would move their tile 0.034, but they are not the tile rule’s.
    const before = withRect(png(800, 40, 0, [255, 255, 255]), [100, 0, 2, 40], [0, 0, 0]);
    const result = compareScreenshots(before, withRect(before, [100, 0, 1, 40], [128, 128, 128]));
    expect(result).toMatchObject({ diffPixels: 0, changedTiles: 0, maxTileDistance: 0 });
    expect(pixel(result.diffPng, 100, 20)).toEqual([255, 255, 0]);
    expect(result.passed).toBe(true);
  });

  it('passes changed tiles up to 0.5% of the tiles and fails above', () => {
    // 320 × 160 is 20 × 10 = 200 tiles: 1 is 0.5%, 2 is over. Each tile is recoloured as above.
    const side = TILE_SIDE;
    const base = png(320, 160, 0, TYPES_BEFORE);
    const oneTile = withRect(base, [0, 0, side, side], TYPES_AFTER);
    const at = compareScreenshots(base, oneTile);
    expect(at).toMatchObject({
      diffPixels: 0,
      changedTiles: 1,
      tileRatio: MAX_DIFF_RATIO,
      passed: true,
    });
    const over = compareScreenshots(base, withRect(oneTile, [side, 0, side, side], TYPES_AFTER));
    expect(over).toMatchObject({ diffPixels: 0, changedTiles: 2, passed: false });
  });

  it('weighs a partial tile at the edge by its pixels: a 1-pixel row 6 darker does not fail', () => {
    // 320 × 161: the last row of tiles is 1 pixel high. 6 darker is 0.021 on each of its pixels,
    // but 16 of a tile's 256 move it 0.0013. The same change over a whole row of tiles fails.
    const before = png(320, 161);
    const row = compareScreenshots(before, withRect(before, [0, 160, 320, 1], [234, 234, 234]));
    expect(row.diffPixels).toBe(0);
    expect(row.changedTiles).toBe(0);
    expect(row.maxTileDistance).toBeLessThan(MAX_TILE_DISTANCE / 4);
    expect(row.passed).toBe(true);
    const tiles = compareScreenshots(before, withRect(before, [0, 144, 320, 16], [234, 234, 234]));
    expect(tiles).toMatchObject({ diffPixels: 0, changedTiles: 20, passed: false });
  });

  it('leaves out pixels that are not flat in the baseline (anti-aliased edges)', () => {
    // A 1-pixel line has only 2 neighbours of its colour (fewer than 3): darkening it under the
    // pixel threshold moves no tile.
    const before = withRect(png(64, 64, 0, [255, 255, 255]), [0, 20, 64, 1], [200, 200, 200]);
    const result = compareScreenshots(before, withRect(before, [0, 20, 64, 1], [190, 190, 190]));
    expect(result).toMatchObject({ diffPixels: 0, changedTiles: 0, maxTileDistance: 0 });
  });

  it('judges flatness in the baseline: a recolouring that comes out noisy still fails', () => {
    // The Types recolouring above, with each channel of the new colour 3 up or down: no pixel of
    // it is flat in the screenshot, but every one is in the baseline, so the 9 tiles change.
    const before = withRect(png(96, 96, 0, [255, 255, 255]), [16, 16, 48, 48], TYPES_BEFORE);
    const result = compareScreenshots(
      before,
      noisy(withRect(before, [16, 16, 48, 48], TYPES_AFTER)),
    );
    expect(result.diffPixels).toBe(0);
    expect(result.changedTiles).toBe(9);
    expect(result.passed).toBe(false);
  });

  it('counts a pixel on the image’s border as having one more neighbour of its colour', () => {
    // A 1-pixel line along the top edge has 2 neighbours of its colour and the border, 3: it is
    // flat (as pixelmatch judges it), so recolouring it moves each tile 16/256 of the 0.0703 of
    // each pixel. The same line inside the image moves no tile (above).
    const before = withRect(png(64, 64, 0, [255, 255, 255]), [0, 0, 64, 1], TYPES_BEFORE);
    const result = compareScreenshots(before, withRect(before, [0, 0, 64, 1], TYPES_AFTER));
    expect(result).toMatchObject({ diffPixels: 0, changedTiles: 0, passed: true });
    expect(result.maxTileDistance).toBeCloseTo((16 / 256) * 0.0703, 4);
  });

  it('fails a black area one step lighter: near black one step is beyond the limit', () => {
    // #000 → #010101 is 0.016 on every pixel. No committed baseline has a black area; the noise
    // pattern below leaves 0 as it is.
    const black = png(64, 64, 0, [0, 0, 0]);
    const lighter = compareScreenshots(black, png(64, 64, 0, [1, 1, 1]));
    expect(lighter.diffPixels).toBe(0);
    expect(lighter.maxTileDistance).toBeGreaterThan(MAX_TILE_DISTANCE);
    expect(lighter.passed).toBe(false);
    expect(compareScreenshots(black, noisy(black)).maxTileDistance).toBe(0);
  });

  it('counts a recoloured area pixel for pixel', () => {
    // A 20 × 20 red square on light grey: 400 pixels, every one well beyond the threshold.
    const base = png(100, 100);
    const result = compareScreenshots(base, withRect(base, [10, 10, 20, 20], [200, 30, 30]));
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

describe('the limits on the committed baselines', () => {
  // Both rules must let small rendering differences through on every baseline, though CI has shown
  // far smaller ones: one step of one channel on a few anti-aliased pixels (compare.ts).
  const baselines = committedBaselines();

  it('has baselines to check', () => {
    expect(baselines.length).toBeGreaterThan(0);
  });

  it.each(baselines)('ignores each channel 3 up or down in %s', (_name, baseline) => {
    const result = compareScreenshots(baseline, noisy(baseline));
    expect(result.diffPixels).toBe(0);
    expect(result.changedTiles).toBe(0);
    // At most 0.0025 on today's baselines: a sixth of the limit.
    expect(result.maxTileDistance).toBeLessThan(MAX_TILE_DISTANCE / 3);
  });

  it.each(baselines)('passes %s 2 lighter, and fails it 5 lighter', (_name, baseline) => {
    // Every channel of every pixel 2 up moves a tile at most 0.0087 on today's baselines; 5 up
    // moves more than 0.5% of the tiles of each at least 0.0175.
    const two = compareScreenshots(
      baseline,
      changed(baseline, (rgb) => rgb.map((v) => v + 2)),
    );
    expect(two.diffPixels).toBe(0);
    expect(two.maxTileDistance).toBeLessThan(MAX_TILE_DISTANCE);
    expect(two.passed).toBe(true);
    const five = compareScreenshots(
      baseline,
      changed(baseline, (rgb) => rgb.map((v) => v + 5)),
    );
    expect(five.diffPixels).toBe(0);
    expect(five.passed).toBe(false);
  });

  it.each(baselines)(
    'changes no tile of %s with text 15% stronger or weaker',
    (_name, baseline) => {
      // The anti-aliased pixels are not flat, so they move a tile at most 0.0024 on today's
      // baselines. pixelmatch counts more than 0.5% of the pixels of linux/chrome-toolbox-errors
      // 15% weaker: that is the pixel rule's.
      for (const factor of [0.15, -0.15]) {
        const result = compareScreenshots(baseline, contrasted(baseline, factor));
        expect(result.changedTiles).toBe(0);
        expect(result.maxTileDistance).toBeLessThan(MAX_TILE_DISTANCE / 3);
      }
    },
  );

  // The W2 review's recolouring (2f747cf), replayed on today's baselines: Types and Files back to
  // their colours before it, light and dark. Only the pixels of exactly the new colour change (not
  // the anti-aliased edges), so this is a smaller change than the real one, which also fails.
  const before2f747cf: [string, string][] = [
    ['#53622c', '#686800'],
    ['#637435', '#767700'],
    ['#3c625a', '#00736e'],
    ['#427269', '#00837e'],
  ];
  const showing = baselines.flatMap(([name, baseline]): [string, Buffer, Buffer][] => {
    let image = baseline;
    let pixels = 0;
    for (const [now, then] of before2f747cf) {
      const result = recoloured(image, now, then);
      image = result.png;
      pixels += result.pixels;
    }
    return pixels > 0 ? [[name, baseline, image]] : [];
  });

  it('has baselines that show the Types or Files colour', () => {
    // linux/chrome-toolbox-{types,files,errors}{,-dark} and windows/chrome-toolbox-{types,files,errors}
    expect(showing.length).toBeGreaterThanOrEqual(9);
  });

  it.each(showing)(
    'fails %s with the Types and Files colours before the W2 review',
    (_name, baseline, old) => {
      const result = compareScreenshots(baseline, old);
      expect(result.diffPixels).toBe(0);
      expect(result.passed).toBe(false);
    },
  );
});
