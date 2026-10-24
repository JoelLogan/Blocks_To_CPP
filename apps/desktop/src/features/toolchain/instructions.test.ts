import { describe, expect, it } from 'vitest';

import spec from '../../../../../docs/spec/04-user-interface.md?raw';
import {
  INSTALL_COMMANDS,
  instructionPlatforms,
  LINUX_PACKAGE_MANAGERS,
  linuxCommands,
  linuxPackageManager,
  MAX_DISTRO_ID_LENGTH,
  MAX_ID_LIKE_ENTRIES,
} from './instructions';

/** The spec's §4.6 with every run of whitespace made one space (lines wrap inside code spans). */
function section46(): string {
  const start = spec.indexOf('## 4.6 Toolchain setup experience');
  const end = spec.indexOf('## 4.7', start);
  expect(start).toBeGreaterThanOrEqual(0);
  expect(end).toBeGreaterThan(start);
  return spec.slice(start, end).replace(/\s+/g, ' ');
}

describe('the install commands', () => {
  it('match docs/spec/04-user-interface.md §4.6 word for word', () => {
    const text = section46();
    for (const command of Object.values(INSTALL_COMMANDS)) {
      expect(text).toContain(`\`${command}\``);
    }
    expect(text).toContain('*MSYS2 UCRT64* shell');
    expect(text).toContain('*I installed it → Rescan*');
    expect(text).toContain('*Choose g++ manually…*');
  });
});

describe('linuxPackageManager', () => {
  it.each([
    ['debian', 'apt'],
    ['ubuntu', 'apt'],
    ['fedora', 'dnf'],
    ['rhel', 'dnf'],
    ['centos', 'dnf'],
    ['arch', 'pacman'],
  ] as const)('chooses by ID: %s gives %s', (id, manager) => {
    expect(linuxPackageManager({ id, idLike: [] })).toBe(manager);
  });

  it('chooses by ID_LIKE when the ID is not known, in its order', () => {
    expect(linuxPackageManager({ id: 'linuxmint', idLike: ['ubuntu', 'debian'] })).toBe('apt');
    expect(linuxPackageManager({ id: 'rocky', idLike: ['rhel', 'centos', 'fedora'] })).toBe('dnf');
    expect(linuxPackageManager({ id: 'manjaro', idLike: ['arch'] })).toBe('pacman');
    expect(linuxPackageManager({ id: 'odd', idLike: ['unknown', 'fedora', 'debian'] })).toBe('dnf');
  });

  it('prefers the ID over ID_LIKE', () => {
    expect(linuxPackageManager({ id: 'fedora', idLike: ['debian'] })).toBe('dnf');
  });

  it('ignores letter case', () => {
    expect(linuxPackageManager({ id: 'Ubuntu', idLike: [] })).toBe('apt');
  });

  it('gives null for an unknown or missing distribution', () => {
    expect(linuxPackageManager(null)).toBeNull();
    expect(linuxPackageManager({ id: 'gentoo', idLike: [] })).toBeNull();
    expect(linuxPackageManager({ id: '', idLike: ['opensuse'] })).toBeNull();
  });

  it('never matches inherited object keys', () => {
    for (const id of ['__proto__', 'constructor', 'toString', 'hasOwnProperty']) {
      expect(linuxPackageManager({ id, idLike: [id] })).toBeNull();
    }
  });

  it('ignores values that are too long, not strings, or beyond the entry limit', () => {
    expect(
      linuxPackageManager({ id: 'x'.repeat(MAX_DISTRO_ID_LENGTH + 1), idLike: [] }),
    ).toBeNull();
    const malformed = { id: 'odd', idLike: [42, null, 'arch'] } as unknown as Parameters<
      typeof linuxPackageManager
    >[0];
    expect(linuxPackageManager(malformed)).toBe('pacman');
    const many = Array.from({ length: MAX_ID_LIKE_ENTRIES }, () => 'other');
    expect(linuxPackageManager({ id: 'odd', idLike: [...many, 'debian'] })).toBeNull();
    const notAList = { id: 'odd', idLike: 'debian' } as unknown as Parameters<
      typeof linuxPackageManager
    >[0];
    expect(linuxPackageManager(notAList)).toBeNull();
  });
});

describe('linuxCommands and instructionPlatforms', () => {
  it('show one command for a known distribution and all three otherwise', () => {
    expect(linuxCommands({ id: 'ubuntu', idLike: ['debian'] })).toEqual(['apt']);
    expect(linuxCommands({ id: 'gentoo', idLike: [] })).toEqual(LINUX_PACKAGE_MANAGERS);
    expect(linuxCommands(null)).toEqual(['apt', 'dnf', 'pacman']);
  });

  it('show one platform when it is known and both otherwise', () => {
    expect(instructionPlatforms('windows')).toEqual(['windows']);
    expect(instructionPlatforms('linux')).toEqual(['linux']);
    expect(instructionPlatforms(null)).toEqual(['windows', 'linux']);
  });
});
