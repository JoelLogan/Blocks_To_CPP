//! The compiler's environment: an allowlist, not a denylist (spec §7.5.2).
//!
//! g++ honours many environment variables that change what it compiles or
//! runs (`CPATH`, `GCC_EXEC_PREFIX`, `COMPILER_PATH`, `LD_PRELOAD`, …). The
//! compiler therefore gets a small, fixed environment built here, plus an
//! optional pass-through list from machine settings that can never contain
//! those variables.

use std::ffi::{OsStr, OsString};
use std::path::Path;

use b2c_ir::Diagnostic;

use crate::codes;
use crate::target::Platform;

/// Variables that can never be passed through, because they change what the
/// compiler reads, runs or writes, or because this module sets them itself.
/// Compared case-insensitively.
const REFUSED: &[&str] = &[
    // Header and library search, compiler programs.
    "CPATH",
    "C_INCLUDE_PATH",
    "CPLUS_INCLUDE_PATH",
    "OBJC_INCLUDE_PATH",
    "LIBRARY_PATH",
    "COMPILER_PATH",
    "GCC_EXEC_PREFIX",
    "GCC_ROOT",
    "COLLECT_GCC",
    "COLLECT_GCC_OPTIONS",
    "COLLECT_LTO_WRAPPER",
    "COLLECT_NO_DEMANGLE",
    "LTO_PLUGIN",
    // Extra outputs and changed diagnostics.
    "DEPENDENCIES_OUTPUT",
    "SUNPRO_DEPENDENCIES",
    "GCC_COLORS",
    "GCC_URLS",
    "TERM_URLS",
    "GCC_EXTRA_DIAGNOSTIC_OUTPUT",
    "GCC_COMPARE_DEBUG",
    "SOURCE_DATE_EPOCH",
    // Code injection into the compiler's own processes.
    "LD_PRELOAD",
    "LD_LIBRARY_PATH",
    "LD_AUDIT",
    "GCONV_PATH",
    "MALLOC_CHECK_",
    // Set here.
    "PATH",
    "HOME",
    "TMPDIR",
    "TEMP",
    "TMP",
    "LANG",
    "LC_ALL",
    "SYSTEMROOT",
    "WINDIR",
    "SYSTEMDRIVE",
];

/// Prefixes that are refused (`DYLD_*`, `LD_*`, `GCC_*`, `COLLECT_*`).
const REFUSED_PREFIXES: &[&str] = &["DYLD_", "LD_", "GCC_", "COLLECT_", "LC_"];

/// What this computer provides for the compiler's environment.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HostEnv {
    /// Linux: `HOME`.
    pub home: Option<OsString>,
    /// Windows: `SystemRoot` (usually `C:\Windows`).
    pub system_root: Option<OsString>,
    /// Windows: `windir`.
    pub windir: Option<OsString>,
    /// Windows: `SystemDrive` (usually `C:`).
    pub system_drive: Option<OsString>,
    /// Variables from the machine settings' pass-through list, with their
    /// values from this process's environment.
    pub passthrough: Vec<(OsString, OsString)>,
}

impl HostEnv {
    /// Reads the variables above from this process's environment.
    /// `passthrough_names` is the machine settings' pass-through list; names
    /// that are not set are skipped.
    pub fn from_process(passthrough_names: &[OsString]) -> Self {
        let var = |name: &str| std::env::var_os(name);
        Self {
            home: var("HOME"),
            system_root: var("SystemRoot"),
            windir: var("windir"),
            system_drive: var("SystemDrive"),
            passthrough: passthrough_names
                .iter()
                .filter_map(|name| std::env::var_os(name).map(|value| (name.clone(), value)))
                .collect(),
        }
    }
}

/// The complete environment for one compiler invocation.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CompilerEnv {
    /// The variables, in a fixed order.
    pub vars: Vec<(OsString, OsString)>,
    /// Warnings about pass-through variables that were refused
    /// (`B2C-T1016`).
    pub refused: Vec<Diagnostic>,
}

