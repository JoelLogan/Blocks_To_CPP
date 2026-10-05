//! Decoding the parsed JSON tree into a [`Document`] while checking every rule
//! of spec §5.6 in the same pass.
//!
//! The decoder mirrors the `serde` attributes of [`crate::document`] exactly
//! (field names, defaults, which keys may be `null`), so any file it accepts
//! means the same as it would to `serde_json::from_str::<Document>`. Unlike
//! `serde`, it does not stop at the first problem: it reports every one it
//! finds, pointing at the block when there is one, and recovers so that later
//! parts of the file are still checked.

mod clipboard;
mod project;
mod workspace;

use std::collections::BTreeMap;
use std::fmt::Write as _;

use b2c_ir::{BlockId, Diagnostic, IdError, Location, ModuleId, Part, SymbolId};
use serde::Serialize;

use crate::codes::{self, Diags};
use crate::document::Document;
use crate::json::Json;
use crate::limits::MAX_STRING_BYTES;
use crate::text_rules::{self, quote};

/// Maximum number of JSON values stored as free-form data in all `extra`
/// maps and `x-ext` together. Free-form objects are stored as B-tree maps,
/// which cost far more memory than their text, so they get their own budget.
/// Real projects store one or two values per block here.
pub(crate) const MAX_UNTYPED_VALUES: usize = 500_000;

/// Keys that are rejected anywhere outside `x-ext` (spec §5.6): JavaScript
/// code that merges parsed objects could be tricked by them.
const RESERVED_KEYS: [&str; 3] = ["__proto__", "constructor", "prototype"];

/// A value that was present but invalid (already reported).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Invalid;

/// What is being decoded. Both kinds of input follow the same rules; this
/// only changes the words of the messages that talk about the input as a
/// whole.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Origin {
    /// A `.b2c` project file (spec §5.3).
    Project,
    /// A clipboard payload (spec §5.12).
    Clipboard,
}

impl Origin {
    /// The subject of a message about the input as a whole.
    pub(crate) fn whole(self) -> &'static str {
        match self {
            Self::Project => "The project file",
            Self::Clipboard => "The pasted data",
        }
    }
}

/// Where a block sits, which decides the canvas-only keys it may have.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Placement {
    /// Directly on a canvas, or directly in a clipboard payload: may have
    /// `x`, `y` and a `stack`.
    Canvas,
    /// In another block's value input or statement list.
    Nested,
    /// An element of a top-level block's `stack` (ADR-0011).
    Stacked,
}

/// One step of the path to the value being decoded.
#[derive(Debug, Clone, Copy)]
enum Seg<'a> {
    Key(&'a str),
    Index(usize),
}

/// Object entries in file order.
type Entries<'a> = &'a [(Box<str>, Json)];

/// The decoder state: diagnostics, the current position, and the tables used
/// for project-wide uniqueness checks.
pub(crate) struct Decoder<'a> {
    diags: Diags,
    origin: Origin,
    path: Vec<Seg<'a>>,
    module: Option<ModuleId>,
    block: Option<BlockId>,
    /// Index in `path` where the current block starts.
    block_base: usize,
    blocks_seen: usize,
    untyped_values: usize,
    untyped_limit_reported: bool,
    /// Block, frame and note IDs, with where each was first used.
    canvas_ids: BTreeMap<BlockId, Location>,
    /// Declared symbol IDs, with where each was first declared.
    symbols: BTreeMap<SymbolId, Location>,
    /// Module IDs, with where each was first used.
    module_ids: BTreeMap<ModuleId, Location>,
    /// Lower-cased module names, with where each was first used.
    module_names: BTreeMap<String, Location>,
}

impl<'a> Decoder<'a> {
    /// A decoder for `origin` that adds to `diags`.
    pub(crate) fn new(diags: Diags, origin: Origin) -> Self {
        Self {
            diags,
            origin,
            path: Vec::new(),
            module: None,
            block: None,
            block_base: 0,
            blocks_seen: 0,
            untyped_values: 0,
            untyped_limit_reported: false,
            canvas_ids: BTreeMap::new(),
            symbols: BTreeMap::new(),
            module_ids: BTreeMap::new(),
            module_names: BTreeMap::new(),
        }
    }

    /// Decodes a whole document; returns it with the diagnostics.
    pub(crate) fn run(mut self, root: &'a Json) -> (Option<Document>, Diags) {
        let document = self.document(root);
        (document, self.diags)
    }

