//! Running: `run_start` with its two channels, `run_input`, `run_resize`,
//! `run_stop` and `run_ack` (`docs/spec/02-architecture.md` §2.4.3).
//!
//! **Channels.** `onOutput` carries the program's output as raw byte batches,
//! numbered from 1 in send order. `onEvent` carries [`RunEvent`]s as JSON; each
//! event that depends on output order names the number of batches sent before it
//! (`afterSeq`), and the frontend applies it only after writing that many batches.
//!
//! **Flow control.** The frontend acknowledges written batches with `run_ack`.
//! When more than [`OUTPUT_UNACKED_MAX`](crate::limits::OUTPUT_UNACKED_MAX) bytes
//! are unacknowledged, the backend keeps only the tail of the output, sends
//! [`RunEvent::Skipped`] and then the tail.

use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::commands::IpcRequest;
use crate::decode::decode_base64;
use crate::dto::id_request;
use crate::error::{InvalidReason, IpcError};
use crate::ids::{BuildId, RunId};
use crate::limits::{MAX_RUN_INPUT_BASE64, MAX_RUN_INPUT_BYTES, MAX_SAFE_INTEGER, RUN_COLS, RUN_ROWS};
use crate::macros::string_enum;
use crate::schema::{FieldSchema, FieldSpec, ObjectSchema};

/// The schema of a `u16` limited to `range`.
// `i64::from` cannot be called in a constant; u16 always fits in i64.
#[allow(clippy::cast_lossless)]
const fn u16_range(start: u16, end: u16) -> FieldSchema {
    FieldSchema::Int {
        min: start as i64,
        max: end as i64,
    }
}

const COLS: FieldSchema = u16_range(*RUN_COLS.start(), *RUN_COLS.end());
const ROWS: FieldSchema = u16_range(*RUN_ROWS.start(), *RUN_ROWS.end());

/// Checks a terminal size against [`RUN_COLS`] and [`RUN_ROWS`].
fn check_size(cols: u16, rows: u16, prefix: &str) -> Result<(), IpcError> {
    if !RUN_COLS.contains(&cols) {
        return Err(IpcError::invalid(
            InvalidReason::OutOfRange,
            Some(&format!("{prefix}cols")),
        ));
    }
    if !RUN_ROWS.contains(&rows) {
        return Err(IpcError::invalid(
            InvalidReason::OutOfRange,
            Some(&format!("{prefix}rows")),
        ));
    }
    Ok(())
}

/// The terminal a program runs in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct RunOptions {
    /// The width in columns, within [`RUN_COLS`].
    pub cols: u16,
    /// The height in rows, within [`RUN_ROWS`].
    pub rows: u16,
}

static RUN_OPTIONS: ObjectSchema = ObjectSchema {
    fields: &[
        FieldSpec::required("cols", COLS),
        FieldSpec::required("rows", ROWS),
    ],
};

/// The request of `run_start`: run the program of a successful build whose
/// content still matches the project. Arguments and the working directory come
/// from the built document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct RunStartRequest {
    /// The build to run.
    pub build_id: BuildId,
    /// The terminal.
    pub run_options: RunOptions,
}

impl IpcRequest for RunStartRequest {
    const COMMAND: &'static str = "run_start";

    fn schema() -> &'static ObjectSchema {
        static SCHEMA: ObjectSchema = ObjectSchema {
            fields: &[
                FieldSpec::required("buildId", BuildId::SCHEMA),
                FieldSpec::required("runOptions", FieldSchema::Object(&RUN_OPTIONS)),
            ],
        };
        &SCHEMA
    }

    fn sample() -> Self {
        Self {
            build_id: BuildId::example(),
            run_options: RunOptions { cols: 80, rows: 24 },
        }
    }

    fn validate(&self) -> Result<(), IpcError> {
        check_size(self.run_options.cols, self.run_options.rows, "runOptions.")
    }
}

/// The response of `run_start`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct RunStartResponse {
    /// The new run.
    pub run_id: RunId,
}

