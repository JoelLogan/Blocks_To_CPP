/**
 * Visible placeholders for characters that do not show or that reorder text
 * (docs/spec/04-user-interface.md §4.3, docs/spec/08-security.md §8.4.6). The panels show
 * `⟨U+200B⟩` instead of such a character, so neither a project nor a program can hide text from the
 * person reading it ("Trojan Source"). The text itself never changes; this is display only, and
 * copying copies the real text.
 */

/**
 * Code points shown as placeholders above U+007F: C1 controls, every format character (Unicode
 * category Cf, which includes the bidi controls), the line and paragraph separators and the
 * noncharacters. The same table as `b2c_ir::text::is_invisible` (Unicode 14 plus the Cf additions of
 * Unicode 15–16), so the panels hide nothing that the generated C++ escapes. Sorted, inclusive.
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
 * Whether a code point is shown as a placeholder: a C0 control other than tab and line feed, DEL,
 * a lone UTF-16 surrogate, or one of the invisible characters of `b2c_ir::text::is_invisible`.
 */
export function isHiddenCodePoint(codePoint: number): boolean {
  if (codePoint < 0x80) {
    return (codePoint < 0x20 && codePoint !== 0x09 && codePoint !== 0x0a) || codePoint === 0x7f;
  }
  if (codePoint >= 0xd800 && codePoint <= 0xdfff) {
    return true;
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

/** The placeholder for one code point: `⟨U+200B⟩`, with at least four upper-case hex digits. */
export function placeholderFor(codePoint: number): string {
  return `⟨U+${codePoint.toString(16).toUpperCase().padStart(4, '0')}⟩`;
}

/** `\u{…}` for a regular-expression character class (the `u` flag). */
function classEscape(codePoint: number): string {
  return `\\u{${codePoint.toString(16)}}`;
}

/**
 * A regular expression (flags `gu`) that matches one hidden character at a time: the same set as
 * {@link isHiddenCodePoint}. Each call returns a new object, because global expressions carry state.
 */
export function hiddenCharacterPattern(): RegExp {
  const ranges = [
    '\\u{0}-\\u{8}',
    '\\u{b}-\\u{1f}',
    '\\u{7f}',
    // Lone surrogates; with the `u` flag a valid pair is one code point and does not match.
    '\\u{d800}-\\u{dfff}',
    ...INVISIBLE_RANGES.map(([from, to]) =>
      from === to ? classEscape(from) : `${classEscape(from)}-${classEscape(to)}`,
    ),
  ];
  return new RegExp(`[${ranges.join('')}]`, 'gu');
}

/**
 * `text` with every hidden character replaced by its placeholder, for display as plain text
 * (Problems, Build output, link confirmations). Text without hidden characters is returned as is.
 */
export function visibleInvisibles(text: string): string {
  return text.replace(hiddenCharacterPattern(), (match) =>
    placeholderFor(match.codePointAt(0) ?? 0),
  );
}
