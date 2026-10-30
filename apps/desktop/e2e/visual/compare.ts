/**
 * Comparing a screenshot with its baseline (docs/spec/09-quality-and-delivery.md §9.2 "Visual
 * diff"), by two rules; more than 0.5% failing by either fails:
 *
 * - pixels: pixelmatch with a colour threshold of 0.1 counts the pixels that differ noticeably
 *   (anti-aliased edges are not counted), which finds what moved, appeared or went;
 * - tiles: a 16×16 tile in which pixelmatch counted no pixel has changed when the mean colour of
 *   its pixels that are flat in the baseline moves more than 0.015, which finds a colour change
 *   too slight for 0.1 on any one pixel. The W2 review's new Types and Files colours (2f747cf) moved every pixel of those blocks
 *   by 0.060 to 0.084, and pixelmatch 8 counted none of them on scenes where a third of the image
 *   changed colour.
 *
 * Both distances are pixelmatch 8's perceptual colour distance (OKLab HyAB with Ottosson's
 * toe-corrected lightness, where 1 is black against white). A pixel is flat when 3 or more of its
 * 8 neighbours have its colour (pixelmatch's test before it calls a pixel anti-aliased); edges and
 * the anti-aliased pixels of text are left out, so a bolder or lighter font does not change a
 * tile, while a recoloured area does. Each tile's sum is divided by 256, the pixels of a whole
 * tile, also for the smaller tiles at the right and bottom edges, so a thin edge row counts no
 * more than its share. The tile rule leaves strong changes to the pixel rule: a tile in which
 * pixelmatch counted a pixel is not measured.
 *
 * Screenshots of an unchanged scene are the same pixel for pixel from run to run: in 9 Desktop
 * runs on each system (24 to 28 October 2026, two Ubuntu runner images), only Windows'
 * chrome-console-scrolled differed, in at most 36 anti-aliased corner pixels by 1 in a channel.
 * Measured on the 59 committed baselines (30 October 2026), and on the W2 review's 9 recoloured
 * scenes before and after it:
 *
 * - {@link MAX_TILE_DISTANCE} lets through each channel ±3 on every pixel (in a pattern whose mean
 *   is 0: at most 0.0025 on a tile, 6 times under the limit; clipped at 0 and 255, 0.0053), every
 *   pixel 2 lighter or darker (at most 0.0089, 1.7 times under) and the anti-aliased pixels of
 *   text 15% stronger or weaker (at most 0.0024 on a tile, 6 times under; pixelmatch counts more
 *   than 0.5% of the pixels of one scene, linux/chrome-toolbox-errors, 15% weaker, as before).
 *   Every pixel 3 lighter still passes (0.0130); 4 lighter fails 49 of the 59 baselines and 5
 *   lighter all of them (the deciding tile moving at least 0.0175, 1.17 times the limit). The W2
 *   recolouring fails all 9 of its scenes: the tile that decides the least changed one moved
 *   0.030, 2.0 times the limit.
 * - Near black, one step of one channel is already a large distance (#000 to #010101 is 0.016):
 *   a black area that comes out a step lighter fails. No committed baseline has a black area.
 * - {@link MAX_DIFF_RATIO} of the tiles is less than one tile on the smallest scenes (the toolbar,
 *   150 tiles): one recoloured tile fails them. A recolouring of about 0.5% of a scene fails or
 *   passes depending on how it falls on the tiles (a 24×24 block on a 420-tile scene passes at 6
 *   of its 256 offsets, a 32×16 one at 53).
 */
import pixelmatch from 'pixelmatch';
import { PNG } from 'pngjs';

/** pixelmatch's per-pixel colour threshold: an OKLab HyAB distance, 0 to 1 (smaller is stricter). */
export const PIXEL_THRESHOLD = 0.1;

/** The largest share of differing pixels, and of changed tiles, that still passes. */
export const MAX_DIFF_RATIO = 0.005;

/** The side of the square tiles whose mean colours are compared, in pixels. */
export const TILE_SIDE = 16;

/**
 * The largest distance (OKLab HyAB, as {@link PIXEL_THRESHOLD}) between a tile's mean colours in
 * the baseline and the screenshot that leaves the tile unchanged; the margins are in the comment
 * at the top.
 */
export const MAX_TILE_DISTANCE = 0.015;

/** The largest screenshot side compared, in pixels (a guard against a corrupt or huge file). */
export const MAX_SIDE = 8_192;

/** The colour pixelmatch marks a counted pixel with in the diff image (its default). */
const DIFF_COLOUR: [number, number, number] = [255, 0, 0];

/** A screenshot or baseline that cannot be compared. */
export class VisualCompareError extends Error {
  override readonly name = 'VisualCompareError';
}

