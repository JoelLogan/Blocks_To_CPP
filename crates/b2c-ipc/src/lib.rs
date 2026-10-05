//! The IPC contract between the Blocks2Cpp desktop frontend and its backend
//! (`docs/spec/02-architecture.md` §2.5, `docs/spec/08-security.md` §8.8,
//! `docs/spec/09-quality-and-delivery.md` §9.5).
//!
//! This crate defines every request, response and channel message, and nothing
//! that does the work: the backend services implement the commands and the Tauri
//! shell adapts them. It does not depend on Tauri, so the contract builds and
//! tests without a webview.
//!
//! * [`commands`]: the command table ([`COMMANDS`], [`COMMAND_NAMES`]) and the
//!   [`IpcRequest`] trait.
//! * [`dto`]: the request, response and event types.
//! * [`ids`]: the opaque IDs that stand in for every path and session.
//! * [`error`]: [`IpcError`], the typed error of every command.
//! * [`diag`]: the diagnostic type, identical in JSON to the CLI's.
//! * [`decode`](mod@decode): [`decode()`] for requests, [`parse_document`] for
//!   project documents, and strict base64.
//! * [`schema`]: request shapes as data, shared with the isolation hook.
//! * [`limits`]: every size, range and rate limit.
//! * [`links`]: the fixed help links.
//! * [`sink`]: the traits that channel senders implement.
//!
//! With the `ts` feature, the `generate` module renders the TypeScript types and client of
//! `packages/ipc-types` and the isolation allowlist; `tests/generate.rs` writes or
//! checks those files. App builds never enable it.
//!
//! **Versioning.** [`IPC_VERSION`] is emitted into the generated TypeScript, and
//! `app_info` reports it, so a frontend and backend from different builds detect
//! each other. A breaking change to any type here increments it.

pub mod commands;
pub mod decode;
pub mod diag;
pub mod dto;
pub mod error;
#[cfg(feature = "ts")]
pub mod generate;
pub mod ids;
pub mod limits;
pub mod links;
mod macros;
pub mod schema;
pub mod sink;

pub use commands::{COMMAND_NAMES, COMMANDS, ChannelSpec, CommandSpec, IpcRequest};
pub use decode::{decode, decode_base64, encode_base64, parse_document};
pub use error::{InvalidReason, IoKind, IpcError};
pub use ids::{BuildId, Handle, RecentId, RunId, SnapshotId, ToolchainId, random_project_id};
pub use links::LinkId;
pub use sink::{ByteSink, EventSink};

/// The version of the IPC contract. Incremented on every breaking change to a
/// command, request, response, channel message or error.
pub const IPC_VERSION: u32 = 1;
