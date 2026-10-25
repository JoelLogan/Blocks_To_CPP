//! Building blocks for friendly diagnostic messages, and the sink that
//! collects diagnostics.
//!
//! User text (names, literal values) is never copied into a message as is:
//! [`quoted`] truncates it and replaces control and invisible characters with
//! visible placeholders, so a message can never hide or reorder text in the UI.

use std::fmt::Write as _;

use b2c_ir::diag::{DiagSource, Diagnostic, Location};
use b2c_ir::text::is_invisible;
use b2c_ir::types::{LibType, Type, TypeClass, TypeFacts};

/// Longest user text shown in a message, in characters.
const MAX_SHOWN_CHARS: usize = 40;

/// User text made safe for a message, without quotes.
pub(crate) fn shown(text: &str) -> String {
    let mut out = String::new();
    for (i, c) in text.chars().enumerate() {
        if i == MAX_SHOWN_CHARS {
            out.push('…');
            break;
        }
        if c.is_control() || is_invisible(c) || c == '`' {
            let _ = write!(out, "<U+{:04X}>", u32::from(c));
        } else {
            out.push(c);
        }
    }
    out
}

/// A user name for a message, in backticks (e.g. `` `score` ``).
pub(crate) fn quoted(name: &str) -> String {
    if name.is_empty() {
        String::from("(unnamed)")
    } else {
        format!("`{}`", shown(name))
    }
}

/// A type with its article, for messages (types.md §1.7): the fundamental
/// types and text keep their M2 phrases ("a whole number", "text"), library
/// types are named by their friendly label, as the type field shows them ("a
/// list of int", "a map from string to list of int"), and user types by their
/// name ("a `Student`").
pub(crate) fn a_type(ty: &Type, facts: &dyn TypeFacts) -> String {
    match ty {
        Type::Void => String::from("nothing"),
        Type::Bool => String::from("a true/false value"),
        Type::Char => String::from("a character"),
        Type::Int => String::from("a whole number"),
        Type::Double => String::from("a decimal number"),
        Type::String => String::from("text"),
        Type::Lib(_) => {
            let label = ty.label(facts).to_string();
            format!("{} {label}", article(&label))
        }
        Type::Named(id) => match facts.user_type(id) {
            Some(user) => format!("{} {}", article(user.name), quoted(user.name)),
            None => String::from("a value of an unknown type"),
        },
        Type::Error => String::from("an unknown value"),
    }
}

/// "an" before a word that starts with a vowel, "a" otherwise.
pub(crate) fn article(word: &str) -> &'static str {
    match word.chars().next().map(|c| c.to_ascii_lowercase()) {
        Some('a' | 'e' | 'i' | 'o' | 'u') => "an",
        _ => "a",
    }
}

/// What a library type is called on its own, without its arguments: "list",
/// "priority queue", "input file", …
pub(crate) fn lib_noun(ty: LibType) -> &'static str {
    match ty {
        LibType::Vector => "list",
        LibType::Deque => "deque",
        LibType::Set => "set",
        LibType::Map => "map",
        LibType::Queue => "queue",
        LibType::Stack => "stack",
        LibType::PriorityQueue => "priority queue",
        LibType::IfStream => "input file",
        LibType::OfStream => "output file",
        LibType::Exception => "error",
        LibType::RuntimeError => "runtime error",
        LibType::InvalidArgument => "invalid argument error",
        LibType::OutOfRange => "out-of-range error",
    }
}

