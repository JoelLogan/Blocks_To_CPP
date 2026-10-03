//! Decoding modules, canvases and blocks (spec §5.3–5.5).

use std::collections::BTreeMap;

use b2c_ir::{BlockId, Location, ModuleId, SymbolId};

use super::{Decoder, Seg, find};
use crate::codes;
use crate::document::{
    Block, BlockComment, BlockInput, ExprInput, FieldValue, Frame, FrameColor, Input, Module, Note,
    SymbolDecl, SymbolRef, Token, Viewport, Workspace,
};
use crate::json::Json;
use crate::limits::{MAX_BLOCKS, MAX_COORDINATE, MAX_EXPR_TOKENS, MAX_IDENT_LEN, MAX_VARIADIC_PARTS};
use crate::text_rules::quote;

/// Every frame colour.
pub(super) const FRAME_COLORS: [FrameColor; 6] = [
    FrameColor::Grey,
    FrameColor::Blue,
    FrameColor::Green,
    FrameColor::Yellow,
    FrameColor::Orange,
    FrameColor::Purple,
];

/// Keys of a block object, in declaration order.
const BLOCK_KEYS: [&str; 12] = [
    "id",
    "type",
    "v",
    "x",
    "y",
    "collapsed",
    "disabled",
    "comment",
    "extra",
    "fields",
    "inputs",
    "statements",
];

/// Token kinds, as their single key.
const TOKEN_KINDS: [&str; 7] = ["num", "str", "chr", "ref", "op", "kw", "text"];

/// Smallest and largest viewport zoom.
const MIN_ZOOM: f64 = 0.1;
const MAX_ZOOM: f64 = 4.0;

/// Windows device names (spec §8.4.1): a file with such a stem refers to a
/// device instead of a file, in any folder, with any extension.
const DEVICE_NAMES: [&str; 30] = [
    "con",
    "prn",
    "aux",
    "nul",
    "com0",
    "com1",
    "com2",
    "com3",
    "com4",
    "com5",
    "com6",
    "com7",
    "com8",
    "com9",
    "lpt0",
    "lpt1",
    "lpt2",
    "lpt3",
    "lpt4",
    "lpt5",
    "lpt6",
    "lpt7",
    "lpt8",
    "lpt9",
    "com\u{b9}",
    "com\u{b2}",
    "com\u{b3}",
    "lpt\u{b9}",
    "lpt\u{b2}",
    "lpt\u{b3}",
];

/// A number for a message: plain when it is of a familiar size, otherwise in
/// scientific notation (`1e308` instead of 309 digits).
fn short_number(value: f64) -> String {
    let magnitude = value.abs();
    if magnitude == 0.0 || (1e-3..1e6).contains(&magnitude) {
        format!("{value}")
    } else {
        format!("{value:e}")
    }
}

/// Whether a module name is valid as a file stem: `[a-z][a-z0-9_-]{0,63}`.
pub(crate) fn is_module_name(name: &str) -> bool {
    let mut bytes = name.bytes();
    bytes.next().is_some_and(|b| b.is_ascii_lowercase())
        && name.len() <= 64
        && bytes.all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
}

/// Whether a module name is a Windows device name (ignoring case, an
/// extension and trailing spaces or dots, as Windows does).
pub(crate) fn is_device_name(name: &str) -> bool {
    let stem = name.split('.').next().unwrap_or_default();
    let stem = stem.trim_end_matches([' ', '.']).to_lowercase();
    DEVICE_NAMES.contains(&stem.as_str())
}

