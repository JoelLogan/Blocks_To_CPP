//! The environment of a program run from the IDE
//! (`docs/spec/07-toolchain-build-run.md` §7.6.2, "Environment").
//!
//! Programs get the user's own environment, because they legitimately need it
//! (`HOME`, `LANG`, `PATH` for a program that runs others, …), minus the IDE's
//! internal variables, plus what the console and the build need:
//!
//! | Variable | When |
//! |----------|------|
//! | `TERM=xterm-256color` | Linux: the console is xterm.js, whatever terminal started the app |
//! | `ASAN_OPTIONS=halt_on_error=1:detect_leaks=1` | sanitizer builds (`detect_leaks=0` where leak detection does not work, `B2C-T1021`) |
//! | `UBSAN_OPTIONS=print_stacktrace=1:halt_on_error=1` | sanitizer builds |
//! | the toolchain's `bin` folder first on `PATH` | Windows, for a program linked dynamically against the compiler's DLLs (`B2C-T1013`) |
//!
//! Removed: `B2C_*`, `TAURI_*`, `WEBVIEW2_*`, `WEBKIT_*`, `APPDIR`, `APPIMAGE`,
//! `ARGV0` and `OWD` (names compared without case on Windows, as Windows
//! does). `B2C_EVENTS` is never set in M2: the event channel arrives in M5.
//! Library `runtimeDirs` arrive with libraries in M4.

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::path::Path;

use b2c_toolchain::target::Platform;

/// What decides the extra variables of a run's environment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RunEnvOptions<'a> {
    /// The platform the program runs on (it decides `TERM` and how names
    /// compare).
    pub platform: Platform,
    /// Whether the program was built with sanitizers (a Debug build on a
    /// toolchain that has them).
    pub sanitizers: bool,
    /// Whether AddressSanitizer's leak detection works where the program runs
    /// (the toolchain probe's `leak_detection`).
    pub leak_detection: bool,
    /// On Windows, a folder to put first on `PATH`: the build record's
    /// `toolchain_bin` (`record.toolchain_bin.as_deref()`), which is set
    /// only for a program linked dynamically against the compiler's runtime
    /// DLLs. Ignored on Linux.
    pub toolchain_bin: Option<&'a Path>,
}

/// The value of `TERM` on Linux: the console is xterm.js.
pub const TERM: &str = "xterm-256color";

/// `ASAN_OPTIONS` for sanitizer builds with leak detection.
pub const ASAN_OPTIONS: &str = "halt_on_error=1:detect_leaks=1";

/// `ASAN_OPTIONS` for sanitizer builds where leak detection does not work.
pub const ASAN_OPTIONS_NO_LEAKS: &str = "halt_on_error=1:detect_leaks=0";

/// `UBSAN_OPTIONS` for sanitizer builds.
pub const UBSAN_OPTIONS: &str = "print_stacktrace=1:halt_on_error=1";

/// Prefixes of the IDE's internal variables, which programs never see.
const INTERNAL_PREFIXES: &[&str] = &["B2C_", "TAURI_", "WEBVIEW2_", "WEBKIT_"];

/// Names of the IDE's internal variables (the `AppImage` runtime's among them).
const INTERNAL_NAMES: &[&str] = &["APPDIR", "APPIMAGE", "ARGV0", "OWD"];

