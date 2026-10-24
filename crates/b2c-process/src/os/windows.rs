//! Windows: `MoveFileExW` with write-through, `SetDefaultDllDirectories` and
//! `ShellExecuteW`. Every `unsafe` block passes only NUL-terminated UTF-16
//! buffers owned by the calling function, which outlive the call.

use std::io;
use std::os::windows::ffi::OsStrExt as _;
use std::path::Path;
use std::ptr;
use std::time::Duration;

use windows_sys::Win32::Foundation::{ERROR_ACCESS_DENIED, ERROR_SHARING_VIOLATION};
use windows_sys::Win32::Storage::FileSystem::{
    MOVE_FILE_FLAGS, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
};
use windows_sys::Win32::System::LibraryLoader::{
    LOAD_LIBRARY_SEARCH_APPLICATION_DIR, LOAD_LIBRARY_SEARCH_SYSTEM32, SetDefaultDllDirectories,
};
use windows_sys::Win32::UI::Shell::{SE_ERR_ASSOCINCOMPLETE, SE_ERR_NOASSOC, ShellExecuteW};
use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

use crate::error::ProcessError;

/// The flags of every atomic replacement: replace an existing target, and
/// return only once the move has been flushed to disk.
const MOVE_FLAGS: MOVE_FILE_FLAGS = MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH;

/// Attempts at the move when another program briefly holds a file open.
const MOVE_ATTEMPTS: u32 = 5;

/// The first pause between attempts; it doubles each time (10 + 20 + 40 +
/// 80 ms, so about 150 ms of waiting plus the attempts themselves).
const FIRST_RETRY_PAUSE: Duration = Duration::from_millis(10);

/// The longest path passed to Win32 as is. `MAX_PATH` is 260 characters
/// including the terminating NUL; some calls reserve 12 more for an 8.3 file
/// name, so anything longer is passed in its `\\?\` form.
const SHORT_PATH_LIMIT: usize = 247;

/// See [`super::atomic_replace`].
pub(super) fn atomic_replace(temp: &Path, target: &Path) -> io::Result<()> {
    let from = wide_path(temp)?;
    let to = wide_path(target)?;
    let mut pause = FIRST_RETRY_PAUSE;
    let mut attempt = 1;
    loop {
        // SAFETY: `from` and `to` are NUL-terminated UTF-16 strings owned by
        // this function that outlive the call, and `MOVE_FLAGS` is a valid
        // combination of MOVEFILE_* flags. The call reads the two strings
        // and writes no caller memory.
        let moved = unsafe { MoveFileExW(from.as_ptr(), to.as_ptr(), MOVE_FLAGS) };
        if moved != 0 {
            return Ok(());
        }
        let error = io::Error::last_os_error();
        let transient = error.raw_os_error().is_some_and(|code| {
            u32::try_from(code)
                .is_ok_and(|code| code == ERROR_ACCESS_DENIED || code == ERROR_SHARING_VIOLATION)
        });
        if !transient || attempt == MOVE_ATTEMPTS {
            return Err(error);
        }
        std::thread::sleep(pause);
        pause = pause.saturating_mul(2);
        attempt += 1;
    }
}

/// See [`super::harden_dll_search`].
pub(super) fn harden_dll_search() -> io::Result<()> {
    // SAFETY: the function takes its flags by value and touches no caller
    // memory; the two LOAD_LIBRARY_SEARCH_* flags are a documented valid
    // combination.
    let ok = unsafe {
        SetDefaultDllDirectories(LOAD_LIBRARY_SEARCH_SYSTEM32 | LOAD_LIBRARY_SEARCH_APPLICATION_DIR)
    };
    if ok == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

/// See [`super::open_https_url`]; the URL has been checked.
pub(super) fn open_https_url(url: &str) -> Result<(), ProcessError> {
    let verb = nul_terminated("open");
    let file = nul_terminated(url);
    // SAFETY: a null window handle means "no owner window"; `verb` and
    // `file` are NUL-terminated UTF-16 strings owned by this function that
    // outlive the call; the parameters and directory may be null; and
    // SW_SHOWNORMAL is a valid show command. The call writes no caller
    // memory.
    let instance = unsafe {
        ShellExecuteW(
            ptr::null_mut(),
            verb.as_ptr(),
            file.as_ptr(),
            ptr::null(),
            ptr::null(),
            SW_SHOWNORMAL,
        )
    };
    // The result is not a real handle: values above 32 mean success, the
    // others are SE_ERR_* or Win32 error codes.
    let code = instance.addr();
    if code > 32 {
        return Ok(());
    }
    if u32::try_from(code).is_ok_and(|code| code == SE_ERR_NOASSOC || code == SE_ERR_ASSOCINCOMPLETE) {
        return Err(ProcessError::NoUrlOpener);
    }
    Err(ProcessError::OpenUrl {
        source: io::Error::other(format!("ShellExecuteW failed with code {code}")),
    })
}

/// `text` as a NUL-terminated UTF-16 string.
fn nul_terminated(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

/// `path` as a NUL-terminated UTF-16 string for Win32, in its `\\?\` form
/// when it is too long for `MAX_PATH`.
///
/// # Errors
/// Fails when the path contains a NUL character, or when a long path cannot
/// be made absolute.
fn wide_path(path: &Path) -> io::Result<Vec<u16>> {
    let mut wide: Vec<u16> = path.as_os_str().encode_wide().collect();
    if wide.len() > SHORT_PATH_LIMIT {
        // `absolute` normalises the path (`GetFullPathNameW`), which the
        // verbatim form requires.
        let absolute: Vec<u16> = std::path::absolute(path)?.as_os_str().encode_wide().collect();
        wide = super::verbatim(&absolute);
    }
    if wide.contains(&0) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "the path contains a NUL character",
        ));
    }
    wide.push(0);
    Ok(wide)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn moves_are_write_through() {
        assert_eq!(MOVE_FLAGS, MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH);
        assert_ne!(MOVE_FLAGS & MOVEFILE_WRITE_THROUGH, 0);
    }

    #[test]
    fn paths_are_nul_terminated_and_nul_free() {
        let wide = wide_path(Path::new(r"C:\x\y.b2c")).unwrap();
        assert_eq!(wide.last(), Some(&0));
        assert_eq!(wide.iter().filter(|&&unit| unit == 0).count(), 1);
        let long = format!(r"C:\{}\y.b2c", "a".repeat(300));
        let wide = wide_path(Path::new(&long)).unwrap();
        assert_eq!(String::from_utf16(&wide[..4]).unwrap(), r"\\?\");
    }
}
