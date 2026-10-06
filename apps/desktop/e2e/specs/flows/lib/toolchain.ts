/**
 * The toolchain setup page in the flow tests (docs/spec/04-user-interface.md §4.6): the install
 * commands it must show on this platform, and hiding the machine's g++ from the app and giving it
 * back.
 *
 * The app looks for g++ only in `B2C_E2E_TOOLCHAIN_DIRS` (the e2e build's seam). The test points
 * it at links that do not exist yet, one per real folder, so discovery finds nothing; creating the
 * links (a symbolic link on Linux, a junction on Windows, which needs no privilege) "installs" g++
 * without restarting the app, and *Rescan* finds it.
 */
import { readFileSync, symlinkSync } from 'node:fs';
import path from 'node:path';

/** The install commands, word for word as 04 §4.6 gives them. */
export const INSTALL_COMMANDS = {
  msys2: 'pacman -S mingw-w64-ucrt-x86_64-gcc',
  winlibs: 'winget install BrechtSanders.WinLibs.POSIX.UCRT',
  apt: 'sudo apt install g++',
  dnf: 'sudo dnf install gcc-c++',
  pacman: 'sudo pacman -S gcc',
} as const;

/** Which Linux distributions use which command (04 §4.6). */
const DISTRO_COMMANDS: Readonly<Record<string, string>> = {
  debian: INSTALL_COMMANDS.apt,
  ubuntu: INSTALL_COMMANDS.apt,
  fedora: INSTALL_COMMANDS.dnf,
  rhel: INSTALL_COMMANDS.dnf,
  centos: INSTALL_COMMANDS.dnf,
  arch: INSTALL_COMMANDS.pacman,
};

/** A distribution as `/etc/os-release` names it. */
export interface OsRelease {
  readonly id: string | null;
  readonly idLike: readonly string[];
}

/** A value of `os-release`, without its quotes. */
function unquote(value: string): string {
  const trimmed = value.trim();
  const quoted = /^(["'])(.*)\1$/.exec(trimmed);
  return quoted?.[2] ?? trimmed;
}

/** The `ID` and `ID_LIKE` of an `os-release` file's text (os-release(5)). */
export function parseOsRelease(text: string): OsRelease {
  let id: string | null = null;
  let idLike: string[] = [];
  for (const line of text.split(/\r?\n/)) {
    const match = /^([A-Z_]+)=(.*)$/.exec(line.trim());
    if (match?.[1] === 'ID' && match[2] !== undefined) {
      id = unquote(match[2]).toLowerCase() || null;
    } else if (match?.[1] === 'ID_LIKE' && match[2] !== undefined) {
      idLike = unquote(match[2])
        .toLowerCase()
        .split(/\s+/)
        .filter((entry) => entry !== '');
    }
  }
  return { id, idLike };
}

/** This machine's `os-release` text, or `null` (as the backend reads it: two places). */
export function readOsRelease(): string | null {
  for (const file of ['/etc/os-release', '/usr/lib/os-release']) {
    try {
      return readFileSync(file, 'utf8');
    } catch {
      // Try the next place.
    }
  }
  return null;
}

/**
 * The commands the setup page must show: on Windows MSYS2's and WinLibs'; on Linux the one of the
 * distribution (by `ID`, then the first `ID_LIKE` that has one), or all three when none matches.
 */
export function expectedInstallCommands(
  platform: NodeJS.Platform,
  osRelease: OsRelease | null,
): string[] {
  if (platform === 'win32') {
    return [INSTALL_COMMANDS.msys2, INSTALL_COMMANDS.winlibs];
  }
  const own = (key: string | null): string | undefined =>
    key !== null && Object.hasOwn(DISTRO_COMMANDS, key) ? DISTRO_COMMANDS[key] : undefined;
  const found =
    own(osRelease?.id ?? null) ??
    (osRelease?.idLike ?? []).map(own).find((command) => command !== undefined);
  return found === undefined
    ? [INSTALL_COMMANDS.apt, INSTALL_COMMANDS.dnf, INSTALL_COMMANDS.pacman]
    : [found];
}

/** The machine's toolchain folders, hidden from the app until {@link HiddenToolchains.restore}. */
export interface HiddenToolchains {
  /** The value of `B2C_E2E_TOOLCHAIN_DIRS` for the app: links that do not exist yet. */
  readonly value: string;
  /** Creates the links, so the folders are where the app looks. */
  restore(): void;
}

/**
 * Hides the folders of `dirs` (an OS path list, as `B2C_E2E_TOOLCHAIN_DIRS` holds it) behind links
 * in `root` that are created only by {@link HiddenToolchains.restore}.
 */
export function hideToolchains(
  root: string,
  dirs: string,
  platform: NodeJS.Platform = process.platform,
): HiddenToolchains {
  const delimiter = platform === 'win32' ? ';' : ':';
  const targets = dirs.split(delimiter).filter((dir) => dir !== '');
  if (targets.length === 0) {
    throw new Error('There is no toolchain folder to hide (B2C_E2E_TOOLCHAIN_DIRS is empty)');
  }
  const links = targets.map((_dir, index) => path.join(root, `toolchain-${String(index + 1)}`));
  return {
    value: links.join(delimiter),
    restore: () => {
      targets.forEach((target, index) => {
        const link = links[index];
        if (link !== undefined) {
          symlinkSync(target, link, platform === 'win32' ? 'junction' : 'dir');
        }
      });
    },
  };
}