/** What a comparison found. */
export interface VisualResult {
  readonly width: number;
  readonly height: number;
  /** Pixels that differ beyond {@link PIXEL_THRESHOLD}. */
  readonly diffPixels: number;
  /** `diffPixels` as a share of all pixels. */
  readonly ratio: number;
  /**
   * Tiles in which no pixel differs beyond {@link PIXEL_THRESHOLD} but whose mean colour moved
   * more than {@link MAX_TILE_DISTANCE}.
   */
  readonly changedTiles: number;
  /** `changedTiles` as a share of all tiles. */
  readonly tileRatio: number;
  /**
   * The largest distance between a tile's mean colours, of the tiles in which no pixel differs
   * beyond {@link PIXEL_THRESHOLD} (0 when there are none, or the images are the same).
   */
  readonly maxTileDistance: number;
  /** Whether `ratio` and `tileRatio` are both at most {@link MAX_DIFF_RATIO}. */
  readonly passed: boolean;
  /**
   * A PNG marking the differing pixels in red over a faded copy of the baseline, and the changed
   * tiles with a blue outline (a changed tile has no red pixel to hide).
   */
  readonly diffPng: Buffer;
}

/** Decodes a PNG, checking its size. */
export function decodePng(bytes: Buffer, what: string): PNG {
  let image: PNG;
  try {
    image = PNG.sync.read(bytes);
  } catch (error: unknown) {
    throw new VisualCompareError(
      `The ${what} is not a PNG image: ${error instanceof Error ? error.message : String(error)}`,
    );
  }
  if (image.width < 1 || image.height < 1 || image.width > MAX_SIDE || image.height > MAX_SIDE) {
    throw new VisualCompareError(
      `The ${what} is ${String(image.width)}×${String(image.height)} pixels; each side must be 1 to ${String(MAX_SIDE)}`,
    );
  }
  return image;
}

/** The size of a PNG image, as `width×height`. */
export function pngSize(bytes: Buffer): string {
  const image = decodePng(bytes, 'image');
  return `${String(image.width)}×${String(image.height)}`;
}

/** An sRGB channel value (0 to 255, fractional after blending) as linear light. */
function linear(value: number): number {
  const c = value / 255;
  return c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
}

/** Ottosson's toe for OKLab lightness, as pixelmatch 8 applies it. */
const TOE_K1 = 0.206;
const TOE_K2 = 0.03;
const TOE_K3 = (1 + TOE_K1) / (1 + TOE_K2);

/**
 * OKLab [Lr, a, b] of an sRGB colour with alpha (0 to 255). A semi-transparent colour is blended
 * over white in sRGB, as pixelmatch does without its checkerboard; screenshots are opaque.
 */
function oklab(r: number, g: number, b: number, alpha: number): readonly [number, number, number] {
  const over = (value: number) => linear((value * alpha + 255 * (255 - alpha)) / 255);
  const [red, green, blue] = [over(r), over(g), over(b)];
  const l = Math.cbrt(0.4122214708 * red + 0.5363325363 * green + 0.0514459929 * blue);
  const m = Math.cbrt(0.2119034982 * red + 0.6806995451 * green + 0.1073969566 * blue);
  const s = Math.cbrt(0.0883024619 * red + 0.2817188376 * green + 0.6299787005 * blue);
  const lightness = 0.2104542553 * l + 0.793617785 * m - 0.0040720468 * s;
  const x = TOE_K3 * lightness - TOE_K1;
  return [
    0.5 * (x + Math.sqrt(x * x + 4 * TOE_K2 * TOE_K3 * lightness)),
    1.9779984951 * l - 2.428592205 * m + 0.4505937099 * s,
    0.0259040371 * l + 0.7827717662 * m - 0.808675766 * s,
  ];
}

/**
 * Whether 3 or more of the 8 neighbours of pixel (`x`, `y`) have its colour, a pixel on the
 * image's border counting one more: pixelmatch's sibling test.
 */
function flat(pixels: Uint32Array, x: number, y: number, width: number, height: number): boolean {
  const value = pixels[y * width + x];
  let same = x === 0 || y === 0 || x === width - 1 || y === height - 1 ? 1 : 0;
  for (let ny = Math.max(y - 1, 0); ny <= Math.min(y + 1, height - 1); ny += 1) {
    for (let nx = Math.max(x - 1, 0); nx <= Math.min(x + 1, width - 1); nx += 1) {
      if ((nx !== x || ny !== y) && pixels[ny * width + nx] === value) {
        same += 1;
        if (same > 2) return true;
      }
    }
  }
  return false;
}

/**
 * The distance between each tile's mean colours in `before` and `after` (row by row), over the
 * pixels flat in `before` and divided by a whole tile's pixels; `NaN` for a tile in which
 * pixelmatch counted a pixel (marked in `diff`).
 */