    // -----------------------------------------------------------------------
    // Position and messages
    // -----------------------------------------------------------------------

    /// Runs `f` with `seg` pushed onto the path.
    fn at<T>(&mut self, seg: Seg<'a>, f: impl FnOnce(&mut Self) -> T) -> T {
        self.path.push(seg);
        let result = f(self);
        self.path.pop();
        result
    }

    /// The current module (for diagnostics outside blocks).
    fn module_location(&self) -> Location {
        Location {
            module: self.module.clone(),
            block: None,
            part: Part::Whole,
        }
    }

    /// Where the value being decoded is.
    fn location(&self) -> Location {
        Location {
            module: self.module.clone(),
            block: self.block.clone(),
            part: if self.block.is_some() {
                self.part()
            } else {
                Part::Whole
            },
        }
    }

    /// The part of the current block the path points at.
    fn part(&self) -> Part {
        match self.path.get(self.block_base..).unwrap_or_default() {
            [Seg::Key("fields"), Seg::Key(name), ..] => Part::Field {
                name: (*name).to_owned(),
            },
            [
                Seg::Key("inputs"),
                Seg::Key(name),
                Seg::Key("expr"),
                Seg::Index(index),
                ..,
            ] => {
                let start = u32::try_from(*index).unwrap_or(u32::MAX);
                Part::Tokens {
                    input: (*name).to_owned(),
                    start,
                    end: start.saturating_add(1),
                }
            }
            [Seg::Key("inputs"), Seg::Key(name), ..] => Part::Input {
                name: (*name).to_owned(),
            },
            _ => Part::Whole,
        }
    }

