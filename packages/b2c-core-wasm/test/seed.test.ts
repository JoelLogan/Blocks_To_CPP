// Seeds for pastePrepare: 256 bits from a cryptographic random source, as 64 hex digits.

import { describe, expect, it, vi } from 'vitest';

import { randomSeedHex, type RandomSource, SEED_BYTES } from '../src/seed';

describe('randomSeedHex', () => {
  it('gives 64 lower-case hex digits of fresh randomness', () => {
    const seeds = new Set<string>();
    for (let i = 0; i < 100; i += 1) {
      const seed = randomSeedHex();
      expect(seed).toMatch(/^[0-9a-f]{64}$/);
      seeds.add(seed);
    }
    expect(seeds.size).toBe(100);
  });

  it('hex-encodes every byte of the source, two digits each', () => {
    let calls = 0;
    const source: RandomSource = {
      getRandomValues<T extends ArrayBufferView>(array: T): T {
        calls += 1;
        const bytes = new Uint8Array(array.buffer, array.byteOffset, array.byteLength);
        bytes.forEach((_, index) => {
          bytes[index] = index * 8;
        });
        return array;
      },
    };
    const seed = randomSeedHex(source);
    expect(seed.slice(0, 8)).toBe('00081018');
    expect(seed.slice(-2)).toBe('f8');
    expect(seed).toHaveLength(SEED_BYTES * 2);
    expect(calls).toBe(1);
  });

  it('uses the global crypto by default', () => {
    const spy = vi.spyOn(globalThis.crypto, 'getRandomValues');
    randomSeedHex();
    expect(spy).toHaveBeenCalledOnce();
  });

  it('refuses to run without a cryptographic source', () => {
    expect(() => randomSeedHex({} as RandomSource)).toThrow(/crypto.getRandomValues/);
  });
});