/// The types of a class with their article, for messages: "a list, a deque
/// or a set" (types.md §1.3, §1.7).
pub(crate) fn a_class(class: TypeClass) -> &'static str {
    match class {
        TypeClass::Any => "a value",
        TypeClass::Number => "a number",
        TypeClass::Integer => "a whole number",
        TypeClass::Bool => "a true/false value",
        TypeClass::Text => "text or a character",
        TypeClass::Value => "a value that a variable can hold",
        TypeClass::Collection => "a list, a deque, a set, a map, a queue, a stack or a priority queue",
        TypeClass::Sequence => "a list or a deque",
        TypeClass::Indexable => "a list, a deque or a map",
        TypeClass::Insertable => "a list, a deque, a set, a queue, a stack or a priority queue",
        TypeClass::Searchable => "a list, a deque, a set or a map",
        TypeClass::Iterable => "a list, a deque, a set or text",
        TypeClass::Keyed => "a set or a map",
        TypeClass::Adaptor => "a queue, a stack or a priority queue",
        TypeClass::Listable => "a list, a deque or a set",
        TypeClass::Sortable => "a list or a deque of values that can be compared",
        TypeClass::Switchable => "a whole number, a character or an enum",
        TypeClass::Comparable => "a value that can be compared",
        TypeClass::Printable => "a value that can be printed",
        TypeClass::Struct => "a struct",
        TypeClass::Enum => "an enum",
        TypeClass::InputFile => "an input file",
        TypeClass::OutputFile => "an output file",
        TypeClass::Exception => "an error",
    }
}

/// The first user type (struct or enum) in `ty` that the class `class` does
/// not hold and whose tick would put `ty` in it, for a hint such as "Tick
/// comparable on `Point`". Only containers whose comparability is their
/// arguments' are looked into (types.md §1.3); nothing makes a container
/// printable.
pub(crate) fn culprit<'f>(ty: &Type, class: TypeClass, facts: &'f dyn TypeFacts) -> Option<&'f str> {
    let mut stack = vec![ty];
    while let Some(ty) = stack.pop() {
        match ty {
            Type::Named(id) => {
                if let Some(user) = facts.user_type(id)
                    && ty.is_in(class, facts) == Some(false)
                {
                    return Some(user.name);
                }
            }
            Type::Lib(app)
                if class == TypeClass::Comparable
                    && matches!(
                        app.ty(),
                        LibType::Vector
                            | LibType::Deque
                            | LibType::Set
                            | LibType::Map
                            | LibType::Queue
                            | LibType::Stack
                    ) =>
            {
                stack.extend(app.args().iter().rev());
            }
            _ => {}
        }
    }
    None
}

/// The message of `B2C-W0505` on the head of a parked stack (ADR-0012): a
/// block left on the canvas outside `when program starts` and every
/// function. `gives_value` is true for a reporter or predicate; otherwise
/// the head is a statement with `stacked` blocks below it.
pub(crate) fn parked(gives_value: bool, stacked: usize) -> String {
    const PLACES: &str = "attached to 'when program starts' or a function";
    if gives_value {
        return String::from(
            "This block gives a value, but it isn't plugged into another block, so it isn't used.",
        );
    }
    match stacked {
        0 => format!("This block isn't {PLACES}, so it doesn't run."),
        1 => format!("This block and the block below it aren't {PLACES}, so they don't run."),
        n => format!("This block and the {n} blocks below it aren't {PLACES}, so they don't run."),
    }
}

/// Collects diagnostics in the order they are found.
#[derive(Debug, Default)]
pub(crate) struct Diags {
    /// The diagnostics so far.
    pub(crate) list: Vec<Diagnostic>,
}

impl Diags {
    /// Reports an error.
    pub(crate) fn error(&mut self, code: &str, location: Location, message: impl Into<String>) {
        self.list
            .push(Diagnostic::error(code, DiagSource::Analyser, location, message));
    }

    /// Reports a warning.
    pub(crate) fn warning(&mut self, code: &str, location: Location, message: impl Into<String>) {
        self.list
            .push(Diagnostic::warning(code, DiagSource::Analyser, location, message));
    }

    /// Reports an informational diagnostic.
    pub(crate) fn info(&mut self, code: &str, location: Location, message: impl Into<String>) {
        self.list
            .push(Diagnostic::info(code, DiagSource::Analyser, location, message));
    }

