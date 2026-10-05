//! The `B2C-T1xxx` diagnostics this crate reports, documented in
//! `docs/reference/diagnostics/toolchain.md`.
//!
//! Every diagnostic has [`DiagSource::Toolchain`] and points at the whole
//! project ([`Location::project`]): toolchain problems are about the
//! computer, not about a block.

use b2c_ir::{DiagSource, Diagnostic, Location};

/// No usable g++ was found.
pub const NO_TOOLCHAIN: &str = "B2C-T1001";
/// A g++ path given explicitly cannot be used.
pub const BAD_TOOLCHAIN_PATH: &str = "B2C-T1002";
/// The compiler could not be started or did not answer.
pub const NOT_RUNNABLE: &str = "B2C-T1003";
/// GCC is older than the minimum version.
pub const TOO_OLD: &str = "B2C-T1004";
/// The compiler is Clang, which is not supported yet.
pub const CLANG: &str = "B2C-T1005";
/// The installation is broken or incomplete.
pub const BROKEN_INSTALL: &str = "B2C-T1006";
/// A Cygwin or MSYS toolchain (programs need `cygwin1.dll`).
pub const CYGWIN: &str = "B2C-T1007";
/// The legacy mingw.org toolchain.
pub const LEGACY_MINGW: &str = "B2C-T1008";
/// The toolchain changed since it was checked.
pub const CHANGED: &str = "B2C-T1009";
/// The project's C++ standard is not supported by this toolchain.
pub const STANDARD_UNSUPPORTED: &str = "B2C-T1010";
/// A sanitizer was left out because the toolchain cannot provide it.
pub const SANITIZER_DROPPED: &str = "B2C-T1011";
/// A hardening option was left out because the toolchain cannot provide it.
pub const HARDENING_DROPPED: &str = "B2C-T1012";
/// Static linking was not possible, so the program is linked dynamically.
pub const STATIC_DROPPED: &str = "B2C-T1013";
/// The output of the compiler is not a GCC this crate recognises.
pub const UNKNOWN_COMPILER: &str = "B2C-T1014";
/// A machine-local extra compiler or linker flag was refused.
pub const EXTRA_FLAG_REFUSED: &str = "B2C-T1015";
/// An environment variable in the pass-through list was refused.
pub const PASSTHROUGH_REFUSED: &str = "B2C-T1016";
/// A flag from `pkg-config` was dropped.
pub const PKG_CONFIG_DROPPED: &str = "B2C-T1017";
/// A library profile is invalid.
pub const BAD_LIBRARY: &str = "B2C-T1018";
/// A project define cannot be passed to the compiler.
pub const BAD_DEFINE: &str = "B2C-T1019";
/// A toolchain on a network (UNC) path.
pub const NETWORK_PATH: &str = "B2C-T1020";
/// Leak detection is not available where programs run.
pub const LEAKS_UNAVAILABLE: &str = "B2C-T1021";
/// The selected toolchain is missing or unusable, so another one is used.
pub const SELECTED_UNAVAILABLE: &str = "B2C-T1022";

/// An error about the toolchain.
pub(crate) fn error(code: &str, message: impl Into<String>) -> Diagnostic {
    Diagnostic::error(code, DiagSource::Toolchain, Location::project(), message)
}

/// A warning about the toolchain.
pub(crate) fn warning(code: &str, message: impl Into<String>) -> Diagnostic {
    Diagnostic::warning(code, DiagSource::Toolchain, Location::project(), message)
}

/// An informational note about the toolchain.
pub(crate) fn info(code: &str, message: impl Into<String>) -> Diagnostic {
    Diagnostic::info(code, DiagSource::Toolchain, Location::project(), message)
}

/// The diagnostic for "no usable g++ found" (`B2C-T1001`), for callers
/// whose discovery found nothing usable.
///
/// ```
/// let diagnostic = b2c_toolchain::codes::no_toolchain();
/// assert_eq!(diagnostic.code.0, "B2C-T1001");
/// ```
pub fn no_toolchain() -> Diagnostic {
    let help = if cfg!(windows) {
        "Install MSYS2 (https://www.msys2.org) and run `pacman -S mingw-w64-ucrt-x86_64-gcc` in its \
         UCRT64 terminal, or install WinLibs, then try again."
    } else {
        "Install it with your package manager (for example `sudo apt install g++` or `sudo dnf install \
         gcc-c++`), then try again."
    };
    error(
        NO_TOOLCHAIN,
        format!("No usable C++ compiler (g++ 11 or newer) was found on this computer. {help}"),
    )
}

/// The diagnostic for a toolchain whose fingerprint changed since it was
/// probed (`B2C-T1009`, info): the caller re-probes it.
pub fn toolchain_changed(path: &std::path::Path) -> Diagnostic {
    info(
        CHANGED,
        format!(
            "The compiler at {} changed since it was last checked (it was probably updated), so it was checked again.",
            path.display()
        ),
    )
}

