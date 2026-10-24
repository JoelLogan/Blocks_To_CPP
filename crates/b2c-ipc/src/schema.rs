//! Request schemas: the shape of every request, as data.
//!
//! One [`ObjectSchema`] per request type drives three checks that must agree:
//!
//! * the backend's structural check in [`crate::decode::decode`] ([`check`]),
//!   which runs before serde and reports the exact field;
//! * the isolation hook's allowlist in the webview
//!   (`apps/desktop/src-tauri/isolation/allowlist.generated.js`), generated from
//!   these schemas;
//! * the serde types themselves: a test compares every schema's keys with the keys
//!   of the type's serialised sample.
//!
//! Keys are exact: an unknown key, a missing required key, `null` for an optional
//! key and the prototype-polluting keys `__proto__`, `constructor` and `prototype`
//! are all rejected (`docs/spec/08-security.md` §8.8, threat T6).

use serde_json::Value;

use crate::error::{InvalidReason, IpcError};

/// Keys that are rejected at every depth, whatever the schema says.
pub const RESERVED_KEYS: [&str; 3] = ["__proto__", "constructor", "prototype"];

/// The longest unknown key that is echoed back in an error's field path; longer
/// keys, and keys with other characters than ASCII letters, digits and `_`, are
/// reported as `…`.
const MAX_ECHOED_KEY: usize = 64;

/// The shape of one JSON value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldSchema {
    /// A string of at most `max_len` UTF-8 bytes.
    String {
        /// The most UTF-8 bytes.
        max_len: usize,
    },
    /// Standard base64 text with padding.
    Base64 {
        /// The most characters.
        max_chars: usize,
        /// The most decoded bytes.
        max_bytes: usize,
    },
    /// An opaque ID: `prefix` followed by exactly `hex_len` lower-case hex digits.
    Id {
        /// The fixed prefix, for example `ph_`.
        prefix: &'static str,
        /// The number of hex digits after the prefix.
        hex_len: usize,
    },
    /// One of a fixed set of strings.
    Enum {
        /// The allowed strings.
        values: &'static [&'static str],
    },
    /// A whole number in `min..=max`.
    Int {
        /// The smallest allowed value.
        min: i64,
        /// The largest allowed value.
        max: i64,
    },
    /// One of a fixed set of whole numbers.
    IntOneOf {
        /// The allowed values.
        values: &'static [i64],
    },
    /// `true` or `false`.
    Bool,
    /// A nested object.
    Object(&'static ObjectSchema),
}

/// One key of an object.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FieldSpec {
    /// The key, in camelCase.
    pub name: &'static str,
    /// The value's shape.
    pub schema: FieldSchema,
    /// Whether the key may be absent. An optional key that is present must hold a
    /// valid value; `null` is not accepted.
    pub optional: bool,
}

impl FieldSpec {
    /// A required key.
    pub const fn required(name: &'static str, schema: FieldSchema) -> Self {
        Self {
            name,
            schema,
            optional: false,
        }
    }

    /// An optional key.
    pub const fn optional(name: &'static str, schema: FieldSchema) -> Self {
        Self {
            name,
            schema,
            optional: true,
        }
    }
}

/// The exact set of keys of a JSON object and the shape of each value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ObjectSchema {
    /// The keys, in declaration order.
    pub fields: &'static [FieldSpec],
}

impl ObjectSchema {
    /// The spec of the key `name`, if the object has one.
    pub fn field(&self, name: &str) -> Option<&FieldSpec> {
        self.fields.iter().find(|f| f.name == name)
    }
}

/// Checks `value` against `schema`.
///
/// # Errors
/// Returns [`IpcError::InvalidRequest`] with the reason and the dotted path of the
/// first offending field, or [`IpcError::PayloadTooLarge`] for a string or base64
/// text over its limit.
pub fn check(value: &Value, schema: &ObjectSchema) -> Result<(), IpcError> {
    check_object(value, schema, "")
}

fn join(path: &str, key: &str) -> String {
    if path.is_empty() {
        key.to_owned()
    } else {
        format!("{path}.{key}")
    }
}

/// The text used for an unknown key in a field path: the key itself when it is
/// short and plain, so the error never echoes arbitrary or huge text.
fn echo_key(key: &str) -> &str {
    let plain = !key.is_empty()
        && key.len() <= MAX_ECHOED_KEY
        && key.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_');
    if plain { key } else { "…" }
}

