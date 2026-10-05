//! Renders the files generated from the IPC contract (feature `ts`).
//!
//! | File (from the repository root)                                  | Contents                          |
//! |------------------------------------------------------------------|-----------------------------------|
//! | `packages/ipc-types/src/generated/types.ts`                      | every IPC type, sorted by name    |
//! | `packages/ipc-types/src/generated/commands.ts`                   | version, command names and client |
//! | `apps/desktop/src-tauri/isolation/allowlist.generated.js`        | the isolation hook's allowlist    |
//! | `apps/desktop/src-tauri/isolation-tests/samples.generated.json`  | one valid payload per command     |
//!
//! `tests/generate.rs` writes them when `B2C_UPDATE_IPC=1` is set and otherwise
//! fails when a committed file differs, so the files cannot go stale. The output
//! is deterministic: fixed order, LF line endings, and a `@generated` header. The
//! JavaScript and JSON follow Prettier's layout for the desktop app's settings
//! (single quotes, 100 columns), so its format check passes unchanged.
//!
//! The types come from `ts-rs` declarations (no `#[ts(export)]` anywhere, so
//! nothing is written implicitly); the client and the allowlist are rendered here
//! from [`COMMANDS`].

use std::collections::BTreeSet;
use std::fmt::Write as _;

use serde_json::Value;
use ts_rs::{Config, TS};

use crate::commands::{COMMAND_NAMES, COMMANDS, ChannelSpec, CommandSpec, IpcRequest};
use crate::diag::{DiagSource, Diagnostic, Location, Part, Related, Severity};
#[allow(clippy::wildcard_imports)] // The generator declares every IPC type.
use crate::dto::*;
use crate::error::{InvalidReason, IoKind, IpcError};
use crate::ids::{BuildId, Handle, RecentId, RunId, SnapshotId, ToolchainId};
use crate::links::LinkId;
use crate::schema::{FieldSchema, ObjectSchema};

/// The environment variable that makes `tests/generate.rs` write the files.
pub const UPDATE_ENV: &str = "B2C_UPDATE_IPC";

/// The command that regenerates the files.
pub const REGENERATE: &str = "B2C_UPDATE_IPC=1 cargo test -p b2c-ipc --features ts --test generate";

/// Prettier's line width in the desktop app and the packages.
const WIDTH: usize = 100;

/// The prefix of a channel argument in an IPC payload (Tauri's
/// `IPC_PAYLOAD_PREFIX`).
pub const CHANNEL_PREFIX: &str = "__CHANNEL__:";

/// A generated file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedFile {
    /// The path relative to the repository root, with `/` separators.
    pub path: &'static str,
    /// The contents.
    pub contents: String,
}

/// Every generated file.
pub fn files() -> Vec<GeneratedFile> {
    vec![
        GeneratedFile {
            path: "packages/ipc-types/src/generated/types.ts",
            contents: types_ts(),
        },
        GeneratedFile {
            path: "packages/ipc-types/src/generated/commands.ts",
            contents: commands_ts(),
        },
        GeneratedFile {
            path: "apps/desktop/src-tauri/isolation/allowlist.generated.js",
            contents: allowlist_js(),
        },
        GeneratedFile {
            path: "apps/desktop/src-tauri/isolation-tests/samples.generated.json",
            contents: samples_json(),
        },
    ]
}

/// The `ts-rs` configuration: 64-bit integers become `number`. Every 64-bit value
/// the backend sends (sizes, durations, counters) stays far below 2^53.
pub fn config() -> Config {
    Config::new().with_large_int("number")
}

/// One TypeScript declaration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeDecl {
    /// The type's name.
    pub name: String,
    /// `export type …;` with its documentation comment.
    pub text: String,
    /// The names of the types it refers to.
    pub dependencies: Vec<String>,
}

fn decl<T: TS + ?Sized + 'static>(cfg: &Config) -> TypeDecl {
    let mut text = T::docs().unwrap_or_default();
    text.push_str("export ");
    text.push_str(&T::decl(cfg));
    TypeDecl {
        name: T::ident(cfg),
        text: tidy(&text),
        dependencies: T::dependencies(cfg).into_iter().map(|d| d.ts_name).collect(),
    }
}