/// The environment for a program run from the IDE
/// (`docs/spec/07-toolchain-build-run.md` §7.6.2): `host` (normally
/// [`std::env::vars_os`]) without the IDE's internal variables, plus:
///
/// * `TERM=`[`TERM`] on Linux, whatever terminal started the app (the
///   console is xterm.js);
/// * for sanitizer builds, `ASAN_OPTIONS=`[`ASAN_OPTIONS`] (or
///   [`ASAN_OPTIONS_NO_LEAKS`] where leak detection does not work,
///   `B2C-T1021`) and `UBSAN_OPTIONS=`[`UBSAN_OPTIONS`], replacing the user's
///   own values;
/// * on Windows, [`RunEnvOptions::toolchain_bin`] before the user's `PATH`
///   (separated by `;`; the existing variable keeps its spelling, `Path` for
///   example, and is created when there is none), so a dynamically linked
///   program finds the compiler's DLLs before any other copy. A folder
///   whose name contains `;` cannot be put on `PATH` and is left out.
///
/// The internal variables are `B2C_*`, `TAURI_*`, `WEBVIEW2_*`, `WEBKIT_*`,
/// `APPDIR`, `APPIMAGE`, `ARGV0` and `OWD`; on Windows names compare without
/// case, as Windows compares them. `B2C_EVENTS` is never set in M2.
///
/// Later entries of `host` replace earlier ones with the same name. Entries
/// that cannot be passed to a program (an empty name, a name with `=` after
/// its first character, a NUL anywhere) are dropped. The result is sorted by
/// name, with one entry per name.
///
/// ```
/// use std::ffi::OsString;
/// use b2c_build::{RunEnvOptions, run_environment};
/// use b2c_toolchain::target::Platform;
///
/// let host = [("HOME", "/home/ada"), ("B2C_LOG", "debug"), ("TERM", "screen")]
///     .map(|(name, value)| (OsString::from(name), OsString::from(value)));
/// let options = RunEnvOptions {
///     platform: Platform::Linux,
///     sanitizers: false,
///     leak_detection: true,
///     toolchain_bin: None,
/// };
/// let env = run_environment(host, &options);
/// assert_eq!(env, [("HOME", "/home/ada"), ("TERM", "xterm-256color")]
///     .map(|(name, value)| (OsString::from(name), OsString::from(value))));
/// ```
pub fn run_environment(
    host: impl IntoIterator<Item = (OsString, OsString)>,
    options: &RunEnvOptions<'_>,
) -> Vec<(OsString, OsString)> {
    let windows = options.platform == Platform::Windows;
    let key = |name: &OsStr| {
        if windows {
            name.to_ascii_uppercase()
        } else {
            name.to_os_string()
        }
    };
    let mut env: BTreeMap<OsString, (OsString, OsString)> = BTreeMap::new();
    for (name, value) in host {
        if is_passable(&name, &value) && !is_internal(&name, windows) {
            env.insert(key(&name), (name, value));
        }
    }
    let mut set = |name: &str, value: &str| {
        let name = OsString::from(name);
        env.insert(key(&name), (name, OsString::from(value)));
    };
    if options.platform == Platform::Linux {
        set("TERM", TERM);
    }
    if options.sanitizers {
        let asan = if options.leak_detection {
            ASAN_OPTIONS
        } else {
            ASAN_OPTIONS_NO_LEAKS
        };
        set("ASAN_OPTIONS", asan);
        set("UBSAN_OPTIONS", UBSAN_OPTIONS);
    }
    if windows && let Some(folder) = options.toolchain_bin {
        prepend_to_path(&mut env, folder.as_os_str());
    }
    env.into_values().collect()
}

/// Puts `folder` first on the Windows `PATH` in `env` (keyed by upper-case
/// name), keeping the existing variable's spelling. A folder that cannot be
/// one `PATH` entry (empty, or with `;` or NUL in it) is left out.
fn prepend_to_path(env: &mut BTreeMap<OsString, (OsString, OsString)>, folder: &OsStr) {
    let bytes = folder.as_encoded_bytes();
    if bytes.is_empty() || bytes.contains(&b';') || bytes.contains(&0) {
        tracing::debug!("the toolchain folder cannot be put on PATH");
        return;
    }
    let (name, old) = env
        .remove(OsStr::new("PATH"))
        .unwrap_or_else(|| (OsString::from("PATH"), OsString::new()));
    let mut value = folder.to_os_string();
    if !old.is_empty() {
        value.push(";");
        value.push(&old);
    }
    env.insert(OsString::from("PATH"), (name, value));
}

/// Whether a variable can be given to a program at all.
fn is_passable(name: &OsStr, value: &OsStr) -> bool {
    let name = name.as_encoded_bytes();
    let value = value.as_encoded_bytes();
    // Windows keeps per-drive folders in variables named `=C:`, so `=` may
    // only come first.
    !name.is_empty() && !name[1..].contains(&b'=') && !name.contains(&0) && !value.contains(&0)
}

