//! Request decoding through the public API: every rejection is a typed error that
//! names the field, and nothing is half-accepted (`docs/spec/08-security.md` §8.8).

// Test code: unwrap/expect/panic are fine here.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use b2c_ipc::commands::{COMMANDS, IpcRequest};
use b2c_ipc::dto::{
    BuildCancelRequest, BuildStartRequest, OpenHelpLinkRequest, ProjectCloseRequest, ProjectNewRequest,
    ProjectOpenRecentRequest, ProjectSaveRequest, RecoveryRestoreRequest, RunAckRequest, RunInputRequest,
    RunResizeRequest, RunStartRequest, RunStopRequest, SettingsPatch, Template, ToolchainSelectRequest,
};
use b2c_ipc::limits::{MAX_DOCUMENT_BYTES, MAX_RUN_INPUT_BASE64, MAX_RUN_INPUT_BYTES};
use b2c_ipc::{InvalidReason, IpcError, decode, encode_base64, parse_document};
use serde_json::{Value, json};

fn invalid(reason: InvalidReason, field: &str) -> IpcError {
    IpcError::invalid(reason, Some(field))
}

fn sample<T: IpcRequest>() -> Value {
    serde_json::to_value(T::sample()).unwrap()
}

const RUN_ID: &str = "rn_0123456789abcdef0123456789abcdef";

#[test]
fn every_sample_decodes_to_itself() {
    fn check<T: IpcRequest + PartialEq + std::fmt::Debug>() {
        assert_eq!(decode::<T>(sample::<T>()).unwrap(), T::sample(), "{}", T::COMMAND);
    }
    check::<ProjectNewRequest>();
    check::<ProjectSaveRequest>();
    check::<BuildStartRequest>();
    check::<RunStartRequest>();
    check::<RunInputRequest>();
    check::<RunResizeRequest>();
    check::<RunAckRequest>();
    check::<SettingsPatch>();
    check::<OpenHelpLinkRequest>();
    assert_eq!(
        decode::<ProjectNewRequest>(json!({"template": "empty"}))
            .unwrap()
            .template,
        Template::Empty
    );
    // Every table sample is accepted by the decoder of its command (checked here
    // through the schema, which `decode` applies first).
    for spec in COMMANDS {
        if let (Some(schema), Some(sample)) = (spec.request, spec.sample) {
            b2c_ipc::schema::check(&sample(), schema()).unwrap();
        }
    }
}

#[test]
fn unknown_fields_are_rejected_at_every_level() {
    let mut value = sample::<RunStartRequest>();
    value["runOptions"]["colour"] = json!(true);
    assert_eq!(
        decode::<RunStartRequest>(value).unwrap_err(),
        invalid(InvalidReason::UnknownField, "runOptions.colour")
    );
    let mut value = sample::<ProjectSaveRequest>();
    value["path"] = json!("/etc/passwd");
    assert_eq!(
        decode::<ProjectSaveRequest>(value).unwrap_err(),
        invalid(InvalidReason::UnknownField, "path")
    );
    // The settings the patch may not touch.
    for key in ["toolchain", "newProject", "buildCache", "formatVersion"] {
        assert_eq!(
            decode::<SettingsPatch>(json!({ key: {} })).unwrap_err(),
            invalid(InvalidReason::UnknownField, key)
        );
    }
    assert_eq!(
        decode::<SettingsPatch>(json!({"codeStyle": {"tabs": true}})).unwrap_err(),
        invalid(InvalidReason::UnknownField, "codeStyle.tabs")
    );
    assert_eq!(
        decode::<SettingsPatch>(json!({"codeStyle": null})).unwrap_err(),
        invalid(InvalidReason::Malformed, "codeStyle")
    );
}

#[test]
fn missing_fields_and_wrong_types_are_rejected() {
    assert_eq!(
        decode::<ProjectSaveRequest>(json!({"handle": "ph_0123456789abcdef0123456789abcdef"})).unwrap_err(),
        invalid(InvalidReason::MissingField, "document")
    );
    assert_eq!(
        decode::<ProjectNewRequest>(json!(null)).unwrap_err(),
        IpcError::invalid(InvalidReason::Malformed, None)
    );
    assert_eq!(
        decode::<ProjectNewRequest>(json!("helloWorld")).unwrap_err(),
        IpcError::invalid(InvalidReason::Malformed, None)
    );
    let mut value = sample::<RunResizeRequest>();
    value["cols"] = json!("80");
    assert_eq!(
        decode::<RunResizeRequest>(value).unwrap_err(),
        invalid(InvalidReason::Malformed, "cols")
    );
}

#[test]
fn enums_are_closed() {
    assert_eq!(
        decode::<ProjectNewRequest>(json!({"template": "guessingGame"})).unwrap_err(),
        invalid(InvalidReason::BadEnum, "template")
    );
    let mut value = sample::<BuildStartRequest>();
    value["config"] = json!("Debug");
    assert_eq!(
        decode::<BuildStartRequest>(value).unwrap_err(),
        invalid(InvalidReason::BadEnum, "config")
    );
    assert_eq!(
        decode::<OpenHelpLinkRequest>(json!({"linkId": "https://example.com/"})).unwrap_err(),
        invalid(InvalidReason::BadEnum, "linkId")
    );
}