/// Removes trailing spaces from every line and turns rustdoc links
/// (`` [`Name`] `` and `` [`Name`](path) ``) into plain code spans.
fn tidy(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for line in text.lines() {
        out.push_str(&plain_links(line.trim_end()));
        out.push('\n');
    }
    out.truncate(out.trim_end().len());
    out
}

fn plain_links(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    while let Some(start) = rest.find("[`") {
        let Some(len) = rest[start + 2..].find("`]") else {
            break;
        };
        out.push_str(&rest[..start]);
        let code_end = start + 2 + len;
        out.push('`');
        out.push_str(&rest[start + 2..code_end]);
        out.push('`');
        rest = &rest[code_end + 2..];
        if rest.starts_with('(')
            && let Some(close) = rest.find(')')
        {
            rest = &rest[close + 1..];
        }
    }
    out.push_str(rest);
    out
}

macro_rules! decls {
    ($cfg:expr; $($ty:ty),+ $(,)?) => {
        vec![$(decl::<$ty>($cfg)),+]
    };
}

/// Every IPC type's declaration, sorted by name.
pub fn type_decls() -> Vec<TypeDecl> {
    let cfg = config();
    let mut decls = decls![&cfg;
        // Errors, diagnostics, IDs and links.
        IpcError, InvalidReason, IoKind,
        Diagnostic, Location, Part, Related, Severity, DiagSource,
        Handle, BuildId, RunId, RecentId, SnapshotId, ToolchainId, LinkId,
        // App.
        AppEvent, AppInfo, Empty, Platform,
        // Projects.
        Template, ProjectNewRequest, ProjectNewResponse, ProjectOpened, ProjectOpenDialogResponse,
        ProjectOpenRecentRequest, ProjectReloadRequest, ProjectReloadResponse, ProjectSaveRequest,
        ProjectSaveResponse, ProjectSaveAsDialogRequest, ProjectSavedAs, ProjectSaveAsDialogResponse,
        ProjectCloseRequest, ProjectSetDirtyRequest,
        // Recent projects and recovery.
        RecentEntry, RecentListResponse, RecentRemoveRequest,
        RecoverySaveRequest, SnapshotInfo, RecoveryListResponse, RecoveryRestoreRequest,
        RecoveryRestoreResponse, RecoveryDiscardRequest,
        // Trust.
        Trust, TrustState, TrustSource, RestrictedReason, TrustGetRequest, TrustGrantRequest,
        TrustRevokeRequest, TrustResponse,
        // Toolchains.
        Toolchain, ToolchainSource, ToolchainCapabilities, CppStandard, ToolchainListResponse,
        ToolchainAddDialogResponse, ToolchainSelectRequest, ToolchainSetupInfo, Distro,
        // Builds.
        BuildConfig, BuildStartRequest, BuildStartResponse, BuildCancelRequest,
        BuildCacheClearResponse, BuildStage, BuildOutcome, BuildEvent,
        // Runs.
        RunOptions, RunStartRequest, RunStartResponse, RunInputRequest, RunResizeRequest,
        RunStopRequest, RunAckRequest, Containment, RunMode, ExitStatus, Crash, SanitizerTool,
        SanitizerKind, SanitizerReport, RunEvent,
        // Settings and help.
        Settings, CodeStyle, IndentWidth, RunSettings, OnErrors, ConsoleSettings, ToolchainSettings,
        NewProjectSettings, BuildCacheSettings, SettingsNotice, NoticeReason, SettingsGetResponse,
        SettingsUpdateResponse, SettingsPatch, CodeStylePatch, RunSettingsPatch, ConsolePatch,
        OpenHelpLinkRequest,
    ];
    decls.sort_by(|a, b| a.name.cmp(&b.name));
    decls
}

