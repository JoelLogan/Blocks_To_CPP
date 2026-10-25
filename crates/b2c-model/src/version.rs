//! Versions: the project format versions this build reads and writes, and
//! which Blocks2Cpp saved a file.
//!
//! **Format versions** (ADR-0013, ADR-0017 §4, types.md §2.3). A file's
//! `formatVersion` is the lowest version whose keys cover it: each version
//! only adds keys, so the writer computes it from the keys present
//! ([`required_format_version`]) and a file that uses nothing newer keeps
//! version 1 and still opens in older apps. The loader reads every version
//! up to [`MAX_FORMAT_VERSION`]; those from [`OLDEST_UNMIGRATED_VERSION`] up
//! load without a migration step, and only an older one runs the migration
//! chain ([`crate::migrate`]), which covers changes of meaning.
//!
//! **App versions**, compared by SemVer 2.0.0 precedence
//! (<https://semver.org/#spec-item-11>). The loader uses them for one rule
//! (ADR-0013): a key this version does not know, in a file whose
//! `generator.app` is a newer version than this app, is a key that newer
//! version added, so it is reported as `B2C-E0108` ("made with a newer
//! version of Blocks2Cpp") instead of `B2C-E0110` ("unknown key"). The block
//! catalog compares `generator.catalog` with its own version the same way
//! (ADR-0015), through [`crate::is_newer_version`]. The version text comes
//! from the file, so it is only compared, never shown.

use std::cmp::Ordering;

use crate::document::Document;
use crate::{terms, walk};

/// The newest project format version this build reads and writes
/// (types.md §2.3). Version 2 adds template type terms (ADR-0017) and
/// *Textbook style*, `options.usingNamespaceStd` (ADR-0013).
pub const MAX_FORMAT_VERSION: u32 = 2;

/// The oldest project format version that loads without a migration step.
/// Versions from here to [`MAX_FORMAT_VERSION`] differ only by added keys
/// (ADR-0013); a future change of meaning raises it and adds its migration
/// steps. `migratedFrom` (05 §5.7) compares with it.
pub const OLDEST_UNMIGRATED_VERSION: u32 = 1;

// Every version from the oldest unmigrated one up loads as it is.
const _: () = assert!(OLDEST_UNMIGRATED_VERSION <= MAX_FORMAT_VERSION);

/// The format version that added template type terms.
const TEMPLATE_TERMS: u32 = 2;

/// The lowest format version whose keys cover the document (ADR-0013): 2
/// when it holds a template type term, in a field or in the `type` of a row
/// of `extra` (`params`, `fields`), at any depth, in any module; else 1. A user type
/// (`{"ref": id}`) and a name are version 1 values.
///
/// `options.usingNamespaceStd` (*Textbook style*) is a version 2 key too,
/// but no document has it yet: [`crate::ProjectOptions`] gains it with
/// Textbook style, and this function with it.
///
/// The canonical writer writes this, not [`Document::format_version`], so a
/// file that stops using templates goes back to version 1.
pub fn required_format_version(document: &Document) -> u32 {
    let mut template = false;
    walk::document(document, |block| {
        template = template || terms::has_template(block);
    });
    if template { TEMPLATE_TERMS } else { 1 }
}

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
/// text), by SemVer precedence: build metadata does not count, and a
/// pre-release is older than its release. Text that is not a SemVer version
/// is never newer, and nothing is newer than text that is not one.
///
/// ```
/// use b2c_model::is_newer_version;
///
/// assert!(is_newer_version("1.2.0", "1.1.0"));
/// assert!(is_newer_version("1.10.0", "1.9.0"));
/// assert!(!is_newer_version("1.1.0", "1.1.0"));
/// assert!(!is_newer_version("1.2.0-beta", "1.2.0"));
/// assert!(!is_newer_version("9.0", "1.1.0"));
/// ```
pub fn is_newer(text: &str, this: &str) -> bool {
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

    /// The test vector the desktop app's copy of this rule
    /// (`apps/desktop/src/features/project/version.ts`) is tested against too.
    fn vector() -> serde_json::Value {
        serde_json::from_str(include_str!("../tests/fixtures/semver-precedence.json")).unwrap()
    }

    /// The strings of a JSON array.
    fn strings(value: &serde_json::Value) -> Vec<&str> {
        value
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect()
    }

    #[test]
    fn precedence_follows_semver() {
        // The precedence example of SemVer 2.0.0 §11, in increasing order,
        // with a larger number than 64 bits hold at the end.
        let vector = vector();
        let ordered = strings(&vector["ordered"]);
        assert!(ordered.len() >= 13);
        for (i, a) in ordered.iter().enumerate() {
            for (j, b) in ordered.iter().enumerate() {
                assert_eq!(is_newer(a, b), i > j, "{a} newer than {b}?");
            }
        }
    }

    #[test]
    fn build_metadata_does_not_count() {
        let vector = vector();
        for pair in vector["equal"].as_array().unwrap() {
            let pair = strings(pair);
            assert!(!is_newer(pair[0], pair[1]), "{pair:?}");
        }
        for pair in vector["newer"].as_array().unwrap() {
            let pair = strings(pair);
            assert!(is_newer(pair[0], pair[1]), "{pair:?}");
        }
    }

    #[test]
    fn text_that_is_not_semver_is_never_newer() {
        let vector = vector();
        for text in strings(&vector["notSemver"]) {
            assert!(Version::parse(text).is_none(), "{text:?}");
            assert!(!is_newer(text, "0.1.0"), "{text:?}");
        }
        // Leading zeros are allowed in alphanumeric pre-release identifiers
        // and in build metadata.
        for text in strings(&vector["semver"]) {
            assert!(Version::parse(text).is_some(), "{text:?}");
        }
        assert!(!is_newer("1.0.0", "not a version"));
    }
}
