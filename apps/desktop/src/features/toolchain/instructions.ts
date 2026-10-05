/**
 * What the setup page tells someone without a compiler (docs/spec/04-user-interface.md §4.6). The
 * commands are fixed texts that match the spec word for word (a test checks them against it); the
 * page only chooses which ones to show, from the platform and, on Linux, the distribution the
 * backend read from `/etc/os-release` (`toolchain_setup_info`).
 */
import type { Distro, Platform } from '@blocks2cpp/ipc-types';

/** The install commands, exactly as 04 §4.6 gives them. */
export const INSTALL_COMMANDS = {
  /** In the *MSYS2 UCRT64* shell, after installing MSYS2. */
  msys2: 'pacman -S mingw-w64-ucrt-x86_64-gcc',
  /** WinLibs, through the Windows package manager. */
  winlibs: 'winget install BrechtSanders.WinLibs.POSIX.UCRT',
  /** Debian, Ubuntu and their derivatives. */
  apt: 'sudo apt install g++',
  /** Fedora, RHEL, CentOS and their derivatives. */
  dnf: 'sudo dnf install gcc-c++',
  /** Arch Linux and its derivatives. */
  pacman: 'sudo pacman -S gcc',
} as const;

/** A Linux package manager the page has a command for. */
export type LinuxPackageManager = 'apt' | 'dnf' | 'pacman';

/** Every Linux package manager, in the order the page lists them when it cannot choose. */
export const LINUX_PACKAGE_MANAGERS: readonly LinuxPackageManager[] = ['apt', 'dnf', 'pacman'];

/** The distribution IDs each package manager's command is shown for (04 §4.6). */
const DISTRO_PACKAGE_MANAGERS: Readonly<Record<string, LinuxPackageManager>> = {
  debian: 'apt',
  ubuntu: 'apt',
  fedora: 'dnf',
  rhel: 'dnf',
  centos: 'dnf',
  arch: 'pacman',
};

/** The most `ID_LIKE` entries looked at; `os-release` files have a handful. */
export const MAX_ID_LIKE_ENTRIES = 32;

/** The longest distribution ID looked at (`os-release` IDs are short). */
export const MAX_DISTRO_ID_LENGTH = 64;

/** `id` as a key of {@link DISTRO_PACKAGE_MANAGERS}, or `null` for anything unusable. */
function packageManagerOf(id: unknown): LinuxPackageManager | null {
  if (typeof id !== 'string' || id.length === 0 || id.length > MAX_DISTRO_ID_LENGTH) {
    return null;
  }
  const key = id.toLowerCase();
  return Object.hasOwn(DISTRO_PACKAGE_MANAGERS, key)
    ? (DISTRO_PACKAGE_MANAGERS[key] ?? null)
    : null;
}

/**
 * The package manager for a Linux distribution: by its `ID` first, then by the first entry of
 * `ID_LIKE` that has one (Linux Mint is like Ubuntu, Rocky Linux like RHEL, Manjaro like Arch).
 * `null` when none matches or the distribution is unknown: the page then shows every command.
 */
export function linuxPackageManager(distro: Distro | null): LinuxPackageManager | null {
  if (distro === null) {
    return null;
  }
  const byId = packageManagerOf(distro.id);
  if (byId !== null) {
    return byId;
  }
  const likes: readonly unknown[] = Array.isArray(distro.idLike) ? distro.idLike : [];
  for (const like of likes.slice(0, MAX_ID_LIKE_ENTRIES)) {
    const match = packageManagerOf(like);
    if (match !== null) {
      return match;
    }
  }
  return null;
}

/** The Linux commands to show: the distribution's one, or all three when it is not known. */
export function linuxCommands(distro: Distro | null): readonly LinuxPackageManager[] {
  const manager = linuxPackageManager(distro);
  return manager === null ? LINUX_PACKAGE_MANAGERS : [manager];
}

/** The names the page gives the distributions of each package manager. */
export const PACKAGE_MANAGER_DISTROS: Readonly<Record<LinuxPackageManager, string>> = {
  apt: 'Debian, Ubuntu and Linux Mint',
  dnf: 'Fedora, Red Hat Enterprise Linux and CentOS',
  pacman: 'Arch Linux and Manjaro',
};

/** Which platforms' instructions to show: one when the platform is known, otherwise both. */
export function instructionPlatforms(platform: Platform | null): readonly Platform[] {
  return platform === null ? ['windows', 'linux'] : [platform];
}