/// The request types with their commands, so a test can check the table's
/// `request_ts` names against the types.
pub fn request_types() -> Vec<(&'static str, String)> {
    fn entry<T: IpcRequest + TS>(cfg: &Config) -> (&'static str, String) {
        (T::COMMAND, T::ident(cfg))
    }
    let cfg = config();
    vec![
        entry::<ProjectNewRequest>(&cfg),
        entry::<ProjectOpenRecentRequest>(&cfg),
        entry::<ProjectReloadRequest>(&cfg),
        entry::<ProjectSaveRequest>(&cfg),
        entry::<ProjectSaveAsDialogRequest>(&cfg),
        entry::<ProjectCloseRequest>(&cfg),
        entry::<ProjectSetDirtyRequest>(&cfg),
        entry::<RecentRemoveRequest>(&cfg),
        entry::<RecoverySaveRequest>(&cfg),
        entry::<RecoveryRestoreRequest>(&cfg),
        entry::<RecoveryDiscardRequest>(&cfg),
        entry::<TrustGetRequest>(&cfg),
        entry::<TrustGrantRequest>(&cfg),
        entry::<TrustRevokeRequest>(&cfg),
        entry::<ToolchainSelectRequest>(&cfg),
        entry::<BuildStartRequest>(&cfg),
        entry::<BuildCancelRequest>(&cfg),
        entry::<RunStartRequest>(&cfg),
        entry::<RunInputRequest>(&cfg),
        entry::<RunResizeRequest>(&cfg),
        entry::<RunStopRequest>(&cfg),
        entry::<RunAckRequest>(&cfg),
        entry::<SettingsPatch>(&cfg),
        entry::<OpenHelpLinkRequest>(&cfg),
    ]
}

/// The header of the generated TypeScript and JavaScript files.
fn header(what: &str) -> String {
    format!(
        "// @generated by crates/b2c-ipc (src/generate.rs): {what}\n\
         // Do not edit. Regenerate with: {REGENERATE}\n"
    )
}

/// `packages/ipc-types/src/generated/types.ts`.
pub fn types_ts() -> String {
    let mut out = header("every IPC type, sorted by name.");
    for decl in type_decls() {
        out.push('\n');
        out.push_str(&decl.text);
        out.push('\n');
    }
    out
}

/// `fooBar` for `foo_bar`.
fn camel_case(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut upper = false;
    for c in name.chars() {
        if c == '_' {
            upper = true;
        } else if upper {
            out.extend(c.to_uppercase());
            upper = false;
        } else {
            out.push(c);
        }
    }
    out
}

/// The parameter list of a client method (without parentheses).
fn method_params(spec: &CommandSpec) -> Vec<String> {
    let mut params = Vec::new();
    if let Some(request_ts) = spec.request_ts {
        params.push(format!("request: {request_ts}"));
    }
    for channel in spec.channels {
        let arg = if channel.raw { "bytes" } else { "event" };
        params.push(format!("{}: ({arg}: {}) => void", channel.name, channel.ts_type));
    }
    params
}

/// The channel argument of a client method's call.
fn channel_arg(channel: &ChannelSpec, indent: &str) -> String {
    if channel.raw {
        format!(
            "{indent}{name}: t.channel((message) => {{\n\
             {indent}  const bytes = toArrayBuffer(message);\n\
             {indent}  if (bytes !== undefined) {{\n\
             {indent}    {name}(bytes);\n\
             {indent}  }}\n\
             {indent}}}),\n",
            name = channel.name
        )
    } else {
        format!(
            "{indent}{name}: t.channel((message) => {{\n\
             {indent}  {name}(message as {ts});\n\
             {indent}}}),\n",
            name = channel.name,
            ts = channel.ts_type
        )
    }
}

/// The names of the generated types the client refers to.
fn client_imports() -> BTreeSet<&'static str> {
    let mut names = BTreeSet::from(["IpcError"]);
    for spec in COMMANDS {
        names.insert(spec.response_ts);
        names.extend(spec.request_ts);
        names.extend(spec.channels.iter().filter(|c| !c.raw).map(|c| c.ts_type));
    }
    names
}

