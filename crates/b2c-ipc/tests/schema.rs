//! The request schemas agree with the serde types, and no request can carry a
//! filesystem path (`docs/spec/02-architecture.md` §2.5 "Opaque handles",
//! `docs/spec/08-security.md` §8.12 T8 and T9).

// Test code: unwrap/expect/panic are fine here.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeSet;

use b2c_ipc::commands::{COMMANDS, IpcRequest};
use b2c_ipc::decode;
use b2c_ipc::dto::{
    BuildCancelRequest, BuildStartRequest, OpenHelpLinkRequest, ProjectCloseRequest, ProjectNewRequest,
    ProjectOpenRecentRequest, ProjectReloadRequest, ProjectSaveAsDialogRequest, ProjectSaveRequest,
    ProjectSetDirtyRequest, RecentRemoveRequest, RecoveryDiscardRequest, RecoveryRestoreRequest,
    RecoverySaveRequest, RunAckRequest, RunInputRequest, RunResizeRequest, RunStartRequest, RunStopRequest,
    SettingsPatch, ToolchainSelectRequest, TrustGetRequest, TrustGrantRequest, TrustRevokeRequest,
};
use b2c_ipc::schema::{FieldSchema, ObjectSchema};
use serde_json::Value;

/// Calls `$check::<T>()` for every request type.
macro_rules! for_every_request {
    ($check:ident) => {
        $check::<ProjectNewRequest>();
        $check::<ProjectOpenRecentRequest>();
        $check::<ProjectReloadRequest>();
        $check::<ProjectSaveRequest>();
        $check::<ProjectSaveAsDialogRequest>();
        $check::<ProjectCloseRequest>();
        $check::<ProjectSetDirtyRequest>();
        $check::<RecentRemoveRequest>();
        $check::<RecoverySaveRequest>();
        $check::<RecoveryRestoreRequest>();
        $check::<RecoveryDiscardRequest>();
        $check::<TrustGetRequest>();
        $check::<TrustGrantRequest>();
        $check::<TrustRevokeRequest>();
        $check::<ToolchainSelectRequest>();
        $check::<BuildStartRequest>();
        $check::<BuildCancelRequest>();
        $check::<RunStartRequest>();
        $check::<RunInputRequest>();
        $check::<RunResizeRequest>();
        $check::<RunStopRequest>();
        $check::<RunAckRequest>();
        $check::<SettingsPatch>();
        $check::<OpenHelpLinkRequest>();
    };
}

/// The keys of `value` and of `schema` are the same at every level.
fn same_keys(value: &Value, schema: &ObjectSchema, path: &str) {
    let Value::Object(map) = value else {
        panic!("{path}: the sample is not an object");
    };
    let sample_keys: BTreeSet<&str> = map.keys().map(String::as_str).collect();
    let schema_keys: BTreeSet<&str> = schema.fields.iter().map(|f| f.name).collect();
    assert_eq!(sample_keys, schema_keys, "{path}: schema and serde disagree");
    for field in schema.fields {
        if let FieldSchema::Object(inner) = field.schema {
            same_keys(&map[field.name], inner, &format!("{path}.{}", field.name));
        }
    }
}

#[test]
fn every_schema_has_exactly_the_serde_keys() {
    fn check<T: IpcRequest>() {
        // Every sample sets every optional key, so the comparison covers them.
        let sample = serde_json::to_value(T::sample()).unwrap();
        same_keys(&sample, T::schema(), T::COMMAND);
        let spec = b2c_ipc::commands::command(T::COMMAND).unwrap();
        assert!(
            std::ptr::eq(spec.request.unwrap()(), T::schema()),
            "{}",
            T::COMMAND
        );
        assert_eq!(spec.sample.unwrap()(), sample, "{}", T::COMMAND);
    }
    for_every_request!(check);
    assert_eq!(COMMANDS.iter().filter(|c| c.request.is_some()).count(), 24);
}