/// Whether `name` is one of the IDE's internal variables.
fn is_internal(name: &OsStr, windows: bool) -> bool {
    let name = name.as_encoded_bytes();
    let name = if windows {
        name.to_ascii_uppercase()
    } else {
        name.to_vec()
    };
    INTERNAL_NAMES.iter().any(|internal| name == internal.as_bytes())
        || INTERNAL_PREFIXES
            .iter()
            .any(|prefix| name.starts_with(prefix.as_bytes()))
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    fn vars(pairs: &[(&str, &str)]) -> Vec<(OsString, OsString)> {
        pairs
            .iter()
            .map(|&(name, value)| (OsString::from(name), OsString::from(value)))
            .collect()
    }

    fn options(platform: Platform, sanitizers: bool, leak_detection: bool) -> RunEnvOptions<'static> {
        RunEnvOptions {
            platform,
            sanitizers,
            leak_detection,
            toolchain_bin: None,
        }
    }

    fn get<'a>(env: &'a [(OsString, OsString)], name: &str) -> Option<&'a OsStr> {
        env.iter()
            .find(|(n, _)| n == name)
            .map(|(_, value)| value.as_os_str())
    }

    #[test]
    fn internal_variables_are_removed() {
        let host = vars(&[
            ("PATH", "/usr/bin:/bin"),
            ("B2C_LOG", "debug"),
            ("B2C_E2E_ROOT", "/tmp/e2e"),
            ("TAURI_ENV_DEBUG", "true"),
            ("WEBVIEW2_USER_DATA_FOLDER", "x"),
            ("WEBKIT_DISABLE_COMPOSITING_MODE", "1"),
            ("APPDIR", "/tmp/.mount"),
            ("APPIMAGE", "/opt/b2c.AppImage"),
            ("ARGV0", "b2c"),
            ("OWD", "/home/ada"),
            ("B2C", "kept: not a prefix match"),
            ("MY_B2C_VAR", "kept"),
            ("APPDIRS", "kept"),
            ("b2c_lower", "kept on Linux"),
        ]);
        let env = run_environment(host, &options(Platform::Linux, false, true));
        assert_eq!(
            env,
            vars(&[
                ("B2C", "kept: not a prefix match"),
                ("MY_B2C_VAR", "kept"),
                ("APPDIRS", "kept"),
                ("PATH", "/usr/bin:/bin"),
                ("TERM", TERM),
                ("b2c_lower", "kept on Linux"),
            ])
            .into_iter()
            .collect::<BTreeMap<_, _>>()
            .into_iter()
            .collect::<Vec<_>>()
        );
    }

    #[test]
    fn windows_compares_names_without_case() {
        let host = vars(&[
            ("Path", r"C:\Windows"),
            ("b2c_log", "debug"),
            ("Tauri_Env", "1"),
            ("AppData", r"C:\Users\ada\AppData\Roaming"),
            ("asan_options", "detect_leaks=1:verbosity=2"),
            ("=C:", r"C:\work"),
        ]);
        let env = run_environment(host, &options(Platform::Windows, true, false));
        assert_eq!(get(&env, "Path"), Some(OsStr::new(r"C:\Windows")));
        assert_eq!(get(&env, "=C:"), Some(OsStr::new(r"C:\work")));
        assert_eq!(get(&env, "b2c_log"), None);
        assert_eq!(get(&env, "Tauri_Env"), None);
        assert_eq!(get(&env, "TERM"), None, "TERM is set on Linux only");
        // The user's variable is replaced, not duplicated.
        assert_eq!(get(&env, "asan_options"), None);
        assert_eq!(get(&env, "ASAN_OPTIONS"), Some(OsStr::new(ASAN_OPTIONS_NO_LEAKS)));
        assert_eq!(env.len(), 5);
    }

    #[test]
    fn sanitizer_options_only_for_sanitizer_builds() {
        let host = vars(&[("HOME", "/home/ada"), ("TERM", "screen-256color")]);
        let plain = run_environment(host.clone(), &options(Platform::Linux, false, true));
        assert_eq!(get(&plain, "TERM"), Some(OsStr::new(TERM)));
        assert_eq!(get(&plain, "ASAN_OPTIONS"), None);
        assert_eq!(get(&plain, "UBSAN_OPTIONS"), None);
        assert_eq!(get(&plain, "B2C_EVENTS"), None);

        let checked = run_environment(host.clone(), &options(Platform::Linux, true, true));
        assert_eq!(get(&checked, "ASAN_OPTIONS"), Some(OsStr::new(ASAN_OPTIONS)));
        assert_eq!(get(&checked, "UBSAN_OPTIONS"), Some(OsStr::new(UBSAN_OPTIONS)));
        let no_leaks = run_environment(host, &options(Platform::Linux, true, false));
        assert_eq!(
            get(&no_leaks, "ASAN_OPTIONS"),
            Some(OsStr::new(ASAN_OPTIONS_NO_LEAKS))
        );
        assert_eq!(get(&no_leaks, "B2C_EVENTS"), None);
    }

    #[test]
    fn unpassable_entries_are_dropped_and_later_ones_win() {
        let host = vars(&[
            ("", "empty name"),
            ("A=B", "equals in the name"),
            ("NUL\0NAME", "x"),
            ("NULVALUE", "a\0b"),
            ("LANG", "C"),
            ("LANG", "en_GB.UTF-8"),
        ]);
        let env = run_environment(host, &options(Platform::Linux, false, true));
        assert_eq!(env, vars(&[("LANG", "en_GB.UTF-8"), ("TERM", TERM)]));
    }

    #[test]
    fn the_toolchain_folder_goes_first_on_the_windows_path() {
        let bin = Path::new(r"C:\msys64\ucrt64\bin");
        let with_bin = |platform| RunEnvOptions {
            toolchain_bin: Some(bin),
            ..options(platform, false, true)
        };
        // The existing variable keeps its spelling, whatever its case.
        let host = vars(&[("Path", r"C:\Windows;C:\Tools"), ("HOME", "x")]);
        let env = run_environment(host.clone(), &with_bin(Platform::Windows));
        assert_eq!(
            get(&env, "Path"),
            Some(OsStr::new(r"C:\msys64\ucrt64\bin;C:\Windows;C:\Tools"))
        );
        assert_eq!(get(&env, "PATH"), None);
        assert_eq!(env.len(), 2);
        // Without a PATH, one is made.
        let env = run_environment(vars(&[("HOME", "x")]), &with_bin(Platform::Windows));
        assert_eq!(get(&env, "PATH"), Some(bin.as_os_str()));
        // A statically linked program (no folder) leaves PATH alone, and so
        // does Linux.
        let env = run_environment(host.clone(), &options(Platform::Windows, false, true));
        assert_eq!(get(&env, "Path"), Some(OsStr::new(r"C:\Windows;C:\Tools")));
        let linux = vars(&[("PATH", "/usr/bin")]);
        let env = run_environment(linux, &with_bin(Platform::Linux));
        assert_eq!(get(&env, "PATH"), Some(OsStr::new("/usr/bin")));
        // A folder that would split into two entries is left out.
        let odd = RunEnvOptions {
            toolchain_bin: Some(Path::new(r"C:\a;b\bin")),
            ..options(Platform::Windows, false, true)
        };
        let env = run_environment(host, &odd);
        assert_eq!(get(&env, "Path"), Some(OsStr::new(r"C:\Windows;C:\Tools")));
    }

    proptest! {
        #[test]
        fn no_internal_variable_ever_reaches_a_program(
            names in proptest::collection::vec("(B2C_|TAURI_|WEBVIEW2_|WEBKIT_|APPDIR|OWD|PATH|HOME)[A-Za-z_]{0,6}", 0..12),
            windows in any::<bool>(),
            sanitizers in any::<bool>(),
        ) {
            let platform = if windows { Platform::Windows } else { Platform::Linux };
            let host: Vec<(OsString, OsString)> = names
                .iter()
                .map(|name| (OsString::from(name), OsString::from("v")))
                .collect();
            let env = run_environment(host, &options(platform, sanitizers, true));
            let mut seen = std::collections::BTreeSet::new();
            for (name, _) in &env {
                prop_assert!(!is_internal(name, windows), "{:?}", name);
                let key = if windows { name.to_ascii_uppercase() } else { name.clone() };
                prop_assert!(seen.insert(key), "duplicate {:?}", name);
            }
            prop_assert_eq!(get(&env, "TERM").is_some(), !windows);
            prop_assert_eq!(get(&env, "ASAN_OPTIONS").is_some(), sanitizers);
        }
    }
}
