//! Writes or checks the files generated from the IPC contract.
//!
//! ```sh
//! cargo test -p b2c-ipc --features ts --test generate                     # check
//! B2C_UPDATE_IPC=1 cargo test -p b2c-ipc --features ts --test generate    # write
//! ```
//!
//! Without `B2C_UPDATE_IPC=1` the test fails when a committed file differs from
//! what the Rust types produce, which is CI's staleness check.
#![cfg(feature = "ts")]
// Test code: unwrap/expect/panic are fine here.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeSet;
use std::path::PathBuf;

use b2c_ipc::generate::{REGENERATE, UPDATE_ENV, files, request_types, type_decls};
use b2c_ipc::{COMMANDS, IPC_VERSION, IpcError};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn generated_files_are_current() {
    let update = std::env::var(UPDATE_ENV).is_ok_and(|v| v == "1");
    let mut stale = Vec::new();
    for file in files() {
        assert!(
            !file.contents.contains('\r'),
            "{}: LF line endings only",
            file.path
        );
        assert!(
            file.contents.ends_with('\n'),
            "{}: ends with a newline",
            file.path
        );
        let path = repo_root().join(file.path);
        let current = std::fs::read(&path).ok();
        if current.as_deref() == Some(file.contents.as_bytes()) {
            continue;
        }
        if update {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, &file.contents).unwrap();
        } else {
            stale.push(file.path);
        }
    }
    assert!(
        stale.is_empty(),
        "these generated files are stale: {stale:?}\nregenerate them with: {REGENERATE}"
    );

    // The committed client carries the Rust IPC version.
    let commands =
        std::fs::read_to_string(repo_root().join("packages/ipc-types/src/generated/commands.ts")).unwrap();
    let version = commands
        .lines()
        .find_map(|line| line.strip_prefix("export const IPC_VERSION = "))
        .and_then(|rest| rest.strip_suffix(';'))
        .expect("commands.ts declares IPC_VERSION");
    assert_eq!(version.parse::<u32>().unwrap(), IPC_VERSION);
}

#[test]
fn generation_is_deterministic() {
    assert_eq!(files(), files());
}

#[test]
fn every_referenced_type_is_declared() {
    let decls = type_decls();
    let names: BTreeSet<&str> = decls.iter().map(|d| d.name.as_str()).collect();
    assert_eq!(names.len(), decls.len(), "type names are unique");
    for decl in &decls {
        assert!(decl.text.starts_with("/**"), "{} is documented", decl.name);
        for dependency in &decl.dependencies {
            assert!(
                names.contains(dependency.as_str()),
                "{} uses undeclared {dependency}",
                decl.name
            );
        }
    }
    for spec in COMMANDS {
        assert!(
            names.contains(spec.response_ts),
            "{}: {}",
            spec.name,
            spec.response_ts
        );
        if let Some(request) = spec.request_ts {
            assert!(names.contains(request), "{}: {request}", spec.name);
        }
        for channel in spec.channels {
            let builtin = channel.raw && channel.ts_type == "ArrayBuffer";
            assert!(
                builtin || names.contains(channel.ts_type),
                "{}: {}",
                spec.name,
                channel.ts_type
            );
        }
    }
    // The table's request type names are the types that implement each command.
    let requests = request_types();
    assert_eq!(
        requests.len(),
        COMMANDS.iter().filter(|c| c.request.is_some()).count()
    );
    for (command, ts_name) in requests {
        let spec = b2c_ipc::commands::command(command).unwrap();
        assert_eq!(spec.request_ts, Some(ts_name.as_str()), "{command}");
    }
}

#[test]
fn the_client_knows_every_error_code() {
    let commands = b2c_ipc::generate::commands_ts();
    for code in IpcError::CODES {
        assert!(commands.contains(&format!("  '{code}',\n")), "{code}");
    }
    for spec in COMMANDS {
        assert!(
            commands.contains(&format!("call(t, '{}'", spec.name)),
            "{}",
            spec.name
        );
    }
}
