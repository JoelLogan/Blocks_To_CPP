//! The Blocks2Cpp desktop app's entry point. Everything else is in the
//! library ([`blocks2cpp_desktop::run`]).
//!
//! `main` does nothing but call `run`, whose first statement restricts where
//! Windows loads DLLs from (`docs/spec/08-security.md` §8.7). Nothing may come
//! before it that could load a DLL by name; `tests/hardening.rs` checks both.

// Release builds are GUI apps on Windows: no console window next to the editor.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::process::ExitCode;

fn main() -> ExitCode {
    blocks2cpp_desktop::run()
}
