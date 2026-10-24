/** The project text rules and invisible-character placeholders (src/text/). */
import { describe, expect, it } from 'vitest';

import {
  MAX_FIELD_TEXT_BYTES,
  codePointCount,
  isInvisible,
  placeholderFor,
  sanitizeFieldText,
  truncateForDisplay,
  visibleInvisibles,
} from '../src';

/** A small deterministic generator (xorshift32), so the random cases are the same every run. */
function generator(seed: number): () => number {
  let state = seed >>> 0 || 1;
  return () => {
    state ^= state << 13;
    state ^= state >>> 17;
    state ^= state << 5;
    return (state >>> 0) / 0x1_0000_0000;
  };
}

/** UTF-8 length as the loader counts it (lone surrogates never reach it). */
function utf8Length(text: string): number {
  return new TextEncoder().encode(text).length;
}

describe('sanitizeFieldText', () => {
  it('accepts ordinary text, tabs, newlines and any script', () => {
    for (const text of [
      '',
      'Hello, world!',
      'a\tb\nc',
      'héllo ✓',
      '日本語',
      '😀 emoji',
      'x'.repeat(1000),
    ]) {
      expect(sanitizeFieldText(text)).toEqual({ ok: true });
    }
  });

  it('rejects NUL', () => {
    expect(sanitizeFieldText('a\u0000b')).toEqual({ ok: false, reason: 'nul' });
  });

  it('rejects every C0 control except tab and newline, and accepts DEL like the loader', () => {
    for (let code = 1; code < 0x20; code++) {
      const result = sanitizeFieldText(`x${String.fromCharCode(code)}y`);
      if (code === 0x09 || code === 0x0a) {
        expect(result.ok).toBe(true);
      } else {
        expect(result).toEqual({ ok: false, reason: 'control' });
      }
    }
    expect(sanitizeFieldText('\u007f').ok).toBe(true);
  });

  it('rejects every bidirectional control', () => {
    const bidi = [
      0x061c, 0x200e, 0x200f, 0x202a, 0x202b, 0x202c, 0x202d, 0x202e, 0x2066, 0x2067, 0x2068,
      0x2069,
    ];
    for (const code of bidi) {
      expect(sanitizeFieldText(`ok${String.fromCodePoint(code)}`)).toEqual({
        ok: false,
        reason: 'bidi',
      });
    }
    // Neighbours of the ranges are allowed (zero-width space is shown as a placeholder instead).
    for (const code of [0x061b, 0x200b, 0x200d, 0x2029, 0x202f, 0x2065, 0x206a]) {
      expect(sanitizeFieldText(String.fromCodePoint(code)).ok).toBe(true);
    }
  });

  it('rejects lone surrogates but accepts pairs', () => {
    expect(sanitizeFieldText('\ud83d')).toEqual({ ok: false, reason: 'surrogate' });
    expect(sanitizeFieldText('a\ude00b')).toEqual({ ok: false, reason: 'surrogate' });
    expect(sanitizeFieldText('\ude00\ud83d')).toEqual({ ok: false, reason: 'surrogate' });
    expect(sanitizeFieldText('😀').ok).toBe(true);
  });

  it('caps text at 64 KiB of UTF-8, counting multi-byte characters', () => {
    expect(MAX_FIELD_TEXT_BYTES).toBe(65_536);
    expect(sanitizeFieldText('a'.repeat(65_536)).ok).toBe(true);
    expect(sanitizeFieldText('a'.repeat(65_537))).toEqual({ ok: false, reason: 'tooLong' });
    // 21,845 three-byte characters are 65,535 bytes; one more ASCII byte fits, two do not.
    const threeByte = '€'.repeat(21_845);
    expect(sanitizeFieldText(`${threeByte}a`).ok).toBe(true);
    expect(sanitizeFieldText(`${threeByte}ab`)).toEqual({ ok: false, reason: 'tooLong' });
    // Four-byte characters: 16,384 fit exactly.
    expect(sanitizeFieldText('😀'.repeat(16_384)).ok).toBe(true);
    expect(sanitizeFieldText('😀'.repeat(16_384) + 'a')).toEqual({ ok: false, reason: 'tooLong' });
  });

  it('reports the first problem in text order, within each rule class', () => {
    expect(sanitizeFieldText('\u0001\u0000')).toEqual({ ok: false, reason: 'control' });
    expect(sanitizeFieldText('\u0000\u0001')).toEqual({ ok: false, reason: 'nul' });
  });

  it('agrees with the UTF-8 byte count on random text', () => {
    const random = generator(42);
    const alphabet = ['a', 'é', '€', '😀', ' ', '\n', '\t', '中'];
    for (let round = 0; round < 200; round++) {
      let text = '';
      const length = Math.floor(random() * 30_000);
      for (let index = 0; index < length; index++) {
        text += alphabet[Math.floor(random() * alphabet.length)] ?? 'a';
      }
      expect(sanitizeFieldText(text).ok).toBe(utf8Length(text) <= MAX_FIELD_TEXT_BYTES);
    }
  });
});

