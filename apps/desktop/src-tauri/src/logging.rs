//! The app's log files (`docs/spec/08-security.md` §8.11,
//! `docs/spec/09-quality-and-delivery.md` §9.1, `docs/spec/02-architecture.md`
//! §2.7).
//!
//! * **Where:** `blocks2cpp.log` in the logs folder ([`b2c_store::Dirs::logs`]),
//!   rotated at 5 MiB into `blocks2cpp.1.log` … `blocks2cpp.4.log`, five
//!   files at most, each `0600` in a `0700` folder
//!   ([`b2c_store::RotatingFile`]). Nothing leaves the machine.
//! * **Format:** one JSON object per line, with its keys in this order:
//!   `timestamp` (RFC 3339, UTC, milliseconds), `level`, `target`, `spans`
//!   (the open spans from the outermost, each `{ "name", "fields" }`),
//!   `fields` (the event's own fields) and `message`.
//! * **Level:** `info` by default; the variable `B2C_LOG` (`error`, `warn`,
//!   `info`, `debug` or `trace`) changes it. Paths are logged at debug level
//!   only, and no level records project content: that is the rule for every
//!   `tracing` call in the Blocks2Cpp crates. Only their events are written
//!   (targets `b2c_*` and `blocks2cpp_desktop`), so no third-party crate can
//!   log content either.
//! * **Bounds:** a field value is cut at [`MAX_VALUE_BYTES`], an event or span
//!   keeps at most [`MAX_FIELDS`] fields and a line at most [`MAX_SPANS`]
//!   spans, and a line over [`MAX_LINE_BYTES`] is replaced by a short one
//!   marked `"truncated": true`.
//! * **Panics:** [`install_panic_hook`] logs where a panic happened, never
//!   its message (which may quote project content).

use std::fmt;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::SystemTime;

use b2c_store::RotatingFile;
use b2c_store::log::{LOG_BASE, LOG_FILES, LOG_MAX_BYTES};
use serde_json::Value;
use tracing::field::{Field, Visit};
use tracing::span::{Attributes, Id, Record};
use tracing::subscriber::Interest;
use tracing::{Event, Level, Metadata, Subscriber};
use tracing_subscriber::layer::{Context, Layer, SubscriberExt as _};
use tracing_subscriber::registry::{LookupSpan, Registry};

/// The environment variable that sets the log level.
pub const LOG_ENV: &str = "B2C_LOG";

/// The level when [`LOG_ENV`] is unset or not a level name.
pub const DEFAULT_LEVEL: Level = Level::INFO;

/// The most bytes of one field value (longer text is cut and ends with `…`).
pub const MAX_VALUE_BYTES: usize = 4 * 1024;

/// The most fields kept per event and per span.
pub const MAX_FIELDS: usize = 32;

/// The most spans written per line (the innermost ones are kept).
pub const MAX_SPANS: usize = 16;

/// The longest line, in bytes; a longer one is replaced by a short version.
pub const MAX_LINE_BYTES: usize = 64 * 1024;

/// The target prefixes whose events are written: the Blocks2Cpp crates.
const OUR_TARGETS: [&str; 2] = ["b2c_", "blocks2cpp_desktop"];

/// Why [`init`] could not start the log file.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum LoggingError {
    /// The log folder or file could not be created or opened (the error has
    /// no path).
    #[error("the log file cannot be opened ({0})")]
    Open(#[source] io::Error),
    /// Another logger was already installed for this process.
    #[error("a logger is already installed")]
    AlreadySet,
}

/// The log level, from the value of [`LOG_ENV`]: one of `error`, `warn`,
/// `info`, `debug` or `trace` (any case, surrounding spaces ignored), else
/// [`DEFAULT_LEVEL`].
pub fn parse_level(value: Option<&str>) -> Level {
    match value.map(|value| value.trim().to_ascii_lowercase()).as_deref() {
        Some("error") => Level::ERROR,
        Some("warn") => Level::WARN,
        Some("info") => Level::INFO,
        Some("debug") => Level::DEBUG,
        Some("trace") => Level::TRACE,
        _ => DEFAULT_LEVEL,
    }
}

