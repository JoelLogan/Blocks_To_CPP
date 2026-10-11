/** The contrast of text in a screenshot (./contrast.ts), on synthetic images. */
import { describe, expect, it } from 'vitest';

import { contrastRatio, type RgbaImage, textContrast } from './contrast';

const TOOLBAR = 0x1d2b4f;
const WHITE = 0xffffff;
const BLACK = 0x000000;

/** A `width`×`height` image of `background`, with `paint(x, y)` giving other pixels' colours. */
function image(
  width: number,
  height: number,
  background: number,
  paint: (x: number, y: number) => number | null = () => null,
): RgbaImage {
  const data = new Uint8Array(width * height * 4);
  for (let y = 0; y < height; y += 1) {
    for (let x = 0; x < width; x += 1) {
      const colour = paint(x, y) ?? background;
      const offset = (y * width + x) * 4;
      data[offset] = (colour >> 16) & 0xff;
      data[offset + 1] = (colour >> 8) & 0xff;
      data[offset + 2] = colour & 0xff;
      data[offset + 3] = 0xff;
    }
  }
  return { width, height, data };
}

/** Vertical strokes, as a label's glyphs give: every fourth column of the middle rows. */
function strokes(colour: number) {
  return (x: number, y: number) => (x % 4 === 0 && y >= 4 && y < 16 ? colour : null);
}

const WHOLE = { left: 0, top: 0, right: 40, bottom: 20 };

describe('contrastRatio', () => {
  it('follows WCAG: 21 for black on white, 1 for a colour on itself', () => {
    expect(contrastRatio(BLACK, WHITE)).toBeCloseTo(21, 5);
    expect(contrastRatio(WHITE, BLACK)).toBeCloseTo(21, 5);
    expect(contrastRatio(TOOLBAR, TOOLBAR)).toBe(1);
    // The toolbar's white text on its background.
    expect(contrastRatio(WHITE, TOOLBAR)).toBeGreaterThan(13);
  });
});

describe('textContrast', () => {
  it('finds white text on the toolbar’s blue', () => {
    const found = textContrast(image(40, 20, TOOLBAR, strokes(WHITE)), WHOLE);
    expect(found.background).toBe('#1d2b4f');
    expect(found.text).toBe('#ffffff');
    expect(found.ratio).toBeCloseTo(contrastRatio(WHITE, TOOLBAR), 5);
  });

  it('reports white text on a light face as unreadable', () => {
    const found = textContrast(image(40, 20, 0xf6f5f4, strokes(WHITE)), WHOLE);
    expect(found.ratio).toBeLessThan(1.2);
  });

  it('reads only the region', () => {
    // Black edges around a light face with white text: the edges are outside the region.
    const framed = image(40, 20, 0xf6f5f4, (x, y) =>
      x < 2 || x >= 38 || y < 2 || y >= 18 ? BLACK : x % 4 === 0 ? WHITE : null,
    );
    expect(textContrast(framed, WHOLE).ratio).toBeGreaterThan(15);
    expect(textContrast(framed, { left: 3, top: 3, right: 37, bottom: 17 }).ratio).toBeLessThan(
      1.2,
    );
  });

  it('lets a few stray pixels count for nothing', () => {
    const specks = image(40, 20, 0xf6f5f4, (x, y) => (x === 10 && y === 10 ? BLACK : null));
    expect(textContrast(specks, WHOLE).ratio).toBe(1);
  });

  it('refuses a region outside the image and a share outside (0, 1)', () => {
    const plain = image(4, 4, WHITE);
    expect(() => textContrast(plain, { left: 0, top: 0, right: 5, bottom: 4 })).toThrow(RangeError);
    expect(() => textContrast(plain, { left: 2, top: 0, right: 2, bottom: 4 })).toThrow(RangeError);
    expect(() => textContrast(plain, { left: 0, top: 0, right: 4, bottom: 4 }, 0)).toThrow(
      RangeError,
    );
    expect(() => textContrast(plain, { left: 0, top: 0, right: 4, bottom: 4 }, 1)).toThrow(
      RangeError,
    );
  });
});
