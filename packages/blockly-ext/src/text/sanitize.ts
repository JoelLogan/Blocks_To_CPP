/**
 * The text rules for everything a user types into a block (docs/spec/05-project-format.md §5.6,
 * docs/spec/08-security.md §8.4). They are the rules `b2c_model` applies when it loads a project,
 * so text the editor accepts always gives a document that loads: a field can never break the whole
 * preview by holding text the loader rejects.
 */

/** The longest text a field may hold, in UTF-8 bytes (05 §5.6, `b2c_model::limits`). */
export const MAX_FIELD_TEXT_BYTES = 64 * 1024;

/** Why a piece of text is rejected. */
export type FieldTextProblem =
  /** It contains U+0000. */
  | 'nul'
  /** It contains a C0 control character other than tab and newline. */
  | 'control'
  /** It contains a Unicode bidirectional control (Trojan Source, CVE-2021-42574). */
  | 'bidi'
  /** It contains half of a UTF-16 surrogate pair, which is not valid Unicode text. */
  | 'surrogate'
  /** It is longer than {@link MAX_FIELD_TEXT_BYTES} in UTF-8. */
  | 'tooLong';

/** The result of {@link sanitizeFieldText}. */
export type FieldTextCheck =
  { readonly ok: true } | { readonly ok: false; readonly reason: FieldTextProblem };

const OK: FieldTextCheck = Object.freeze({ ok: true });

/**
 * The bidirectional controls the loader rejects: U+061C, U+200E–U+200F, U+202A–U+202E and
 * U+2066–U+2069 (`b2c_model::text_rules::is_bidi_control`).
 */
export function isBidiControl(codePoint: number): boolean {
  return (
    codePoint === 0x061c ||
    codePoint === 0x200e ||
    codePoint === 0x200f ||
    (codePoint >= 0x202a && codePoint <= 0x202e) ||
    (codePoint >= 0x2066 && codePoint <= 0x2069)
  );
}

/**
 * Checks text against the project text rules: no NUL, no C0 control characters other than tab and
 * newline, no bidirectional controls, no lone surrogates, and at most 64 KiB of UTF-8. The text is
 * never changed; the first rule it breaks, in the order above, is the reason.
 *
 * It runs in time linear in the length and stops at the first problem, so it is safe on untrusted
 * input of any size.
 */
export function sanitizeFieldText(text: string): FieldTextCheck {
  // A UTF-16 unit is at most 3 UTF-8 bytes, and a surrogate pair (2 units) is 4, so text with at
  // most a third of the limit in units always fits: most text needs no byte count at all.
  let bytes = 0;
  const count = text.length * 3 > MAX_FIELD_TEXT_BYTES;
  for (let index = 0; index < text.length; index++) {
    const unit = text.charCodeAt(index);
    if (unit === 0) {
      return { ok: false, reason: 'nul' };
    }
    if (unit < 0x20 && unit !== 0x09 && unit !== 0x0a) {
      return { ok: false, reason: 'control' };
    }
    if (isBidiControl(unit)) {
      return { ok: false, reason: 'bidi' };
    }
    if (unit >= 0xd800 && unit <= 0xdfff) {
      const next = index + 1 < text.length ? text.charCodeAt(index + 1) : 0;
      if (unit > 0xdbff || next < 0xdc00 || next > 0xdfff) {
        return { ok: false, reason: 'surrogate' };
      }
      // A valid pair: one supplementary code point, 4 UTF-8 bytes.
      index++;
      bytes += 4;
    } else {
      bytes += unit < 0x80 ? 1 : unit < 0x800 ? 2 : 3;
    }
    if (count && bytes > MAX_FIELD_TEXT_BYTES) {
      return { ok: false, reason: 'tooLong' };
    }
  }
  return OK;
}

/** Whether `text` follows every project text rule ({@link sanitizeFieldText} is ok). */
export function isCleanFieldText(text: string): boolean {
  return sanitizeFieldText(text).ok;
}

/** The number of Unicode code points in `text` (a surrogate pair counts once). */
export function codePointCount(text: string): number {
  let count = 0;
  for (let index = 0; index < text.length; index++) {
    const unit = text.charCodeAt(index);
    if (unit >= 0xd800 && unit <= 0xdbff && index + 1 < text.length) {
      const next = text.charCodeAt(index + 1);
      if (next >= 0xdc00 && next <= 0xdfff) {
        index++;
      }
    }
    count++;
  }
  return count;
}