/// `packages/ipc-types/src/generated/commands.ts`.
pub fn commands_ts() -> String {
    let mut out = header("the IPC version, the command names and the typed client.");
    out.push_str("\nimport type {\n");
    for name in client_imports() {
        let _ = writeln!(out, "  {name},");
    }
    out.push_str("} from './types';\n\n");
    let _ = writeln!(
        out,
        "/**\n * The IPC contract version. `app_info` reports the backend's; the frontend refuses to\n * run against a different one (docs/spec/09-quality-and-delivery.md §9.5).\n */\nexport const IPC_VERSION = {};\n",
        crate::IPC_VERSION
    );
    out.push_str(
        "/** Every command, in the order of the Rust command table. */\nexport const COMMAND_NAMES = [\n",
    );
    for name in COMMAND_NAMES {
        let _ = writeln!(out, "  '{name}',");
    }
    out.push_str("] as const;\n\n/** The name of a command. */\nexport type CommandName = (typeof COMMAND_NAMES)[number];\n\n");
    out.push_str("/** Every `IpcError` code. */\nexport const IPC_ERROR_CODES = [\n");
    for code in IpcError::CODES {
        let _ = writeln!(out, "  '{code}',");
    }
    out.push_str("] as const;\n\n");
    out.push_str(CLIENT_SUPPORT);
    out.push_str(
        "\n/** The typed client: one method per command. Every method rejects with an `IpcCallError`. */\n\
         export interface IpcClient {\n",
    );
    for spec in COMMANDS {
        let method = camel_case(spec.name);
        let params = method_params(spec);
        let _ = writeln!(out, "  /** Calls `{}`. */", spec.name);
        let line = format!(
            "  {method}({}): Promise<{}>;",
            params.join(", "),
            spec.response_ts
        );
        if line.len() <= WIDTH {
            out.push_str(&line);
            out.push('\n');
        } else {
            let _ = writeln!(out, "  {method}(");
            for param in &params {
                let _ = writeln!(out, "    {param},");
            }
            let _ = writeln!(out, "  ): Promise<{}>;", spec.response_ts);
        }
    }
    out.push_str(
        "}\n\n/** Creates the client over a transport. */\n\
         export function createIpcClient(t: IpcTransport): IpcClient {\n  return {\n",
    );
    for spec in COMMANDS {
        out.push_str(&client_method(spec));
    }
    out.push_str("  };\n}\n");
    out
}

/// One method of the object `createIpcClient` returns.
fn client_method(spec: &CommandSpec) -> String {
    let mut names: Vec<&str> = Vec::new();
    if spec.request.is_some() {
        names.push("request");
    }
    names.extend(spec.channels.iter().map(|c| c.name));
    let params = format!("({})", names.join(", "));
    let method = camel_case(spec.name);
    let cast = format!(" as Promise<{}>,\n", spec.response_ts);
    if spec.channels.is_empty() {
        let args = if spec.request.is_some() {
            "{ request }"
        } else {
            "{}"
        };
        let line = format!("    {method}: {params} => call(t, '{}', {args}){cast}", spec.name);
        if line.trim_end().len() <= WIDTH {
            return line;
        }
        return format!(
            "    {method}: {params} =>\n      call(t, '{}', {args}){cast}",
            spec.name
        );
    }
    let mut out = format!("    {method}: {params} =>\n      call(t, '{}', {{\n", spec.name);
    if spec.request.is_some() {
        out.push_str("        request,\n");
    }
    for channel in spec.channels {
        out.push_str(&channel_arg(channel, "        "));
    }
    let _ = write!(out, "      }}){cast}");
    out
}

/// The hand-written part of `commands.ts`: the transport, the error and helpers.
const CLIENT_SUPPORT: &str = r"/**
 * How the client reaches the backend: Tauri's `invoke` and `Channel` in the app, a fake in tests.
 */
export interface IpcTransport {
  /** Calls a command; rejects with the command's error. */
  invoke(cmd: string, args: Record<string, unknown>): Promise<unknown>;
  /** Creates a channel whose messages go to `onMessage`; the result is passed as an argument. */
  channel(onMessage: (message: unknown) => void): unknown;
}