/// Every enum value and integer bound the schema allows is one serde accepts, so
/// the isolation hook never lets through a value the backend cannot decode.
#[test]
fn every_value_the_schema_allows_decodes() {
    fn variants(value: &Value, schema: &ObjectSchema, out: &mut Vec<Value>) {
        for field in schema.fields {
            let choices: Vec<Value> = match field.schema {
                FieldSchema::Enum { values } => values.iter().map(|v| Value::from(*v)).collect(),
                FieldSchema::IntOneOf { values } => values.iter().map(|v| Value::from(*v)).collect(),
                FieldSchema::Int { min, max } => vec![Value::from(min), Value::from(max)],
                FieldSchema::Bool => vec![Value::from(true), Value::from(false)],
                FieldSchema::Object(inner) => {
                    let mut nested = Vec::new();
                    variants(&value[field.name], inner, &mut nested);
                    nested
                }
                _ => Vec::new(),
            };
            for choice in choices {
                let mut copy = value.clone();
                copy[field.name] = choice;
                out.push(copy);
            }
        }
    }
    fn check<T: IpcRequest>() {
        let sample = serde_json::to_value(T::sample()).unwrap();
        let mut cases = Vec::new();
        variants(&sample, T::schema(), &mut cases);
        for case in cases {
            if let Err(error) = decode::<T>(case.clone()) {
                panic!("{}: {case} was refused: {error:?}", T::COMMAND);
            }
        }
    }
    for_every_request!(check);
}

/// Walks every field of every request schema, with its dotted path.
fn all_fields(mut visit: impl FnMut(&str, &str, &FieldSchema)) {
    fn walk(
        command: &str,
        schema: &ObjectSchema,
        path: &str,
        visit: &mut dyn FnMut(&str, &str, &FieldSchema),
    ) {
        for field in schema.fields {
            let field_path = if path.is_empty() {
                field.name.to_owned()
            } else {
                format!("{path}.{}", field.name)
            };
            visit(command, &field_path, &field.schema);
            if let FieldSchema::Object(inner) = field.schema {
                walk(command, inner, &field_path, visit);
            }
        }
    }
    for spec in COMMANDS {
        if let Some(schema) = spec.request {
            walk(spec.name, schema(), "", &mut visit);
        }
    }
}

#[test]
fn no_request_names_a_path() {
    // Names that contain one of the words by accident. Each must be a field that
    // cannot hold text at all.
    const NOT_PATHS: [&str; 1] = ["dirty"];
    let mut count = 0;
    all_fields(|command, path, schema| {
        count += 1;
        let name = path.rsplit('.').next().unwrap_or(path);
        let lower = path.to_ascii_lowercase();
        for word in ["path", "dir", "file", "folder", "url", "uri"] {
            if !lower.contains(word) {
                continue;
            }
            assert!(
                NOT_PATHS.contains(&name),
                "{command}: request field {path} looks like a path"
            );
            assert!(
                matches!(
                    schema,
                    FieldSchema::Bool | FieldSchema::Int { .. } | FieldSchema::Enum { .. }
                ),
                "{command}: {path} is excepted only because it cannot hold text"
            );
        }
    });
    assert!(count > 30);
    for spec in COMMANDS {
        for channel in spec.channels {
            assert!(channel.name.starts_with("on"), "{}: {}", spec.name, channel.name);
        }
    }
}

#[test]
fn no_request_carries_free_text_but_documents() {
    // Free-form strings are the only fields that could carry a path. The only one is
    // the BDM document, which the strict loader parses (and which may not contain
    // absolute paths, docs/spec/05-project-format.md §5.8).
    all_fields(|command, path, schema| {
        if let FieldSchema::String { max_len } = schema {
            assert!(path == "document", "{command}: free-text field {path}");
            assert_eq!(*max_len, b2c_ipc::limits::MAX_DOCUMENT_BYTES);
        }
    });
}