function tileDistances(before: PNG, after: PNG, diff: PNG, columns: number): Float64Array {
  const { width, height } = before;
  const rows = Math.ceil(height / TILE_SIDE);
  const a32 = new Uint32Array(before.data.buffer, before.data.byteOffset, width * height);
  const b32 = new Uint32Array(after.data.buffer, after.data.byteOffset, width * height);
  const sums = new Float64Array(columns * rows * 3);
  const counted = new Uint8Array(columns * rows);
  // A screenshot has a few thousand colours; each is converted once.
  const colours = new Map<number, readonly [number, number, number]>();
  const colour = (data: Buffer, at: number) => {
    const key = data.readUInt32BE(at);
    let found = colours.get(key);
    if (found === undefined) {
      found = oklab(data[at] ?? 0, data[at + 1] ?? 0, data[at + 2] ?? 0, data[at + 3] ?? 0);
      colours.set(key, found);
    }
    return found;
  };
  for (let y = 0; y < height; y += 1) {
    const rowStart = Math.floor(y / TILE_SIDE) * columns;
    for (let x = 0; x < width; x += 1) {
      const index = y * width + x;
      if (a32[index] === b32[index]) continue;
      const tile = rowStart + Math.floor(x / TILE_SIDE);
      const at = index * 4;
      if (DIFF_COLOUR.every((value, channel) => diff.data[at + channel] === value)) {
        counted[tile] = 1;
      } else if (flat(a32, x, y, width, height)) {
        const [p, q] = [colour(before.data, at), colour(after.data, at)];
        for (let channel = 0; channel < 3; channel += 1) {
          sums[tile * 3 + channel] =
            (sums[tile * 3 + channel] ?? 0) + (q[channel] ?? 0) - (p[channel] ?? 0);
        }
      }
    }
  }
  const area = TILE_SIDE * TILE_SIDE;
  return Float64Array.from({ length: columns * rows }, (_, tile) =>
    counted[tile] === 1
      ? Number.NaN
      : Math.abs((sums[tile * 3] ?? 0) / area) +
        Math.hypot((sums[tile * 3 + 1] ?? 0) / area, (sums[tile * 3 + 2] ?? 0) / area),
  );
}

/** Outlines tile `tile` (of `columns` a row) of `image` in blue. */
function outlineTile(image: PNG, tile: number, columns: number): void {
  const left = (tile % columns) * TILE_SIDE;
  const top = Math.floor(tile / columns) * TILE_SIDE;
  const right = Math.min(left + TILE_SIDE, image.width) - 1;
  const bottom = Math.min(top + TILE_SIDE, image.height) - 1;
  for (let y = top; y <= bottom; y += 1) {
    for (let x = left; x <= right; x += 1) {
      if (x === left || x === right || y === top || y === bottom) {
        image.data.set([0, 64, 255, 255], (y * image.width + x) * 4);
      }
    }
  }
}

/**
 * Compares `candidate` with `baseline` (both PNG files).
 *
 * @throws VisualCompareError when either is not a PNG within {@link MAX_SIDE}, or their sizes
 *   differ (a different window size, zoom or scaling: no pixel comparison is meaningful then).
 */
export function compareScreenshots(baseline: Buffer, candidate: Buffer): VisualResult {
  const expected = decodePng(baseline, 'baseline');
  const actual = decodePng(candidate, 'screenshot');
  if (expected.width !== actual.width || expected.height !== actual.height) {
    throw new VisualCompareError(
      `The screenshot is ${String(actual.width)}×${String(actual.height)} pixels but the baseline is ` +
        `${String(expected.width)}×${String(expected.height)}: the window size, zoom or display scaling differs`,
    );
  }
  const { width, height } = expected;
  const diff = new PNG({ width, height });
  const diffPixels = pixelmatch(expected.data, actual.data, diff.data, width, height, {
    threshold: PIXEL_THRESHOLD,
    diffColor: DIFF_COLOUR,
  });
  const ratio = diffPixels / (width * height);
  const columns = Math.ceil(width / TILE_SIDE);
  const tiles = columns * Math.ceil(height / TILE_SIDE);
  let changedTiles = 0;
  let maxTileDistance = 0;
  if (!expected.data.equals(actual.data)) {
    const distances = tileDistances(expected, actual, diff, columns);
    distances.forEach((distance, tile) => {
      // NaN (a tile with a counted pixel) is neither larger nor over the limit.
      maxTileDistance = Math.max(maxTileDistance, Number.isNaN(distance) ? 0 : distance);
      if (distance > MAX_TILE_DISTANCE) {
        changedTiles += 1;
        outlineTile(diff, tile, columns);
      }
    });
  }
  const tileRatio = changedTiles / tiles;
  return {
    width,
    height,
    diffPixels,
    ratio,
    changedTiles,
    tileRatio,
    maxTileDistance,
    passed: ratio <= MAX_DIFF_RATIO && tileRatio <= MAX_DIFF_RATIO,
    diffPng: PNG.sync.write(diff),
  };
}