/** A failure below the command, such as a command the backend does not know. */
export interface TransportError {
  code: 'transport';
  message: string;
}

/** The error every client method rejects with. */
export class IpcCallError extends Error {
  /** The command that failed. */
  readonly command: CommandName;
  /** The command's typed error, or a transport failure. */
  readonly error: IpcError | TransportError;

  constructor(command: CommandName, error: IpcError | TransportError) {
    super(`IPC command ${command} failed: ${error.code}`);
    this.name = 'IpcCallError';
    this.command = command;
    this.error = error;
  }
}

const ERROR_CODES: ReadonlySet<string> = new Set(IPC_ERROR_CODES);

/** Whether `value` is an `IpcError`: an object whose `code` is one of `IPC_ERROR_CODES`. */
export function isIpcError(value: unknown): value is IpcError {
  if (typeof value !== 'object' || value === null || !('code' in value)) {
    return false;
  }
  const code: unknown = value.code;
  return typeof code === 'string' && ERROR_CODES.has(code);
}

function describe(reason: unknown): string {
  if (reason instanceof Error) {
    return reason.message;
  }
  return typeof reason === 'string' ? reason : 'unknown transport failure';
}

async function call(
  t: IpcTransport,
  command: CommandName,
  args: Record<string, unknown>,
): Promise<unknown> {
  try {
    return await t.invoke(command, args);
  } catch (reason: unknown) {
    const error = isIpcError(reason)
      ? reason
      : { code: 'transport' as const, message: describe(reason) };
    throw new IpcCallError(command, error);
  }
}

/** The bytes of a raw channel message (an `ArrayBuffer`, or a view of one). */
function toArrayBuffer(message: unknown): ArrayBuffer | undefined {
  if (message instanceof ArrayBuffer) {
    return message;
  }
  if (ArrayBuffer.isView(message)) {
    return new Uint8Array(message.buffer, message.byteOffset, message.byteLength).slice().buffer;
  }
  return undefined;
}
";

/// A JavaScript value of the allowlist.
#[derive(Debug, Clone)]
enum Js {
    /// A frozen object; `expanded` keeps one key per line, as Prettier does for an
    /// object written that way.
    Object(Vec<(String, Js)>, bool),
    /// A frozen array.
    Array(Vec<Js>),
    /// A string.
    Str(String),
    /// A whole number.
    Int(i64),
    /// `true`.
    True,
}

/// A JavaScript string literal in single quotes.
fn js_string(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('\'');
    for c in text.chars() {
        match c {
            '\'' => out.push_str("\\'"),
            '\\' => out.push_str("\\\\"),
            c if c.is_ascii_graphic() || c == ' ' => out.push(c),
            c => {
                let mut units = [0_u16; 2];
                for unit in c.encode_utf16(&mut units) {
                    let _ = write!(out, "\\u{unit:04x}");
                }
            }
        }
    }
    out.push('\'');
    out
}

impl Js {
    fn inline(&self) -> String {
        match self {
            Self::Object(fields, _) if fields.is_empty() => "Object.freeze({})".to_owned(),
            Self::Object(fields, _) => {
                let parts: Vec<String> = fields
                    .iter()
                    .map(|(k, v)| format!("{k}: {}", v.inline()))
                    .collect();
                format!("Object.freeze({{ {} }})", parts.join(", "))
            }
            Self::Array(items) => {
                let parts: Vec<String> = items.iter().map(Self::inline).collect();
                format!("Object.freeze([{}])", parts.join(", "))
            }
            Self::Str(text) => js_string(text),
            Self::Int(n) => n.to_string(),
            Self::True => "true".to_owned(),
        }
    }

