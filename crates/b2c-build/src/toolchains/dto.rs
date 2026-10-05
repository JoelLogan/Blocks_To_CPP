//! The IPC view of a toolchain (`docs/spec/07-toolchain-build-run.md` §7.3):
//! what the setup page, the toolchain list and the status bar show.

use std::path::Path;

use b2c_ipc::ToolchainId;
use b2c_ipc::dto::{CppStandard, Toolchain as ToolchainDto, ToolchainCapabilities, ToolchainSource};
use b2c_toolchain::probe::{DiagnosticsFormat, Toolchain};
use b2c_toolchain::target::Platform;

/// The IPC [`ToolchainDto`] for a probed toolchain.
///
/// * `id` is [`ToolchainId::for_driver`] of the canonical driver path;
/// * `displayPath` is `found_as`, the path it was found as (for display
///   only, never accepted back);
/// * `flavor` comes from [`flavor`];
/// * `usable` is [`Toolchain::is_usable`] and `selected` whether `selected`
///   is this toolchain's ID;
/// * `capabilities.sanitizers` says whether the Debug configuration's
///   sanitizers work: AddressSanitizer with the undefined-behaviour
///   sanitizer for Linux programs, the undefined-behaviour sanitizer in trap
///   mode for Windows programs (AddressSanitizer does not exist for
///   MinGW-w64); `sarif` whether g++ writes SARIF diagnostics;
/// * `problems` are the probe's problems, location warnings first.
pub fn toolchain_dto(
    toolchain: &Toolchain,
    source: ToolchainSource,
    found_as: &Path,
    selected: Option<&ToolchainId>,
) -> ToolchainDto {
    let id = ToolchainId::for_driver(toolchain.path());
    let capabilities = &toolchain.capabilities;
    let standards = [
        (CppStandard::Cpp17, &capabilities.standards.cpp17),
        (CppStandard::Cpp20, &capabilities.standards.cpp20),
        (CppStandard::Cpp23, &capabilities.standards.cpp23),
        (CppStandard::Cpp26, &capabilities.standards.cpp26),
    ]
    .into_iter()
    .filter(|(_, spelling)| spelling.is_some())
    .map(|(standard, _)| standard)
    .collect();
    let sanitizers = match toolchain.platform() {
        Platform::Linux => capabilities.sanitizers.address_undefined,
        Platform::Windows => capabilities.sanitizers.undefined_trap,
    };
    ToolchainDto {
        selected: selected == Some(&id),
        id,
        version: toolchain.version.map(|version| version.to_string()),
        target: Some(toolchain.target.triple.clone()).filter(|triple| !triple.is_empty()),
        flavor: flavor(toolchain.path()).or_else(|| flavor(found_as)),
        display_path: found_as.to_string_lossy().into_owned(),
        source,
        usable: toolchain.is_usable(),
        capabilities: ToolchainCapabilities {
            standards,
            std_format: capabilities.library.format,
            sanitizers,
            sarif: matches!(
                capabilities.diagnostics,
                Some(DiagnosticsFormat::AddOutputSarif | DiagnosticsFormat::SarifFile)
            ),
        },
        problems: b2c_ipc::diag::convert_all(&toolchain.problems),
    }
}

/// MSYS2 environments and their display names.
const MSYS2_ENVIRONMENTS: &[(&str, &str)] = &[
    ("ucrt64", "MSYS2 UCRT64"),
    ("mingw64", "MSYS2 MINGW64"),
    ("clang64", "MSYS2 CLANG64"),
    ("mingw32", "MSYS2 MINGW32"),
];