impl<'a> Decoder<'a> {
    pub(super) fn module(&mut self, value: &'a Json) -> Option<Module> {
        let entries = self.object(value, &["id", "name", "workspace"])?;
        let id = self.required(entries, "id", |d, v| d.id(v, ModuleId::new));
        let saved = std::mem::replace(&mut self.module, id.clone());
        if let Some(id) = &id {
            let location = self.module_location();
            if let Some(first) = self.module_ids.get(id) {
                let message = format!(
                    "Another module already uses the ID {}. Every module needs its own ID.",
                    quote(id.as_str())
                );
                let diagnostic = b2c_ir::Diagnostic::error(
                    codes::DUPLICATE_MODULE_ID,
                    b2c_ir::DiagSource::Loader,
                    location,
                    message,
                )
                .with_related(first.clone(), "the ID is first used here");
                self.diags.push(diagnostic);
            } else {
                self.module_ids.insert(id.clone(), location);
            }
        }
        let name = self.required(entries, "name", |d, v| {
            let name = d.string(v)?;
            d.check_module_name(&name);
            Some(name)
        });
        let workspace = self.required(entries, "workspace", Self::workspace);
        self.module = saved;
        Some(Module {
            id: id?,
            name: name?,
            workspace: workspace?,
        })
    }

    /// Module names become file names: check the pattern, Windows device
    /// names and case-insensitive clashes.
    fn check_module_name(&mut self, name: &str) {
        // The device check comes first: it is the more specific explanation
        // for names such as "CON" or "com¹", which break the pattern too.
        if is_device_name(name) {
            let message = format!(
                "The module name {} is reserved by Windows for a device, so it cannot be used as a file name. Choose another name.",
                quote(name)
            );
            self.report(codes::DEVICE_MODULE_NAME, message);
        } else if !is_module_name(name) {
            let message = format!(
                "The module name {} cannot be used as a file name. Use 1 to 64 characters: a lower-case letter first, then lower-case letters, digits, _ or -.",
                quote(name)
            );
            self.report(codes::BAD_MODULE_NAME, message);
        }
        let folded = name.to_lowercase();
        let location = self.module_location();
        if let Some(first) = self.module_names.get(&folded) {
            let message = format!(
                "Two modules are named {} (ignoring upper and lower case). Their files would overwrite each other on Windows, so every module needs a different name.",
                quote(name)
            );
            let diagnostic = b2c_ir::Diagnostic::error(
                codes::MODULE_NAME_CLASH,
                b2c_ir::DiagSource::Loader,
                location,
                message,
            )
            .with_related(first.clone(), "the other module with this name");
            self.diags.push(diagnostic);
        } else {
            self.module_names.insert(folded, location);
        }
    }

    fn workspace(&mut self, value: &'a Json) -> Option<Workspace> {
        let entries = self.object(value, &["blocks", "frames", "notes", "viewport"])?;
        let blocks = self.or_default(entries, "blocks", Vec::new, |d, v| {
            d.list(v, |d, item| d.block(item, true))
        });
        let frames = self.or_default(entries, "frames", Vec::new, |d, v| d.list(v, Self::frame));
        let notes = self.or_default(entries, "notes", Vec::new, |d, v| d.list(v, Self::note));
        let viewport = self.nullable(entries, "viewport", Self::viewport);
        Some(Workspace {
            blocks: blocks?,
            frames: frames?,
            notes: notes?,
            viewport: viewport.ok()?,
        })
    }

    /// A canvas coordinate (`min` is `-MAX_COORDINATE`) or size (`min` is 0).
    fn coordinate(&mut self, value: &Json, min: i64) -> Option<i32> {
        let Json::Number(number) = value else {
            self.wrong(value, "a whole number");
            return None;
        };
        let in_range = number
            .as_i64()
            .filter(|n| (min..=i64::from(MAX_COORDINATE)).contains(n))
            .and_then(|n| i32::try_from(n).ok());
        if in_range.is_none() {
            if number.is_f64() {
                self.wrong(value, "a whole number");
            } else {
                let message = format!(
                    "{} is {number}, but it must be between {min} and {MAX_COORDINATE}.",
                    self.subject()
                );
                self.report(codes::BAD_COORDINATE, message);
            }
        }
        in_range
    }

