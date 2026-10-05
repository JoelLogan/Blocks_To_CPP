//! The Blocks2Cpp desktop app's entry point. Everything else is in the
//! library ([`blocks2cpp_desktop::run`]).

// Release builds are GUI apps on Windows: no console window next to the editor.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::process::ExitCode;

fn main() -> ExitCode {
    blocks2cpp_desktop::run()
}