fn check_object(value: &Value, schema: &ObjectSchema, path: &str) -> Result<(), IpcError> {
    let Value::Object(map) = value else {
        return Err(invalid(InvalidReason::Malformed, path));
    };
    for key in map.keys() {
        if RESERVED_KEYS.contains(&key.as_str()) || schema.field(key).is_none() {
            return Err(IpcError::invalid(
                InvalidReason::UnknownField,
                Some(&join(path, echo_key(key))),
            ));
        }
    }
    for spec in schema.fields {
        let field_path = join(path, spec.name);
        match map.get(spec.name) {
            None if spec.optional => {}
            None => return Err(IpcError::invalid(InvalidReason::MissingField, Some(&field_path))),
            Some(value) => check_value(value, &spec.schema, &field_path)?,
        }
    }
    Ok(())
}

fn invalid(reason: InvalidReason, path: &str) -> IpcError {
    IpcError::invalid(reason, (!path.is_empty()).then_some(path))
}

fn check_value(value: &Value, schema: &FieldSchema, path: &str) -> Result<(), IpcError> {
    match *schema {
        FieldSchema::String { max_len } => {
            let text = as_str(value, path)?;
            if text.len() > max_len {
                return Err(IpcError::too_large(max_len));
            }
        }
        FieldSchema::Base64 { max_chars, .. } => {
            // The decoded size and the alphabet are checked by the request's
            // `validate`, which decodes the text once.
            let text = as_str(value, path)?;
            if text.len() > max_chars {
                return Err(IpcError::too_large(max_chars));
            }
        }
        FieldSchema::Id { prefix, hex_len } => {
            let text = as_str(value, path)?;
            if !is_id(text, prefix, hex_len) {
                return Err(invalid(InvalidReason::BadId, path));
            }
        }
        FieldSchema::Enum { values } => {
            let text = as_str(value, path)?;
            if !values.contains(&text) {
                return Err(invalid(InvalidReason::BadEnum, path));
            }
        }
        FieldSchema::Int { min, max } => {
            let number = as_int(value, path)?;
            if !(min..=max).contains(&number) {
                return Err(invalid(InvalidReason::OutOfRange, path));
            }
        }
        FieldSchema::IntOneOf { values } => {
            let number = as_int(value, path)?;
            if !values.contains(&number) {
                return Err(invalid(InvalidReason::OutOfRange, path));
            }
        }
        FieldSchema::Bool => {
            if !value.is_boolean() {
                return Err(invalid(InvalidReason::Malformed, path));
            }
        }
        FieldSchema::Object(schema) => check_object(value, schema, path)?,
    }
    Ok(())
}

fn as_str<'a>(value: &'a Value, path: &str) -> Result<&'a str, IpcError> {
    value
        .as_str()
        .ok_or_else(|| invalid(InvalidReason::Malformed, path))
}

/// A whole number. Whole numbers too large for `i64` are out of range for every
/// schema; fractions and other types are malformed.
fn as_int(value: &Value, path: &str) -> Result<i64, IpcError> {
    let Value::Number(number) = value else {
        return Err(invalid(InvalidReason::Malformed, path));
    };
    if let Some(n) = number.as_i64() {
        Ok(n)
    } else if number.is_u64() {
        Err(invalid(InvalidReason::OutOfRange, path))
    } else {
        Err(invalid(InvalidReason::Malformed, path))
    }
}