    fn position(&mut self, value: &Json) -> Option<i32> {
        self.coordinate(value, -i64::from(MAX_COORDINATE))
    }

    fn size(&mut self, value: &Json) -> Option<i32> {
        self.coordinate(value, 0)
    }

    fn frame(&mut self, value: &'a Json) -> Option<Frame> {
        let entries = self.object(value, &["id", "title", "x", "y", "w", "h", "color", "emitBanner"])?;
        let id = self.required(entries, "id", |d, v| d.id(v, BlockId::new));
        if let Some(id) = &id {
            let location = Location::block(self.module.clone(), id.clone());
            self.claim_canvas_id(id, location);
        }
        let title = self.required(entries, "title", Self::string);
        let x = self.required(entries, "x", Self::position);
        let y = self.required(entries, "y", Self::position);
        let w = self.required(entries, "w", Self::size);
        let h = self.required(entries, "h", Self::size);
        let color = self.required(entries, "color", |d, v| d.choice(v, &FRAME_COLORS));
        let emit_banner = self.or_default(entries, "emitBanner", || false, Self::bool);
        Some(Frame {
            id: id?,
            title: title?,
            x: x?,
            y: y?,
            w: w?,
            h: h?,
            color: color?,
            emit_banner: emit_banner?,
        })
    }

    fn note(&mut self, value: &'a Json) -> Option<Note> {
        let entries = self.object(value, &["id", "text", "x", "y"])?;
        let id = self.required(entries, "id", |d, v| d.id(v, BlockId::new));
        if let Some(id) = &id {
            let location = Location::block(self.module.clone(), id.clone());
            self.claim_canvas_id(id, location);
        }
        let text = self.required(entries, "text", Self::string);
        let x = self.required(entries, "x", Self::position);
        let y = self.required(entries, "y", Self::position);
        Some(Note {
            id: id?,
            text: text?,
            x: x?,
            y: y?,
        })
    }

    fn viewport(&mut self, value: &'a Json) -> Option<Viewport> {
        let entries = self.object(value, &["x", "y", "scale"])?;
        let x = self.required(entries, "x", Self::position);
        let y = self.required(entries, "y", Self::position);
        let scale = self.required(entries, "scale", |d, v| {
            let scale = d.f64(v)?;
            if (MIN_ZOOM..=MAX_ZOOM).contains(&scale) {
                Some(scale)
            } else {
                let message = format!(
                    "{} is {}, but the zoom must be between {MIN_ZOOM} and {MAX_ZOOM:.1}.",
                    d.subject(),
                    short_number(scale)
                );
                d.report(codes::BAD_ZOOM, message);
                None
            }
        });
        Some(Viewport {
            x: x?,
            y: y?,
            scale: scale?,
        })
    }

    // -----------------------------------------------------------------------
    // Blocks
    // -----------------------------------------------------------------------

    /// Decodes a block. `top_level` is true for blocks directly on a canvas.
    pub(super) fn block(&mut self, value: &'a Json, top_level: bool) -> Option<Block> {
        self.blocks_seen += 1;
        if self.blocks_seen > MAX_BLOCKS {
            if self.blocks_seen == MAX_BLOCKS + 1 {
                let message = format!(
                    "The project has more than {MAX_BLOCKS} blocks, which is the most a project can have. Split it into smaller projects."
                );
                self.diags
                    .error(codes::TOO_MANY_BLOCKS, Location::project(), message);
            }
            return None;
        }
        let Json::Object(entries) = value else {
            self.wrong(value, "a block (an object)");
            return None;
        };
        let id = if let Some(value) = find(entries, "id") {
            self.at(Seg::Key("id"), |d| d.id(value, BlockId::new))
        } else {
            let message = format!("{} is a block without an \"id\".", self.subject());
            self.report(codes::MISSING_KEY, message);
            None
        };
        let saved = (
            std::mem::replace(&mut self.block, id.clone()),
            std::mem::replace(&mut self.block_base, self.path.len()),
        );
        if let Some(id) = &id {
            let location = self.location();
            self.claim_canvas_id(id, location);
        }
        let block = self.block_body(value, entries, top_level, id);
        (self.block, self.block_base) = saved;
        block
    }

