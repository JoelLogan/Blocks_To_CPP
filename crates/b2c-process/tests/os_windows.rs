//! DLL search hardening on Windows (`docs/spec/08-security.md` §8.7; run in
//! CI on windows-2025): after [`harden_dll_search`], a DLL that exists only
//! in the current directory or in a folder on `PATH` is never loaded by bare
//! name (DLL planting), while System32 DLLs and DLLs next to the executable
//! still are.
//!
//! Hardening changes the whole process for good, so this binary holds a
//! single test, which checks the default search order first (the control:
//! the planted DLLs do load) and the hardened one afterwards.
#![cfg(windows)]
// Helper functions outside `#[test]`s fail the test by panicking.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
// `LoadLibraryW`, `FreeLibrary` and changing `PATH`; each block has a SAFETY
// comment.
#![allow(unsafe_code)]

use std::ffi::OsString;
use std::fs;
use std::io;
use std::os::windows::ffi::OsStrExt as _;
use std::path::{Path, PathBuf};

use b2c_process::os::harden_dll_search;
use windows_sys::Win32::Foundation::{ERROR_MOD_NOT_FOUND, FreeLibrary};
use windows_sys::Win32::System::LibraryLoader::LoadLibraryW;

/// A System32 DLL whose initialisation is harmless to run, copied under the
/// planted names.
const DONOR: &str = "version.dll";

/// How loading a DLL by bare name went.
#[derive(Debug, PartialEq, Eq)]
enum Load {
    Loaded,
    /// `LoadLibraryW` failed with this Win32 error code.
    Failed(i32),
}

/// Loads `name` by bare name (the search order decides where from) and, if
/// it loaded, unloads it again.
fn load(name: &str) -> Load {
    let wide: Vec<u16> = std::ffi::OsStr::new(name)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    // SAFETY: `wide` is a NUL-terminated UTF-16 string that outlives the
    // call. Every DLL loaded here is a copy of a System32 DLL whose
    // initialisation is safe to run.
    let module = unsafe { LoadLibraryW(wide.as_ptr()) };
    if module.is_null() {
        let code = io::Error::last_os_error().raw_os_error().unwrap_or_default();
        return Load::Failed(code);
    }
    // SAFETY: `module` was just returned by `LoadLibraryW` and is released
    // exactly once.
    unsafe {
        FreeLibrary(module);
    }
    Load::Loaded
}

/// `ERROR_MOD_NOT_FOUND` as `LoadLibraryW` reports it.
fn not_found() -> Load {
    Load::Failed(i32::try_from(ERROR_MOD_NOT_FOUND).unwrap())
}

/// Copies the donor DLL to `path`.
fn plant(system32: &Path, path: &Path) {
    fs::copy(system32.join(DONOR), path)
        .unwrap_or_else(|error| panic!("copying {DONOR} to {}: {error}", path.display()));
}

/// Restores the working directory and `PATH`, and removes the DLL planted
/// next to the test executable, when the test ends (even on failure).
struct Restore {
    dir: PathBuf,
    path: Option<OsString>,
    app_dll: PathBuf,
}

impl Drop for Restore {
    fn drop(&mut self) {
        let _ = std::env::set_current_dir(&self.dir);
        if let Some(path) = &self.path {
            // SAFETY: this binary runs a single test, so no other thread
            // reads or writes the environment while it does.
            unsafe {
                std::env::set_var("PATH", path);
            }
        }
        let _ = fs::remove_file(&self.app_dll);
    }
}

#[test]
fn only_system32_and_the_application_folder_are_searched_after_hardening() {
    let system32 = PathBuf::from(std::env::var_os("SystemRoot").unwrap()).join("System32");
    let current = tempfile::tempdir().unwrap();
    let on_path = tempfile::tempdir().unwrap();
    let exe = std::env::current_exe().unwrap();
    let app_dir = exe.parent().unwrap();
    let app_dll_name = format!("b2c_app_dir_{}.dll", std::process::id());

    let restore = Restore {
        dir: std::env::current_dir().unwrap(),
        path: std::env::var_os("PATH"),
        app_dll: app_dir.join(&app_dll_name),
    };
    // Separate names before and after, so nothing found by the first loads
    // can be reused by the second.
    for name in ["b2c_cwd_before.dll", "b2c_cwd_after.dll"] {
        plant(&system32, &current.path().join(name));
    }
    for name in ["b2c_path_before.dll", "b2c_path_after.dll"] {
        plant(&system32, &on_path.path().join(name));
    }
    plant(&system32, &restore.app_dll);

    std::env::set_current_dir(current.path()).unwrap();
    let mut path = vec![on_path.path().to_path_buf()];
    path.extend(std::env::split_paths(&restore.path.clone().unwrap_or_default()));
    let path = std::env::join_paths(path).unwrap();
    // SAFETY: this binary runs a single test, so no other thread reads or
    // writes the environment while it does.
    unsafe {
        std::env::set_var("PATH", &path);
    }

    // Control: the default search order includes the current directory and
    // `PATH`, so the test can tell hardening from a broken setup.
    assert_eq!(
        load("b2c_cwd_before.dll"),
        Load::Loaded,
        "control: current directory"
    );
    assert_eq!(load("b2c_path_before.dll"), Load::Loaded, "control: PATH");

    harden_dll_search().unwrap();
    // Calling it again (the CLI and the app each call it once; a library
    // might too) is harmless.
    harden_dll_search().unwrap();

    assert_eq!(load("b2c_cwd_after.dll"), not_found(), "current directory");
    assert_eq!(load("b2c_path_after.dll"), not_found(), "PATH");
    assert_eq!(load(DONOR), Load::Loaded, "System32");
    assert_eq!(load(&app_dll_name), Load::Loaded, "the application's folder");
    drop(restore);
}