/// The request of `run_input`: bytes typed into the program's terminal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct RunInputRequest {
    /// The run.
    pub run_id: RunId,
    /// The bytes as standard base64 with padding: at most
    /// [`MAX_RUN_INPUT_BYTES`] bytes ([`MAX_RUN_INPUT_BASE64`] characters).
    pub data: String,
}

impl RunInputRequest {
    /// The decoded input bytes.
    ///
    /// # Errors
    /// Returns [`IpcError::PayloadTooLarge`] for too much data and
    /// [`IpcError::InvalidRequest`] (`badEncoding` at `data`) for text that is not
    /// strict base64.
    pub fn bytes(&self) -> Result<Vec<u8>, IpcError> {
        if self.data.len() > MAX_RUN_INPUT_BASE64 {
            return Err(IpcError::too_large(MAX_RUN_INPUT_BASE64));
        }
        let bytes = decode_base64(&self.data).map_err(|error| error.at_field("data"))?;
        if bytes.len() > MAX_RUN_INPUT_BYTES {
            return Err(IpcError::too_large(MAX_RUN_INPUT_BYTES));
        }
        Ok(bytes)
    }
}

impl IpcRequest for RunInputRequest {
    const COMMAND: &'static str = "run_input";

    fn schema() -> &'static ObjectSchema {
        static SCHEMA: ObjectSchema = ObjectSchema {
            fields: &[
                FieldSpec::required("runId", RunId::SCHEMA),
                FieldSpec::required(
                    "data",
                    FieldSchema::Base64 {
                        max_chars: MAX_RUN_INPUT_BASE64,
                        max_bytes: MAX_RUN_INPUT_BYTES,
                    },
                ),
            ],
        };
        &SCHEMA
    }

    fn sample() -> Self {
        Self {
            run_id: RunId::example(),
            // "42\n"
            data: "NDIK".to_owned(),
        }
    }

    fn validate(&self) -> Result<(), IpcError> {
        self.bytes().map(|_| ())
    }
}

/// The request of `run_resize`: the terminal changed size.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct RunResizeRequest {
    /// The run.
    pub run_id: RunId,
    /// The new width in columns, within [`RUN_COLS`].
    pub cols: u16,
    /// The new height in rows, within [`RUN_ROWS`].
    pub rows: u16,
}

impl IpcRequest for RunResizeRequest {
    const COMMAND: &'static str = "run_resize";

    fn schema() -> &'static ObjectSchema {
        static SCHEMA: ObjectSchema = ObjectSchema {
            fields: &[
                FieldSpec::required("runId", RunId::SCHEMA),
                FieldSpec::required("cols", COLS),
                FieldSpec::required("rows", ROWS),
            ],
        };
        &SCHEMA
    }

    fn sample() -> Self {
        Self {
            run_id: RunId::example(),
            cols: 120,
            rows: 40,
        }
    }

    fn validate(&self) -> Result<(), IpcError> {
        check_size(self.cols, self.rows, "")
    }
}

id_request!(
    /// The request of `run_stop`: kill the program's whole process tree.
    RunStopRequest, command = "run_stop",
    /// The run.
    run_id: RunId = "runId"
);

/// The request of `run_ack`: the terminal has written every output batch up to
/// `seq`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct RunAckRequest {
    /// The run.
    pub run_id: RunId,
    /// The highest output batch written, at most [`MAX_SAFE_INTEGER`].
    pub seq: u64,
}

// `MAX_SAFE_INTEGER` is 2^53 - 1, so it fits in i64.
#[allow(clippy::cast_possible_wrap)]
const SEQ: FieldSchema = FieldSchema::Int {
    min: 0,
    max: MAX_SAFE_INTEGER as i64,
};

impl IpcRequest for RunAckRequest {
    const COMMAND: &'static str = "run_ack";