    fn block_body(
        &mut self,
        value: &'a Json,
        entries: &'a [(Box<str>, Json)],
        top_level: bool,
        id: Option<BlockId>,
    ) -> Option<Block> {
        self.object(value, &BLOCK_KEYS);
        let block_type = self.required(entries, "type", Self::string);
        let version = self.required(entries, "v", Self::u32);
        let x = self.nullable(entries, "x", Self::position);
        let y = self.nullable(entries, "y", Self::position);
        if !top_level && (matches!(x, Ok(Some(_))) || matches!(y, Ok(Some(_)))) {
            self.report(
                codes::NESTED_POSITION,
                String::from(
                    "This block is inside another block, so it cannot have a canvas position (\"x\" and \"y\"). Remove them.",
                ),
            );
        }
        let collapsed = self.or_default(entries, "collapsed", || false, Self::bool);
        let disabled = self.or_default(entries, "disabled", || false, Self::bool);
        let comment = self.nullable(entries, "comment", Self::comment);
        let extra = self.or_default(entries, "extra", BTreeMap::new, Self::extra);
        let fields = self.or_default(entries, "fields", BTreeMap::new, Self::fields);
        let inputs = self.or_default(entries, "inputs", BTreeMap::new, Self::inputs);
        let statements = self.or_default(entries, "statements", BTreeMap::new, Self::statements);
        Some(Block {
            id: id?,
            block_type: block_type?,
            v: version?,
            x: x.unwrap_or(None),
            y: y.unwrap_or(None),
            collapsed: collapsed.unwrap_or_default(),
            disabled: disabled.unwrap_or_default(),
            comment: comment.unwrap_or(None),
            extra: extra.unwrap_or_default(),
            fields: fields.unwrap_or_default(),
            inputs: inputs.unwrap_or_default(),
            statements: statements.unwrap_or_default(),
        })
    }

    fn comment(&mut self, value: &'a Json) -> Option<BlockComment> {
        let entries = self.object(value, &["text", "pinned"])?;
        let text = self.required(entries, "text", Self::string);
        let pinned = self.or_default(entries, "pinned", || false, Self::bool);
        Some(BlockComment {
            text: text?,
            pinned: pinned?,
        })
    }

    /// Decodes a map with user-chosen keys.
    fn map<T>(
        &mut self,
        value: &'a Json,
        mut f: impl FnMut(&mut Self, &'a str, &'a Json) -> Option<T>,
    ) -> Option<BTreeMap<String, T>> {
        let Json::Object(entries) = value else {
            self.wrong(value, "an object");
            return None;
        };
        let mut out = BTreeMap::new();
        for (key, item) in entries {
            if !self.map_key(key, false) {
                continue;
            }
            if let Some(decoded) = self.at(Seg::Key(key), |d| f(d, key, item)) {
                out.insert(key.to_string(), decoded);
            }
        }
        Some(out)
    }

    fn extra(&mut self, value: &'a Json) -> Option<BTreeMap<String, serde_json::Value>> {
        self.map(value, |d, key, item| {
            d.check_variadic(item);
            if key == "params" {
                d.declare_params(item);
            }
            d.untyped(item, false)
        })
    }

