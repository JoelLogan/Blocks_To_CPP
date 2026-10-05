/**
 * The Settings page's fixed texts that depend on data: what a settings notice means
 * (docs/spec/05-project-format.md §5.9, 04 §4.12), how much space clearing the build cache freed
 * (07 §7.5.1), and the limits of the values the page edits.
 */
import type { BuildCacheClearResponse, SettingsNotice } from '@blocks2cpp/ipc-types';

import { visibleInvisibles } from '../../panels/shared/invisibles';

/** The console scrollback the settings accept, in lines (05 §5.9, `b2c_ipc::limits`). */
export const SCROLLBACK_LIMITS = { min: 1000, max: 100_000 } as const;

/** The most notices listed; `settings.json` has a dozen keys, so this only bounds a broken answer. */
export const MAX_SHOWN_NOTICES = 20;

/** The names of the settings keys a notice can be about. */
const KEY_LABELS: Readonly<Record<string, string>> = {
  codeStyle: 'Code style',
  'codeStyle.indentWidth': 'Code style: indent width',
  run: 'Run on errors',
  'run.onErrors': 'Run on errors',
  console: 'Console',
  'console.scrollbackLines': 'Console: scrollback lines',
  toolchain: 'Toolchain',
  'toolchain.selectedId': 'Toolchain: the default compiler',
  newProject: 'New projects',
  'newProject.standard': 'New projects: C++ standard',
  buildCache: 'Build cache',
  'buildCache.maxBytes': 'Build cache: largest size',
};

/** The longest unknown key shown as it is. */
const MAX_KEY_LENGTH = 64;

/** How the page names the setting `key` (a dotted path such as `console.scrollbackLines`). */
export function settingLabel(key: string): string {
  if (Object.hasOwn(KEY_LABELS, key)) {
    return KEY_LABELS[key] ?? key;
  }
  const visible = visibleInvisibles(key);
  const shown =
    visible.length <= MAX_KEY_LENGTH ? visible : `${visible.slice(0, MAX_KEY_LENGTH - 1)}…`;
  return `The setting “${shown}”`;
}

/** What a notice means, as a sentence. */
export function noticeText(notice: SettingsNotice): string {
  switch (notice.reason) {
    case 'corruptFile':
      return 'The settings file could not be read, so every setting was reset to its default.';
    case 'newerVersion':
      return 'The settings file comes from a newer version of Blocks2Cpp. The settings this version knows are used, and the others are kept as they are.';
    case 'invalidValue':
      return notice.key === ''
        ? 'A setting had a value that is not allowed and was reset to its default.'
        : `${settingLabel(notice.key)} had a value that is not allowed and was reset to its default.`;
  }
}

/** The notices to list (at most {@link MAX_SHOWN_NOTICES}) and how many more there are. */
export function shownNotices(notices: readonly SettingsNotice[]): {
  shown: SettingsNotice[];
  more: number;
} {
  const shown = notices.slice(0, MAX_SHOWN_NOTICES);
  return { shown, more: notices.length - shown.length };
}

const UNITS = ['KB', 'MB', 'GB', 'TB'] as const;

/** A size for people: `512 bytes`, `1.5 KB`, `12 MB`, `3.2 GB` (powers of 1024). */
export function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes < 0) {
    return 'an unknown amount';
  }
  if (bytes < 1024) {
    const whole = Math.round(bytes);
    return whole === 1 ? '1 byte' : `${String(whole)} bytes`;
  }
  let value = bytes / 1024;
  let unit = 0;
  // 1023.5 KB and more would round to 1024 KB: show it as 1 MB instead.
  while (value >= 1023.5 && unit < UNITS.length - 1) {
    value /= 1024;
    unit += 1;
  }
  // One decimal below 10 (1.5 MB), whole numbers above (12 MB); 9.96 rounds up to 10 MB.
  const text = value < 9.95 ? value.toFixed(1).replace(/\.0$/, '') : String(Math.round(value));
  return `${text} ${UNITS[unit] ?? 'TB'}`;
}

/** What clearing the build cache did, as one or two sentences. */
export function cacheClearedText(result: BuildCacheClearResponse): string {
  const freed =
    result.freedBytes > 0
      ? `The build cache was cleared: ${formatBytes(result.freedBytes)} freed.`
      : 'The build cache was already empty.';
  if (result.skippedInUse <= 0) {
    return freed;
  }
  const kept =
    result.skippedInUse === 1
      ? '1 build was kept because it is in use right now.'
      : `${String(result.skippedInUse)} builds were kept because they are in use right now.`;
  return `${freed} ${kept}`;
}

/**
 * Reads the scrollback field: a whole number of lines within {@link SCROLLBACK_LIMITS}, with
 * thousands separators (`10,000`, `10 000`, `10'000`, `10_000`) allowed. `null` when it is not one.
 */
export function parseScrollback(text: string): number | null {
  const compact = text.trim().replace(/[\s,'_\u00a0\u202f]/g, '');
  if (!/^\d{1,7}$/.test(compact)) {
    return null;
  }
  const lines = Number(compact);
  return lines >= SCROLLBACK_LIMITS.min && lines <= SCROLLBACK_LIMITS.max ? lines : null;
}

/** `10,000`: a line count with thousands separators. */
export function formatLines(lines: number): string {
  return String(lines).replace(/\B(?=(\d{3})+(?!\d))/g, ',');
}
