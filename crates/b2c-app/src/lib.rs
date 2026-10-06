//! The Blocks2Cpp backend: the services behind every IPC command, with no
//! Tauri dependency (`docs/spec/02-architecture.md` §2.2–§2.6,
//! [ADR-0007](../../../docs/adr/0007-backend-crates-and-ipc-contract.md)).
//!
//! The desktop app's Tauri commands are thin adapters: each decodes its
//! `request` with [`b2c_ipc::decode()`] and calls one blocking method of
//! [`Backend`], typed with the `b2c_ipc` DTOs, on Tauri's blocking thread
//! pool. Channels are passed as [`b2c_ipc::EventSink`] and
//! [`b2c_ipc::ByteSink`]; native dialogs come through the [`Dialogs`] trait.
//! Everything here is testable headless.
//!
//! # Commands
//!
//! | Command | Method | Spec |
//! | --- | --- | --- |
//! | `app_info` | [`Backend::app_info`] | 02 §2.5.7 |
//! | `app_subscribe` | [`Backend::app_subscribe`] | 02 §2.5.3 |
//! | `app_quit` | [`Backend::app_quit`] | 02 §2.6 |
//! | `project_new` | [`Backend::project_new`] | 02 §2.5.2, 04 §4.10, 08 §8.3.1 |
//! | `project_open_dialog` | [`Backend::project_open_dialog`] | 05 §5.6–§5.7, 08 §8.3, §8.6 |
//! | `project_open_recent` | [`Backend::project_open_recent`] | 05 §5.9 |
//! | `project_reload` | [`Backend::project_reload`] | 05 §5.10, 08 §8.3 |
//! | `project_save` | [`Backend::project_save`] | 05 §5.10–§5.11, 08 §8.3.1 |
//! | `project_save_as_dialog` | [`Backend::project_save_as_dialog`] | 05 §5.10, 08 §8.3.1 |
//! | `project_close` | [`Backend::project_close`] | 07 §7.5.4, 05 §5.10 |
//! | `project_set_dirty` | [`Backend::project_set_dirty`] | 02 §2.6 |
//! | `recent_list` | [`Backend::recent_list`] | 05 §5.9, 04 §4.10 |
//! | `recent_remove` | [`Backend::recent_remove`] | 05 §5.9 |
//! | `recovery_save` | [`Backend::recovery_save`] | 05 §5.10 |
//! | `recovery_list` | [`Backend::recovery_list`] | 05 §5.10 |
//! | `recovery_restore` | [`Backend::recovery_restore`] | 05 §5.10, 08 §8.3.1 |
//! | `recovery_discard` | [`Backend::recovery_discard`] | 05 §5.10 |
//! | `trust_get` | [`Backend::trust_get`] | 08 §8.3 |
//! | `trust_grant` | [`Backend::trust_grant`] | 08 §8.3, §8.3.1 |
//! | `trust_revoke` | [`Backend::trust_revoke`] | 08 §8.3.1 |
//! | `toolchain_list` | [`Backend::toolchain_list`] | 07 §7.2–§7.3 |
//! | `toolchain_rescan` | [`Backend::toolchain_rescan`] | 07 §7.2 |
//! | `toolchain_add_dialog` | [`Backend::toolchain_add_dialog`] | 07 §7.2, 08 §8.5 |
//! | `toolchain_select` | [`Backend::toolchain_select`] | 07 §7.3, 05 §5.9 |
//! | `toolchain_setup_info` | [`Backend::toolchain_setup_info`] | 04 §4.6 |
//! | `build_start` | [`Backend::build_start`] | 02 §2.4.2, 07 §7.5, 08 §8.3 |
//! | `build_cancel` | [`Backend::build_cancel`] | 07 §7.5.4 |
//! | `build_cache_clear` | [`Backend::build_cache_clear`] | 07 §7.5.1 |
//! | `run_start` | [`Backend::run_start`] | 02 §2.4.3, 07 §7.6.1–§7.6.2, 08 §8.7 |
//! | `run_input` | [`Backend::run_input`] | 07 §7.6.5, 08 §8.8 |
//! | `run_resize` | [`Backend::run_resize`] | 02 §2.5.6 |
//! | `run_stop` | [`Backend::run_stop`] | 07 §7.5.4, §7.6.4 |
//! | `run_ack` | [`Backend::run_ack`] | 02 §2.5.3, 07 §7.6.5 |
//! | `settings_get` | [`Backend::settings_get`] | 05 §5.9 |
//! | `settings_update` | [`Backend::settings_update`] | 05 §5.9 |
//! | `open_help_link` | [`Backend::open_help_link`] | 08 §8.8 |
//!
//! Besides the commands, the adapter calls [`Backend::request_close`] when the
//! window asks to close, [`Backend::has_dirty`], and [`Backend::shutdown`] when
//! the app exits.
//!
//! # Rules
//!
//! * **No request carries a path.** Paths come only from [`Dialogs`], from
//!   the recent list, from recovery metadata or from the backend's own
//!   folders. Opaque IDs map to them; an unknown ID is a typed error.
//! * **Untrusted input is validated before any state changes**: a document is
//!   checked for size before it is parsed, then loaded by the strict loader.
//! * **Restricted Mode is enforced here**, not in the UI: `build_start` and
//!   `run_start` refuse a project that is not trusted before touching the
//!   build cache or starting a process. Only the native trust dialog, the
//!   first save of a project created here and *Save as* of a trusted project
//!   change trust.
//! * **Bounded state**: at most 32 open projects, one native dialog at a
//!   time, one `trust_grant` per project every 2 s, 8 running programs
//!   ([`limits`]).
//! * **Errors are typed** ([`b2c_ipc::IpcError`]) and never carry a path or
//!   project content; details go to the log at debug level. Nothing here
//!   panics across the boundary.
//! * **Logging**: one debug span per command, without arguments or content.
//!
//! # Recovery and outside changes
//!
//! * **Recovery snapshots** (05 §5.10): `recovery_save` keeps one snapshot
//!   per open project in this instance's folder of the recovery directory; a
//!   clean save, *Save as*, a reload (which discards the unsaved changes),
//!   close, and shutdown for projects without unsaved changes delete it. Snapshots of instances that exited or crashed are
//!   listed and restored under the trust rules of 08 §8.3.1.
//! * **The file watcher** watches the folder of every open project file and
//!   sends `projectChangedOnDisk { handle, deleted }` on the app channel once
//!   per outside change, after a 300 ms debounce
//!   ([`WATCH_DEBOUNCE`]) and only when the file's SHA-256 differs from the
//!   baseline; the app's own saves never notify. `project_save` checks the
//!   hash again before it writes.

mod backend;
mod build_run;
mod dialogs;
mod errors;
mod events;
pub mod limits;
mod projects;
mod recent;
mod recovery;
mod settings;
mod templates;
mod toolchains;
mod trust;
mod watcher;

pub use backend::{Backend, BackendConfig, Services, SystemOpener, UrlOpener};
pub use dialogs::{
    Dialogs, INTERNET_WARNING, TRUST_WARNING, TrustChoice, TrustPrompt, display_text, suggested_file_name,
    trust_dialog_text,
};
pub use errors::StartError;
pub use watcher::{WATCH_DEBOUNCE, WATCH_MAX_DELAY};