    /// Counts and lists in `extra` describe ⊕ parts: at most
    /// [`MAX_VARIADIC_PARTS`] of them.
    fn check_variadic(&mut self, value: &Json) {
        let limit = u32::try_from(MAX_VARIADIC_PARTS).unwrap_or(u32::MAX);
        let message = match value {
            Json::Number(n)
                if n.as_u64().map_or_else(
                    || n.as_f64().is_some_and(|x| x > f64::from(limit)),
                    |n| n > u64::from(limit),
                ) =>
            {
                format!(
                    "{} is {n}, but a block can have at most {MAX_VARIADIC_PARTS} parts.",
                    self.subject()
                )
            }
            Json::Array(items) if items.len() > MAX_VARIADIC_PARTS => format!(
                "{} has {} entries, but a block can have at most {MAX_VARIADIC_PARTS}.",
                self.subject(),
                items.len()
            ),
            _ => return,
        };
        self.report(codes::TOO_MANY_PARTS, message);
    }

    /// Function parameter rows (`extra.params`) declare symbols. Their exact
    /// shape is checked by the catalog; here only the symbol IDs are claimed
    /// and the names' length is limited.
    fn declare_params(&mut self, value: &'a Json) {
        let Json::Array(rows) = value else {
            return;
        };
        for (index, row) in rows.iter().enumerate() {
            self.at(Seg::Index(index), |d| {
                if let Some(Json::String(sym)) = row.get("sym")
                    && let Ok(sym) = SymbolId::new(sym)
                {
                    d.declare_symbol(&sym);
                }
                if let Some(Json::String(name)) = row.get("name") {
                    d.at(Seg::Key("name"), |d| d.check_name_length(name));
                }
            });
        }
    }

    fn check_name_length(&mut self, name: &str) -> bool {
        if name.len() <= MAX_IDENT_LEN {
            return true;
        }
        let message = format!(
            "{} is {} characters long, but names can be at most {MAX_IDENT_LEN}.",
            self.subject(),
            name.chars().count()
        );
        self.report(codes::TOO_LONG, message);
        false
    }

    fn fields(&mut self, value: &'a Json) -> Option<BTreeMap<String, FieldValue>> {
        self.map(value, |d, _, item| d.field_value(item))
    }

    fn field_value(&mut self, value: &'a Json) -> Option<FieldValue> {
        match value {
            Json::Bool(b) => Some(FieldValue::Bool(*b)),
            Json::String(_) => self.string(value).map(FieldValue::Text),
            Json::Object(entries) if find(entries, "ref").is_some() => {
                let entries = self.object(value, &["ref"])?;
                let target = self.required(entries, "ref", |d, v| d.id(v, SymbolId::new))?;
                Some(FieldValue::Ref(SymbolRef { target }))
            }
            Json::Object(_) => {
                let entries = self.object(value, &["sym", "name"])?;
                let sym = self.required(entries, "sym", |d, v| d.id(v, SymbolId::new));
                let name = self.required(entries, "name", |d, v| {
                    let name = d.string(v)?;
                    d.check_name_length(&name).then_some(name)
                });
                if let Some(sym) = &sym {
                    self.declare_symbol(sym);
                }
                Some(FieldValue::Decl(SymbolDecl {
                    sym: sym?,
                    name: name?,
                }))
            }
            Json::Number(_) => {
                self.wrong(
                    value,
                    "text (numbers in fields are stored as text, for example \"42\")",
                );
                None
            }
            _ => {
                self.wrong(
                    value,
                    "text, true or false, a symbol declaration {\"sym\": …, \"name\": …} or a reference {\"ref\": …}",
                );
                None
            }
        }
    }

    fn inputs(&mut self, value: &'a Json) -> Option<BTreeMap<String, Input>> {
        self.map(value, |d, _, item| d.input(item))
    }