    fn schema() -> &'static ObjectSchema {
        static SCHEMA: ObjectSchema = ObjectSchema {
            fields: &[
                FieldSpec::required("runId", RunId::SCHEMA),
                FieldSpec::required("seq", SEQ),
            ],
        };
        &SCHEMA
    }

    fn sample() -> Self {
        Self {
            run_id: RunId::example(),
            seq: 7,
        }
    }

    fn validate(&self) -> Result<(), IpcError> {
        if self.seq > MAX_SAFE_INTEGER {
            return Err(IpcError::invalid(InvalidReason::OutOfRange, Some("seq")));
        }
        Ok(())
    }
}

string_enum! {
    /// How the program's process tree is contained.
    pub enum Containment {
        /// A Windows Job Object.
        JobObject = "jobObject",
        /// A Linux cgroup v2 scope.
        Cgroup = "cgroup",
        /// Only a process group (Linux without cgroups): a program can escape it.
        ProcessGroupOnly = "processGroupOnly",
    }
}

string_enum! {
    /// How the program's terminal is connected.
    pub enum RunMode {
        /// A pseudo-terminal (`ConPTY` or `openpty`).
        Pty = "pty",
        /// Plain pipes, when no pseudo-terminal is available.
        Pipes = "pipes",
    }
}

/// How a program ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum ExitStatus {
    /// It returned or called `exit`.
    Exited {
        /// The exit code.
        code: i32,
    },
    /// A signal ended it (Linux).
    Signaled {
        /// The signal number.
        signal: i32,
    },
    /// An exception ended it (Windows).
    Exception {
        /// The `NTSTATUS` code, for example `0xC0000005`.
        ntstatus: u32,
    },
    /// The user stopped it.
    Stopped,
}

string_enum! {
    /// Why a program crashed, when it did.
    pub enum Crash {
        /// It accessed memory it may not (segmentation fault, access violation).
        MemoryAccess = "memoryAccess",
        /// It ran out of stack, usually through endless recursion.
        StackOverflow = "stackOverflow",
        /// It divided an integer by zero.
        DivisionByZero = "divisionByZero",
        /// It called `abort` (often a failed check).
        Aborted = "aborted",
        /// It hit a trap instruction.
        Trap = "trap",
        /// It was interrupted (Ctrl+C).
        Interrupted = "interrupted",
        /// It was asked to terminate.
        Terminated = "terminated",
        /// It was killed.
        Killed = "killed",
        /// It ran out of memory.
        OutOfMemory = "outOfMemory",
        /// The heap was corrupted.
        HeapCorruption = "heapCorruption",
        /// A DLL it needs is missing (Windows).
        MissingDll = "missingDll",
        /// It wrote to a closed pipe.
        BrokenPipe = "brokenPipe",
        /// It exceeded a resource limit.
        ResourceLimit = "resourceLimit",
        /// Another crash.
        Other = "other",
    }
}

string_enum! {
    /// The sanitizer that reported a problem.
    pub enum SanitizerTool {
        /// `AddressSanitizer`.
        Address = "address",
        /// `UndefinedBehaviorSanitizer`.
        Undefined = "undefined",
    }
}

/// The kind of problem a sanitizer reported, for example `heap-buffer-overflow`:
/// 1 to 64 characters from `a`–`z`, `0`–`9` and `-`.
///
/// The backend parses it from the program's output, which the program controls,
/// so the format is enforced whenever a value is made.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(type = "string"))]
pub struct SanitizerKind(Box<str>);

impl SanitizerKind {
    /// The longest kind.
    pub const MAX_LEN: usize = 64;

    /// Validates a kind, or returns `None` when `text` does not match the format.
    pub fn new(text: &str) -> Option<Self> {
        let valid = (1..=Self::MAX_LEN).contains(&text.len())
            && text
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
        valid.then(|| Self(text.into()))
    }

    /// The kind's text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for SanitizerKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl Serialize for SanitizerKind {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for SanitizerKind {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Self::new(&text).ok_or_else(|| serde::de::Error::custom("malformed sanitizer kind"))
    }
}

/// A sanitizer report found in the program's output.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct SanitizerReport {
    /// The sanitizer.
    pub tool: SanitizerTool,
    /// What it found.
    pub kind: SanitizerKind,
}