/// The diagnostic for a compiler that lies inside the open project's folder
/// (`B2C-T1002`, error): it is never run, so a project cannot bring its own
/// "compiler" (spec §7.2, binary planting).
///
/// ```
/// let diagnostic = b2c_toolchain::codes::inside_project("/home/ada/game/g++".as_ref());
/// assert_eq!(diagnostic.code.0, "B2C-T1002");
/// ```
pub fn inside_project(path: &std::path::Path) -> Diagnostic {
    error(
        BAD_TOOLCHAIN_PATH,
        format!(
            "The compiler {} is inside the project's folder, so it is not used: a project must never bring its own compiler.",
            path.display()
        ),
    )
}

/// Why the selected toolchain cannot be used (the reason given by
/// [`selected_unavailable`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionProblem {
    /// It is no longer on this computer (or no longer in the toolchain
    /// list).
    Missing,
    /// It failed its checks; the toolchain list shows its problems.
    Unusable,
    /// It is inside the open project's folder.
    InsideProject,
}

impl SelectionProblem {
    /// The reason as a clause: "The selected compiler {reason}".
    fn reason(self) -> &'static str {
        match self {
            Self::Missing => "is no longer available on this computer",
            Self::Unusable => "failed its checks (the toolchain list shows why)",
            Self::InsideProject => "is inside the project's folder, where compilers are never used",
        }
    }
}

/// The diagnostic for a selected toolchain that cannot be used
/// (`B2C-T1022`, warning): the build falls back to the first usable
/// toolchain in discovery order, `fallback`, and says so, never silently
/// (spec §7.2). Without a fallback the build has no compiler, and this
/// warning comes with [`no_toolchain`].
///
/// ```
/// use b2c_toolchain::codes::{SelectionProblem, selected_unavailable};
///
/// let warning = selected_unavailable(SelectionProblem::Missing, Some("/usr/bin/g++".as_ref()));
/// assert_eq!(warning.code.0, "B2C-T1022");
/// assert!(warning.message.contains("/usr/bin/g++"));
/// ```
pub fn selected_unavailable(problem: SelectionProblem, fallback: Option<&std::path::Path>) -> Diagnostic {
    let reason = problem.reason();
    let message = match fallback {
        Some(path) => format!(
            "The selected compiler {reason}, so {} was used instead. Select another compiler in the toolchain list to stop this warning.",
            path.display()
        ),
        None => format!("The selected compiler {reason}, and no other usable compiler was found."),
    };
    warning(SELECTED_UNAVAILABLE, message)
}

#[cfg(test)]
mod tests {
    use b2c_ir::Severity;

    use super::*;

    #[test]
    fn codes_are_unique_and_well_formed() {
        let codes = [
            NO_TOOLCHAIN,
            BAD_TOOLCHAIN_PATH,
            NOT_RUNNABLE,
            TOO_OLD,
            CLANG,
            BROKEN_INSTALL,
            CYGWIN,
            LEGACY_MINGW,
            CHANGED,
            STANDARD_UNSUPPORTED,
            SANITIZER_DROPPED,
            HARDENING_DROPPED,
            STATIC_DROPPED,
            UNKNOWN_COMPILER,
            EXTRA_FLAG_REFUSED,
            PASSTHROUGH_REFUSED,
            PKG_CONFIG_DROPPED,
            BAD_LIBRARY,
            BAD_DEFINE,
            NETWORK_PATH,
            LEAKS_UNAVAILABLE,
            SELECTED_UNAVAILABLE,
        ];
        let mut sorted = codes.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), codes.len());
        for code in codes {
            assert!(code.starts_with("B2C-T1") && code.len() == 9, "{code}");
        }
    }

    #[test]
    fn constructors_set_source_and_severity() {
        let diagnostic = no_toolchain();
        assert_eq!(diagnostic.severity, Severity::Error);
        assert_eq!(diagnostic.source, DiagSource::Toolchain);
        assert_eq!(
            toolchain_changed(std::path::Path::new("/usr/bin/g++")).severity,
            Severity::Info
        );
        assert_eq!(warning(CYGWIN, "x").severity, Severity::Warning);
        let refused = inside_project(std::path::Path::new("/home/ada/game/g++"));
        assert_eq!(refused.severity, Severity::Error);
        assert_eq!(refused.code.0, BAD_TOOLCHAIN_PATH);
        assert!(refused.message.contains("/home/ada/game/g++"));
    }

    #[test]
    fn selected_unavailable_names_the_reason_and_the_fallback() {
        for problem in [
            SelectionProblem::Missing,
            SelectionProblem::Unusable,
            SelectionProblem::InsideProject,
        ] {
            let fallback = selected_unavailable(problem, Some(std::path::Path::new("/usr/bin/g++-14")));
            assert_eq!(fallback.code.0, SELECTED_UNAVAILABLE);
            assert_eq!(fallback.severity, Severity::Warning);
            assert_eq!(fallback.source, DiagSource::Toolchain);
            assert!(
                fallback.message.contains(problem.reason()),
                "{}",
                fallback.message
            );
            assert!(fallback.message.contains("/usr/bin/g++-14 was used instead"));
            let none = selected_unavailable(problem, None);
            assert_eq!(none.severity, Severity::Warning);
            assert!(none.message.contains(problem.reason()));
            assert!(none.message.contains("no other usable compiler"));
        }
    }
}
