//! Small operating-system helpers for the rest of Blocks2Cpp, as safe
//! functions (`docs/spec/05-project-format.md` §5.10,
//! `docs/spec/08-security.md` §8.6–§8.8, `docs/spec/02-architecture.md` §2.3).
//!
//! Each one needs either `unsafe` Win32 calls or starting a process, and this
//! crate is the only one allowed to do either, so they live here:
//!
//! * [`atomic_replace`]: the last step of an atomic save. Windows:
//!   `MoveFileExW(MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH)`.
//!   Unix: `rename(2)`, then an `fsync` of the parent directory so the new
//!   directory entry itself survives a power cut.
//! * [`harden_dll_search`]: called first thing at startup on Windows, so DLLs
//!   are only ever loaded from System32 and the application's own folder,
//!   never from the current directory or `PATH` (DLL planting, §8.7).
//! * [`open_https_url`]: opens one of the app's fixed help links in the
//!   user's browser (§8.8: no arbitrary URLs, nothing from the webview).
//! * [`open_read_nonblocking`]: opens a file for reading so that a FIFO
//!   swapped in by someone else can never block the open (§8.6). It needs
//!   neither of the above, only open flags the standard library does not
//!   name; they come from `rustix`, which this crate already uses.
//!
//! The Windows code is in a private submodule, the only place here where
//! `unsafe` is allowed; every `unsafe` block carries a `// SAFETY:` comment.
//! The Unix side needs no `unsafe` at all.

use std::fs::File;
use std::io;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::error::ProcessError;

#[cfg(unix)]
mod unix;
// Win32 calls: `MoveFileExW`, `SetDefaultDllDirectories`, `ShellExecuteW`.
#[cfg(windows)]
#[allow(unsafe_code)]
mod windows;

/// The longest URL [`open_https_url`] accepts, in bytes (all characters it
/// accepts are ASCII, so this is also the length in characters).
pub const MAX_URL_LEN: usize = 2048;

/// Directory syncs completed by [`atomic_replace`] in this process.
static DIRECTORY_SYNCS: AtomicU64 = AtomicU64::new(0);

/// Atomically replaces `target` with `temp`, durably.
///
/// `temp` must be a complete, already flushed and synced file in the **same
/// directory** as `target` (so the rename never crosses file systems). After
/// a crash at any point, `target` is either its old contents or the new ones,
/// never a mix (01 N10, 05 §5.10).
///
/// * **Windows:** `MoveFileExW(temp, target, MOVEFILE_REPLACE_EXISTING |
///   MOVEFILE_WRITE_THROUGH)`, so the call returns only once the move is on
///   disk. Paths longer than `MAX_PATH` are passed in their `\\?\` form. A
///   sharing violation or access-denied error (typically a virus scanner or
///   indexer briefly holding the new file) is retried a few times over about
///   150 ms before it is reported.
/// * **Unix:** `rename(2)`, then `fsync` of the parent directory. A file
///   system that cannot sync directories (`EINVAL`, `ENOTSUP`) is accepted
///   as is: it offers nothing stronger.
///
/// A symbolic link at `target` is replaced by the file, never followed.
///
/// # Errors
/// The rename's error, in which case `target` is unchanged and `temp` still
/// exists (the caller removes it). On Unix an error from the directory sync
/// is also reported; then the rename has already happened and only its
/// durability is in doubt.
pub fn atomic_replace(temp: &Path, target: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        unix::atomic_replace(temp, target)
    }
    #[cfg(windows)]
    {
        windows::atomic_replace(temp, target)
    }
}

/// How many directory syncs [`atomic_replace`] has completed in this process
/// so far. Always 0 on Windows, where the move itself is write-through. Lets
/// tests (and diagnostics) check that saves really sync their directory.
pub fn directory_syncs() -> u64 {
    DIRECTORY_SYNCS.load(Ordering::Relaxed)
}

/// Records one completed directory sync.
#[cfg_attr(not(unix), allow(dead_code))]
fn count_directory_sync() {
    DIRECTORY_SYNCS.fetch_add(1, Ordering::Relaxed);
}

/// Opens `path` for reading without ever waiting in the open itself.
///
/// A plain [`File::open`] of a FIFO (named pipe) waits until another process
/// opens it for writing, which may be never. Checking the path first does
/// not prevent that: whoever can write the folder can swap a FIFO in between
/// the check and the open. Code that reads files from folders other people
/// may write (project folders, which may be shared or synced) therefore
/// opens them with this function and then checks the opened file through
/// its handle ([`File::metadata`]), refusing anything that is not a regular
/// file.
///
/// * **Unix:** `open(2)` with `O_RDONLY | O_NONBLOCK | O_NOCTTY` (plus
///   `O_CLOEXEC`, as for every file the standard library opens): a FIFO
///   opens at once, and a terminal never becomes this process's controlling
///   terminal. Links are followed. The handle stays non-blocking, which
///   changes nothing for regular files: reading one never fails with
///   `WouldBlock`.
/// * **Windows:** [`File::open`]. There are no FIFOs in the file system
///   there (named pipes have their own namespace, `\\.\pipe\`).
///
/// # Errors
/// The error of the open, for example [`io::ErrorKind::NotFound`].
pub fn open_read_nonblocking(path: &Path) -> io::Result<File> {
    #[cfg(unix)]
    {
        unix::open_read_nonblocking(path)
    }
    #[cfg(not(unix))]
    {
        File::open(path)
    }
}