#[test]
fn malformed_ids_of_every_kind_are_rejected() {
    fn bad_ids(prefix: &str, digits: usize) -> Vec<String> {
        let good: String = "0123456789abcdef".chars().cycle().take(digits).collect();
        vec![
            format!("{prefix}{}", good.to_uppercase()),
            format!("{prefix}{}", &good[1..]),
            format!("{prefix}{good}0"),
            format!("xx_{good}"),
            format!("{prefix}{}", "z".repeat(digits)),
            String::new(),
        ]
    }
    fn check<T: IpcRequest + std::fmt::Debug>(field: &str, prefix: &str, digits: usize) {
        for bad in bad_ids(prefix, digits) {
            assert_eq!(
                decode::<T>(json!({ field: bad })).unwrap_err(),
                invalid(InvalidReason::BadId, field),
                "{} {bad}",
                T::COMMAND
            );
        }
        // A valid ID of another kind is still the wrong ID.
        if prefix != "ph_" {
            let handle = b2c_ipc::Handle::example();
            assert_eq!(
                decode::<T>(json!({ field: handle.as_str() })).unwrap_err(),
                invalid(InvalidReason::BadId, field)
            );
        }
    }
    check::<ProjectCloseRequest>("handle", "ph_", 32);
    check::<BuildCancelRequest>("buildId", "bd_", 32);
    check::<RunStopRequest>("runId", "rn_", 32);
    check::<ProjectOpenRecentRequest>("recentId", "rc_", 32);
    check::<RecoveryRestoreRequest>("snapshotId", "sn_", 32);
    check::<ToolchainSelectRequest>("toolchainId", "tc_", 16);
}

#[test]
fn run_input_is_bounded_and_strict() {
    let input = |data: String| decode::<RunInputRequest>(json!({"runId": RUN_ID, "data": data}));
    let max = encode_base64(&vec![0xa5; MAX_RUN_INPUT_BYTES]);
    let request = input(max).unwrap();
    assert_eq!(request.bytes().unwrap().len(), MAX_RUN_INPUT_BYTES);
    // 65,537 bytes: still 87,384 characters, so only the decoded size catches it.
    let over = encode_base64(&vec![0xa5; MAX_RUN_INPUT_BYTES + 1]);
    assert_eq!(over.len(), MAX_RUN_INPUT_BASE64);
    assert_eq!(
        input(over).unwrap_err(),
        IpcError::PayloadTooLarge { limit: 65_536 }
    );
    // 87,385 characters are rejected before decoding.
    assert_eq!(
        input("A".repeat(MAX_RUN_INPUT_BASE64 + 1)).unwrap_err(),
        IpcError::PayloadTooLarge { limit: 87_384 }
    );
    for bad in ["NDI", "N DI=", "NDI=NDIK", "!!!!", "NDJ=", "NDIK\n"] {
        assert_eq!(
            input(bad.to_owned()).unwrap_err(),
            invalid(InvalidReason::BadEncoding, "data"),
            "{bad:?}"
        );
    }
    assert_eq!(input(String::new()).unwrap().bytes().unwrap(), b"");
}

#[test]
fn terminal_sizes_are_bounded() {
    for (cols, rows, field) in [
        (1, 24, "cols"),
        (1001, 24, "cols"),
        (80, 0, "rows"),
        (80, 1001, "rows"),
        (70_000, 24, "cols"),
        (-3, 24, "cols"),
    ] {
        assert_eq!(
            decode::<RunResizeRequest>(json!({"runId": RUN_ID, "cols": cols, "rows": rows})).unwrap_err(),
            invalid(InvalidReason::OutOfRange, field),
            "{cols}x{rows}"
        );
        let start = json!({"buildId": "bd_0123456789abcdef0123456789abcdef", "runOptions": {"cols": cols, "rows": rows}});
        assert_eq!(
            decode::<RunStartRequest>(start).unwrap_err(),
            invalid(InvalidReason::OutOfRange, &format!("runOptions.{field}"))
        );
    }
    for (cols, rows) in [(2, 1), (1000, 1000)] {
        decode::<RunResizeRequest>(json!({"runId": RUN_ID, "cols": cols, "rows": rows})).unwrap();
    }
    assert_eq!(
        decode::<RunAckRequest>(json!({"runId": RUN_ID, "seq": 9_007_199_254_740_992_u64})).unwrap_err(),
        invalid(InvalidReason::OutOfRange, "seq")
    );
    assert_eq!(
        decode::<SettingsPatch>(json!({"codeStyle": {"indentWidth": 3}})).unwrap_err(),
        invalid(InvalidReason::OutOfRange, "codeStyle.indentWidth")
    );
    assert_eq!(
        decode::<SettingsPatch>(json!({"console": {"scrollbackLines": 100_001}})).unwrap_err(),
        invalid(InvalidReason::OutOfRange, "console.scrollbackLines")
    );
}

#[test]
fn documents_are_size_checked_in_requests_too() {
    let mut value = sample::<ProjectSaveRequest>();
    value["document"] = json!(" ".repeat(MAX_DOCUMENT_BYTES + 1));
    assert_eq!(
        decode::<ProjectSaveRequest>(value).unwrap_err(),
        IpcError::PayloadTooLarge { limit: 33_554_432 }
    );
}

#[test]
fn documents_are_parsed_by_the_strict_loader() {
    let request = decode::<ProjectSaveRequest>(sample::<ProjectSaveRequest>()).unwrap();
    let document = parse_document(&request.document).unwrap();
    assert_eq!(document.modules.len(), 1);
    let IpcError::InvalidDocument { diagnostics } =
        parse_document(r#"{"format": "blocks2cpp/project", "format": "x"}"#).unwrap_err()
    else {
        panic!("expected invalidDocument");
    };
    assert_eq!(diagnostics[0].code, "B2C-E0105");
}