/// Checks a variable name for the pass-through list.
///
/// ```
/// use b2c_toolchain::env::check_passthrough_name;
///
/// assert!(check_passthrough_name("CCACHE_DIR".as_ref()).is_ok());
/// assert!(check_passthrough_name("LD_PRELOAD".as_ref()).is_err());
/// assert!(check_passthrough_name("cpath".as_ref()).is_err());
/// ```
///
/// # Errors
/// Returns a friendly reason when the name is refused: a variable that
/// changes what the compiler reads, runs or writes, one this module sets
/// itself, or a malformed name.
pub fn check_passthrough_name(name: &OsStr) -> Result<(), String> {
    let Some(text) = name.to_str() else {
        return Err(String::from("the name is not valid text"));
    };
    let valid = !text.is_empty()
        && text.len() <= 128
        && text.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
        && !text.as_bytes().first().is_some_and(u8::is_ascii_digit);
    if !valid {
        return Err(format!("`{text}` is not a valid variable name"));
    }
    let upper = text.to_ascii_uppercase();
    if REFUSED.contains(&upper.as_str()) || REFUSED_PREFIXES.iter().any(|prefix| upper.starts_with(prefix)) {
        return Err(format!(
            "`{text}` can change what the compiler reads, runs or writes, so it is never passed to it"
        ));
    }
    Ok(())
}