/// A message on the `onEvent` channel of `run_start`. [`RunEvent::Started`] comes
/// first and [`RunEvent::Exit`] last, exactly once each.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum RunEvent {
    /// The program started.
    Started {
        /// How its process tree is contained.
        containment: Containment,
        /// How its terminal is connected.
        mode: RunMode,
        /// Whether the IDE helper unit was linked in (UTF-8 console on Windows).
        ide_helpers: bool,
    },
    /// Output was dropped because the terminal fell too far behind.
    Skipped {
        /// How many lines were dropped.
        lines: u64,
        /// How many output batches were sent before this event.
        after_seq: u64,
    },
    /// The program ended. Nothing follows.
    Exit {
        /// How many output batches were sent before this event.
        after_seq: u64,
        /// How long it ran, in milliseconds.
        elapsed_ms: u64,
        /// How it ended.
        status: ExitStatus,
        /// Why it crashed, when it did.
        crash: Option<Crash>,
        /// The first sanitizer report in its output, if any.
        sanitizer: Option<SanitizerReport>,
        /// The friendly summary, for example `Finished (exit code 0)`.
        message: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitizer_kinds_are_validated() {
        assert_eq!(
            SanitizerKind::new("heap-buffer-overflow").unwrap().as_str(),
            "heap-buffer-overflow"
        );
        assert!(SanitizerKind::new(&"a".repeat(64)).is_some());
        for bad in ["", "Heap", "a b", "<x>", "a_b", "é", &"a".repeat(65)] {
            assert!(SanitizerKind::new(bad).is_none(), "{bad:?}");
            assert!(serde_json::from_value::<SanitizerKind>(serde_json::json!(bad)).is_err());
        }
    }

    #[test]
    fn input_bytes_are_bounded() {
        let request = |data: String| RunInputRequest {
            run_id: RunId::example(),
            data,
        };
        let max = crate::decode::encode_base64(&vec![b'x'; MAX_RUN_INPUT_BYTES]);
        assert_eq!(max.len(), MAX_RUN_INPUT_BASE64);
        assert_eq!(request(max).bytes().unwrap().len(), MAX_RUN_INPUT_BYTES);
        // 65,537 bytes still fit in 87,384 characters (one `=` of padding).
        let over = crate::decode::encode_base64(&vec![b'x'; MAX_RUN_INPUT_BYTES + 1]);
        assert_eq!(over.len(), MAX_RUN_INPUT_BASE64);
        assert_eq!(
            request(over).validate().unwrap_err(),
            IpcError::too_large(MAX_RUN_INPUT_BYTES)
        );
        assert_eq!(
            request("A".repeat(MAX_RUN_INPUT_BASE64 + 3))
                .validate()
                .unwrap_err(),
            IpcError::too_large(MAX_RUN_INPUT_BASE64)
        );
        assert_eq!(
            request("NDI=K".to_owned()).validate().unwrap_err(),
            IpcError::invalid(InvalidReason::BadEncoding, Some("data"))
        );
    }

    #[test]
    fn sizes_are_rechecked() {
        let mut request = RunResizeRequest::sample();
        request.validate().unwrap();
        request.cols = 1;
        assert_eq!(
            request.validate().unwrap_err(),
            IpcError::invalid(InvalidReason::OutOfRange, Some("cols"))
        );
        request.cols = 1000;
        request.rows = 0;
        assert_eq!(
            request.validate().unwrap_err(),
            IpcError::invalid(InvalidReason::OutOfRange, Some("rows"))
        );
        let mut start = RunStartRequest::sample();
        start.run_options.cols = 1001;
        assert_eq!(
            start.validate().unwrap_err(),
            IpcError::invalid(InvalidReason::OutOfRange, Some("runOptions.cols"))
        );
        let mut ack = RunAckRequest::sample();
        ack.seq = MAX_SAFE_INTEGER + 1;
        assert_eq!(
            ack.validate().unwrap_err(),
            IpcError::invalid(InvalidReason::OutOfRange, Some("seq"))
        );
    }
}