/// The log level from the process's environment ([`parse_level`]).
pub fn level_from_env() -> Level {
    let value = std::env::var_os(LOG_ENV);
    parse_level(value.as_deref().and_then(|value| value.to_str()))
}

/// A shared, changeable log level. [`subscriber`] reads it for every event,
/// so changing it takes effect at once.
#[derive(Debug, Clone)]
pub struct LevelHandle {
    rank: Arc<AtomicU8>,
}

impl LevelHandle {
    /// A level handle starting at `level`.
    pub fn new(level: Level) -> Self {
        Self {
            rank: Arc::new(AtomicU8::new(rank(level))),
        }
    }

    /// Changes the level.
    pub fn set(&self, level: Level) {
        self.rank.store(rank(level), Ordering::Relaxed);
    }

    /// The current level.
    pub fn get(&self) -> Level {
        match self.rank.load(Ordering::Relaxed) {
            1 => Level::ERROR,
            2 => Level::WARN,
            3 => Level::INFO,
            4 => Level::DEBUG,
            _ => Level::TRACE,
        }
    }

    /// Whether events at `level` are written.
    fn allows(&self, level: Level) -> bool {
        rank(level) <= self.rank.load(Ordering::Relaxed)
    }
}

/// 1 for errors up to 5 for trace: a higher rank is more verbose.
fn rank(level: Level) -> u8 {
    match level {
        Level::ERROR => 1,
        Level::WARN => 2,
        Level::INFO => 3,
        Level::DEBUG => 4,
        Level::TRACE => 5,
    }
}

/// The running app's log: its level and its files.
#[derive(Debug, Clone)]
pub struct LogHandle {
    level: LevelHandle,
    paths: Vec<PathBuf>,
}

impl LogHandle {
    /// The level, which can be changed while the app runs.
    pub fn level(&self) -> &LevelHandle {
        &self.level
    }

    /// The log files, newest first (whether they exist yet or not).
    pub fn paths(&self) -> &[PathBuf] {
        &self.paths
    }
}

/// Starts the app's log: `blocks2cpp.log` in `dir` with the rotation of the
/// module documentation, at `level`, as the process's global logger.
///
/// # Errors
/// [`LoggingError::Open`] when the folder or file cannot be used;
/// [`LoggingError::AlreadySet`] when a global logger exists already.
pub fn init(dir: &Path, level: Level) -> Result<LogHandle, LoggingError> {
    let file = RotatingFile::open(dir, LOG_BASE, LOG_MAX_BYTES, LOG_FILES).map_err(LoggingError::Open)?;
    let paths = file.paths();
    let level = LevelHandle::new(level);
    tracing::subscriber::set_global_default(subscriber(file, level.clone()))
        .map_err(|_| LoggingError::AlreadySet)?;
    Ok(LogHandle { level, paths })
}

/// A subscriber that writes JSON lines (see the module documentation) to
/// `writer`, at the level `level` holds. [`init`] installs one over the
/// rotating log file; tests use it with their own writer.
pub fn subscriber<W: Write + Send + 'static>(writer: W, level: LevelHandle) -> impl Subscriber + Send + Sync {
    Registry::default().with(JsonLines {
        writer: Mutex::new(writer),
        level,
    })
}

/// Logs every panic's location (file and line), never its message, which may
/// quote project content. In development builds the previous hook runs as
/// well, so the panic also shows in the terminal.
pub fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let location = info.location();
        tracing::error!(
            target: "blocks2cpp_desktop::panic",
            file = location.map(std::panic::Location::file),
            line = location.map(std::panic::Location::line),
            "a thread panicked"
        );
        if cfg!(debug_assertions) {
            previous(info);
        }
    }));
}

/// The layer that writes one JSON line per event.
struct JsonLines<W> {
    writer: Mutex<W>,
    level: LevelHandle,
}

/// The recorded fields of a span, kept in its extensions.
struct SpanFields(Fields);

/// Whether `target` belongs to a Blocks2Cpp crate.
fn is_ours(target: &str) -> bool {
    OUR_TARGETS.iter().any(|prefix| target.starts_with(prefix))
}