/// Builds the compiler's environment (spec §7.5.2).
///
/// * Linux: `PATH=<toolchain dir>:/usr/local/bin:/usr/bin:/bin`, `HOME`,
///   `TMPDIR=<temp_dir>`, `LC_ALL=C.UTF-8` (glibc 2.35+ has this locale built
///   in; where it is missing, the C library falls back to `C`, the spec's
///   fallback).
/// * Windows: `SystemRoot`, `windir`, `SystemDrive`, `TEMP` and `TMP` =
///   `<temp_dir>`, `PATH=<toolchain bin>;%SystemRoot%\System32;%SystemRoot%`,
///   `LANG=C`.
/// * Then the pass-through variables that [`check_passthrough_name`]
///   accepts; the others are reported in [`CompilerEnv::refused`].
///
/// `temp_dir` must be a private directory (for example a
/// `tempfile::TempDir`), never a shared `/tmp`.
///
/// ```
/// use std::path::Path;
/// use b2c_toolchain::env::{HostEnv, compiler_env};
/// use b2c_toolchain::target::Platform;
///
/// let host = HostEnv { home: Some("/home/ada".into()), ..HostEnv::default() };
/// let env = compiler_env(Platform::Linux, Path::new("/usr/bin"), Path::new("/tmp/b2c-x"), &host);
/// let path = env.vars.iter().find(|(name, _)| name == "PATH").unwrap();
/// assert_eq!(path.1, "/usr/bin:/usr/local/bin:/usr/bin:/bin");
/// assert!(env.vars.iter().all(|(name, _)| name != "CPATH"));
/// ```
pub fn compiler_env(
    platform: Platform,
    toolchain_bin: &Path,
    temp_dir: &Path,
    host: &HostEnv,
) -> CompilerEnv {
    let mut vars: Vec<(OsString, OsString)> = Vec::new();
    match platform {
        Platform::Linux => {
            let mut path = toolchain_bin.as_os_str().to_os_string();
            path.push(":/usr/local/bin:/usr/bin:/bin");
            vars.push(("PATH".into(), path));
            if let Some(home) = &host.home {
                vars.push(("HOME".into(), home.clone()));
            }
            vars.push(("TMPDIR".into(), temp_dir.as_os_str().to_os_string()));
            vars.push(("LC_ALL".into(), "C.UTF-8".into()));
        }
        Platform::Windows => {
            let system_root = host.system_root.clone().or_else(|| host.windir.clone());
            if let Some(root) = &system_root {
                vars.push(("SystemRoot".into(), root.clone()));
            }
            if let Some(windir) = host.windir.clone().or_else(|| system_root.clone()) {
                vars.push(("windir".into(), windir));
            }
            if let Some(drive) = &host.system_drive {
                vars.push(("SystemDrive".into(), drive.clone()));
            }
            vars.push(("TEMP".into(), temp_dir.as_os_str().to_os_string()));
            vars.push(("TMP".into(), temp_dir.as_os_str().to_os_string()));
            let mut path = toolchain_bin.as_os_str().to_os_string();
            if let Some(root) = &system_root {
                path.push(";");
                path.push(root);
                path.push("\\System32;");
                path.push(root);
            }
            vars.push(("PATH".into(), path));
            vars.push(("LANG".into(), "C".into()));
        }
    }
    let mut refused = Vec::new();
    for (name, value) in &host.passthrough {
        match check_passthrough_name(name) {
            Ok(()) => {
                if !vars
                    .iter()
                    .any(|(existing, _)| existing.eq_ignore_ascii_case(name))
                {
                    vars.push((name.clone(), value.clone()));
                }
            }
            Err(reason) => refused.push(codes::warning(
                codes::PASSTHROUGH_REFUSED,
                format!("The environment variable was not passed to the compiler: {reason}."),
            )),
        }
    }
    CompilerEnv { vars, refused }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn get<'a>(env: &'a CompilerEnv, name: &str) -> Option<&'a OsStr> {
        env.vars
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_os_str())
    }

    #[test]
    fn linux_environment() {
        let host = HostEnv {
            home: Some("/home/ada".into()),
            system_root: Some("C:\\Windows".into()),
            ..HostEnv::default()
        };
        let env = compiler_env(
            Platform::Linux,
            Path::new("/opt/gcc/bin"),
            Path::new("/tmp/b2c-1"),
            &host,
        );
        let names: Vec<_> = env
            .vars
            .iter()
            .map(|(n, _)| n.to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, ["PATH", "HOME", "TMPDIR", "LC_ALL"]);
        assert_eq!(
            get(&env, "PATH"),
            Some(OsStr::new("/opt/gcc/bin:/usr/local/bin:/usr/bin:/bin"))
        );
        assert_eq!(get(&env, "TMPDIR"), Some(OsStr::new("/tmp/b2c-1")));
        assert_eq!(get(&env, "LC_ALL"), Some(OsStr::new("C.UTF-8")));
    }

    #[test]
    fn windows_environment() {
        let host = HostEnv {
            system_root: Some(r"C:\Windows".into()),
            windir: Some(r"C:\Windows".into()),
            system_drive: Some("C:".into()),
            ..HostEnv::default()
        };
        let env = compiler_env(
            Platform::Windows,
            Path::new(r"C:\msys64\ucrt64\bin"),
            Path::new(r"C:\Users\ada\AppData\Local\Temp\b2c-1"),
            &host,
        );
        let names: Vec<_> = env
            .vars
            .iter()
            .map(|(n, _)| n.to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            names,
            [
                "SystemRoot",
                "windir",
                "SystemDrive",
                "TEMP",
                "TMP",
                "PATH",
                "LANG"
            ]
        );
        assert_eq!(
            get(&env, "PATH"),
            Some(OsStr::new(r"C:\msys64\ucrt64\bin;C:\Windows\System32;C:\Windows"))
        );
        assert_eq!(get(&env, "LANG"), Some(OsStr::new("C")));
    }

    #[test]
    fn dangerous_variables_are_never_passed_through() {
        for name in [
            "CPATH",
            "CPLUS_INCLUDE_PATH",
            "LIBRARY_PATH",
            "COMPILER_PATH",
            "GCC_EXEC_PREFIX",
            "DEPENDENCIES_OUTPUT",
            "SUNPRO_DEPENDENCIES",
            "GCC_COLORS",
            "LD_PRELOAD",
            "ld_preload",
            "LD_ANYTHING",
            "DYLD_INSERT_LIBRARIES",
            "PATH",
            "Path",
            "TMPDIR",
            "LC_CTYPE",
            "",
            "A=B",
            "1ABC",
            "WITH SPACE",
        ] {
            assert!(check_passthrough_name(OsStr::new(name)).is_err(), "{name}");
        }
        for name in ["CCACHE_DIR", "HTTP_PROXY", "MY_VAR", "_X"] {
            assert!(check_passthrough_name(OsStr::new(name)).is_ok(), "{name}");
        }
    }

    /// Set in the child process started by [`host_env_is_read_from_the_process`].
    const HOST_ENV_PROBE: &str = "B2C_TEST_HOST_ENV_PROBE";

    /// What the child prints once its checks passed (a filter that matched
    /// no test would also exit successfully).
    const HOST_ENV_PROBE_PASSED: &str = "host env probe passed";

    /// [`HostEnv::from_process`] reads this process's environment, which a
    /// test cannot change safely while other tests run. So the test binary
    /// is started again with exactly the variables below and checks what it
    /// reads in [`host_env_probe`].
    #[cfg(unix)]
    #[test]
    fn host_env_is_read_from_the_process() {
        let exe = std::env::current_exe().unwrap();
        let mut command = b2c_process::Command::new(exe, std::env::temp_dir()).unwrap();
        command
            .args(["--exact", "env::tests::host_env_probe", "--nocapture"])
            .env(HOST_ENV_PROBE, "1")
            .env("HOME", "/home/ada")
            .env("SystemRoot", r"C:\Windows")
            .env("windir", r"C:\WINDOWS")
            .env("SystemDrive", "D:")
            .env("CCACHE_DIR", "/var/cache/ccache")
            .env("MY_FLAGS", "")
            .env("NOT_LISTED", "x")
            .timeout(std::time::Duration::from_mins(1));
        // A coverage run collects the child's counters through this.
        if let Some(value) = std::env::var_os("LLVM_PROFILE_FILE") {
            command.env("LLVM_PROFILE_FILE", value);
        }
        let captured = b2c_process::run_captured(&command).unwrap();
        let stdout = String::from_utf8_lossy(&captured.stdout);
        assert!(
            captured.status.success() && stdout.contains(HOST_ENV_PROBE_PASSED),
            "{stdout}{}",
            String::from_utf8_lossy(&captured.stderr)
        );
    }

    /// Not a check of its own: in the child process started by
    /// [`host_env_is_read_from_the_process`] it checks what
    /// [`HostEnv::from_process`] reads. In an ordinary test run it does
    /// nothing.
    #[test]
    fn host_env_probe() {
        if std::env::var_os(HOST_ENV_PROBE).is_none() {
            return;
        }
        let names: Vec<OsString> = ["MY_FLAGS", "UNSET_VARIABLE", "CCACHE_DIR"]
            .into_iter()
            .map(OsString::from)
            .collect();
        let host = HostEnv::from_process(&names);
        assert_eq!(
            host,
            HostEnv {
                home: Some("/home/ada".into()),
                system_root: Some(r"C:\Windows".into()),
                windir: Some(r"C:\WINDOWS".into()),
                system_drive: Some("D:".into()),
                // In list order; names that are not set are skipped, empty
                // values are kept, and unlisted variables are never read.
                passthrough: vec![
                    ("MY_FLAGS".into(), OsString::new()),
                    ("CCACHE_DIR".into(), "/var/cache/ccache".into()),
                ],
            }
        );
        println!("{HOST_ENV_PROBE_PASSED}");
    }

    #[test]
    fn passthrough_names_are_at_most_128_characters() {
        let longest = format!("V{}", "_".repeat(127));
        assert!(check_passthrough_name(OsStr::new(&longest)).is_ok());
        let too_long = format!("{longest}X");
        let reason = check_passthrough_name(OsStr::new(&too_long)).unwrap_err();
        assert!(reason.contains("is not a valid variable name"), "{reason}");
    }

    #[cfg(unix)]
    #[test]
    fn passthrough_names_must_be_text() {
        use std::os::unix::ffi::OsStrExt as _;
        let reason = check_passthrough_name(OsStr::from_bytes(b"MY_\xffVAR")).unwrap_err();
        assert_eq!(reason, "the name is not valid text");
    }

    #[test]
    fn passthrough_adds_allowed_and_reports_refused() {
        let host = HostEnv {
            passthrough: vec![
                ("CCACHE_DIR".into(), "/cache".into()),
                ("LD_PRELOAD".into(), "/evil.so".into()),
            ],
            ..HostEnv::default()
        };
        let env = compiler_env(Platform::Linux, Path::new("/usr/bin"), Path::new("/t"), &host);
        assert_eq!(get(&env, "CCACHE_DIR"), Some(OsStr::new("/cache")));
        assert_eq!(get(&env, "LD_PRELOAD"), None);
        assert_eq!(env.refused.len(), 1);
        assert_eq!(env.refused[0].code.0, codes::PASSTHROUGH_REFUSED);
    }
}