    fn input(&mut self, value: &'a Json) -> Option<Input> {
        let Json::Object(entries) = value else {
            self.wrong(value, "an object with \"block\" or \"expr\"");
            return None;
        };
        if find(entries, "block").is_some() {
            let entries = self.object(value, &["block"])?;
            let block = self.required(entries, "block", |d, v| d.block(v, false))?;
            Some(Input::Block(BlockInput {
                block: Box::new(block),
            }))
        } else if find(entries, "expr").is_some() {
            let entries = self.object(value, &["expr", "draft"])?;
            let expr = self.required(entries, "expr", Self::tokens);
            let draft = self.or_default(entries, "draft", || false, Self::bool);
            Some(Input::Expr(ExprInput {
                expr: expr?,
                draft: draft?,
            }))
        } else {
            self.object(value, &[]);
            let message = format!(
                "{} should contain either a block (\"block\") or an expression (\"expr\").",
                self.subject()
            );
            self.report(codes::MISSING_KEY, message);
            None
        }
    }

    fn tokens(&mut self, value: &'a Json) -> Option<Vec<Token>> {
        if let Json::Array(items) = value
            && items.len() > MAX_EXPR_TOKENS
        {
            let message = format!(
                "This expression has {} parts (tokens), but a slot can hold at most {MAX_EXPR_TOKENS}. Split it into smaller pieces.",
                items.len()
            );
            self.report(codes::TOO_MANY_TOKENS, message);
            return None;
        }
        self.list(value, Self::token)
    }

    fn token(&mut self, value: &'a Json) -> Option<Token> {
        const EXPECTED: &str =
            "a token such as {\"num\": \"42\"}, {\"str\": \"text\"} or {\"ref\": \"sym_x\"}";
        let Json::Object(entries) = value else {
            self.wrong(value, EXPECTED);
            return None;
        };
        let [(kind, inner)] = &**entries else {
            self.wrong(value, EXPECTED);
            return None;
        };
        if !TOKEN_KINDS.contains(&&**kind) {
            self.object(value, &TOKEN_KINDS);
            return None;
        }
        self.at(Seg::Key(kind), |d| {
            if &**kind == "ref" {
                return d.id(inner, SymbolId::new).map(Token::Ref);
            }
            let text = d.string(inner)?;
            Some(match &**kind {
                "num" => Token::Num(text),
                "str" => Token::Str(text),
                "chr" => Token::Chr(text),
                "op" => Token::Op(text),
                "kw" => Token::Kw(text),
                _ => Token::Text(text),
            })
        })
    }

    fn statements(&mut self, value: &'a Json) -> Option<BTreeMap<String, Vec<Block>>> {
        self.map(value, |d, _, item| d.list(item, |d, block| d.block(block, false)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_in_messages_stay_short() {
        assert_eq!(short_number(0.0), "0");
        assert_eq!(short_number(0.05), "0.05");
        assert_eq!(short_number(-1.5), "-1.5");
        assert_eq!(short_number(4.01), "4.01");
        assert_eq!(short_number(1e308), "1e308");
        assert_eq!(short_number(-2.5e-7), "-2.5e-7");
    }

    #[test]
    fn module_names() {
        for good in [
            "main",
            "a",
            "game-loop",
            "util_2",
            &format!("a{}", "b".repeat(63)),
        ] {
            assert!(is_module_name(good), "{good}");
        }
        for bad in [
            "",
            "Main",
            "1main",
            "_x",
            "-x",
            "a.b",
            "a b",
            "ä",
            "a/b",
            "..",
            &"a".repeat(65),
        ] {
            assert!(!is_module_name(bad), "{bad}");
        }
    }

    #[test]
    fn device_names() {
        for device in [
            "con",
            "CON",
            "prn",
            "aux",
            "nul",
            "com0",
            "com9",
            "lpt0",
            "lpt9",
            "com\u{b9}",
            "COM\u{b2}",
            "lpt\u{b3}",
            "con.txt",
            "nul.cpp",
            "con ",
            "aux.",
        ] {
            assert!(is_device_name(device), "{device}");
        }
        for name in [
            "main",
            "conx",
            "com10",
            "lpt",
            "com",
            "console",
            "nul-1",
            "com\u{b4}",
        ] {
            assert!(!is_device_name(name), "{name}");
        }
    }
}