impl<S, W> Layer<S> for JsonLines<W>
where
    S: Subscriber + for<'lookup> LookupSpan<'lookup>,
    W: Write + Send + 'static,
{
    fn register_callsite(&self, metadata: &'static Metadata<'static>) -> Interest {
        if is_ours(metadata.target()) {
            // The level can change at run time, so ask each time.
            Interest::sometimes()
        } else {
            Interest::never()
        }
    }

    fn enabled(&self, metadata: &Metadata<'_>, _context: Context<'_, S>) -> bool {
        is_ours(metadata.target()) && self.level.allows(*metadata.level())
    }

    fn max_level_hint(&self) -> Option<tracing::level_filters::LevelFilter> {
        Some(tracing::level_filters::LevelFilter::TRACE)
    }

    fn on_new_span(&self, attributes: &Attributes<'_>, id: &Id, context: Context<'_, S>) {
        let Some(span) = context.span(id) else {
            return;
        };
        let mut fields = Fields::default();
        attributes.record(&mut fields);
        span.extensions_mut().insert(SpanFields(fields));
    }

    fn on_record(&self, id: &Id, values: &Record<'_>, context: Context<'_, S>) {
        let Some(span) = context.span(id) else {
            return;
        };
        let mut extensions = span.extensions_mut();
        if let Some(SpanFields(fields)) = extensions.get_mut::<SpanFields>() {
            values.record(fields);
        }
    }

    fn on_event(&self, event: &Event<'_>, context: Context<'_, S>) {
        let mut fields = Fields::default();
        event.record(&mut fields);
        let mut spans = Vec::new();
        if let Some(scope) = context.event_scope(event) {
            for span in scope.from_root() {
                let extensions = span.extensions();
                let span_fields = extensions.get::<SpanFields>().map_or_else(
                    || Value::Object(serde_json::Map::new()),
                    |SpanFields(f)| f.to_json(),
                );
                spans.push((span.name(), span_fields));
            }
        }
        let skip = spans.len().saturating_sub(MAX_SPANS);
        let line = json_line(
            &b2c_store::rfc3339_utc(SystemTime::now()),
            *event.metadata().level(),
            event.metadata().target(),
            &spans[skip..],
            &fields,
        );
        let mut writer = self.writer.lock().unwrap_or_else(PoisonError::into_inner);
        // A logger has nowhere to report its own failure: a line that
        // cannot be written is lost.
        let _ = writer.write_all(line.as_bytes()).and_then(|()| writer.flush());
    }
}

/// One JSON line, ending in a line feed, with the keys in a fixed order.
fn json_line(
    timestamp: &str,
    level: Level,
    target: &str,
    spans: &[(&str, Value)],
    fields: &Fields,
) -> String {
    let mut line = String::with_capacity(256);
    line.push_str("{\"timestamp\":");
    push_json(&mut line, &Value::from(timestamp));
    line.push_str(",\"level\":");
    push_json(&mut line, &Value::from(level.as_str()));
    line.push_str(",\"target\":");
    push_json(&mut line, &Value::from(clip(target)));
    let head_len = line.len();
    line.push_str(",\"spans\":[");
    for (index, (name, span_fields)) in spans.iter().enumerate() {
        if index > 0 {
            line.push(',');
        }
        line.push_str("{\"name\":");
        push_json(&mut line, &Value::from(*name));
        line.push_str(",\"fields\":");
        push_json(&mut line, span_fields);
        line.push('}');
    }
    line.push_str("],\"fields\":");
    push_json(&mut line, &fields.to_json());
    line.push_str(",\"message\":");
    push_json(
        &mut line,
        &Value::from(fields.message.as_deref().unwrap_or_default()),
    );
    line.push('}');
    if line.len() + 1 > MAX_LINE_BYTES {
        // Keep the head and a short message; drop spans and fields.
        line.truncate(head_len);
        line.push_str(",\"spans\":[],\"fields\":{},\"message\":");
        let message = fields.message.as_deref().unwrap_or_default();
        push_json(&mut line, &Value::from(cut(message, 1024)));
        line.push_str(",\"truncated\":true}");
    }
    line.push('\n');
    line
}

