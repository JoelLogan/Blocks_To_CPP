import { describe, expect, it } from 'vitest';

import vector from '../../../../../crates/b2c-model/tests/fixtures/semver-precedence.json';
import { isNewerVersion } from './version';

/** The compiler core's test vector (crates/b2c-model/src/version.rs reads it too). */
const VECTOR: {
  readonly ordered: readonly string[];
  readonly equal: readonly (readonly [string, string])[];
  readonly newer: readonly (readonly [string, string])[];
  readonly notSemver: readonly string[];
  readonly semver: readonly string[];
} = vector as never;

describe('isNewerVersion', () => {
  it('follows SemVer precedence, as the compiler core does', () => {
    expect(VECTOR.ordered.length).toBeGreaterThanOrEqual(13);
    VECTOR.ordered.forEach((a, i) => {
      VECTOR.ordered.forEach((b, j) => {
        expect(isNewerVersion(a, b), `${a} newer than ${b}?`).toBe(i > j);
      });
    });
  });

  it('ignores build metadata', () => {
    for (const [a, b] of VECTOR.equal) {
      expect(isNewerVersion(a, b), `${a} newer than ${b}?`).toBe(false);
    }
    for (const [a, b] of VECTOR.newer) {
      expect(isNewerVersion(a, b), `${a} newer than ${b}?`).toBe(true);
    }
  });

  it('never counts text that is not a SemVer version as newer', () => {
    for (const text of VECTOR.notSemver) {
      expect(isNewerVersion(text, '0.1.0'), JSON.stringify(text)).toBe(false);
      expect(isNewerVersion('99.0.0', text), JSON.stringify(text)).toBe(false);
    }
    for (const text of VECTOR.semver) {
      expect(isNewerVersion(text, '0.1.0'), text).toBe(true);
    }
    expect(isNewerVersion('1.0.0', 'not a version')).toBe(false);
  });
});