    /// Renders path segments as `a.b[2].c`, quoting unusual keys.
    fn render(segments: &[Seg<'_>]) -> String {
        let mut out = String::new();
        for seg in segments {
            match seg {
                Seg::Key(key) => {
                    let plain = !key.is_empty()
                        && key.len() <= 64
                        && key
                            .bytes()
                            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-');
                    if plain {
                        if !out.is_empty() {
                            out.push('.');
                        }
                        out.push_str(key);
                    } else {
                        out.push('[');
                        out.push_str(&quote(key));
                        out.push(']');
                    }
                }
                Seg::Index(index) => {
                    let _ = write!(out, "[{index}]");
                }
            }
        }
        out
    }

    /// The start of a message about the current value: `This block`,
    /// `"fields.NAME" in this block`, `"project.name"` or `The project file`.
    fn subject(&self) -> String {
        if self.block.is_some() {
            let relative = Self::render(self.path.get(self.block_base..).unwrap_or_default());
            if relative.is_empty() {
                String::from("This block")
            } else {
                format!("\"{relative}\" in this block")
            }
        } else {
            let path = Self::render(&self.path);
            if path.is_empty() {
                String::from(self.origin.whole())
            } else {
                format!("\"{path}\"")
            }
        }
    }

    /// Reports an error at the current position.
    fn report(&mut self, code: &str, message: String) {
        let location = self.location();
        self.diags.error(code, location, message);
    }

    /// Reports a value of the wrong kind.
    fn wrong(&mut self, value: &Json, expected: &str) {
        let message = format!(
            "{} should be {expected}, but it is {}.",
            self.subject(),
            describe(value)
        );
        self.report(codes::WRONG_VALUE, message);
    }

    // -----------------------------------------------------------------------
    // Objects and keys
    // -----------------------------------------------------------------------

    /// Expects an object whose keys are all in `known`; reports the others.
    fn object(&mut self, value: &'a Json, known: &[&str]) -> Option<Entries<'a>> {
        let Json::Object(entries) = value else {
            self.wrong(value, "an object");
            return None;
        };
        for (key, _) in entries {
            if known.contains(&&**key) {
                continue;
            }
            if RESERVED_KEYS.contains(&&**key) {
                self.reserved_key(key);
            } else {
                let message = format!(
                    "{} has an unknown key {}. Remove it or check its spelling.",
                    self.subject(),
                    quote(key)
                );
                self.report(codes::UNKNOWN_KEY, message);
            }
        }
        Some(entries)
    }

    /// Reports the reserved keys of an object that is rejected as a whole
    /// (such as a token with two keys), so they are named like everywhere
    /// else.
    fn reserved_keys_in(&mut self, entries: Entries<'a>) {
        for (key, _) in entries {
            if RESERVED_KEYS.contains(&&**key) {
                self.reserved_key(key);
            }
        }
    }

    fn reserved_key(&mut self, key: &str) {
        let place = match self.origin {
            Origin::Project => "project files",
            Origin::Clipboard => "pasted blocks",
        };
        let message = format!(
            "{} uses the key {}, which is not allowed in {place} because it could be used to tamper with the editor.",
            self.subject(),
            quote(key)
        );
        self.report(codes::RESERVED_KEY, message);
    }

    /// Checks a user-chosen map key (field, input, statement or `extra`
    /// name). Returns `false` when the entry must be skipped.
    fn map_key(&mut self, key: &str, reserved_allowed: bool) -> bool {
        if !reserved_allowed && RESERVED_KEYS.contains(&key) {
            self.reserved_key(key);
            return false;
        }
        self.text_ok(key, true)
    }

    /// Decodes a required key.
    fn required<T>(
        &mut self,
        entries: Entries<'a>,
        key: &'static str,
        f: impl FnOnce(&mut Self, &'a Json) -> Option<T>,
    ) -> Option<T> {
        if let Some(value) = find(entries, key) {
            self.at(Seg::Key(key), |d| f(d, value))
        } else {
            let message = format!("{} is missing {}.", self.subject(), quote(key));
            self.report(codes::MISSING_KEY, message);
            None
        }
    }

    /// Decodes a key that may be absent (giving `default`).
    fn or_default<T>(
        &mut self,
        entries: Entries<'a>,
        key: &'static str,
        default: impl FnOnce() -> T,
        f: impl FnOnce(&mut Self, &'a Json) -> Option<T>,
    ) -> Option<T> {
        match find(entries, key) {
            Some(value) => self.at(Seg::Key(key), |d| f(d, value)),
            None => Some(default()),
        }
    }

    /// Decodes an optional key, where `null` also means absent (as for
    /// `Option` fields in `serde`).
    fn nullable<T>(
        &mut self,
        entries: Entries<'a>,
        key: &'static str,
        f: impl FnOnce(&mut Self, &'a Json) -> Option<T>,
    ) -> Result<Option<T>, Invalid> {
        match find(entries, key) {
            None | Some(Json::Null) => Ok(None),
            Some(value) => self.at(Seg::Key(key), |d| f(d, value)).map(Some).ok_or(Invalid),
        }
    }

    /// Decodes every item of a list, skipping the invalid ones.
    fn list<T>(
        &mut self,
        value: &'a Json,
        mut f: impl FnMut(&mut Self, &'a Json) -> Option<T>,
    ) -> Option<Vec<T>> {
        let Json::Array(items) = value else {
            self.wrong(value, "a list");
            return None;
        };
        let mut out = Vec::with_capacity(items.len());
        for (index, item) in items.iter().enumerate() {
            if let Some(decoded) = self.at(Seg::Index(index), |d| f(d, item)) {
                out.push(decoded);
            }
        }
        Some(out)
    }

    // -----------------------------------------------------------------------
    // Scalars
    // -----------------------------------------------------------------------

    /// Checks text against the length limit and the text rules, reporting
    /// every rule it breaks. `is_key` says the text is an object key.
    fn text_ok(&mut self, text: &str, is_key: bool) -> bool {
        let lead = if is_key {
            format!("{} has a key that", self.subject())
        } else {
            self.subject()
        };
        let mut ok = true;
        if text.len() > MAX_STRING_BYTES {
            let message = format!(
                "{lead} is {} bytes long, but text can be at most {} bytes (64 KiB).",
                text.len(),
                MAX_STRING_BYTES
            );
            self.report(codes::TOO_LONG, message);
            ok = false;
        }
        let problems = text_rules::check(text);
        if problems.nul {
            let message =
                format!("{lead} contains the NUL character (U+0000), which is not allowed in project text.");
            self.report(codes::NUL_CHAR, message);
            ok = false;
        }
        if let Some(c) = problems.control {
            let message = format!(
                "{lead} contains {} (U+{:04X}), which is not allowed in project text. Only tabs and new lines are allowed.",
                text_rules::control_name(c),
                u32::from(c)
            );
            self.report(codes::CONTROL_CHAR, message);
            ok = false;
        }
        if let Some(c) = problems.bidi {
            let message = format!(
                "{lead} contains an invisible {} character (U+{:04X}). Such characters can make text look different from what it really is, so they are not allowed.",
                text_rules::bidi_name(c),
                u32::from(c)
            );
            self.report(codes::BIDI_CHAR, message);
            ok = false;
        }
        ok
    }

    /// Decodes text that follows the text rules.
    fn string(&mut self, value: &'a Json) -> Option<String> {
        let Json::String(text) = value else {
            self.wrong(value, "text");
            return None;
        };
        self.text_ok(text, false).then(|| text.to_string())
    }

    fn bool(&mut self, value: &Json) -> Option<bool> {
        if let Json::Bool(b) = value {
            Some(*b)
        } else {
            self.wrong(value, "true or false");
            None
        }
    }

    fn u32(&mut self, value: &Json) -> Option<u32> {
        let parsed = match value {
            Json::Number(n) => n.as_u64().and_then(|n| u32::try_from(n).ok()),
            _ => None,
        };
        if parsed.is_none() {
            self.wrong(value, "a whole number from 0 to 4294967295");
        }
        parsed
    }

    fn i64(&mut self, value: &Json) -> Option<i64> {
        let parsed = match value {
            Json::Number(n) => n.as_i64(),
            _ => None,
        };
        if parsed.is_none() {
            self.wrong(
                value,
                "a whole number from -9223372036854775808 to 9223372036854775807",
            );
        }
        parsed
    }

    fn f64(&mut self, value: &Json) -> Option<f64> {
        if let Json::Number(n) = value
            && let Some(x) = n.as_f64()
        {
            return Some(x);
        }
        self.wrong(value, "a number");
        None
    }

    /// One of the unit variants in `all`, by its `serde` name.
    fn choice<T: Copy + Serialize>(&mut self, value: &Json, all: &[T]) -> Option<T> {
        let names: Vec<String> = all.iter().filter_map(serde_name).collect();
        if let Json::String(text) = value
            && let Some(index) = names.iter().position(|name| name == &**text)
        {
            return all.get(index).copied();
        }
        let expected = format!("one of {}", list_names(&names));
        self.wrong(value, &expected);
        None
    }

    /// A validated ID.
    fn id<T>(&mut self, value: &Json, make: fn(&str) -> Result<T, IdError>) -> Option<T> {
        let Json::String(text) = value else {
            self.wrong(value, "an ID (text)");
            return None;
        };
        if let Ok(id) = make(text) {
            Some(id)
        } else {
            let message = format!(
                "{} is not a valid ID: {} is not 1 to 32 characters from A–Z, a–z, 0–9 and _.",
                self.subject(),
                quote(text)
            );
            self.report(codes::BAD_ID, message);
            None
        }
    }

    // -----------------------------------------------------------------------
    // Free-form data (`extra` and `x-ext`)
    // -----------------------------------------------------------------------

    /// Converts free-form JSON, checking text rules on every string and key,
    /// reserved keys (unless `reserved_allowed`) and the free-form budget.
    fn untyped(&mut self, value: &'a Json, reserved_allowed: bool) -> Option<serde_json::Value> {
        self.untyped_values += 1;
        if self.untyped_values > MAX_UNTYPED_VALUES {
            if !self.untyped_limit_reported {
                self.untyped_limit_reported = true;
                let message = match self.origin {
                    Origin::Project => format!(
                        "This project stores more than {MAX_UNTYPED_VALUES} values of free-form data in \"extra\" and \"x-ext\", which is far more than any real project needs."
                    ),
                    Origin::Clipboard => format!(
                        "The pasted data stores more than {MAX_UNTYPED_VALUES} values of free-form data in \"extra\", which is far more than any real blocks need."
                    ),
                };
                self.diags
                    .error(codes::TOO_MUCH_EXTRA_DATA, Location::project(), message);
            }
            return None;
        }
        Some(match value {
            Json::String(text) => {
                serde_json::Value::String(self.string(value).unwrap_or_else(|| text.to_string()))
            }
            Json::Array(items) => {
                let mut out = Vec::with_capacity(items.len());
                for (index, item) in items.iter().enumerate() {
                    out.push(self.at(Seg::Index(index), |d| d.untyped(item, reserved_allowed))?);
                }
                serde_json::Value::Array(out)
            }
            Json::Object(entries) => {
                let mut out = serde_json::Map::new();
                for (key, item) in entries {
                    let key_ok = self.map_key(key, reserved_allowed);
                    let item = self.at(Seg::Key(key), |d| d.untyped(item, reserved_allowed))?;
                    if key_ok {
                        out.insert(key.to_string(), item);
                    }
                }
                serde_json::Value::Object(out)
            }
            other => other.to_value(),
        })
    }

    // -----------------------------------------------------------------------
    // Project-wide uniqueness
    // -----------------------------------------------------------------------

    /// Records a block, frame or note ID; reports it if already used.
    fn claim_canvas_id(&mut self, id: &BlockId, location: Location) {
        if let Some(first) = self.canvas_ids.get(id) {
            let diagnostic = Diagnostic::error(
                codes::DUPLICATE_BLOCK_ID,
                b2c_ir::DiagSource::Loader,
                location,
                format!(
                    "Another block, frame or note already uses the ID {}. Every block, frame and note needs its own ID.",
                    quote(id.as_str())
                ),
            )
            .with_related(first.clone(), "the ID is first used here");
            self.diags.push(diagnostic);
        } else {
            self.canvas_ids.insert(id.clone(), location);
        }
    }

    /// Records a symbol declaration; reports it if already declared.
    fn declare_symbol(&mut self, sym: &SymbolId) {
        let location = self.location();
        if let Some(first) = self.symbols.get(sym) {
            let diagnostic = Diagnostic::error(
                codes::DUPLICATE_SYMBOL,
                b2c_ir::DiagSource::Loader,
                location,
                format!(
                    "The symbol ID {} is declared more than once. Every variable, parameter and function needs its own symbol ID.",
                    quote(sym.as_str())
                ),
            )
            .with_related(first.clone(), "the symbol is first declared here");
            self.diags.push(diagnostic);
        } else {
            self.symbols.insert(sym.clone(), location);
        }
    }
}

/// The value of `key` among `entries`.
fn find<'a>(entries: Entries<'a>, key: &str) -> Option<&'a Json> {
    entries.iter().find(|(k, _)| &**k == key).map(|(_, v)| v)
}

/// Describes a value for a message, quoting text safely.
fn describe(value: &Json) -> String {
    match value {
        Json::Null => String::from("null"),
        Json::Bool(b) => b.to_string(),
        Json::Number(n) => format!("the number {n}"),
        Json::String(text) => format!("the text {}", quote(text)),
        other => String::from(other.kind()),
    }
}

/// The `serde` name of a unit enum variant.
fn serde_name<T: Serialize>(value: &T) -> Option<String> {
    match serde_json::to_value(value) {
        Ok(serde_json::Value::String(name)) => Some(name),
        _ => None,
    }
}

/// `"a", "b" or "c"`.
fn list_names(names: &[String]) -> String {
    let quoted: Vec<String> = names.iter().map(|n| format!("\"{n}\"")).collect();
    match quoted.split_last() {
        None => String::new(),
        Some((last, [])) => last.clone(),
        Some((last, rest)) => format!("{} or {last}", rest.join(", ")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_render_safely() {
        let path = [
            Seg::Key("modules"),
            Seg::Index(2),
            Seg::Key("workspace"),
            Seg::Key("a b"),
            Seg::Key("x\u{202e}"),
        ];
        assert_eq!(
            Decoder::render(&path),
            "modules[2].workspace[\"a b\"][\"x\\u{202E}\"]"
        );
        assert_eq!(Decoder::render(&[]), "");
    }

    #[test]
    fn names_are_listed() {
        let names = |list: &[&str]| list.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        assert_eq!(list_names(&names(&[])), "");
        assert_eq!(list_names(&names(&["a"])), "\"a\"");
        assert_eq!(list_names(&names(&["a", "b", "c"])), "\"a\", \"b\" or \"c\"");
    }

    #[test]
    fn values_are_described_safely() {
        assert_eq!(describe(&Json::Null), "null");
        assert_eq!(describe(&Json::Bool(true)), "true");
        assert_eq!(describe(&Json::Number(5u64.into())), "the number 5");
        assert_eq!(
            describe(&Json::String("\u{1b}x".into())),
            "the text \"\\u{001B}x\""
        );
        assert_eq!(describe(&Json::Array(Box::default())), "a list");
        assert_eq!(describe(&Json::Object(Box::default())), "an object");
    }
}