    /// Renders the value starting at column `column` of a line indented by
    /// `indent` spaces, followed by `suffix` on its last line.
    fn render(&self, indent: usize, column: usize, suffix: &str) -> String {
        let inline = self.inline();
        let fits = column + inline.len() + suffix.len() <= WIDTH;
        let pad = " ".repeat(indent);
        match self {
            Self::Object(fields, expanded) if !fields.is_empty() && (*expanded || !fits) => {
                let mut out = "Object.freeze({\n".to_owned();
                for (key, value) in fields {
                    let start = indent + 2 + key.len() + 2;
                    let _ = writeln!(out, "{pad}  {key}: {}", value.render(indent + 2, start, ","));
                }
                let _ = write!(out, "{pad}}}){suffix}");
                out
            }
            Self::Array(items) if !fits => {
                let mut out = "Object.freeze([\n".to_owned();
                for item in items {
                    let _ = writeln!(out, "{pad}  {}", item.render(indent + 2, indent + 2, ","));
                }
                let _ = write!(out, "{pad}]){suffix}");
                out
            }
            _ => format!("{inline}{suffix}"),
        }
    }
}

fn field_js(schema: &FieldSchema, optional: bool) -> Js {
    let mut fields: Vec<(String, Js)> = match *schema {
        FieldSchema::String { max_len } => vec![
            ("type".into(), Js::Str("string".into())),
            ("maxLength".into(), int(max_len)),
        ],
        FieldSchema::Base64 { max_chars, max_bytes } => vec![
            ("type".into(), Js::Str("base64".into())),
            ("maxChars".into(), int(max_chars)),
            ("maxBytes".into(), int(max_bytes)),
        ],
        FieldSchema::Id { prefix, hex_len } => vec![
            ("type".into(), Js::Str("id".into())),
            ("prefix".into(), Js::Str(prefix.into())),
            ("hexLength".into(), int(hex_len)),
        ],
        FieldSchema::Enum { values } => vec![
            ("type".into(), Js::Str("enum".into())),
            (
                "values".into(),
                Js::Array(values.iter().map(|v| Js::Str((*v).into())).collect()),
            ),
        ],
        FieldSchema::Int { min, max } => vec![
            ("type".into(), Js::Str("int".into())),
            ("min".into(), Js::Int(min)),
            ("max".into(), Js::Int(max)),
        ],
        FieldSchema::IntOneOf { values } => vec![
            ("type".into(), Js::Str("intOneOf".into())),
            (
                "values".into(),
                Js::Array(values.iter().map(|v| Js::Int(*v)).collect()),
            ),
        ],
        FieldSchema::Bool => vec![("type".into(), Js::Str("bool".into()))],
        FieldSchema::Object(object) => vec![
            ("type".into(), Js::Str("object".into())),
            ("fields".into(), object_js(object)),
        ],
    };
    if optional {
        fields.push(("optional".into(), Js::True));
    }
    let expanded = matches!(schema, FieldSchema::Object(_));
    Js::Object(fields, expanded)
}

fn int(value: usize) -> Js {
    Js::Int(i64::try_from(value).unwrap_or(i64::MAX))
}

fn object_js(schema: &ObjectSchema) -> Js {
    let fields = schema
        .fields
        .iter()
        .map(|f| (f.name.to_owned(), field_js(&f.schema, f.optional)))
        .collect();
    Js::Object(fields, true)
}

/// The allowlist entry of a command: its exact top-level argument keys.
fn command_js(spec: &CommandSpec) -> Js {
    let mut args = Vec::new();
    if let Some(schema) = spec.request {
        args.push((
            "request".to_owned(),
            Js::Object(
                vec![
                    ("type".into(), Js::Str("object".into())),
                    ("fields".into(), object_js(schema())),
                ],
                true,
            ),
        ));
    }
    for channel in spec.channels {
        args.push((
            channel.name.to_owned(),
            Js::Object(vec![("type".into(), Js::Str("channel".into()))], false),
        ));
    }
    Js::Object(args, true)
}

