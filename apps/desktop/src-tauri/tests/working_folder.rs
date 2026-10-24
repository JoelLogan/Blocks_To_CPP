//! The desktop app's current directory (`docs/spec/07-toolchain-build-run.md`
//! §7.2): at startup the app makes the empty, owner-only folder `<cache>/cwd`
//! its current directory, whatever folder it was started from (W3-10: started
//! from `/`, the app found no g++, and started from the home folder it skipped
//! `~/.local/bin`, because discovery excluded the current directory with its
//! whole tree).
//!
//! This test binary holds one test, because it changes the process's current
//! directory.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, missing_docs)]

use std::fs;
use std::path::{Path, PathBuf};

use blocks2cpp_desktop::{WORKING_FOLDER, enter_working_folder};

/// The current directory, resolved.
fn current() -> PathBuf {
    fs::canonicalize(std::env::current_dir().unwrap()).unwrap()
}

#[test]
fn the_app_works_in_an_empty_private_folder_inside_the_cache() {
    let dir = tempfile::tempdir().unwrap();
    let cache = dir.path().join("cache");
    // Started from the file-system root, as a desktop launcher may do.
    let top = Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().last().unwrap();
    std::env::set_current_dir(top).unwrap();

    let folder = enter_working_folder(&cache).unwrap();
    assert_eq!(folder, cache.join(WORKING_FOLDER));
    let resolved_cache = fs::canonicalize(&cache).unwrap();
    assert!(current().starts_with(&resolved_cache), "{}", current().display());
    assert_eq!(current(), fs::canonicalize(&folder).unwrap());
    assert!(
        fs::read_dir(&folder).unwrap().next().is_none(),
        "the folder is empty"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        for private in [&cache, &folder] {
            let mode = fs::metadata(private).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o700, "{}", private.display());
        }
    }

    // The next start enters the same folder.
    std::env::set_current_dir(dir.path()).unwrap();
    assert_eq!(enter_working_folder(&cache).unwrap(), folder);
    assert_eq!(current(), fs::canonicalize(&folder).unwrap());

    // A link in place of the folder is refused, and the current directory
    // stays where it was.
    #[cfg(unix)]
    {
        let elsewhere = tempfile::tempdir().unwrap();
        let linked_cache = dir.path().join("linked-cache");
        fs::create_dir_all(&linked_cache).unwrap();
        std::os::unix::fs::symlink(elsewhere.path(), linked_cache.join(WORKING_FOLDER)).unwrap();
        std::env::set_current_dir(dir.path()).unwrap();
        assert!(enter_working_folder(&linked_cache).is_err());
        assert_eq!(current(), fs::canonicalize(dir.path()).unwrap());
    }

    // Leave the folder before the temporary folder is removed (Windows cannot
    // remove a process's current directory).
    std::env::set_current_dir(env!("CARGO_MANIFEST_DIR")).unwrap();
}
