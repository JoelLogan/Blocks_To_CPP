import { describe, expect, it } from 'vitest';

import {
  hiddenCharacterPattern,
  isHiddenCodePoint,
  placeholderFor,
  visibleInvisibles,
} from './invisibles';

describe('hidden characters', () => {
  it('matches controls, format characters, separators and noncharacters, but not text', () => {
    for (const code of [0x00, 0x07, 0x1b, 0x7f, 0x85, 0xad, 0x200b, 0x200e, 0x202e, 0x2066]) {
      expect(isHiddenCodePoint(code), code.toString(16)).toBe(true);
    }
    for (const code of [0x2028, 0x2029, 0xfeff, 0xfffe, 0xe0041, 0x10ffff, 0xd800, 0xdfff]) {
      expect(isHiddenCodePoint(code), code.toString(16)).toBe(true);
    }
    for (const code of [0x09, 0x0a, 0x20, 0x41, 0xe9, 0x2713, 0x65e5, 0x1f600, 0xfffd]) {
      expect(isHiddenCodePoint(code), code.toString(16)).toBe(false);
    }
  });

  it('builds a pattern that matches exactly the hidden code points', () => {
    const samples = [0x00, 0x09, 0x0a, 0x0b, 0x41, 0x7f, 0x80, 0xad, 0x200b, 0x2713, 0xe0020];
    samples.push(0xfeff, 0x1f600, 0x10fffe);
    for (const code of samples) {
      const pattern = hiddenCharacterPattern();
      expect(pattern.test(String.fromCodePoint(code)), code.toString(16)).toBe(
        isHiddenCodePoint(code),
      );
    }
    // A valid surrogate pair is one character; a lone surrogate is hidden.
    expect(hiddenCharacterPattern().test('😀')).toBe(false);
    expect(hiddenCharacterPattern().test('\ud800')).toBe(true);
  });

  it('replaces hidden characters by placeholders and keeps everything else', () => {
    expect(placeholderFor(0x200b)).toBe('⟨U+200B⟩');
    expect(placeholderFor(0x7)).toBe('⟨U+0007⟩');
    expect(placeholderFor(0xe0041)).toBe('⟨U+E0041⟩');
    expect(visibleInvisibles('a​b‮c')).toBe('a⟨U+200B⟩b⟨U+202E⟩c');
    expect(visibleInvisibles('tab\tand\nline héllo ✓ 😀')).toBe('tab\tand\nline héllo ✓ 😀');
    expect(visibleInvisibles('\u{e0041}')).toBe('⟨U+E0041⟩');
  });
});