/// A display name for where a compiler comes from, guessed from its path
/// (only for display; nothing depends on it):
///
/// * `<msys root>\{ucrt64,mingw64,clang64,mingw32}\bin\g++.exe`, where the
///   root folder's name starts with `msys` (`C:\msys64`, `D:\tools\msys2`):
///   *MSYS2 UCRT64* and so on;
/// * a `WinLibs` folder (`BrechtSanders.WinLibs.*` from `WinGet`, Scoop's
///   `mingw-winlibs`, or any other folder whose name contains `winlibs`):
///   *`WinLibs`*;
/// * Strawberry Perl's `Strawberry\c\bin`: *Strawberry Perl*;
/// * anything else under `scoop\apps`: *Scoop*;
/// * `/usr/bin` or `/bin`: *System*;
/// * otherwise `None`.
///
/// Folder names are compared without regard to case, and both `/` and `\`
/// separate them, so Windows paths are recognised on every host.
///
/// ```
/// use b2c_build::toolchains::flavor;
///
/// assert_eq!(flavor(r"C:\msys64\ucrt64\bin\g++.exe".as_ref()).as_deref(), Some("MSYS2 UCRT64"));
/// assert_eq!(flavor("/usr/bin/x86_64-linux-gnu-g++-13".as_ref()).as_deref(), Some("System"));
/// assert_eq!(flavor("/opt/gcc-15/bin/g++".as_ref()), None);
/// ```
pub fn flavor(path: &Path) -> Option<String> {
    let text = path.to_string_lossy().to_lowercase();
    let parts: Vec<&str> = text.split(['/', '\\']).filter(|part| !part.is_empty()).collect();
    // The folders above the file, innermost first.
    let folders: Vec<&str> = parts.iter().rev().skip(1).copied().collect();
    if let [bin, environment, root, ..] = folders.as_slice()
        && *bin == "bin"
        && root.starts_with("msys")
        && let Some((_, name)) = MSYS2_ENVIRONMENTS.iter().find(|(env, _)| env == environment)
    {
        return Some((*name).to_owned());
    }
    // WinGet's `BrechtSanders.WinLibs.*`, Scoop's `mingw-winlibs`, `C:\winlibs`.
    if folders.iter().any(|folder| folder.contains("winlibs")) {
        return Some(String::from("WinLibs"));
    }
    if let [bin, c, strawberry, ..] = folders.as_slice()
        && *bin == "bin"
        && *c == "c"
        && *strawberry == "strawberry"
    {
        return Some(String::from("Strawberry Perl"));
    }
    if parts.windows(2).any(|pair| pair == ["scoop", "apps"]) {
        return Some(String::from("Scoop"));
    }
    let unix_folder = path.parent().and_then(Path::to_str);
    if matches!(unix_folder, Some("/usr/bin" | "/bin")) {
        return Some(String::from("System"));
    }
    None
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use b2c_ir::{DiagSource, Diagnostic, Location};
    use b2c_toolchain::fingerprint::Fingerprint;
    use b2c_toolchain::probe::{Capabilities, CompilerKind, PROBE_FORMAT, Standards};
    use b2c_toolchain::target::{GccVersion, Target};

    use super::*;

    fn named(path: &str) -> Option<String> {
        flavor(Path::new(path))
    }

    #[test]
    fn flavors_from_windows_install_folders() {
        assert_eq!(
            named(r"C:\msys64\ucrt64\bin\g++.exe").as_deref(),
            Some("MSYS2 UCRT64")
        );
        assert_eq!(
            named(r"C:\MSYS64\MINGW64\BIN\G++.EXE").as_deref(),
            Some("MSYS2 MINGW64")
        );
        assert_eq!(
            named(r"D:\tools\msys2\clang64\bin\g++.exe").as_deref(),
            Some("MSYS2 CLANG64")
        );
        assert_eq!(
            named(r"C:\msys64\mingw32\bin\g++.exe").as_deref(),
            Some("MSYS2 MINGW32")
        );
        // An environment folder without an MSYS2 root, or the wrong layout.
        assert_eq!(named(r"D:\dev\ucrt64\bin\g++.exe"), None);
        assert_eq!(named(r"C:\msys64\usr\bin\g++.exe"), None);
        assert_eq!(named(r"C:\msys64\ucrt64\g++.exe"), None);
        assert_eq!(
            named(r"C:\Users\Ada\AppData\Local\Microsoft\WinGet\Packages\BrechtSanders.WinLibs.POSIX.UCRT_Microsoft.Winget.Source_8wekyb3d8bbwe\mingw64\bin\g++.exe")
                .as_deref(),
            Some("WinLibs")
        );
        assert_eq!(
            named(r"C:\Users\Ada\scoop\apps\mingw-winlibs\current\bin\g++.exe").as_deref(),
            Some("WinLibs")
        );
        assert_eq!(
            named(r"C:\winlibs\mingw64\bin\g++.exe").as_deref(),
            Some("WinLibs")
        );
        assert_eq!(
            named(r"C:\Strawberry\c\bin\g++.exe").as_deref(),
            Some("Strawberry Perl")
        );
        assert_eq!(
            named(r"C:\Users\Ada\scoop\apps\gcc\current\bin\g++.exe").as_deref(),
            Some("Scoop")
        );
        assert_eq!(named(r"C:\TDM-GCC-64\bin\g++.exe"), None);
        assert_eq!(named(r"C:\MinGW\bin\g++.exe"), None);
    }

    #[test]
    fn flavors_on_linux() {
        assert_eq!(named("/usr/bin/g++").as_deref(), Some("System"));
        assert_eq!(named("/bin/g++-13").as_deref(), Some("System"));
        assert_eq!(named("/usr/local/bin/g++"), None);
        assert_eq!(named("/opt/rh/gcc-toolset-14/root/usr/bin/g++"), None);
        assert_eq!(named("/home/linuxbrew/.linuxbrew/bin/g++-15"), None);
        assert_eq!(named("/usr/bin"), None);
        assert_eq!(named(""), None);
    }

    fn probed(path: &str, triple: &str) -> Toolchain {
        let mut capabilities = Capabilities {
            standards: Standards {
                cpp17: Some(String::from("c++17")),
                cpp20: Some(String::from("c++20")),
                cpp23: Some(String::from("c++2b")),
                cpp26: None,
            },
            ..Capabilities::default()
        };
        capabilities.library.format = true;
        capabilities.diagnostics = Some(DiagnosticsFormat::SarifFile);
        capabilities.sanitizers.address_undefined = true;
        Toolchain {
            format: PROBE_FORMAT,
            fingerprint: Fingerprint {
                path: PathBuf::from(path),
                size: 1,
                modified_ns: 2,
                sha256: "0".repeat(64),
            },
            kind: CompilerKind::Gcc,
            version: GccVersion::parse("13.3.0"),
            version_text: String::from("g++ (Ubuntu 13.3.0) 13.3.0"),
            target: Target::parse(triple),
            capabilities,
            problems: vec![Diagnostic::warning(
                "B2C-T1008",
                DiagSource::Toolchain,
                Location::project(),
                "old",
            )],
        }
    }

    #[test]
    fn the_dto_reports_what_the_pages_show() {
        let toolchain = probed("/usr/bin/x86_64-linux-gnu-g++-13", "x86_64-linux-gnu");
        let id = ToolchainId::for_driver(toolchain.path());
        let dto = toolchain_dto(
            &toolchain,
            ToolchainSource::Path,
            Path::new("/usr/bin/g++"),
            Some(&id),
        );
        assert_eq!(dto.id, id);
        assert!(dto.selected);
        assert!(dto.usable);
        assert_eq!(dto.version.as_deref(), Some("13.3.0"));
        assert_eq!(dto.target.as_deref(), Some("x86_64-linux-gnu"));
        assert_eq!(dto.display_path, "/usr/bin/g++");
        assert_eq!(dto.flavor.as_deref(), Some("System"));
        assert_eq!(
            dto.capabilities.standards,
            [CppStandard::Cpp17, CppStandard::Cpp20, CppStandard::Cpp23]
        );
        assert!(dto.capabilities.std_format && dto.capabilities.sanitizers && dto.capabilities.sarif);
        assert_eq!(dto.problems.len(), 1);
        assert_eq!(dto.problems[0].code, "B2C-T1008");

        let other = ToolchainId::for_driver(Path::new("/other"));
        assert!(!toolchain_dto(&toolchain, ToolchainSource::Path, Path::new("/x"), Some(&other)).selected);
        assert!(!toolchain_dto(&toolchain, ToolchainSource::Path, Path::new("/x"), None).selected);
    }

    #[test]
    fn windows_programs_report_the_trap_sanitizer_and_empty_targets_are_null() {
        let mut toolchain = probed(r"C:\msys64\ucrt64\bin\g++.exe", "x86_64-w64-mingw32");
        // AddressSanitizer never exists for MinGW-w64: only the trap-mode
        // undefined-behaviour sanitizer counts.
        assert!(
            !toolchain_dto(&toolchain, ToolchainSource::WellKnown, Path::new("/x"), None)
                .capabilities
                .sanitizers
        );
        toolchain.capabilities.sanitizers.undefined_trap = true;
        let dto = toolchain_dto(&toolchain, ToolchainSource::WellKnown, Path::new("/x"), None);
        assert!(dto.capabilities.sanitizers);
        assert_eq!(dto.flavor.as_deref(), Some("MSYS2 UCRT64"));

        toolchain.target = Target::parse("");
        toolchain.version = None;
        toolchain.capabilities.diagnostics = Some(DiagnosticsFormat::Json);
        let dto = toolchain_dto(&toolchain, ToolchainSource::Manual, Path::new("/x"), None);
        assert_eq!(dto.target, None);
        assert_eq!(dto.version, None);
        assert!(!dto.capabilities.sarif);
    }
}
