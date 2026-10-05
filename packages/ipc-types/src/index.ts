/**
 * The Blocks2Cpp IPC contract for the frontend (docs/spec/02-architecture.md §2.5): every request,
 * response, channel message and error type, the command names, `IPC_VERSION` and the typed
 * client. Everything here is generated from the Rust crate `crates/b2c-ipc`; regenerate with
 * `B2C_UPDATE_IPC=1 cargo test -p b2c-ipc --features ts --test generate`.
 */
export type * from './generated/types';
export * from './generated/commands';
