import { Buffer } from 'node:buffer';
import { randomBytes } from 'node:crypto';

import { describe, expect, it } from 'vitest';

import { decodeBase64 } from '../src/base64';

const text = (bytes: Uint8Array) => Buffer.from(bytes).toString('latin1');

describe('decodeBase64', () => {
  it('decodes the RFC 4648 test vectors', () => {
    const vectors: [string, string][] = [
      ['', ''],
      ['Zg==', 'f'],
      ['Zm8=', 'fo'],
      ['Zm9v', 'foo'],
      ['Zm9vYg==', 'foob'],
      ['Zm9vYmE=', 'fooba'],
      ['Zm9vYmFy', 'foobar'],
    ];
    for (const [encoded, decoded] of vectors) {
      expect(text(decodeBase64(encoded))).toBe(decoded);
    }
  });

  it('round-trips random bytes of every length remainder', () => {
    for (let length = 0; length < 200; length++) {
      const bytes = new Uint8Array(randomBytes(length));
      const decoded = decodeBase64(Buffer.from(bytes).toString('base64'));
      expect(decoded).toEqual(bytes);
      expect(decoded.buffer).toBeInstanceOf(ArrayBuffer);
    }
  });

  it('decodes every byte value', () => {
    const all = Uint8Array.from({ length: 256 }, (_, i) => i);
    expect(decodeBase64(Buffer.from(all).toString('base64'))).toEqual(all);
  });

  it.each([
    ['a length that is not a multiple of 4', 'Zm9'],
    ['a character outside the alphabet', 'Zm9-'],
    ['URL-safe characters', 'Zm_v'],
    ['white space', 'Zm9v Zg=='],
    ['a line break', 'Zm9v\nZg=='],
    ['padding in the middle', 'Zg==Zm9v'],
    ['three padding characters', 'Z==='],
    ['only padding', '===='],
    ['a non-ASCII character', 'Zm9é'],
    ['non-zero bits before two padding characters', 'Zh=='],
    ['non-zero bits before one padding character', 'Zm9='],
  ])('refuses %s', (_, input) => {
    expect(() => decodeBase64(input)).toThrow(TypeError);
  });
});