/// Restricts where this process loads DLLs from, to prevent DLL planting
/// (08 §8.7). Call it first thing in `main`, before anything loads a DLL by
/// name.
///
/// * **Windows:** `SetDefaultDllDirectories(LOAD_LIBRARY_SEARCH_SYSTEM32 |
///   LOAD_LIBRARY_SEARCH_APPLICATION_DIR)`: later loads by bare name search
///   only System32 and the folder of the executable, never the current
///   directory or `PATH`.
/// * **Elsewhere:** does nothing and returns `Ok(())`.
///
/// # Errors
/// The Windows error if the call fails. The app logs it as a warning and
/// carries on (the default search order is then still in effect).
pub fn harden_dll_search() -> io::Result<()> {
    #[cfg(windows)]
    {
        windows::harden_dll_search()
    }
    #[cfg(not(windows))]
    {
        Ok(())
    }
}

/// Opens a fixed `https://` link in the user's web browser.
///
/// The URL must be a compile-time constant (`&'static str`): this is for the
/// app's own help links (08 §8.8, 02 §2.5 `open_help_link`), never for text
/// from a project, a program or the webview. It must also pass a strict
/// check before anything is started: it starts with `https://`, has a host
/// of letters, digits, dots and hyphens (optionally `:port`) and no user
/// name, is at most [`MAX_URL_LEN`] bytes, and contains only the characters
/// `A–Z a–z 0–9 - . _ ~ : / ? # [ ] @ ! & ( ) * + , ; = %` with every `%`
/// followed by two hexadecimal digits. Spaces, quotes, backslashes, `$`,
/// backticks, `<`, `>`, `^`, `{`, `|`, `}` and control characters are
/// refused.
///
/// * **Linux and other Unix systems:** starts `/usr/bin/xdg-open`, or
///   `/bin/xdg-open` when that is missing (an absolute path, never a `PATH`
///   lookup), with the URL as its only argument, in a new process group, with
///   standard input, output and error connected to nothing and `/` as the
///   working directory. It inherits this process's environment (the opener
///   needs the desktop session's variables) except the IDE's own variables
///   (`B2C_*`, `TAURI_*`, `WEBVIEW2_*`, `WEBKIT_*`, `APPDIR`, `APPIMAGE`,
///   `ARGV0`, `OWD`). The call returns as soon as the opener has started; a
///   background thread waits for it so it never lingers as a zombie.
/// * **Windows:** `ShellExecuteW` with the `open` verb, which hands the URL
///   to the registered browser. It may block briefly while the shell starts
///   the browser, so call it off the UI thread.
///
/// # Errors
/// * [`ProcessError::InvalidUrl`] when the URL fails the check (nothing is
///   started);
/// * [`ProcessError::NoUrlOpener`] when there is no `xdg-open` (Linux) or no
///   program registered for `https` links (Windows);
/// * [`ProcessError::OpenUrl`] when the opener cannot be started or reports
///   another failure.
pub fn open_https_url(url: &'static str) -> Result<(), ProcessError> {
    check_https_url(url)?;
    #[cfg(unix)]
    {
        unix::open_https_url(url)
    }
    #[cfg(windows)]
    {
        windows::open_https_url(url)
    }
}

/// The URL check of [`open_https_url`].
fn check_https_url(url: &str) -> Result<(), ProcessError> {
    let invalid = || Err(ProcessError::InvalidUrl);
    if url.len() > MAX_URL_LEN {
        return invalid();
    }
    let Some(rest) = url.strip_prefix("https://") else {
        return invalid();
    };
    let bytes = url.as_bytes();
    for (index, &byte) in bytes.iter().enumerate() {
        let allowed = byte.is_ascii_alphanumeric() || b"-._~:/?#[]@!&()*+,;=%".contains(&byte);
        if !allowed {
            return invalid();
        }
        if byte == b'%' {
            let hex = |offset: usize| bytes.get(index + offset).is_some_and(u8::is_ascii_hexdigit);
            if !(hex(1) && hex(2)) {
                return invalid();
            }
        }
    }
    // The authority runs up to the first `/`, `?` or `#`.
    let authority_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = &rest[..authority_end];
    let (host, port) = match authority.split_once(':') {
        Some((host, port)) => (host, Some(port)),
        None => (authority, None),
    };
    let host_ok = !host.is_empty()
        && host.len() <= 253
        && host.split('.').all(|label| {
            !label.is_empty() && label.len() <= 63 && !label.starts_with('-') && !label.ends_with('-')
        })
        && host
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-');
    let port_ok = port
        .is_none_or(|port| !port.is_empty() && port.len() <= 5 && port.bytes().all(|b| b.is_ascii_digit()));
    if host_ok && port_ok { Ok(()) } else { invalid() }
}

