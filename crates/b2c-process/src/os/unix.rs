//! Unix: `rename(2)` plus a directory sync, and links opened with
//! `xdg-open`. No `unsafe` needed.

use std::fs::File;
use std::io;
use std::os::unix::fs::PermissionsExt as _;
use std::os::unix::process::CommandExt as _;
use std::path::Path;
use std::process::Stdio;

use crate::error::ProcessError;

/// Where `xdg-open` may be, in order. Never looked up on `PATH`.
const XDG_OPEN: [&str; 2] = ["/usr/bin/xdg-open", "/bin/xdg-open"];

/// See [`super::atomic_replace`].
pub(super) fn atomic_replace(temp: &Path, target: &Path) -> io::Result<()> {
    std::fs::rename(temp, target)?;
    let parent = match target.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    };
    sync_directory(parent)
}

/// `fsync`s a directory, so a rename inside it is durable.
fn sync_directory(dir: &Path) -> io::Result<()> {
    let handle = File::open(dir)?;
    match handle.sync_all() {
        Ok(()) => {
            super::count_directory_sync();
            Ok(())
        }
        // The file system cannot sync directories; it offers nothing
        // stronger, so the rename is as durable as it gets.
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::InvalidInput | io::ErrorKind::Unsupported
            ) =>
        {
            Ok(())
        }
        Err(error) => Err(error),
    }
}

/// See [`super::open_https_url`]; the URL has been checked.
pub(super) fn open_https_url(url: &str) -> Result<(), ProcessError> {
    let opener = XDG_OPEN
        .iter()
        .map(Path::new)
        .find(|path| is_executable_file(path))
        .ok_or(ProcessError::NoUrlOpener)?;
    spawn_detached(opener, url)
}

/// Whether `path` is a regular file (after following links) that someone
/// may execute.
fn is_executable_file(path: &Path) -> bool {
    std::fs::metadata(path)
        .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
}

/// Starts `program argument` in a new process group with no standard
/// streams, `/` as its working directory and the environment of this process
/// minus the IDE's own variables, and reaps it on a background thread.
fn spawn_detached(program: &Path, argument: &str) -> Result<(), ProcessError> {
    // This crate is the one place that starts processes (08 §8.5): an
    // absolute program and a single argv entry, never a shell.
    #[allow(clippy::disallowed_methods)]
    let mut command = std::process::Command::new(program);
    command
        .arg(argument)
        .current_dir("/")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0);
    for (name, _) in std::env::vars_os() {
        if super::is_internal_env_var(&name) {
            command.env_remove(name);
        }
    }
    let mut child = command
        .spawn()
        .map_err(|source| ProcessError::OpenUrl { source })?;
    // The opener may run for as long as the browser does (some desktops run
    // the browser in the foreground of `xdg-open`), so it is waited for on a
    // thread of its own. If that thread cannot be created, the finished
    // opener stays a zombie until this process exits: harmless.
    let _ = std::thread::Builder::new()
        .name("b2c-link-opener".to_owned())
        .spawn(move || {
            let _ = child.wait();
        });
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::*;

    #[test]
    fn a_detached_program_gets_exactly_the_one_argument() {
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join("arguments");
        // A stand-in for xdg-open that records its arguments, one per line,
        // and its working directory.
        let opener = dir.path().join("opener");
        std::fs::write(
            &opener,
            format!(
                "#!/bin/sh\n{{ printf '%s\\n' \"$#\" \"$@\"; pwd; }} > '{0}.part' && mv '{0}.part' '{0}'\n",
                marker.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&opener, std::fs::Permissions::from_mode(0o700)).unwrap();
        spawn_detached(&opener, "https://example.com/a?b=c&d=e#f").unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        while !marker.exists() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        let recorded = std::fs::read_to_string(&marker).unwrap();
        assert_eq!(recorded, "1\nhttps://example.com/a?b=c&d=e#f\n/\n");
    }

    #[test]
    fn a_missing_program_is_an_open_error() {
        let dir = tempfile::tempdir().unwrap();
        assert!(matches!(
            spawn_detached(&dir.path().join("missing"), "https://example.com/"),
            Err(ProcessError::OpenUrl { .. })
        ));
    }

    #[test]
    fn executables_are_recognised() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("file");
        std::fs::write(&file, "").unwrap();
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert!(!is_executable_file(&file));
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o700)).unwrap();
        assert!(is_executable_file(&file));
        assert!(!is_executable_file(dir.path()));
        assert!(!is_executable_file(&dir.path().join("missing")));
    }

    #[test]
    fn replacing_syncs_the_directory() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("file");
        let temp = dir.path().join("file.tmp");
        std::fs::write(&target, "old").unwrap();
        std::fs::write(&temp, "new").unwrap();
        let before = super::super::directory_syncs();
        atomic_replace(&temp, &target).unwrap();
        assert!(super::super::directory_syncs() > before);
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "new");
        assert!(!temp.exists());
    }
}
