//! Decoding type terms (types.md §2): a template in a type field, and a
//! reference or a template in the `type` of a row of `extra`.
//!
//! The loader checks a term's shape and limits only. A name is any text
//! that follows the name rule; whether it is a type the catalog decides
//! (`B2C-E0607`, `B2C-E0614`, `B2C-E0602`), and whether a reference names a
//! user type the analyser decides (`B2C-E0201`, `B2C-E0207`).
//!
//! * A name is a string: 1 to 64 ASCII characters, identifiers joined by
//!   `::` or single spaces.
//! * A user type is `{"ref": id}`, read like a reference in a field.
//! * A template is an object with exactly one key, its qualified name
//!   (with `::`), whose value is its argument, or an array of its two or
//!   more arguments. One argument is never a one-element array and an empty
//!   array is never written, so each type has one spelling.
//! * At most [`Type::MAX_DEPTH`] levels of templates and [`Type::MAX_NAMES`]
//!   names and references in one term (`B2C-E0140`). They are counted
//!   before anything else is checked, and the count stops one past either
//!   limit, so a hostile term costs no more than a small one and gets one
//!   problem.
//!
//! A wrong shape is `B2C-E0112`, an invalid ID `B2C-E0113` and a reserved
//! key `B2C-E0127`, as for other values.

use b2c_ir::SymbolId;
use b2c_ir::types::{Type, TypeTerm};

use super::{Decoder, Entries, RESERVED_KEYS, Seg, find};
use crate::codes;
use crate::json::Json;
use crate::limits::MAX_STRING_BYTES;
use crate::terms::is_template_key;
use crate::text_rules::{self, quote};

/// What a term should be, for messages.
const EXPECTED: &str = "a type: a name such as \"int\", a user type {\"ref\": …} or a template such as {\"std::vector\": \"int\"}";

/// Where a term is read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Place {
    /// A type field: every problem is reported here.
    Field,
    /// The `type` of a row of `extra`: free-form data, whose text rules and
    /// reserved keys [`Decoder::untyped`] reports, so they are not reported
    /// here a second time.
    Row,
}

/// Whether an object reads as a template (types.md §2.2): exactly one key,
/// and that key a qualified name.
pub(super) fn is_template_object(entries: Entries<'_>) -> bool {
    matches!(entries, [(key, _)] if is_template_key(key))
}

/// Whether a template keeps within [`Type::MAX_DEPTH`] levels and
/// [`Type::MAX_NAMES`] names and references, counted as
/// [`TypeTerm`] counts them: every name, reference and template is one name,
/// and every template one level more than the one it is in. Values that are
/// not terms count as one name. Stops one past either limit.
fn within_limits(value: &Json) -> bool {
    let mut names = 0usize;
    let mut pending = vec![(value, 0usize)];
    while let Some((value, level)) = pending.pop() {
        names += 1;
        if names > Type::MAX_NAMES {
            return false;
        }
        let Json::Object(entries) = value else {
            continue;
        };
        let [(key, inner)] = &**entries else {
            continue;
        };
        if !is_template_key(key) {
            continue;
        }
        let level = level + 1;
        if level > Type::MAX_DEPTH {
            return false;
        }
        match inner {
            // The template itself is counted already, so this many
            // arguments are over the limit.
            Json::Array(items) => {
                pending.extend(items.iter().take(Type::MAX_NAMES).map(|item| (item, level)));
            }
            arg => pending.push((arg, level)),
        }
    }
    true
}

impl<'a> Decoder<'a> {
    /// Reads a template term: its limits first, then its shape.
    pub(super) fn template(&mut self, value: &'a Json, place: Place) -> Option<TypeTerm> {
        if !within_limits(value) {
            let message = format!(
                "{} is a type with more than {} levels or more than {} names, more than a type can have.",
                self.subject(),
                Type::MAX_DEPTH,
                Type::MAX_NAMES
            );
            self.report(codes::TYPE_TOO_LARGE, message);
            return None;
        }
        self.term(value, place)
    }

    /// The `type` of a row of a known row key (types.md §2.2): an object
    /// read as a reference or a template is checked like one in a type
    /// field. Text and anything else are left to the catalog, which checks
    /// rows (`B2C-E0614`), as before type terms.
    pub(super) fn row_type(&mut self, value: &'a Json) {
        let Json::Object(entries) = value else {
            return;
        };
        if find(entries, "ref").is_some() {
            self.reference(value, Place::Row);
        } else if is_template_object(entries) {
            self.template(value, Place::Row);
        }
    }

    /// A reference, `{"ref": id}`, in a field or in a term.
    pub(super) fn reference(&mut self, value: &'a Json, place: Place) -> Option<SymbolId> {
        let entries = self.object_in(value, &["ref"], place == Place::Field)?;
        self.required(entries, "ref", |d, v| d.id(v, SymbolId::new))
    }