/// `apps/desktop/src-tauri/isolation/allowlist.generated.js`.
pub fn allowlist_js() -> String {
    let mut out = header("the isolation allowlist.");
    out.push_str(
        "//\n\
         // Every command the editor may call, with the exact shape of its arguments\n\
         // (docs/spec/08-security.md §8.8). validate.js checks every IPC message against it.\n\
         'use strict';\n\
         /* exported B2C_IPC_ALLOWLIST */\n\n\
         const B2C_IPC_ALLOWLIST = Object.freeze({\n",
    );
    for spec in COMMANDS {
        let start = 2 + spec.name.len() + 2;
        let _ = writeln!(out, "  {}: {}", spec.name, command_js(spec).render(2, start, ","));
    }
    out.push_str("});\n");
    out
}

/// Writes `value` as JSON in Prettier's layout: two-space indentation, one key
/// per line, keys sorted (so the output does not depend on `serde_json`'s map
/// order).
fn write_json(value: &Value, indent: usize, out: &mut String) {
    let pad = " ".repeat(indent);
    match value {
        Value::Object(map) if map.is_empty() => out.push_str("{}"),
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            out.push_str("{\n");
            for (index, key) in keys.iter().enumerate() {
                let _ = write!(out, "{pad}  {}: ", Value::String((*key).clone()));
                if let Some(item) = map.get(*key) {
                    write_json(item, indent + 2, out);
                }
                out.push_str(if index + 1 < keys.len() { ",\n" } else { "\n" });
            }
            let _ = write!(out, "{pad}}}");
        }
        Value::Array(items) if items.is_empty() => out.push_str("[]"),
        Value::Array(items) => {
            out.push_str("[\n");
            for (index, item) in items.iter().enumerate() {
                let _ = write!(out, "{pad}  ");
                write_json(item, indent + 2, out);
                out.push_str(if index + 1 < items.len() { ",\n" } else { "\n" });
            }
            let _ = write!(out, "{pad}]");
        }
        scalar => out.push_str(&scalar.to_string()),
    }
}

/// One valid payload (the arguments of `invoke`) per command, with channels as
/// Tauri serialises them.
pub fn sample_payloads() -> Vec<(&'static str, Value)> {
    COMMANDS
        .iter()
        .map(|spec| {
            let mut args = serde_json::Map::new();
            if let Some(sample) = spec.sample {
                args.insert("request".to_owned(), sample());
            }
            for (index, channel) in spec.channels.iter().enumerate() {
                args.insert(
                    channel.name.to_owned(),
                    Value::String(format!("{CHANNEL_PREFIX}{}", 1000 + index)),
                );
            }
            (spec.name, Value::Object(args))
        })
        .collect()
}

/// `apps/desktop/src-tauri/isolation-tests/samples.generated.json`.
pub fn samples_json() -> String {
    let mut out = String::from("{\n");
    let samples = sample_payloads();
    for (index, (name, payload)) in samples.iter().enumerate() {
        let _ = write!(out, "  {}: ", Value::String((*name).to_owned()));
        write_json(payload, 2, &mut out);
        out.push_str(if index + 1 < samples.len() { ",\n" } else { "\n" });
    }
    out.push_str("}\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rustdoc_links_become_code_spans() {
        assert_eq!(
            plain_links("see [`Foo`] and [`Bar`](crate::Bar)."),
            "see `Foo` and `Bar`."
        );
        assert_eq!(plain_links("a [`b"), "a [`b");
        assert_eq!(plain_links("`[\"x\"]`"), "`[\"x\"]`");
        assert_eq!(tidy("a  \n b \n\n"), "a\n b");
    }

    #[test]
    fn names() {
        assert_eq!(camel_case("project_save_as_dialog"), "projectSaveAsDialog");
        assert_eq!(camel_case("app_info"), "appInfo");
        assert_eq!(js_string("it's a\\b\u{e9}"), "'it\\'s a\\\\b\\u00e9'");
    }

    #[test]
    fn long_arrays_break() {
        let values: Vec<Js> = (0..30).map(|i| Js::Str(format!("value{i}"))).collect();
        let rendered = Js::Array(values).render(4, 10, ",");
        assert!(
            rendered.starts_with("Object.freeze([\n      'value0',\n"),
            "{rendered}"
        );
        assert!(rendered.ends_with("    ]),"), "{rendered}");
    }
}