/// Whether `text` is `prefix` followed by exactly `hex_len` lower-case hex digits.
pub fn is_id(text: &str, prefix: &str, hex_len: usize) -> bool {
    text.strip_prefix(prefix).is_some_and(|hex| {
        hex.len() == hex_len && hex.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    static INNER: ObjectSchema = ObjectSchema {
        fields: &[
            FieldSpec::required("cols", FieldSchema::Int { min: 2, max: 1000 }),
            FieldSpec::optional("width", FieldSchema::IntOneOf { values: &[2, 4] }),
        ],
    };

    static OUTER: ObjectSchema = ObjectSchema {
        fields: &[
            FieldSpec::required(
                "id",
                FieldSchema::Id {
                    prefix: "ph_",
                    hex_len: 4,
                },
            ),
            FieldSpec::required("text", FieldSchema::String { max_len: 5 }),
            FieldSpec::required("pick", FieldSchema::Enum { values: &["a", "bB"] }),
            FieldSpec::required("flag", FieldSchema::Bool),
            FieldSpec::required(
                "data",
                FieldSchema::Base64 {
                    max_chars: 8,
                    max_bytes: 6,
                },
            ),
            FieldSpec::optional("inner", FieldSchema::Object(&INNER)),
        ],
    };

    fn valid() -> Value {
        // "héll" is 4 characters and 5 UTF-8 bytes: exactly the limit.
        json!({"id": "ph_00af", "text": "héll", "pick": "bB", "flag": true, "data": "AAAA"})
    }

    fn reject(value: &Value) -> IpcError {
        check(value, &OUTER).unwrap_err()
    }

    fn reason(reason: InvalidReason, field: &str) -> IpcError {
        IpcError::invalid(reason, Some(field))
    }

    #[test]
    fn accepts_valid_values() {
        check(&valid(), &OUTER).unwrap();
        let mut value = valid();
        value["inner"] = json!({"cols": 2});
        check(&value, &OUTER).unwrap();
        value["inner"] = json!({"cols": 1000, "width": 4});
        check(&value, &OUTER).unwrap();
    }

    #[test]
    fn rejects_bad_keys() {
        assert_eq!(
            check(&json!([]), &OUTER).unwrap_err(),
            IpcError::invalid(InvalidReason::Malformed, None)
        );
        let mut value = valid();
        value["extra"] = json!(1);
        assert_eq!(reject(&value), reason(InvalidReason::UnknownField, "extra"));
        let mut value = valid();
        value["inner"] = json!({"cols": 2, "bogus": true});
        assert_eq!(reject(&value), reason(InvalidReason::UnknownField, "inner.bogus"));
        for key in RESERVED_KEYS {
            let mut value = valid();
            value[key] = json!({});
            assert_eq!(reject(&value), reason(InvalidReason::UnknownField, key));
        }
        let mut value = valid();
        value["<script>"] = json!(1);
        assert_eq!(reject(&value), reason(InvalidReason::UnknownField, "…"));
        let mut value = valid();
        value["k".repeat(65)] = json!(1);
        assert_eq!(reject(&value), reason(InvalidReason::UnknownField, "…"));
        let mut value = valid();
        value.as_object_mut().unwrap().remove("flag");
        assert_eq!(reject(&value), reason(InvalidReason::MissingField, "flag"));
        let mut value = valid();
        value["inner"] = json!({});
        assert_eq!(reject(&value), reason(InvalidReason::MissingField, "inner.cols"));
        // `null` never stands for an absent optional key.
        let mut value = valid();
        value["inner"] = Value::Null;
        assert_eq!(reject(&value), reason(InvalidReason::Malformed, "inner"));
    }

    #[test]
    fn rejects_bad_values() {
        let cases = [
            ("id", json!("ph_00AF"), reason(InvalidReason::BadId, "id")),
            ("id", json!("ph_00a"), reason(InvalidReason::BadId, "id")),
            ("id", json!("bd_00af"), reason(InvalidReason::BadId, "id")),
            ("id", json!(7), reason(InvalidReason::Malformed, "id")),
            ("text", json!("toolong"), IpcError::too_large(5)),
            // Five characters, six bytes: the limit counts bytes.
            ("text", json!("héllo"), IpcError::too_large(5)),
            ("text", json!(null), reason(InvalidReason::Malformed, "text")),
            ("pick", json!("bb"), reason(InvalidReason::BadEnum, "pick")),
            ("pick", json!(["a"]), reason(InvalidReason::Malformed, "pick")),
            ("flag", json!("true"), reason(InvalidReason::Malformed, "flag")),
            ("data", json!("AAAAAAAAAAAA"), IpcError::too_large(8)),
            (
                "inner",
                json!({"cols": 1}),
                reason(InvalidReason::OutOfRange, "inner.cols"),
            ),
            (
                "inner",
                json!({"cols": 1001}),
                reason(InvalidReason::OutOfRange, "inner.cols"),
            ),
            (
                "inner",
                json!({"cols": -5}),
                reason(InvalidReason::OutOfRange, "inner.cols"),
            ),
            (
                "inner",
                json!({"cols": u64::MAX}),
                reason(InvalidReason::OutOfRange, "inner.cols"),
            ),
            (
                "inner",
                json!({"cols": 2.5}),
                reason(InvalidReason::Malformed, "inner.cols"),
            ),
            (
                "inner",
                json!({"cols": "2"}),
                reason(InvalidReason::Malformed, "inner.cols"),
            ),
            (
                "inner",
                json!({"cols": 2, "width": 3}),
                reason(InvalidReason::OutOfRange, "inner.width"),
            ),
            ("inner", json!(5), reason(InvalidReason::Malformed, "inner")),
        ];
        for (key, bad, expected) in cases {
            let mut value = valid();
            value[key] = bad.clone();
            assert_eq!(reject(&value), expected, "{key}: {bad}");
        }
    }

    #[test]
    fn ids() {
        assert!(is_id("ph_0123456789abcdef", "ph_", 16));
        assert!(!is_id("ph_0123456789abcdeF", "ph_", 16));
        assert!(!is_id("ph_0123456789abcde", "ph_", 16));
        assert!(!is_id("PH_0123456789abcdef", "ph_", 16));
        assert!(!is_id("ph_0123456789abcdef0", "ph_", 16));
        assert!(!is_id("", "ph_", 0));
        assert!(is_id("ph_", "ph_", 0));
    }
}
