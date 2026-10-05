/**
 * Text from outside the app (project names, file names, paths and loader messages) shown by the
 * project feature. It is always rendered as React text, never as HTML; these helpers also make
 * invisible and reordering characters visible (`⟨U+202E⟩`, 08 §8.4.6) and bound its length, so a
 * crafted name can neither hide text nor flood a dialog.
 */
import { visibleInvisibles } from '../../panels/shared/invisibles';

/** The longest project or file name shown, in UTF-16 code units; longer ones are cut with `…`. */
export const MAX_SHOWN_NAME_CHARS = 120;

/** The longest path shown in the recent-projects list. */
export const MAX_SHOWN_PATH_CHARS = 400;

/** The longest loader message shown for one problem. */
export const MAX_SHOWN_MESSAGE_CHARS = 600;

/**
 * `text` for display: hidden characters as visible placeholders, then cut to `max` code units
 * (never inside a surrogate pair), with `…` marking the cut.
 */
export function shownText(text: string, max: number): string {
  const visible = visibleInvisibles(text);
  if (visible.length <= max) {
    return visible;
  }
  let end = Math.max(0, max - 1);
  const last = visible.charCodeAt(end - 1);
  if (last >= 0xd800 && last <= 0xdbff) {
    end -= 1;
  }
  return `${visible.slice(0, end)}…`;
}

/** A project or file name for display (see {@link shownText}). */
export function shownName(name: string): string {
  const trimmed = name.trim();
  return shownText(trimmed === '' ? 'Untitled project' : trimmed, MAX_SHOWN_NAME_CHARS);
}

/**
 * An RFC 3339 timestamp as a local date and time in the user's format (`5 Oct 2026, 10:42`), or
 * `null` when it does not parse.
 */
export function localDateTime(timestamp: string): string | null {
  const date = new Date(timestamp);
  if (Number.isNaN(date.getTime())) {
    return null;
  }
  return new Intl.DateTimeFormat(undefined, { dateStyle: 'medium', timeStyle: 'short' }).format(
    date,
  );
}
