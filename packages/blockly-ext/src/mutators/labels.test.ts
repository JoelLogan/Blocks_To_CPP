/** How names are shown in labels. */
import { describe, expect, it } from 'vitest';

import { displayName, MAX_LABEL_CHARS } from './labels';

describe('displayName', () => {
  it('shows valid identifiers unchanged', () => {
    expect(displayName('score')).toBe('score');
    expect(displayName('_x2')).toBe('_x2');
    expect(displayName('a'.repeat(64))).toBe('a'.repeat(64));
  });

  it('shows other printable ASCII as it is', () => {
    expect(displayName('two words')).toBe('two words');
    expect(displayName('2x')).toBe('2x');
  });

  it('shows every other character as its code point', () => {
    expect(displayName('café')).toBe('caf⟨U+00E9⟩');
    expect(displayName('a​b')).toBe('a⟨U+200B⟩b');
    expect(displayName('‮gnp.exe')).toBe('⟨U+202E⟩gnp.exe');
    expect(displayName('x\ty')).toBe('x⟨U+0009⟩y');
    expect(displayName('😀')).toBe('⟨U+1F600⟩');
  });

  it('cuts long names', () => {
    const shown = displayName(`${'b'.repeat(70)} `);
    expect(shown).toBe(`${'b'.repeat(MAX_LABEL_CHARS)}…`);
  });
});