/// Appends `value` as compact JSON. Serialising a `Value` to a `String`
/// cannot fail; nothing is appended if it ever did.
fn push_json(line: &mut String, value: &Value) {
    if let Ok(text) = serde_json::to_string(value) {
        line.push_str(&text);
    }
}

/// `text` cut to [`MAX_VALUE_BYTES`].
fn clip(text: &str) -> String {
    cut(text, MAX_VALUE_BYTES)
}

/// `text` cut to at most `max` bytes at a character boundary, ending with
/// `…` when it was cut.
fn cut(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_owned();
    }
    let mut end = max.saturating_sub('…'.len_utf8());
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &text[..end])
}

/// The fields of an event or span, in the order they were recorded, with the
/// event's `message` kept apart.
#[derive(Default)]
struct Fields {
    message: Option<String>,
    values: Vec<(&'static str, Value)>,
    dropped: usize,
}

impl Fields {
    fn insert(&mut self, name: &'static str, value: Value) {
        if let Some(slot) = self.values.iter_mut().find(|(key, _)| *key == name) {
            slot.1 = value;
        } else if self.values.len() < MAX_FIELDS {
            self.values.push((name, value));
        } else {
            self.dropped += 1;
        }
    }

    fn to_json(&self) -> Value {
        let mut object: serde_json::Map<String, Value> = self
            .values
            .iter()
            .map(|(key, value)| ((*key).to_owned(), value.clone()))
            .collect();
        if self.dropped > 0 {
            object.insert(String::from("fieldsDropped"), Value::from(self.dropped));
        }
        Value::Object(object)
    }
}

impl Visit for Fields {
    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        let text = clip(&format!("{value:?}"));
        if field.name() == "message" {
            self.message = Some(text);
        } else {
            self.insert(field.name(), Value::from(text));
        }
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "message" {
            self.message = Some(clip(value));
        } else {
            self.insert(field.name(), Value::from(clip(value)));
        }
    }

