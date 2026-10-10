//! App versions: which Blocks2Cpp saved a file, compared with this one by
//! SemVer 2.0.0 precedence (<https://semver.org/#spec-item-11>).
//!
//! Only one rule uses it (ADR-0013): a key this version does not know, in a
//! file whose `generator.app` is a newer version than this app, is a key that
//! newer version added, so it is reported as `B2C-E0108` ("made with a newer
//! version of Blocks2Cpp") instead of `B2C-E0110` ("unknown key"). The
//! version text comes from the file, so it is only compared, never shown.

use std::cmp::Ordering;

/// This app's version. Every crate of the workspace, the desktop app and the
/// CLI share one version, which is what the app writes as `generator.app`.
pub(crate) const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

/// A parsed SemVer version, borrowing its parts from the text. Build metadata
/// (`+…`) is checked but not kept, because it does not count for precedence.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Version<'a> {
    /// Major, minor and patch: digits without leading zeros, of any length.
    core: [&'a str; 3],
    /// Pre-release identifiers (`alpha`, `1`), empty for a release.
    pre: Vec<&'a str>,
}

/// Whether `text` is a SemVer version newer than `this` (both SemVer 2.0.0
/// text). Text that is not a SemVer version is never newer.
pub(crate) fn is_newer(text: &str, this: &str) -> bool {
    match (Version::parse(text), Version::parse(this)) {
        (Some(version), Some(this)) => version.cmp(&this) == Ordering::Greater,
        _ => false,
    }
}

/// Whether `part` is a SemVer numeric identifier: digits, and no leading zero
/// unless it is `0` itself.
fn is_number(part: &str) -> bool {
    !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()) && (part == "0" || !part.starts_with('0'))
}

/// Whether `part` is a SemVer identifier: one or more ASCII letters, digits
/// and hyphens.
fn is_identifier(part: &str) -> bool {
    !part.is_empty() && part.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

/// Compares two numeric identifiers by value: without leading zeros, the
/// longer one is larger, and equal lengths compare digit by digit. This works
/// for numbers of any size.
fn cmp_numbers(a: &str, b: &str) -> Ordering {
    a.len().cmp(&b.len()).then_with(|| a.cmp(b))
}

/// Compares two pre-release identifiers: numbers by value, other identifiers
/// in ASCII order, and a number before any other identifier.
fn cmp_identifiers(a: &str, b: &str) -> Ordering {
    match (is_number(a), is_number(b)) {
        (true, true) => cmp_numbers(a, b),
        (true, false) => Ordering::Less,
        (false, true) => Ordering::Greater,
        (false, false) => a.cmp(b),
    }
}

impl<'a> Version<'a> {
    /// Parses `MAJOR.MINOR.PATCH[-PRERELEASE][+BUILD]`; `None` for anything
    /// else.
    fn parse(text: &'a str) -> Option<Self> {
        let (rest, build) = match text.split_once('+') {
            Some((rest, build)) => (rest, Some(build)),
            None => (text, None),
        };
        if build.is_some_and(|build| !build.split('.').all(is_identifier)) {
            return None;
        }
        let (core, pre) = match rest.split_once('-') {
            Some((core, pre)) => (core, Some(pre)),
            None => (rest, None),
        };
        let mut numbers = core.split('.');
        let core = [numbers.next()?, numbers.next()?, numbers.next()?];
        if numbers.next().is_some() || !core.iter().all(|part| is_number(part)) {
            return None;
        }
        let pre: Vec<&str> = match pre {
            Some(pre) => pre.split('.').collect(),
            None => Vec::new(),
        };
        let valid_pre = pre.iter().all(|part| {
            is_identifier(part) && (is_number(part) || !part.bytes().all(|b| b.is_ascii_digit()))
        });
        valid_pre.then_some(Self { core, pre })
    }
}

impl Ord for Version<'_> {
    fn cmp(&self, other: &Self) -> Ordering {
        let core = self
            .core
            .iter()
            .zip(&other.core)
            .map(|(a, b)| cmp_numbers(a, b))
            .find(|order| order.is_ne())
            .unwrap_or(Ordering::Equal);
        core.then_with(|| match (self.pre.is_empty(), other.pre.is_empty()) {
            // A release comes after its pre-releases.
            (true, true) => Ordering::Equal,
            (true, false) => Ordering::Greater,
            (false, true) => Ordering::Less,
            (false, false) => self
                .pre
                .iter()
                .zip(&other.pre)
                .map(|(a, b)| cmp_identifiers(a, b))
                .find(|order| order.is_ne())
                .unwrap_or_else(|| self.pre.len().cmp(&other.pre.len())),
        })
    }
}

impl PartialOrd for Version<'_> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn this_app_has_a_semver_version() {
        assert!(Version::parse(APP_VERSION).is_some(), "{APP_VERSION}");
        assert!(!is_newer(APP_VERSION, APP_VERSION));
    }

    #[test]
    fn precedence_follows_semver() {
        // The precedence example of SemVer 2.0.0 §11, in increasing order,
        // with a larger number than 64 bits hold at the end.
        let ordered = [
            "1.0.0-alpha",
            "1.0.0-alpha.1",
            "1.0.0-alpha.beta",
            "1.0.0-beta",
            "1.0.0-beta.2",
            "1.0.0-beta.11",
            "1.0.0-rc.1",
            "1.0.0",
            "1.0.1",
            "1.1.0",
            "1.10.0",
            "2.0.0",
            "99999999999999999999999.0.0",
        ];
        for (i, a) in ordered.iter().enumerate() {
            for (j, b) in ordered.iter().enumerate() {
                assert_eq!(is_newer(a, b), i > j, "{a} newer than {b}?");
            }
        }
    }

    #[test]
    fn build_metadata_does_not_count() {
        assert!(!is_newer("1.0.0+build.7", "1.0.0"));
        assert!(!is_newer("1.0.0", "1.0.0+build.7"));
        assert!(is_newer("1.0.1+0", "1.0.0"));
    }

    #[test]
    fn text_that_is_not_semver_is_never_newer() {
        for text in [
            "",
            "9",
            "9.0",
            "9.0.0.0",
            "v9.0.0",
            "09.0.0",
            "9.00.0",
            "9.0.0-",
            "9.0.0-01",
            "9.0.0-a..b",
            "9.0.0+",
            "9.0.0+a+b",
            "9.0.0+a_b",
            "9.0.0 ",
            " 9.0.0",
            "9.0.0-é",
            "-9.0.0",
            "9..0",
        ] {
            assert!(Version::parse(text).is_none(), "{text:?}");
            assert!(!is_newer(text, "0.1.0"), "{text:?}");
        }
        // Leading zeros are allowed in alphanumeric pre-release identifiers
        // and in build metadata.
        assert!(Version::parse("9.0.0-0a.0").is_some());
        assert!(Version::parse("9.0.0+007").is_some());
        assert!(!is_newer("1.0.0", "not a version"));
    }
}
