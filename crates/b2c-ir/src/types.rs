//! Static types (spec §6.6). Milestone M1 covers the fundamental types and
//! `std::string`; containers, user types and generics are added later.

use serde::{Deserialize, Serialize};

/// A static type.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Type {
    /// `void` (function results only).
    Void,
    /// `bool`.
    Bool,
    /// `char`.
    Char,
    /// `int`.
    Int,
    /// `double`.
    Double,
    /// `std::string`.
    String,
    /// The type of an expression that already has an error. It is compatible
    /// with everything, so one mistake does not cause a cascade of errors.
    Error,
}

impl Type {
    /// The C++ spelling, e.g. `std::string`.
    pub fn cpp_name(&self) -> &'static str {
        match self {
            Self::Void => "void",
            Self::Bool => "bool",
            Self::Char => "char",
            Self::Int => "int",
            Self::Double => "double",
            Self::String => "std::string",
            Self::Error => "<error>",
        }
    }

    /// The friendly spelling used in messages (spec §3.5.2).
    pub fn friendly_name(&self) -> &'static str {
        match self {
            Self::Void => "nothing",
            Self::Bool => "true/false",
            Self::Char => "character",
            Self::Int => "whole number",
            Self::Double => "decimal number",
            Self::String => "text",
            Self::Error => "unknown",
        }
    }

    /// Parses a type field value from a project file (`"int"`, `"std::string"`, …).
    /// `auto` is handled by the caller. Returns `None` for unknown types.
    pub fn from_field(value: &str) -> Option<Self> {
        Some(match value {
            "void" => Self::Void,
            "bool" => Self::Bool,
            "char" => Self::Char,
            "int" => Self::Int,
            "double" => Self::Double,
            "std::string" => Self::String,
            _ => return None,
        })
    }

    /// Whether this is `int`, `double` or `char` (usable in arithmetic).
    pub fn is_numeric(&self) -> bool {
        matches!(self, Self::Int | Self::Double | Self::Char)
    }

    /// The header that declares this type, if any.
    pub fn header(&self) -> Option<&'static str> {
        match self {
            Self::String => Some("<string>"),
            _ => None,
        }
    }
}