describe('codePointCount', () => {
  it('counts surrogate pairs once', () => {
    expect(codePointCount('')).toBe(0);
    expect(codePointCount('abc')).toBe(3);
    expect(codePointCount('😀')).toBe(1);
    expect(codePointCount('a😀b')).toBe(3);
    expect(codePointCount('\ud83d')).toBe(1);
  });
});

describe('visibleInvisibles', () => {
  it('shows format characters as placeholders and leaves visible text alone', () => {
    expect(visibleInvisibles('a\u200bb')).toBe('a⟨U+200B⟩b');
    expect(visibleInvisibles('\ufeffx')).toBe('⟨U+FEFF⟩x');
    expect(visibleInvisibles('soft\u00adhyphen')).toBe('soft⟨U+00AD⟩hyphen');
    expect(visibleInvisibles('rlo\u202e')).toBe('rlo⟨U+202E⟩');
    expect(visibleInvisibles('tag\u{e0041}')).toBe('tag⟨U+E0041⟩');
    expect(visibleInvisibles('héllo ✓ 😀')).toBe('héllo ✓ 😀');
    const plain = 'nothing to replace';
    expect(visibleInvisibles(plain)).toBe(plain);
  });

  it('shows controls, DEL, C1 controls and lone surrogates', () => {
    expect(visibleInvisibles('\u0001\u007f\u0085')).toBe('⟨U+0001⟩⟨U+007F⟩⟨U+0085⟩');
    expect(visibleInvisibles('x\ud800y')).toBe('x⟨U+D800⟩y');
  });

  it('keeps tabs and newlines unless lineBreaks is set', () => {
    expect(visibleInvisibles('a\tb\nc')).toBe('a\tb\nc');
    expect(visibleInvisibles('a\tb\nc', { lineBreaks: true })).toBe('a⟨U+0009⟩b⟨U+000A⟩c');
  });

  it('matches the table of b2c_ir::text::is_invisible at its edges', () => {
    expect(isInvisible(0x7f)).toBe(false);
    expect(isInvisible(0x80)).toBe(true);
    expect(isInvisible(0x9f)).toBe(true);
    expect(isInvisible(0xa0)).toBe(false);
    expect(isInvisible(0x200a)).toBe(false);
    expect(isInvisible(0x200b)).toBe(true);
    expect(isInvisible(0xfffd)).toBe(false);
    expect(isInvisible(0xfffe)).toBe(true);
    expect(isInvisible(0x10ffff)).toBe(true);
    expect(isInvisible(0x41)).toBe(false);
  });

  it('writes at least four hex digits', () => {
    expect(placeholderFor(0x9)).toBe('⟨U+0009⟩');
    expect(placeholderFor(0x1d173)).toBe('⟨U+1D173⟩');
  });
});

describe('truncateForDisplay', () => {
  it('cuts long text with an ellipsis and never splits a surrogate pair', () => {
    expect(truncateForDisplay('short', 10)).toBe('short');
    expect(truncateForDisplay('exactly10!', 10)).toBe('exactly10!');
    expect(truncateForDisplay('abcdefghijk', 5)).toBe('abcd…');
    expect(truncateForDisplay('😀😀😀😀😀😀', 4)).toBe('😀😀😀…');
    expect(truncateForDisplay('😀😀', 2)).toBe('😀😀');
    expect(truncateForDisplay('abc', 0)).toBe('abc');
  });
});
