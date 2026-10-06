/**
 * Comparing a screenshot with its baseline (docs/spec/09-quality-and-delivery.md §9.2 "Visual
 * diff"): pixelmatch with a colour threshold of 0.1 counts the pixels that differ noticeably
 * (anti-aliased edges are not counted), and more than 0.5% of them fails.
 */
import pixelmatch from 'pixelmatch';
import { PNG } from 'pngjs';

/** pixelmatch's per-pixel colour threshold (0 to 1; smaller is stricter). */
export const PIXEL_THRESHOLD = 0.1;

/** The largest share of differing pixels that still passes. */
export const MAX_DIFF_RATIO = 0.005;

/** The largest screenshot side compared, in pixels (a guard against a corrupt or huge file). */
export const MAX_SIDE = 8_192;

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
  /** Whether `ratio` is at most {@link MAX_DIFF_RATIO}. */
  readonly passed: boolean;
  /** A PNG marking the differing pixels in red over a faded copy of the baseline. */
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
  });
  const ratio = diffPixels / (width * height);
  return {
    width,
    height,
    diffPixels,
    ratio,
    passed: ratio <= MAX_DIFF_RATIO,
    diffPng: PNG.sync.write(diff),
  };
}
