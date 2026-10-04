//! Platforms, target triples and GCC versions.

use std::fmt;

use serde::{Deserialize, Serialize};

/// The operating-system family Blocks2Cpp runs on (or builds for).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Platform {
    /// Linux (and, best effort, other Unix systems).
    Linux,
    /// Windows.
    Windows,
}

impl Platform {
    /// The platform this program was built for.
    pub fn host() -> Self {
        if cfg!(windows) { Self::Windows } else { Self::Linux }
    }

    /// The suffix of executables: `.exe` on Windows, nothing on Linux.
    pub fn exe_suffix(self) -> &'static str {
        match self {
            Self::Linux => "",
            Self::Windows => ".exe",
        }
    }
}

/// What a toolchain's target triple (`g++ -dumpmachine`) says about the
/// programs it builds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetOs {
    /// `*-linux-gnu` and other Linux targets.
    Linux,
    /// MinGW-w64 (`x86_64-w64-mingw32`): native Windows programs.
    MinGw,
    /// Legacy mingw.org (`mingw32`, `i686-pc-mingw32`): outdated.
    LegacyMinGw,
    /// Cygwin or MSYS (`*-cygwin`, `*-msys`): programs need `cygwin1.dll`.
    Cygwin,
    /// Anything else.
    Other,
}

/// A target triple and its classification.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Target {
    /// The triple exactly as `-dumpmachine` printed it (trimmed).
    pub triple: String,
    /// Its classification.
    pub os: TargetOs,
}

impl Target {
    /// Classifies a target triple.
    ///
    /// ```
    /// use b2c_toolchain::target::{Target, TargetOs};
    ///
    /// assert_eq!(Target::parse("x86_64-linux-gnu").os, TargetOs::Linux);
    /// assert_eq!(Target::parse("x86_64-w64-mingw32").os, TargetOs::MinGw);
    /// assert_eq!(Target::parse("x86_64-pc-cygwin").os, TargetOs::Cygwin);
    /// ```
    pub fn parse(triple: &str) -> Self {
        let triple = triple.trim();
        let lower = triple.to_ascii_lowercase();
        let os = if lower.contains("cygwin") || lower.ends_with("-msys") || lower.contains("-msys-") {
            TargetOs::Cygwin
        } else if lower.contains("-w64-mingw32") || lower.contains("windows-gnu") {
            TargetOs::MinGw
        } else if lower.contains("mingw32") {
            TargetOs::LegacyMinGw
        } else if lower.contains("linux") {
            TargetOs::Linux
        } else {
            TargetOs::Other
        };
        Self {
            triple: triple.to_owned(),
            os,
        }
    }

    /// The platform the produced programs run on, for flag choices
    /// (spec §7.4.3): Windows for MinGW and Cygwin targets, Linux otherwise.
    pub fn platform(&self) -> Platform {
        match self.os {
            TargetOs::MinGw | TargetOs::LegacyMinGw | TargetOs::Cygwin => Platform::Windows,
            TargetOs::Linux | TargetOs::Other => Platform::Linux,
        }
    }
}

/// A GCC version (`-dumpfullversion`, or `-dumpversion` which may print only
/// the major version).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct GccVersion {
    /// Major version, e.g. 13.
    pub major: u32,
    /// Minor version.
    pub minor: u32,
    /// Patch level.
    pub patch: u32,
}

impl GccVersion {
    /// Parses `13.3.0`, `13.3` or `13` (surrounding whitespace allowed).
    ///
    /// ```
    /// use b2c_toolchain::target::GccVersion;
    ///
    /// let version = GccVersion::parse("13.3.0\n").unwrap();
    /// assert_eq!((version.major, version.minor, version.patch), (13, 3, 0));
    /// assert_eq!(GccVersion::parse("11").unwrap().major, 11);
    /// assert!(GccVersion::parse("clang version 18").is_none());
    /// ```
    pub fn parse(text: &str) -> Option<Self> {
        let mut parts = text.trim().split('.');
        let mut number = |required: bool| -> Option<u32> {
            match parts.next() {
                Some(part)
                    if !part.is_empty() && part.len() <= 6 && part.bytes().all(|b| b.is_ascii_digit()) =>
                {
                    part.parse().ok()
                }
                None if !required => Some(0),
                _ => None,
            }
        };
        let major = number(true)?;
        let minor = number(false)?;
        let patch = number(false)?;
        if parts.next().is_some() || major == 0 {
            return None;
        }
        Some(Self { major, minor, patch })
    }
}

impl fmt::Display for GccVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    #[test]
    fn targets_classify() {
        let cases = [
            ("x86_64-linux-gnu", TargetOs::Linux, Platform::Linux),
            ("aarch64-unknown-linux-gnu", TargetOs::Linux, Platform::Linux),
            ("x86_64-w64-mingw32", TargetOs::MinGw, Platform::Windows),
            ("i686-w64-mingw32", TargetOs::MinGw, Platform::Windows),
            ("mingw32", TargetOs::LegacyMinGw, Platform::Windows),
            ("i686-pc-mingw32", TargetOs::LegacyMinGw, Platform::Windows),
            ("x86_64-pc-cygwin", TargetOs::Cygwin, Platform::Windows),
            ("x86_64-pc-msys", TargetOs::Cygwin, Platform::Windows),
            ("x86_64-apple-darwin23", TargetOs::Other, Platform::Linux),
            ("", TargetOs::Other, Platform::Linux),
        ];
        for (triple, os, platform) in cases {
            let target = Target::parse(triple);
            assert_eq!(target.os, os, "{triple}");
            assert_eq!(target.platform(), platform, "{triple}");
        }
    }

    #[test]
    fn versions_parse_and_order() {
        assert_eq!(
            GccVersion::parse("14.2.1"),
            Some(GccVersion {
                major: 14,
                minor: 2,
                patch: 1
            })
        );
        assert_eq!(
            GccVersion::parse("13.3").map(|v| v.to_string()),
            Some(String::from("13.3.0"))
        );
        for bad in ["", "x", "13.", ".1", "13.3.0.1", "0", "13.a", "1234567"] {
            assert_eq!(GccVersion::parse(bad), None, "{bad:?}");
        }
        assert!(GccVersion::parse("11.4.0") < GccVersion::parse("13.1.0"));
        assert!(GccVersion::parse("13.10.0") > GccVersion::parse("13.9.0"));
    }

    proptest! {
        #[test]
        fn version_round_trips(major in 1_u32..1000, minor in 0_u32..1000, patch in 0_u32..1000) {
            let version = GccVersion { major, minor, patch };
            prop_assert_eq!(GccVersion::parse(&version.to_string()), Some(version));
        }

        #[test]
        fn parsing_never_panics(text in "\\PC{0,40}") {
            let _ = GccVersion::parse(&text);
            let _ = Target::parse(&text);
        }
    }
}