/// Whether an environment variable belongs to the IDE itself and must not
/// reach programs it starts on the user's behalf (07 §7.6.2).
#[cfg_attr(not(unix), allow(dead_code))]
fn is_internal_env_var(name: &std::ffi::OsStr) -> bool {
    let Some(name) = name.to_str() else {
        return false;
    };
    ["B2C_", "TAURI_", "WEBVIEW2_", "WEBKIT_"]
        .iter()
        .any(|prefix| name.starts_with(prefix))
        || matches!(name, "APPDIR" | "APPIMAGE" | "ARGV0" | "OWD")
}

/// The `\\?\` (verbatim) form of an absolute Windows path given as UTF-16,
/// which lifts the `MAX_PATH` limit: `C:\x` becomes `\\?\C:\x` and
/// `\\server\share\x` becomes `\\?\UNC\server\share\x`. Paths already in a
/// `\\?\` or `\\.\` form are returned unchanged. The input must be fully
/// normalised (as `std::path::absolute` returns it on Windows): verbatim
/// paths are not normalised again by Windows.
#[cfg(any(windows, test))]
fn verbatim(absolute: &[u16]) -> Vec<u16> {
    let starts_with = |prefix: &str| {
        let prefix: Vec<u16> = prefix.encode_utf16().collect();
        absolute.starts_with(&prefix)
    };
    if starts_with(r"\\?\") || starts_with(r"\\.\") {
        absolute.to_vec()
    } else if starts_with(r"\\") {
        r"\\?\UNC\"
            .encode_utf16()
            .chain(absolute.iter().skip(2).copied())
            .collect()
    } else {
        r"\\?\".encode_utf16().chain(absolute.iter().copied()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_the_help_links() {
        for url in [
            "https://www.msys2.org/",
            "https://winlibs.com/",
            "https://joellogan.github.io/Blocks_To_CPP/reference/diagnostics/",
            "https://example.com",
            "https://example.com:8443/a/b?c=d&e=f#g",
            "https://example.com/caf%C3%A9",
            "https://sub-domain.example.org/~user/(x)*!+,;=",
        ] {
            assert!(check_https_url(url).is_ok(), "{url}");
        }
    }

    #[test]
    fn refuses_anything_else() {
        let long = format!("https://example.com/{}", "a".repeat(MAX_URL_LEN));
        for url in [
            "",
            "http://example.com/",
            "HTTPS://example.com/",
            "file:///etc/passwd",
            "javascript:alert(1)",
            "https://",
            "https:///path",
            "https://example.com/a b",
            "https://example.com/\"quoted\"",
            "https://example.com/'quoted'",
            "https://example.com/$(id)",
            "https://example.com/`id`",
            "https://example.com/a\\b",
            "https://example.com/<script>",
            "https://example.com/{x}",
            "https://example.com/a|b",
            "https://example.com/a^b",
            "https://example.com/\n",
            "https://example.com/\u{7f}",
            "https://example.com/caf\u{e9}",
            "https://example.com/%",
            "https://example.com/%4",
            "https://example.com/%zz",
            "https://user@example.com/",
            "https://user:pass@example.com/",
            "https://-example.com/",
            "https://example-.com/",
            "https://example..com/",
            "https://.example.com/",
            "https://exa_mple.com/",
            "https://example.com:/",
            "https://example.com:123456/",
            "https://example.com:80a/",
            "https://[::1]/",
            " https://example.com/",
            long.as_str(),
        ] {
            assert!(
                matches!(check_https_url(url), Err(ProcessError::InvalidUrl)),
                "{url:?}"
            );
        }
    }

    #[test]
    fn internal_variables_are_recognised() {
        for name in [
            "B2C_EVENTS",
            "TAURI_ENV_DEBUG",
            "WEBVIEW2_X",
            "WEBKIT_DISABLE_DMABUF_RENDERER",
            "APPIMAGE",
            "OWD",
        ] {
            assert!(is_internal_env_var(std::ffi::OsStr::new(name)), "{name}");
        }
        for name in ["PATH", "HOME", "DISPLAY", "XDG_CURRENT_DESKTOP", "B2", "APPDIRS"] {
            assert!(!is_internal_env_var(std::ffi::OsStr::new(name)), "{name}");
        }
    }

    #[test]
    fn long_windows_paths_get_the_verbatim_prefix() {
        let wide = |text: &str| text.encode_utf16().collect::<Vec<u16>>();
        let text = |wide: Vec<u16>| String::from_utf16(&wide).unwrap();
        assert_eq!(
            text(verbatim(&wide(r"C:\Users\a\x.b2c"))),
            r"\\?\C:\Users\a\x.b2c"
        );
        assert_eq!(
            text(verbatim(&wide(r"\\server\share\x.b2c"))),
            r"\\?\UNC\server\share\x.b2c"
        );
        assert_eq!(text(verbatim(&wide(r"\\?\C:\x"))), r"\\?\C:\x");
        assert_eq!(text(verbatim(&wide(r"\\.\pipe\x"))), r"\\.\pipe\x");
    }
}