    fn record_error(&mut self, field: &Field, value: &(dyn std::error::Error + 'static)) {
        self.insert(field.name(), Value::from(clip(&value.to_string())));
    }

    fn record_bool(&mut self, field: &Field, value: bool) {
        self.insert(field.name(), Value::from(value));
    }

    fn record_i64(&mut self, field: &Field, value: i64) {
        self.insert(field.name(), Value::from(value));
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        self.insert(field.name(), Value::from(value));
    }

    fn record_f64(&mut self, field: &Field, value: f64) {
        // JSON has no NaN or infinity; those are written as text.
        let json =
            serde_json::Number::from_f64(value).map_or_else(|| Value::from(value.to_string()), Value::Number);
        self.insert(field.name(), json);
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    /// A writer whose bytes the test can read.
    #[derive(Clone, Default)]
    struct Buffer(Arc<Mutex<Vec<u8>>>);

    impl Write for Buffer {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    impl Buffer {
        fn lines(&self) -> Vec<Value> {
            String::from_utf8(self.0.lock().unwrap().clone())
                .unwrap()
                .lines()
                .map(|line| serde_json::from_str(line).unwrap())
                .collect()
        }

        fn text(&self) -> String {
            String::from_utf8(self.0.lock().unwrap().clone()).unwrap()
        }
    }

    fn logged(level: Level, emit: impl FnOnce()) -> Buffer {
        let buffer = Buffer::default();
        tracing::subscriber::with_default(subscriber(buffer.clone(), LevelHandle::new(level)), emit);
        buffer
    }

    #[test]
    fn levels_come_from_the_environment_value() {
        assert_eq!(parse_level(None), Level::INFO);
        assert_eq!(parse_level(Some("debug")), Level::DEBUG);
        assert_eq!(parse_level(Some(" DEBUG ")), Level::DEBUG);
        assert_eq!(parse_level(Some("trace")), Level::TRACE);
        assert_eq!(parse_level(Some("warn")), Level::WARN);
        assert_eq!(parse_level(Some("error")), Level::ERROR);
        assert_eq!(parse_level(Some("info")), Level::INFO);
        assert_eq!(parse_level(Some("verbose")), Level::INFO);
        assert_eq!(parse_level(Some("")), Level::INFO);
        let handle = LevelHandle::new(Level::WARN);
        for level in [Level::ERROR, Level::WARN, Level::INFO, Level::DEBUG, Level::TRACE] {
            handle.set(level);
            assert_eq!(handle.get(), level);
        }
    }

    #[test]
    fn lines_are_json_with_a_fixed_key_order() {
        let buffer = logged(Level::INFO, || {
            let span = tracing::info_span!("build", build_id = "bd_1", config = "debug");
            let _entered = span.enter();
            tracing::info!(elapsed_ms = 12_u64, ok = true, ratio = 0.5, "build finished");
        });
        let text = buffer.text();
        let line = text.lines().next().unwrap();
        let order: Vec<usize> = [
            "\"timestamp\"",
            "\"level\"",
            "\"target\"",
            "\"spans\"",
            "\"fields\"",
            "\"message\"",
        ]
        .iter()
        .map(|key| line.find(key).unwrap())
        .collect();
        assert!(order.windows(2).all(|pair| pair[0] < pair[1]), "{line}");
        let [value] = buffer.lines().try_into().unwrap();
        assert_eq!(value["level"], "INFO");
        assert_eq!(value["target"], "blocks2cpp_desktop::logging::tests");
        assert_eq!(value["message"], "build finished");
        assert_eq!(value["fields"]["elapsed_ms"], 12);
        assert_eq!(value["fields"]["ok"], true);
        assert_eq!(value["fields"]["ratio"], 0.5);
        assert_eq!(value["spans"][0]["name"], "build");
        assert_eq!(value["spans"][0]["fields"]["build_id"], "bd_1");
        assert_eq!(value["spans"][0]["fields"]["config"], "debug");
        let timestamp = value["timestamp"].as_str().unwrap();
        assert!(b2c_store::parse_rfc3339_utc(timestamp).is_some(), "{timestamp}");
        assert!(timestamp.ends_with('Z'));
    }

    #[test]
    fn span_fields_named_like_the_span_do_not_collide() {
        let buffer = logged(Level::DEBUG, || {
            let span = tracing::debug_span!("command", name = "project_save");
            let _entered = span.enter();
            span.record("name", "project_save");
            tracing::debug!("saved");
        });
        let [value] = buffer.lines().try_into().unwrap();
        assert_eq!(value["spans"][0]["name"], "command");
        assert_eq!(value["spans"][0]["fields"]["name"], "project_save");
    }

    #[test]
    fn the_level_filters_and_can_change() {
        let level = LevelHandle::new(Level::INFO);
        let buffer = Buffer::default();
        tracing::subscriber::with_default(subscriber(buffer.clone(), level.clone()), || {
            tracing::debug!(path = "/home/ada/secret", "hidden at info");
            tracing::info!("shown");
            level.set(Level::DEBUG);
            tracing::debug!(path = "/home/ada/visible", "shown at debug");
            level.set(Level::WARN);
            tracing::info!("hidden at warn");
            tracing::warn!("a warning");
        });
        let messages: Vec<Value> = buffer
            .lines()
            .iter()
            .map(|line| line["message"].clone())
            .collect();
        assert_eq!(messages, ["shown", "shown at debug", "a warning"]);
        assert!(!buffer.text().contains("/home/ada/secret"));
        assert!(buffer.text().contains("/home/ada/visible"));
    }

    #[test]
    fn only_blocks2cpp_targets_are_written() {
        let buffer = logged(Level::TRACE, || {
            tracing::info!(target: "wry::webview", "third-party text");
            tracing::info!(target: "b2c_build::session", "ours");
            tracing::info!(target: "blocks2cpp_desktop::panic", "ours too");
        });
        let messages: Vec<Value> = buffer
            .lines()
            .iter()
            .map(|line| line["message"].clone())
            .collect();
        assert_eq!(messages, ["ours", "ours too"]);
    }

    #[test]
    fn values_fields_and_lines_are_bounded() {
        let huge = "é".repeat(MAX_VALUE_BYTES);
        let buffer = logged(Level::INFO, || {
            tracing::info!(text = huge.as_str(), "{huge}");
        });
        let [value] = buffer.lines().try_into().unwrap();
        let text = value["fields"]["text"].as_str().unwrap();
        assert!(text.len() <= MAX_VALUE_BYTES && text.ends_with('…'));
        assert!(value["message"].as_str().unwrap().len() <= MAX_VALUE_BYTES);

        let mut fields = Fields::default();
        for index in 0..MAX_FIELDS + 5 {
            let name: &'static str = Box::leak(format!("f{index}").into_boxed_str());
            fields.insert(name, Value::from(index));
        }
        let json = fields.to_json();
        assert_eq!(json.as_object().unwrap().len(), MAX_FIELDS + 1);
        assert_eq!(json["fieldsDropped"], 5);

        // Many long span fields overflow the line limit: the line is cut down.
        let spans: Vec<(&str, Value)> = (0..MAX_SPANS)
            .map(|_| {
                (
                    "span",
                    serde_json::json!({ "a": "x".repeat(MAX_VALUE_BYTES), "b": "y".repeat(MAX_VALUE_BYTES) }),
                )
            })
            .collect();
        let message = Fields {
            message: Some(String::from("long")),
            ..Fields::default()
        };
        let line = json_line("2026-01-01T00:00:00.000Z", Level::INFO, "b2c_x", &spans, &message);
        assert!(line.len() <= MAX_LINE_BYTES);
        let value: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(value["truncated"], true);
        assert_eq!(value["message"], "long");
    }

    #[test]
    fn cutting_keeps_characters_whole() {
        assert_eq!(cut("abc", 3), "abc");
        assert_eq!(cut("abcdef", 4), "a…");
        assert_eq!(cut("ééé", 5), "é…");
        assert_eq!(cut("", 0), "");
    }

    #[test]
    fn more_than_25_mib_through_the_logger_leaves_at_most_five_files_of_5_mib() {
        let dir = tempfile::tempdir().unwrap();
        let file = RotatingFile::open(dir.path(), LOG_BASE, LOG_MAX_BYTES, LOG_FILES).unwrap();
        let paths = file.paths();
        let padding = "p".repeat(1000);
        let events = 27_000_u64;
        tracing::subscriber::with_default(subscriber(file, LevelHandle::new(Level::INFO)), || {
            for index in 0..events {
                tracing::info!(index, padding = padding.as_str(), "line");
            }
        });
        let mut total = 0;
        let mut count = 0;
        for entry in fs::read_dir(dir.path()).unwrap() {
            let entry = entry.unwrap();
            let size = entry.metadata().unwrap().len();
            assert!(size <= LOG_MAX_BYTES, "{:?} has {size} bytes", entry.file_name());
            total += size;
            count += 1;
        }
        assert!(count <= LOG_FILES, "{count} files");
        assert!(total > 20 * 1024 * 1024, "only {total} bytes kept");
        // The newest line is in the current file; the oldest were dropped.
        let newest = fs::read_to_string(&paths[0]).unwrap();
        let last: Value = serde_json::from_str(newest.lines().last().unwrap()).unwrap();
        assert_eq!(last["fields"]["index"], events - 1);
        let oldest = fs::read_to_string(&paths[LOG_FILES - 1]).unwrap();
        let first: Value = serde_json::from_str(oldest.lines().next().unwrap()).unwrap();
        assert!(first["fields"]["index"].as_u64().unwrap() > 0);
        // Every line in every file is whole JSON.
        for path in &paths {
            for line in fs::read_to_string(path).unwrap().lines() {
                serde_json::from_str::<Value>(line).unwrap();
            }
        }
    }

    #[cfg(unix)]
    #[test]
    fn log_files_are_private() {
        use std::os::unix::fs::PermissionsExt as _;

        let dir = tempfile::tempdir().unwrap();
        let logs = dir.path().join("state").join("logs");
        let file = RotatingFile::open(&logs, LOG_BASE, LOG_MAX_BYTES, LOG_FILES).unwrap();
        let paths = file.paths();
        tracing::subscriber::with_default(subscriber(file, LevelHandle::new(Level::INFO)), || {
            tracing::info!("hello");
        });
        let mode = |path: &Path| fs::metadata(path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&logs), 0o700);
        assert_eq!(mode(&paths[0]), 0o600);
    }
}