    /// Reports an already built diagnostic.
    pub(crate) fn push(&mut self, diagnostic: Diagnostic) {
        self.list.push(diagnostic);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_made_safe() {
        assert_eq!(quoted("score"), "`score`");
        assert_eq!(quoted(""), "(unnamed)");
        assert_eq!(shown("a\u{202E}b"), "a<U+202E>b");
        assert_eq!(shown("a\nb`c"), "a<U+000A>b<U+0060>c");
        let long = "x".repeat(100);
        let s = shown(&long);
        assert_eq!(s.chars().count(), MAX_SHOWN_CHARS + 1);
        assert!(s.ends_with('…'));
    }

    #[test]
    fn parked_messages() {
        assert_eq!(
            parked(false, 0),
            "This block isn't attached to 'when program starts' or a function, so it doesn't run."
        );
        assert_eq!(
            parked(false, 1),
            "This block and the block below it aren't attached to 'when program starts' or a function, so they don't run."
        );
        assert_eq!(
            parked(false, 12),
            "This block and the 12 blocks below it aren't attached to 'when program starts' or a function, so they don't run."
        );
        // A value block's message is the same with or without a stack (the
        // stack itself is the catalog's B2C-E0604).
        for stacked in [0, 3] {
            assert_eq!(
                parked(true, stacked),
                "This block gives a value, but it isn't plugged into another block, so it isn't used."
            );
        }
    }

    #[test]
    fn articles() {
        use b2c_ir::types::NoFacts;
        let facts = &NoFacts;
        assert_eq!(a_type(&Type::Int, facts), "a whole number");
        assert_eq!(a_type(&Type::String, facts), "text");
        assert_eq!(a_type(&Type::Void, facts), "nothing");
        assert_eq!(a_type(&Type::Error, facts), "an unknown value");
        assert_eq!(a_type(&Type::Bool, facts), "a true/false value");
        assert_eq!(a_type(&Type::Char, facts), "a character");
        assert_eq!(a_type(&Type::Double, facts), "a decimal number");
    }

    /// Facts that know a struct `sym_Point` (not comparable) and an enum
    /// `sym_Item`.
    struct Facts;

    impl TypeFacts for Facts {
        fn user_type(&self, id: &b2c_ir::ids::SymbolId) -> Option<b2c_ir::types::UserType<'_>> {
            use b2c_ir::types::{UserKind, UserType};
            match id.as_str() {
                "sym_Point" => Some(UserType {
                    name: "Point",
                    kind: UserKind::Struct,
                    comparable: false,
                    printable: true,
                }),
                "sym_Item" => Some(UserType {
                    name: "Item",
                    kind: UserKind::Enum,
                    comparable: true,
                    printable: true,
                }),
                _ => None,
            }
        }
    }

    fn named(id: &str) -> Type {
        Type::Named(b2c_ir::ids::SymbolId::new(id).expect("id"))
    }

    fn lib(ty: LibType, args: Vec<Type>) -> Type {
        Type::lib(ty, args).expect("type")
    }

    #[test]
    fn constructed_and_user_types_are_named_by_their_labels() {
        let list = lib(LibType::Vector, vec![Type::Int]);
        assert_eq!(a_type(&list, &Facts), "a list of int");
        let map = lib(LibType::Map, vec![Type::String, list.clone()]);
        assert_eq!(a_type(&map, &Facts), "a map from string to list of int");
        assert_eq!(a_type(&lib(LibType::IfStream, vec![]), &Facts), "an input file");
        assert_eq!(
            a_type(&lib(LibType::OutOfRange, vec![]), &Facts),
            "an out-of-range error"
        );
        assert_eq!(a_type(&named("sym_Point"), &Facts), "a `Point`");
        assert_eq!(a_type(&named("sym_Item"), &Facts), "an `Item`");
        assert_eq!(a_type(&named("sym_Gone"), &Facts), "a value of an unknown type");
        for ty in LibType::ALL {
            assert!(!lib_noun(ty).is_empty());
        }
        for class in TypeClass::ALL {
            let phrase = a_class(class);
            assert!(phrase.starts_with("a ") || phrase.starts_with("an ") || phrase.starts_with("text"));
        }
    }

    #[test]
    fn culprits_are_the_user_types_a_class_does_not_hold() {
        let points = lib(LibType::Vector, vec![named("sym_Point")]);
        assert_eq!(culprit(&points, TypeClass::Comparable, &Facts), Some("Point"));
        assert_eq!(culprit(&points, TypeClass::Printable, &Facts), None);
        let items = lib(LibType::Set, vec![named("sym_Item")]);
        assert_eq!(culprit(&items, TypeClass::Comparable, &Facts), None);
        assert_eq!(culprit(&named("sym_Gone"), TypeClass::Comparable, &Facts), None);
    }
}
