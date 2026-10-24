/**
 * Which of two versions a file names is newer, by SemVer 2.0.0 precedence
 * (https://semver.org/#spec-item-11): the rule of `b2c_model::is_newer_version`
 * (crates/b2c-model/src/version.rs), which the compiler core applies to `generator.catalog`
 * (ADR-0015). Both are tested against one test vector
 * (crates/b2c-model/tests/fixtures/semver-precedence.json), so the app and the core always agree.
 * The text comes from a file, so it is only compared, never shown.
 */

/** A parsed version: major, minor and patch, and the pre-release identifiers (empty for a release). */
interface Version {
  readonly core: readonly [string, string, string];
  readonly pre: readonly string[];
}

/** A SemVer numeric identifier: digits without a leading zero, of any length. */
const NUMBER = /^(?:0|[1-9][0-9]*)$/;

/** A SemVer identifier: one or more ASCII letters, digits and hyphens. */
const IDENTIFIER = /^[0-9A-Za-z-]+$/;

/** Only digits (a numeric pre-release identifier must not have a leading zero). */
const DIGITS = /^[0-9]+$/;

/** Parses `MAJOR.MINOR.PATCH[-PRERELEASE][+BUILD]`; `null` for anything else. */
function parse(text: string): Version | null {
  const plus = text.indexOf('+');
  const rest = plus === -1 ? text : text.slice(0, plus);
  if (
    plus !== -1 &&
    !text
      .slice(plus + 1)
      .split('.')
      .every((part) => IDENTIFIER.test(part))
  ) {
    return null;
  }
  const dash = rest.indexOf('-');
  const core = (dash === -1 ? rest : rest.slice(0, dash)).split('.');
  const [major, minor, patch] = core;
  if (
    core.length !== 3 ||
    major === undefined ||
    minor === undefined ||
    patch === undefined ||
    !core.every((part) => NUMBER.test(part))
  ) {
    return null;
  }
  const pre = dash === -1 ? [] : rest.slice(dash + 1).split('.');
  const validPre = pre.every(
    (part) => IDENTIFIER.test(part) && (NUMBER.test(part) || !DIGITS.test(part)),
  );
  return validPre ? { core: [major, minor, patch], pre } : null;
}

/** Compares two numeric identifiers by value: the longer one is larger, then digit by digit. */
function compareNumbers(a: string, b: string): number {
  if (a.length !== b.length) {
    return a.length < b.length ? -1 : 1;
  }
  return a < b ? -1 : a > b ? 1 : 0;
}

/** Compares pre-release identifiers: numbers by value, others in ASCII order, numbers first. */
function compareIdentifiers(a: string, b: string): number {
  const aNumber = NUMBER.test(a);
  const bNumber = NUMBER.test(b);
  if (aNumber && bNumber) {
    return compareNumbers(a, b);
  }
  if (aNumber !== bNumber) {
    return aNumber ? -1 : 1;
  }
  return a < b ? -1 : a > b ? 1 : 0;
}

/** SemVer precedence of two parsed versions (build metadata does not count). */
function compare(a: Version, b: Version): number {
  for (let i = 0; i < 3; i += 1) {
    const order = compareNumbers(a.core[i] ?? '', b.core[i] ?? '');
    if (order !== 0) {
      return order;
    }
  }
  if (a.pre.length === 0 || b.pre.length === 0) {
    // A release comes after its pre-releases.
    return a.pre.length === b.pre.length ? 0 : a.pre.length === 0 ? 1 : -1;
  }
  const shared = Math.min(a.pre.length, b.pre.length);
  for (let i = 0; i < shared; i += 1) {
    const order = compareIdentifiers(a.pre[i] ?? '', b.pre[i] ?? '');
    if (order !== 0) {
      return order;
    }
  }
  return a.pre.length - b.pre.length;
}

/**
 * Whether `text` is a SemVer version newer than `than`. Text that is not a SemVer version is never
 * newer, and nothing is newer than text that is not one.
 */
export function isNewerVersion(text: string, than: string): boolean {
  const version = parse(text);
  const other = parse(than);
  return version !== null && other !== null && compare(version, other) > 0;
}
