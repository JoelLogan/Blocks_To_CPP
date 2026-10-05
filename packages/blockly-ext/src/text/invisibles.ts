/**
 * Visible placeholders for characters that do not show, or that change how text looks
 * (docs/spec/08-security.md §8.4.6, M2 decision "Hidden characters in block text fields"). Block
 * fields and tooltips show `⟨U+200B⟩` instead of an invisible character, so a project cannot hide
 * text from the person reading its blocks. The stored text never changes; this is display only.
 */

/**
 * Code points shown as placeholders besides the controls: C1 controls, every format character
 * (Unicode category Cf, which includes the bidi controls), the line and paragraph separators and the
 * noncharacters. The same table as `b2c_ir::text::is_invisible` (Unicode 14 plus the Cf additions
 * of Unicode 15–16), so the blocks hide nothing that the generated C++ escapes. Sorted, inclusive.
 */
const INVISIBLE_RANGES: readonly (readonly [number, number])[] = [
  [0x0080, 0x009f],
  [0x00ad, 0x00ad],
  [0x0600, 0x0605],
  [0x061c, 0x061c],
  [0x06dd, 0x06dd],
  [0x070f, 0x070f],
  [0x0890, 0x0891],
  [0x08e2, 0x08e2],
  [0x180e, 0x180e],
  [0x200b, 0x200f],
  [0x2028, 0x202e],
  [0x2060, 0x2064],
  [0x2066, 0x206f],
  [0xfdd0, 0xfdef],
  [0xfeff, 0xfeff],
  [0xfff9, 0xfffb],
  [0xfffe, 0xffff],
  [0x110bd, 0x110bd],
  [0x110cd, 0x110cd],
  [0x13430, 0x1343f],
  [0x1bca0, 0x1bca3],
  [0x1d173, 0x1d17a],
  [0x1fffe, 0x1ffff],
  [0x2fffe, 0x2ffff],
  [0x3fffe, 0x3ffff],
  [0x4fffe, 0x4ffff],
  [0x5fffe, 0x5ffff],
  [0x6fffe, 0x6ffff],
  [0x7fffe, 0x7ffff],
  [0x8fffe, 0x8ffff],
  [0x9fffe, 0x9ffff],
  [0xafffe, 0xaffff],
  [0xbfffe, 0xbffff],
  [0xcfffe, 0xcffff],
  [0xdfffe, 0xdffff],
  [0xe0001, 0xe0001],
  [0xe0020, 0xe007f],
  [0xefffe, 0xeffff],
  [0xffffe, 0xfffff],
  [0x10fffe, 0x10ffff],
];

/**
 * Whether a code point is invisible or reorders text: a C1 control, a format character (Cf), a line
 * or paragraph separator, or a noncharacter (`b2c_ir::text::is_invisible`).
 */
export function isInvisible(codePoint: number): boolean {
  if (codePoint < 0x80) {
    return false;
  }
  let low = 0;
  let high = INVISIBLE_RANGES.length - 1;
  while (low <= high) {
    const middle = (low + high) >>> 1;
    const range = INVISIBLE_RANGES[middle];
    if (range === undefined) {
      return false;
    }
    if (range[1] < codePoint) {
      low = middle + 1;
    } else if (range[0] > codePoint) {
      high = middle - 1;
    } else {
      return true;
    }
  }
  return false;
}

/** Options for {@link visibleInvisibles}. */
export interface VisibleInvisiblesOptions {
  /**
   * Also show tab and newline as placeholders. Use it for one-line displays such as block fields,
   * where a line break would otherwise look like a space. Default false: multi-line text (a
   * tooltip) keeps them.
   */
  readonly lineBreaks?: boolean;
}

/** The placeholder for one code point: `⟨U+XXXX⟩`, at least four upper-case hex digits. */
export function placeholderFor(codePoint: number): string {
  return `⟨U+${codePoint.toString(16).toUpperCase().padStart(4, '0')}⟩`;
}

/**
 * `text` with every invisible character replaced by a visible placeholder such as `⟨U+200B⟩`:
 * C0 controls (tab and newline only with `lineBreaks`), DEL, everything {@link isInvisible} reports,
 * and lone surrogates. Everything else is unchanged.
 */
export function visibleInvisibles(text: string, options: VisibleInvisiblesOptions = {}): string {
  const lineBreaks = options.lineBreaks === true;
  let out = '';
  let start = 0;
  for (let index = 0; index < text.length; index++) {
    const unit = text.charCodeAt(index);
    let codePoint = unit;
    let width = 1;
    if (unit >= 0xd800 && unit <= 0xdbff && index + 1 < text.length) {
      const next = text.charCodeAt(index + 1);
      if (next >= 0xdc00 && next <= 0xdfff) {
        codePoint = (unit - 0xd800) * 0x400 + (next - 0xdc00) + 0x10000;
        width = 2;
      }
    }
    if (shouldReplace(codePoint, width, lineBreaks)) {
      out += text.slice(start, index) + placeholderFor(codePoint);
      start = index + width;
    }
    index += width - 1;
  }
  return start === 0 ? text : out + text.slice(start);
}

function shouldReplace(codePoint: number, width: number, lineBreaks: boolean): boolean {
  if (codePoint === 0x09 || codePoint === 0x0a) {
    return lineBreaks;
  }
  if (codePoint < 0x20 || codePoint === 0x7f) {
    return true;
  }
  // A surrogate that did not pair up (width 1 in the surrogate range) is not a character.
  if (width === 1 && codePoint >= 0xd800 && codePoint <= 0xdfff) {
    return true;
  }
  return isInvisible(codePoint);
}

/**
 * Shortens display text to at most `max` code points, ending with `…` when it was cut. Used for
 * labels that show user text, so a 64 KiB string cannot make a block thousands of pixels wide. The
 * cut never splits a surrogate pair.
 */
export function truncateForDisplay(text: string, max: number): string {
  if (max < 1 || text.length <= max) {
    return text;
  }
  let units = 0;
  let points = 0;
  while (units < text.length && points < max - 1) {
    const unit = text.charCodeAt(units);
    units += unit >= 0xd800 && unit <= 0xdbff && units + 1 < text.length ? 2 : 1;
    points++;
  }
  if (units >= text.length) {
    return text;
  }
  // Is the rest exactly one code point? Then the text fits without cutting.
  const lead = text.charCodeAt(units);
  const restWidth = lead >= 0xd800 && lead <= 0xdbff && units + 1 < text.length ? 2 : 1;
  if (units + restWidth === text.length) {
    return text;
  }
  return `${text.slice(0, units)}…`;
}