    /// A term within a template.
    fn term(&mut self, value: &'a Json, place: Place) -> Option<TypeTerm> {
        match value {
            Json::String(text) => self.type_name(value, text, place),
            Json::Object(entries) if find(entries, "ref").is_some() => {
                self.reference(value, place).map(TypeTerm::Ref)
            }
            Json::Object(entries) => match &**entries {
                [(key, inner)] if is_template_key(key) => {
                    self.at(Seg::Key(key), |d| d.arguments(key, inner, place))
                }
                [(key, _)] if RESERVED_KEYS.contains(&&**key) => {
                    if place == Place::Field {
                        self.reserved_key(key);
                    }
                    None
                }
                _ => {
                    self.wrong(value, EXPECTED);
                    if place == Place::Field {
                        self.reserved_keys_in(entries);
                    }
                    None
                }
            },
            _ => {
                self.wrong(value, EXPECTED);
                None
            }
        }
    }

    /// A name: text that follows the name rule.
    fn type_name(&mut self, value: &'a Json, text: &str, place: Place) -> Option<TypeTerm> {
        match place {
            Place::Field => {
                self.string(value)?;
            }
            // `untyped` reports text that breaks the text rules.
            Place::Row => {
                if text.len() > MAX_STRING_BYTES || !text_rules::check(text).is_clean() {
                    return None;
                }
            }
        }
        if let Ok(term) = TypeTerm::name(text) {
            return Some(term);
        }
        let message = format!(
            "{} is not a type name: {} is not 1 to 64 letters, digits and _ in names joined by :: or single spaces, each name starting with a letter or _.",
            self.subject(),
            quote(text)
        );
        self.report(codes::WRONG_VALUE, message);
        None
    }

    /// A template's arguments: one argument as it is, or an array of two or
    /// more. Every argument is checked, so all their problems are reported.
    fn arguments(&mut self, name: &str, value: &'a Json, place: Place) -> Option<TypeTerm> {
        let args = match value {
            Json::Array(items) if items.len() >= 2 => {
                let mut args = Vec::with_capacity(items.len());
                let mut valid = true;
                for (index, item) in items.iter().enumerate() {
                    match self.at(Seg::Index(index), |d| d.term(item, place)) {
                        Some(arg) => args.push(arg),
                        None => valid = false,
                    }
                }
                valid.then_some(args)?
            }
            Json::Array(items) => {
                let message = if items.is_empty() {
                    format!(
                        "{} is an empty list, but a template needs its arguments: one written as it is, or two or more in a list.",
                        self.subject()
                    )
                } else {
                    format!(
                        "{} is a list of one argument. Write a template's only argument without the list, as in {{\"std::vector\": \"int\"}}.",
                        self.subject()
                    )
                };
                self.report(codes::WRONG_VALUE, message);
                return None;
            }
            arg => vec![self.term(arg, place)?],
        };
        // The name and the limits were checked before: this cannot fail.
        TypeTerm::app(name, args).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::json::parse;

    fn json(text: &str) -> Json {
        parse(text).unwrap().value
    }

    /// `levels` vectors nested around `int`.
    fn nested(levels: usize) -> String {
        let mut text = String::from("\"int\"");
        for _ in 0..levels {
            text = format!("{{\"std::vector\": {text}}}");
        }
        text
    }

    /// A template with `args` arguments: `args + 1` names.
    fn wide(args: usize) -> String {
        let list: Vec<String> = (0..args).map(|i| format!("\"t{i}\"")).collect();
        format!("{{\"a::b\": [{}]}}", list.join(", "))
    }

    #[test]
    fn limits_are_exact() {
        assert!(within_limits(&json(&nested(8))));
        assert!(!within_limits(&json(&nested(9))));
        assert!(within_limits(&json(&wide(31))));
        assert!(!within_limits(&json(&wide(32))));
        // References count as names, and so does what is not a term.
        let refs: Vec<&str> = vec!["{\"ref\": \"s\"}"; 31];
        assert!(within_limits(&json(&format!(
            "{{\"a::b\": [{}]}}",
            refs.join(", ")
        ))));
        let junk: Vec<&str> = vec!["[1, 2, 3]"; 32];
        assert!(!within_limits(&json(&format!(
            "{{\"a::b\": [{}]}}",
            junk.join(", ")
        ))));
        // A map key at the eighth level is fine; one level deeper is not.
        let deep_key = format!("{{\"std::map\": [{}, \"int\"]}}", nested(7));
        assert!(within_limits(&json(&deep_key)));
        let deeper = format!("{{\"std::map\": [{}, \"int\"]}}", nested(8));
        assert!(!within_limits(&json(&deeper)));
        // Objects that are not templates are not levels.
        assert!(within_limits(&json("{\"a::b\": {\"x\": {\"y\": {\"z\": 1}}}}")));
    }

    #[test]
    fn templates_are_one_key_objects_with_qualified_names() {
        let entries = |text: &str| match json(text) {
            Json::Object(entries) => entries,
            _ => unreachable!(),
        };
        assert!(is_template_object(&entries("{\"std::vector\": \"int\"}")));
        assert!(is_template_object(&entries("{\"my lib::x\": []}")));
        assert!(!is_template_object(&entries("{\"vector\": \"int\"}")));
        assert!(!is_template_object(&entries(
            "{\"sm\": \"sym_x\", \"name\": \"score\"}"
        )));
        assert!(!is_template_object(&entries(
            "{\"std::vector\": \"int\", \"std::set\": \"int\"}"
        )));
        assert!(!is_template_object(&entries("{}")));
    }
}
