/** What the setup page must show, and hiding g++ from the app until it is given back. */
import { existsSync, mkdirSync, mkdtempSync, realpathSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';

import { afterEach, describe, expect, it } from 'vitest';

import {
  expectedInstallCommands,
  hideToolchains,
  INSTALL_COMMANDS,
  parseOsRelease,
} from './toolchain';

const folders: string[] = [];

afterEach(() => {
  for (const folder of folders.splice(0)) {
    rmSync(folder, { recursive: true, force: true });
  }
});

describe('parseOsRelease', () => {
  it('reads ID and ID_LIKE, quoted or not', () => {
    const text = [
      'NAME="Linux Mint"',
      '# a comment',
      'ID=linuxmint',
      'ID_LIKE="ubuntu debian"',
      'VERSION_ID="22"',
    ].join('\n');
    expect(parseOsRelease(text)).toEqual({ id: 'linuxmint', idLike: ['ubuntu', 'debian'] });
    expect(parseOsRelease("ID='fedora'\r\n")).toEqual({ id: 'fedora', idLike: [] });
    expect(parseOsRelease('')).toEqual({ id: null, idLike: [] });
  });
});

describe('expectedInstallCommands', () => {
  it('gives MSYS2 and WinLibs on Windows', () => {
    expect(expectedInstallCommands('win32', null)).toEqual([
      INSTALL_COMMANDS.msys2,
      INSTALL_COMMANDS.winlibs,
    ]);
  });

  it('gives the command of the distribution, by ID first, then by ID_LIKE', () => {
    expect(expectedInstallCommands('linux', { id: 'ubuntu', idLike: ['debian'] })).toEqual([
      INSTALL_COMMANDS.apt,
    ]);
    expect(expectedInstallCommands('linux', { id: 'linuxmint', idLike: ['ubuntu'] })).toEqual([
      INSTALL_COMMANDS.apt,
    ]);
    expect(
      expectedInstallCommands('linux', { id: 'rocky', idLike: ['rhel', 'centos', 'fedora'] }),
    ).toEqual([INSTALL_COMMANDS.dnf]);
    expect(expectedInstallCommands('linux', { id: 'manjaro', idLike: ['arch'] })).toEqual([
      INSTALL_COMMANDS.pacman,
    ]);
  });

  it('gives every Linux command when the distribution is unknown', () => {
    const all = [INSTALL_COMMANDS.apt, INSTALL_COMMANDS.dnf, INSTALL_COMMANDS.pacman];
    expect(expectedInstallCommands('linux', { id: 'gentoo', idLike: [] })).toEqual(all);
    expect(expectedInstallCommands('linux', null)).toEqual(all);
    // Not a key of the table, even though every object has it.
    expect(expectedInstallCommands('linux', { id: 'constructor', idLike: [] })).toEqual(all);
  });
});

describe('hideToolchains', () => {
  it('names links that do not exist yet, in an OS path list', () => {
    expect(hideToolchains('/r', '/usr/bin:/opt/gcc/bin', 'linux').value).toBe(
      `${path.join('/r', 'toolchain-1')}:${path.join('/r', 'toolchain-2')}`,
    );
    expect(hideToolchains('C:\\r', 'C:\\msys64\\ucrt64\\bin', 'win32').value).toBe(
      path.join('C:\\r', 'toolchain-1'),
    );
    expect(() => hideToolchains('/r', '', 'linux')).toThrow('no toolchain folder');
  });

  it('gives the folders back as links to them', () => {
    const root = mkdtempSync(path.join(tmpdir(), 'b2c-e2e-flows-'));
    folders.push(root);
    const bin = path.join(root, 'bin');
    mkdirSync(bin);
    const hidden = hideToolchains(root, bin);
    expect(existsSync(hidden.value)).toBe(false);
    hidden.restore();
    expect(realpathSync(hidden.value)).toBe(realpathSync(bin));
  });
});
