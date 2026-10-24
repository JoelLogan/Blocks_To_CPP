/**
 * The contrast of text in a screenshot, as WCAG 2.2 measures it (1.4.3): the flow tests check
 * what the webview really draws, where the style sheet's tokens say nothing about a control the
 * webview draws itself (docs/spec/04-user-interface.md §4.8). ./contrast.test.ts checks it on
 * synthetic images.
 */

/** An RGBA image, as pngjs decodes one: 4 bytes per pixel, row by row. */
export interface RgbaImage {
  readonly width: number;
  readonly height: number;
  readonly data: Uint8Array;
}

/** A rectangle of pixels: from `left`/`top` (included) to `right`/`bottom` (excluded). */
export interface PixelRegion {
  readonly left: number;
  readonly top: number;
  readonly right: number;
  readonly bottom: number;
}

/** What {@link textContrast} found. */
export interface TextContrast {
  /** The region's most common colour, as `#rrggbb`. */
  readonly background: string;
  /** The colour that stands out from it (see {@link textContrast}), as `#rrggbb`. */
  readonly text: string;
  /** Their contrast ratio, from 1 to 21. */
  readonly ratio: number;
}

/** An sRGB colour channel (0–255) in linear light. */
function linear(channel: number): number {
  const value = channel / 255;
  return value <= 0.04045 ? value / 12.92 : ((value + 0.055) / 1.055) ** 2.4;
}

/** The relative luminance of an sRGB colour. */
function luminance(colour: number): number {
  const r = (colour >> 16) & 0xff;
  const g = (colour >> 8) & 0xff;
  const b = colour & 0xff;
  return 0.2126 * linear(r) + 0.7152 * linear(g) + 0.0722 * linear(b);
}

/** The WCAG contrast ratio of two colours given as `0xrrggbb`. */
export function contrastRatio(first: number, second: number): number {
  const a = luminance(first);
  const b = luminance(second);
  return (Math.max(a, b) + 0.05) / (Math.min(a, b) + 0.05);
}

function hex(colour: number): string {
  return `#${colour.toString(16).padStart(6, '0')}`;
}

/**
 * The contrast of the text in `region` of `image`: the background is the region's most common
 * colour, and the text colour is the one that `share` of the region's pixels stand out from the
 * background by at least. Taking a share rather than the single most different pixel lets a few
 * stray pixels (an edge, a speck) count for nothing, while glyph strokes, which cover far more
 * than 2 % of a label's box, decide. Anti-aliased edges are lighter than the strokes, so the share
 * must stay small.
 *
 * Pixels are read as opaque (a screenshot is).
 *
 * @throws RangeError for a region outside the image or without pixels, or a share outside (0, 1).
 */
export function textContrast(image: RgbaImage, region: PixelRegion, share = 0.02): TextContrast {
  const { left, top, right, bottom } = region;
  if (
    left < 0 ||
    top < 0 ||
    right > image.width ||
    bottom > image.height ||
    left >= right ||
    top >= bottom
  ) {
    throw new RangeError(
      `The region ${String(left)},${String(top)}–${String(right)},${String(bottom)} is not inside the ${String(image.width)}×${String(image.height)} image`,
    );
  }
  if (!(share > 0 && share < 1)) {
    throw new RangeError(`The share ${String(share)} is not between 0 and 1`);
  }

  const colours: number[] = [];
  const counts = new Map<number, number>();
  for (let y = top; y < bottom; y += 1) {
    for (let x = left; x < right; x += 1) {
      const offset = (y * image.width + x) * 4;
      const colour =
        ((image.data[offset] ?? 0) << 16) |
        ((image.data[offset + 1] ?? 0) << 8) |
        (image.data[offset + 2] ?? 0);
      colours.push(colour);
      counts.set(colour, (counts.get(colour) ?? 0) + 1);
    }
  }

  let background = 0;
  let most = -1;
  for (const [colour, count] of counts) {
    if (count > most) {
      background = colour;
      most = count;
    }
  }

  const byContrast = colours
    .map((colour) => ({ colour, ratio: contrastRatio(colour, background) }))
    .sort((a, b) => b.ratio - a.ratio);
  const index = Math.min(byContrast.length - 1, Math.floor(byContrast.length * share));
  const found = byContrast[index] ?? { colour: background, ratio: 1 };
  return { background: hex(background), text: hex(found.colour), ratio: found.ratio };
}
