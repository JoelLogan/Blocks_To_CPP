//! Machine-local storage for Blocks2Cpp: the per-user folders and the files
//! the desktop app keeps in them (`docs/spec/02-architecture.md` §2.7,
//! `docs/spec/05-project-format.md` §5.9–§5.10, `docs/spec/08-security.md`
//! §8.1, §8.3, §8.6 and §8.11, `docs/spec/01-overview.md` N10).
//!
//! CONTRACT (implemented in milestone M2):
//! * [`dirs`]: [`Dirs`] computes the folders of 02 §2.7 from the
//!   environment (`%APPDATA%`/`%LOCALAPPDATA%` on Windows; the XDG variables
//!   with the `~/.config`, `~/.cache` and `~/.local/state` fallbacks on
//!   Linux, ignoring relative values and values with `..` parts), never
//!   through Tauri's path resolver. [`ensure_private_dir`] creates folders
//!   one level at a time, `0700` on Unix, and refuses links, junctions and
//!   reparse points (08 §8.6).
//! * [`atomic`]: [`write_atomic`] writes a temporary file in the same folder
//!   (exclusive, random name, `0600`), flushes and syncs it, then renames it
//!   over the target with `MoveFileExW(REPLACE_EXISTING | WRITE_THROUGH)` on
//!   Windows or `rename(2)` plus a directory `fsync` on Unix, so a crash
//!   leaves the old file or the new one, never a mix (05 §5.10, 01 N10).
//!   The file is always `0600` on Unix; [`write_atomic_keeping_mode`], for
//!   project files only, keeps the mode of the file it replaces.
//!   [`Backup::KeepPrevious`] keeps one `<name>.bak` generation, written the
//!   same way, so a link planted there is replaced, never followed. Links at
//!   the target are refused; temporary files never outlive an error.
//! * [`read`]: [`read_bounded`] reads regular files only, at most
//!   `limit + 1` bytes (08 §8.6 size-bounded reads), and a FIFO swapped in
//!   for the file can never block it.
//! * [`project_file`]: project bytes in and out: bounded to 32 MiB
//!   (05 §5.6), saved with a `.b2c.bak`, hashed with SHA-256, and paths
//!   canonicalised.
//! * [`settings`]: `settings.json` (05 §5.9) with per-value validation,
//!   reset-with-notice, unknown keys kept, strict partial updates and
//!   byte-stable saves.
//! * [`recent`]: `recent.json` (05 §5.9): at most 10 projects, newest first,
//!   behind opaque `rc_` IDs.
//! * [`trust`]: `trust.json` (05 §5.9, 08 §8.3.1) and the trust evaluation
//!   ([`TrustStore::evaluate`]): project records matched by project ID,
//!   canonical path and security hash, folder records covering everything
//!   below them, fail-closed loading (a missing, oversized or invalid file
//!   trusts nothing), and changes serialised across app instances.
//! * [`motw`]: [`mark_of_the_web`], the Windows `Zone.Identifier` stream
//!   read with a 64 KiB bound and parsed strictly (08 §8.3).
//! * [`log`]: [`RotatingFile`], the size-rotating log writer behind the
//!   app's `tracing` subscriber (08 §8.11: 5 files of 5 MiB, `0600` in a
//!   `0700` folder).
//! * [`recovery`]: [`RecoveryStore`], the autosave snapshots of projects
//!   with unsaved changes (05 §5.10): one folder per running app instance,
//!   guarded by an exclusive lock on `<instanceId>.lock`; documents and
//!   their metadata written atomically as pairs that a crash cannot split;
//!   and the snapshots of instances that exited or crashed offered for
//!   restore, never those of a running one.
//! * [`time`] and [`ids`]: RFC 3339 UTC timestamps and random opaque IDs
//!   from the OS random number generator.
//!
//! No error message contains a path (08 §8.11): callers log the `path`
//! fields of [`StoreError`] at debug level only. This crate never logs
//! itself (it has no `tracing` dependency); where a store finds a problem
//! worth a warning, it reports it ([`SettingsNotice`],
//! [`TrustStore::problem`]) for the app to log. Nothing in this crate reads
//! or writes outside the paths its callers give it, and nothing from a
//! project file ever ends up in the machine-local files except the recent
//! list's display name, the trust store's project IDs and the recovery
//! snapshots (copies of open projects, kept only in the private recovery
//! folder).
//!
//! This crate is native only and contains no `unsafe` code: the OS calls it
//! needs that the standard library lacks (the write-through rename and the
//! non-blocking open) come from [`b2c_process::os`].

pub mod atomic;
pub mod dirs;
mod error;
mod fs_checks;
pub mod ids;
pub mod log;
pub mod motw;
pub mod project_file;
pub mod read;
pub mod recent;
pub mod recovery;
pub mod settings;
pub mod time;
pub mod trust;

pub use atomic::{Backup, write_atomic, write_atomic_keeping_mode};
pub use dirs::{Dirs, cache_root_from_env, ensure_private_dir};
pub use error::{ReadError, StoreError};
pub use ids::random_hex_id;
pub use log::RotatingFile;
pub use motw::mark_of_the_web;
pub use project_file::{canonical_path, read_project, save_project, sha256_hex};
pub use read::read_bounded;
pub use recent::{RecentEntry, RecentStore};
pub use recovery::{RecoveryStore, SnapshotListing, SnapshotMeta};
pub use settings::{
    CodeStyle, NoticeReason, OnErrors, Settings, SettingsNotice, SettingsPatch, SettingsStore,
};
pub use time::{parse_rfc3339_utc, rfc3339_utc};
pub use trust::{ProjectIdentity, RestrictedReason, TrustFileProblem, TrustSource, TrustStore, TrustVerdict};
