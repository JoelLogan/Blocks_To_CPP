//! Static types (spec §6.6, ADR-0017; the shapes are those of
//! `docs/plans/m3-contracts/types.md` §1).
//!
//! * [`Type`]: the analyser's static type. Besides the fundamental types and
//!   `std::string` of milestone M2 it holds standard-library types from a
//!   closed table ([`LibType`]) with their template arguments, and user
//!   structs and enums by the symbol ID of their definition.
//! * [`TypeTerm`]: a type as project files and the wire write it, before its
//!   names are looked up (`"int"`, `{"std::vector": "int"}`, `{"ref": id}`).
//! * [`TypeFacts`]: what the type rules need to know about user types.
//! * [`TypeClass`]: named sets of types, for a catalog input's `check` and a
//!   signature's `where`; [`TypeFn`]: the type functions of catalog patterns.
//!
//! A [`Type`] serialises as its term, with `"error"` for [`Type::Error`]
//! ([`term_form`], types.md §3.1): `"int"`, `"std::string"`,
//! `{"std::vector": "int"}`, `{"ref": "sym_x"}`. Reading one checks the
//! term's shape and limits and looks its names up.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::{self, Write as _};
use std::sync::Arc;

use serde::de::{self, MapAccess, SeqAccess, Visitor};
use serde::ser::{SerializeMap, SerializeSeq};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::headers::Header;
use crate::ids::SymbolId;

/// A static type (06 §6.6). Equality, order and hash are structural.
/// At most 24 bytes; a clone never copies a type's arguments.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
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
    /// A standard-library type with its template arguments, if any. Made
    /// only by [`Type::lib`].
    Lib(LibApp),
    /// A user struct or enum: the symbol ID of its definition.
    Named(SymbolId),
    /// The type of an expression that already has an error. It is compatible
    /// with everything, so one mistake does not cause a cascade of errors.
    Error,
}

/// A standard-library type applied to its template arguments. Built only by
/// [`Type::lib`]: the arity is right, every argument is a value type, and the
/// type keeps within [`Type::MAX_DEPTH`] and [`Type::MAX_NAMES`].
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LibApp {
    ty: LibType,
    args: Arc<[Type]>,
}

impl LibApp {
    /// The library type.
    pub fn ty(&self) -> LibType {
        self.ty
    }

    /// Its template arguments, in order (none for streams and errors).
    pub fn args(&self) -> &[Type] {
        &self.args
    }
}

/// The standard-library types of M3 (types.md §1.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LibType {
    /// `std::vector<T>`.
    Vector,
    /// `std::deque<T>`.
    Deque,
    /// `std::set<T>`.
    Set,
    /// `std::map<K, V>`.
    Map,
    /// `std::queue<T>`.
    Queue,
    /// `std::stack<T>`.
    Stack,
    /// `std::priority_queue<T>`.
    PriorityQueue,
    /// `std::ifstream`.
    IfStream,
    /// `std::ofstream`.
    OfStream,
    /// `std::exception`.
    Exception,
    /// `std::runtime_error`.
    RuntimeError,
    /// `std::invalid_argument`.
    InvalidArgument,
    /// `std::out_of_range`.
    OutOfRange,
}

impl LibType {
    /// Every library type, in declaration order.
    pub const ALL: [LibType; 13] = [
        Self::Vector,
        Self::Deque,
        Self::Set,
        Self::Map,
        Self::Queue,
        Self::Stack,
        Self::PriorityQueue,
        Self::IfStream,
        Self::OfStream,
        Self::Exception,
        Self::RuntimeError,
        Self::InvalidArgument,
        Self::OutOfRange,
    ];

    /// The number of template arguments: 1, 2 for `std::map`, 0 for the
    /// streams and errors.
    pub const fn arity(self) -> usize {
        match self {
            Self::Vector | Self::Deque | Self::Set | Self::Queue | Self::Stack | Self::PriorityQueue => 1,
            Self::Map => 2,
            Self::IfStream
            | Self::OfStream
            | Self::Exception
            | Self::RuntimeError
            | Self::InvalidArgument
            | Self::OutOfRange => 0,
        }
    }

    /// The qualified C++ name (`std::vector`), which is also its JSON name.
    pub const fn cpp_name(self) -> &'static str {
        match self {
            Self::Vector => "std::vector",
            Self::Deque => "std::deque",
            Self::Set => "std::set",
            Self::Map => "std::map",
            Self::Queue => "std::queue",
            Self::Stack => "std::stack",
            Self::PriorityQueue => "std::priority_queue",
            Self::IfStream => "std::ifstream",
            Self::OfStream => "std::ofstream",
            Self::Exception => "std::exception",
            Self::RuntimeError => "std::runtime_error",
            Self::InvalidArgument => "std::invalid_argument",
            Self::OutOfRange => "std::out_of_range",
        }
    }

    /// The library type with this qualified C++ name, if any.
    pub fn from_cpp_name(name: &str) -> Option<LibType> {
        Self::ALL.into_iter().find(|ty| ty.cpp_name() == name)
    }

    /// The standard header that declares it.
    pub const fn header(self) -> Header {
        Header::known(match self {
            Self::Vector => "<vector>",
            Self::Deque => "<deque>",
            Self::Set => "<set>",
            Self::Map => "<map>",
            Self::Queue | Self::PriorityQueue => "<queue>",
            Self::Stack => "<stack>",
            Self::IfStream | Self::OfStream => "<fstream>",
            Self::Exception => "<exception>",
            Self::RuntimeError | Self::InvalidArgument | Self::OutOfRange => "<stdexcept>",
        })
    }

    /// The friendly label, with `%1` and `%2` for the arguments' labels
    /// (`list of %1`, `map from %1 to %2`, `input file`).
    pub const fn label(self) -> &'static str {
        match self {
            Self::Vector => "list of %1",
            Self::Deque => "deque of %1",
            Self::Set => "set of %1",
            Self::Map => "map from %1 to %2",
            Self::Queue => "queue of %1",
            Self::Stack => "stack of %1",
            Self::PriorityQueue => "priority queue of %1",
            Self::IfStream => "input file",
            Self::OfStream => "output file",
            Self::Exception => "error",
            Self::RuntimeError => "runtime error",
            Self::InvalidArgument => "invalid argument error",
            Self::OutOfRange => "out-of-range error",
        }
    }

    /// Whether a value of this type is a *value* (types.md §1.1): what a
    /// variable can hold and a template take as an argument. The containers
    /// and `std::map` are; the streams and errors are not.
    pub const fn is_value(self) -> bool {
        self.arity() > 0
    }
}

/// Why a type or a type term was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TermError {
    /// More than [`Type::MAX_DEPTH`] levels of templates (the loader's
    /// `E0140`, types.md §6).
    #[error("the type has more than 8 levels of templates")]
    TooDeep,
    /// More than [`Type::MAX_NAMES`] names and references (the loader's
    /// `E0140`).
    #[error("the type has more than 32 names")]
    TooManyNames,
    /// A name that breaks the name rule: 1 to 64 ASCII characters,
    /// identifiers joined by `::` or single spaces (a template's name must be
    /// qualified).
    #[error("not a valid type name")]
    BadName,
    /// A shape no type has: a template without arguments, a one-element or
    /// empty argument list, an object with other keys.
    #[error("not a valid type shape")]
    BadShape,
    /// A name that is neither a known type nor a known library type.
    #[error("unknown type")]
    Unknown,
    /// A library type with the wrong number of template arguments.
    #[error("`{name}` takes {expected} type arguments, not {found}")]
    Arity {
        /// The library type's C++ name.
        name: &'static str,
        /// How many it takes.
        expected: usize,
        /// How many it was given.
        found: usize,
    },
    /// `auto`, which is not a type.
    #[error("`auto` is not a type")]
    Auto,
    /// A template argument that is not a value type: `void`, a stream or an
    /// error.
    #[error("this type cannot be a template argument")]
    NotAValue,
}

impl Type {
    /// The most library types nested in one type.
    pub const MAX_DEPTH: usize = 8;
    /// The most names and references in one type.
    pub const MAX_NAMES: usize = 32;

    /// A library type applied to its arguments: the only way to make a
    /// [`Type::Lib`].
    ///
    /// Returns `Ok(Type::Error)` when an argument is [`Type::Error`], so an
    /// error is reported once.
    ///
    /// # Errors
    /// [`TermError::Arity`] for the wrong number of arguments,
    /// [`TermError::NotAValue`] for an argument that is `void`, a stream or
    /// an error, and [`TermError::TooDeep`] or [`TermError::TooManyNames`]
    /// when the result would break the limits.
    pub fn lib(ty: LibType, args: Vec<Type>) -> Result<Type, TermError> {
        if args.len() != ty.arity() {
            return Err(TermError::Arity {
                name: ty.cpp_name(),
                expected: ty.arity(),
                found: args.len(),
            });
        }
        if args.contains(&Type::Error) {
            return Ok(Type::Error);
        }
        if !args.iter().all(Type::is_argument) {
            return Err(TermError::NotAValue);
        }
        let ty = Type::Lib(LibApp {
            ty,
            args: args.into(),
        });
        if ty.depth() > Self::MAX_DEPTH {
            return Err(TermError::TooDeep);
        }
        if ty.names() > Self::MAX_NAMES {
            return Err(TermError::TooManyNames);
        }
        Ok(ty)
    }

    /// Looks the term's names up. `auto` is `Err(TermError::Auto)`: it is
    /// not a type.
    ///
    /// # Errors
    /// The term's [`TermError`]: a name that is not a type, a library type
    /// with the wrong arguments, or a term over the limits.
    pub fn from_term(term: &TypeTerm) -> Result<Type, TermError> {
        // Checked first, so the recursion below stays shallow whatever the
        // term's origin.
        let (depth, names) = term.measure();
        if depth > Self::MAX_DEPTH {
            return Err(TermError::TooDeep);
        }
        if names > Self::MAX_NAMES {
            return Err(TermError::TooManyNames);
        }
        Self::lookup(term)
    }

    fn lookup(term: &TypeTerm) -> Result<Type, TermError> {
        match term {
            TypeTerm::Name(name) => Self::from_name(name),
            TypeTerm::Ref(id) => Ok(Type::Named(id.clone())),
            TypeTerm::App(name, args) => {
                let ty = LibType::from_cpp_name(name).ok_or(TermError::Unknown)?;
                if ty.arity() == 0 {
                    return Err(TermError::Arity {
                        name: ty.cpp_name(),
                        expected: 0,
                        found: args.len(),
                    });
                }
                let args = args.iter().map(Self::lookup).collect::<Result<Vec<_>, _>>()?;
                Type::lib(ty, args)
            }
        }
    }

    /// A type written as a name: a fundamental type, `std::string` or a
    /// library type without arguments.
    fn from_name(name: &str) -> Result<Type, TermError> {
        Ok(match name {
            "void" => Type::Void,
            "bool" => Type::Bool,
            "char" => Type::Char,
            "int" => Type::Int,
            "double" => Type::Double,
            "std::string" => Type::String,
            "auto" => return Err(TermError::Auto),
            _ => {
                let ty = LibType::from_cpp_name(name).ok_or(TermError::Unknown)?;
                return Type::lib(ty, Vec::new());
            }
        })
    }

    /// The type as a term; `None` for [`Type::Error`], which has none.
    pub fn to_term(&self) -> Option<TypeTerm> {
        Some(match self {
            Type::Error => return None,
            Type::Named(id) => TypeTerm::Ref(id.clone()),
            Type::Lib(app) if app.args.is_empty() => TypeTerm::Name(app.ty.cpp_name().into()),
            Type::Lib(app) => TypeTerm::App(
                app.ty.cpp_name().into(),
                app.args
                    .iter()
                    .map(Type::to_term)
                    .collect::<Option<Vec<_>>>()?
                    .into_boxed_slice(),
            ),
            Type::Void | Type::Bool | Type::Char | Type::Int | Type::Double | Type::String => {
                TypeTerm::Name(self.primitive_name().into())
            }
        })
    }

    /// The C++ name of a fundamental type or `std::string`; empty for the
    /// other variants.
    fn primitive_name(&self) -> &'static str {
        match self {
            Type::Void => "void",
            Type::Bool => "bool",
            Type::Char => "char",
            Type::Int => "int",
            Type::Double => "double",
            Type::String => "std::string",
            Type::Lib(_) | Type::Named(_) | Type::Error => "",
        }
    }

    /// The standard headers the type's own spelling needs, sorted and
    /// unique: `<string>` for `std::string`, each library type's header,
    /// and those of its arguments.
    pub fn headers(&self) -> impl Iterator<Item = Header> + '_ {
        let mut headers = BTreeSet::new();
        self.collect_headers(&mut headers);
        headers.into_iter()
    }

    fn collect_headers(&self, headers: &mut BTreeSet<Header>) {
        match self {
            Type::String => {
                headers.insert(Header::known("<string>"));
            }
            Type::Lib(app) => {
                headers.insert(app.ty.header());
                for arg in app.args.iter() {
                    arg.collect_headers(headers);
                }
            }
            Type::Void
            | Type::Bool
            | Type::Char
            | Type::Int
            | Type::Double
            | Type::Named(_)
            | Type::Error => {}
        }
    }

    /// Class membership (types.md §1.3), as `where` uses it.
    ///
    /// `Some(true)` for [`Type::Error`], which is in every class, and
    /// `Some(false)` for `void`, which is in none. `None` when it is not
    /// known: a [`Type::Named`] that `facts` does not know, or a library
    /// type whose answer depends on such an argument (`sortable`, and
    /// `comparable` for a container).
    pub fn is_in(&self, class: TypeClass, facts: &dyn TypeFacts) -> Option<bool> {
        use LibType as L;
        let lib = match self {
            Type::Error => return Some(true),
            Type::Void => return Some(false),
            Type::Named(id) => return Some(Self::user_is_in(facts.user_type(id)?, class)),
            Type::Lib(app) => Some(app),
            Type::Bool | Type::Char | Type::Int | Type::Double | Type::String => None,
        };
        let lib_in = |types: &[LibType]| lib.is_some_and(|app| types.contains(&app.ty));
        Some(match class {
            TypeClass::Any => true,
            TypeClass::Number => matches!(self, Type::Int | Type::Double | Type::Char),
            TypeClass::Integer | TypeClass::Switchable => matches!(self, Type::Int | Type::Char),
            TypeClass::Bool => *self == Type::Bool,
            TypeClass::Text => matches!(self, Type::String | Type::Char),
            TypeClass::Value => lib.is_none_or(|app| app.ty.is_value()),
            TypeClass::Collection => lib.is_some_and(|app| app.ty.is_value()),
            TypeClass::Sequence => lib_in(&[L::Vector, L::Deque]),
            TypeClass::Indexable => lib_in(&[L::Vector, L::Deque, L::Map]),
            TypeClass::Insertable => {
                lib_in(&[L::Vector, L::Deque, L::Set, L::Queue, L::Stack, L::PriorityQueue])
            }
            TypeClass::Searchable => lib_in(&[L::Vector, L::Deque, L::Set, L::Map]),
            TypeClass::Keyed => lib_in(&[L::Set, L::Map]),
            TypeClass::Adaptor => lib_in(&[L::Queue, L::Stack, L::PriorityQueue]),
            TypeClass::Listable => lib_in(&[L::Vector, L::Deque, L::Set]),
            TypeClass::Iterable => *self == Type::String || lib_in(&[L::Vector, L::Deque, L::Set]),
            TypeClass::Printable => lib.is_none(),
            TypeClass::Struct | TypeClass::Enum => false,
            TypeClass::InputFile => lib_in(&[L::IfStream]),
            TypeClass::OutputFile => lib_in(&[L::OfStream]),
            TypeClass::Exception => {
                lib_in(&[L::Exception, L::RuntimeError, L::InvalidArgument, L::OutOfRange])
            }
            TypeClass::Sortable => {
                return match lib {
                    Some(app) if matches!(app.ty, L::Vector | L::Deque) => {
                        app.args.first()?.is_in(TypeClass::Comparable, facts)
                    }
                    _ => Some(false),
                };
            }
            TypeClass::Comparable => match lib {
                None => true,
                Some(app)
                    if matches!(
                        app.ty,
                        L::Vector | L::Deque | L::Set | L::Map | L::Queue | L::Stack
                    ) =>
                {
                    return all_of(app.args.iter().map(|arg| arg.is_in(TypeClass::Comparable, facts)));
                }
                Some(_) => false,
            },
        })
    }

    /// Class membership of a user type the facts know.
    fn user_is_in(user: UserType<'_>, class: TypeClass) -> bool {
        let is_enum = user.kind == UserKind::Enum;
        match class {
            TypeClass::Any | TypeClass::Value => true,
            TypeClass::Comparable => is_enum || user.comparable,
            TypeClass::Printable => is_enum || user.printable,
            TypeClass::Switchable | TypeClass::Enum => is_enum,
            TypeClass::Struct => !is_enum,
            TypeClass::Number
            | TypeClass::Integer
            | TypeClass::Bool
            | TypeClass::Text
            | TypeClass::Collection
            | TypeClass::Sequence
            | TypeClass::Indexable
            | TypeClass::Insertable
            | TypeClass::Searchable
            | TypeClass::Iterable
            | TypeClass::Keyed
            | TypeClass::Adaptor
            | TypeClass::Listable
            | TypeClass::Sortable
            | TypeClass::InputFile
            | TypeClass::OutputFile
            | TypeClass::Exception => false,
        }
    }

    /// The C++ spelling, with the standard names in full and user types by
    /// their name: `std::vector<Student>`. For messages and the editor's C++
    /// labels; code generation prints types its own way (types.md §5).
    pub fn cpp<'a>(&'a self, facts: &'a dyn TypeFacts) -> impl fmt::Display + 'a {
        Spelling {
            ty: self,
            facts,
            friendly: false,
        }
    }

    /// The friendly label: `list of Student`, `map from string to list of
    /// int`. The first argument of a map is put in parentheses when it is
    /// itself a map.
    pub fn label<'a>(&'a self, facts: &'a dyn TypeFacts) -> impl fmt::Display + 'a {
        Spelling {
            ty: self,
            facts,
            friendly: true,
        }
    }

    /// Whether the type may be a template argument: not `void`, a stream or
    /// an error ([`Type::Error`] is handled by [`Type::lib`] itself).
    fn is_argument(&self) -> bool {
        match self {
            Type::Void => false,
            Type::Lib(app) => app.ty.is_value(),
            Type::Bool
            | Type::Char
            | Type::Int
            | Type::Double
            | Type::String
            | Type::Named(_)
            | Type::Error => true,
        }
    }

    /// Library types nested in the type (0 for a type written as a name).
    fn depth(&self) -> usize {
        match self {
            Type::Lib(app) if !app.args.is_empty() => 1 + app.args.iter().map(Type::depth).max().unwrap_or(0),
            _ => 0,
        }
    }

    /// Names and references in the type's term.
    fn names(&self) -> usize {
        match self {
            Type::Lib(app) => 1 + app.args.iter().map(Type::names).sum::<usize>(),
            Type::Error => 0,
            _ => 1,
        }
    }
}

/// The deprecated helpers of milestone M2, which the integration pull
/// request of M3's first wave removes. They answer a placeholder for
/// [`Type::Lib`] and [`Type::Named`], which no M2 path produces.
impl Type {
    /// The C++ spelling, e.g. `std::string`. A library type gives its name
    /// without arguments, a user type `<user type>`.
    #[deprecated(note = "use `Type::cpp`")]
    pub fn cpp_name(&self) -> &'static str {
        match self {
            Self::Lib(app) => app.ty.cpp_name(),
            Self::Named(_) => "<user type>",
            Self::Error => "<error>",
            Self::Void | Self::Bool | Self::Char | Self::Int | Self::Double | Self::String => {
                self.primitive_name()
            }
        }
    }

    /// The friendly spelling used in M2's messages (spec §3.5.2); `unknown`
    /// for a library or user type.
    #[deprecated(note = "use `Type::label`")]
    pub fn friendly_name(&self) -> &'static str {
        match self {
            Self::Void => "nothing",
            Self::Bool => "true/false",
            Self::Char => "character",
            Self::Int => "whole number",
            Self::Double => "decimal number",
            Self::String => "text",
            Self::Lib(_) | Self::Named(_) | Self::Error => "unknown",
        }
    }

    /// Parses an M2 type field value from a project file (`"int"`,
    /// `"std::string"`, …). `auto` is handled by the caller. Returns `None`
    /// for unknown types; it never gives a library or user type.
    #[deprecated(note = "use `Type::from_term`")]
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
    #[deprecated(note = "use `Type::is_in` with `TypeClass::Number`")]
    pub fn is_numeric(&self) -> bool {
        matches!(self, Self::Int | Self::Double | Self::Char)
    }

    /// The header that declares this type, if any: `<string>`, or a library
    /// type's own header (not its arguments').
    #[deprecated(note = "use `Type::headers`")]
    pub fn header(&self) -> Option<&'static str> {
        match self {
            Self::String => Some("<string>"),
            Self::Lib(app) => Some(app.ty.header().as_str()),
            Self::Void
            | Self::Bool
            | Self::Char
            | Self::Int
            | Self::Double
            | Self::Named(_)
            | Self::Error => None,
        }
    }
}

/// `false` as soon as one answer is `false`, `true` when all are `true`, and
/// `None` otherwise.
fn all_of(answers: impl Iterator<Item = Option<bool>>) -> Option<bool> {
    let mut all = Some(true);
    for answer in answers {
        match answer {
            Some(false) => return Some(false),
            None => all = None,
            Some(true) => {}
        }
    }
    all
}

/// [`Type::cpp`] and [`Type::label`].
struct Spelling<'a> {
    ty: &'a Type,
    facts: &'a dyn TypeFacts,
    friendly: bool,
}

impl Spelling<'_> {
    fn of<'b>(&'b self, ty: &'b Type) -> Spelling<'b> {
        Spelling {
            ty,
            facts: self.facts,
            friendly: self.friendly,
        }
    }
}

impl fmt::Display for Spelling<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.ty {
            Type::Lib(app) if self.friendly => {
                let mut chars = app.ty.label().chars();
                while let Some(c) = chars.next() {
                    if c != '%' {
                        f.write_char(c)?;
                        continue;
                    }
                    let index = chars.next().and_then(|d| d.to_digit(10)).unwrap_or(0) as usize;
                    let Some(arg) = index.checked_sub(1).and_then(|i| app.args.get(i)) else {
                        continue;
                    };
                    let nested_map = index == 1
                        && app.ty == LibType::Map
                        && matches!(arg, Type::Lib(inner) if inner.ty == LibType::Map);
                    if nested_map {
                        write!(f, "({})", self.of(arg))?;
                    } else {
                        write!(f, "{}", self.of(arg))?;
                    }
                }
                Ok(())
            }
            Type::Lib(app) => {
                f.write_str(app.ty.cpp_name())?;
                if app.args.is_empty() {
                    return Ok(());
                }
                f.write_char('<')?;
                for (i, arg) in app.args.iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{}", self.of(arg))?;
                }
                f.write_char('>')
            }
            Type::Named(id) => match self.facts.user_type(id) {
                Some(user) => f.write_str(user.name),
                None if self.friendly => f.write_str("unknown type"),
                None => f.write_str("<unknown type>"),
            },
            Type::Error if self.friendly => f.write_str("unknown"),
            Type::Error => f.write_str("<error>"),
            Type::String if self.friendly => f.write_str("string"),
            Type::Void | Type::Bool | Type::Char | Type::Int | Type::Double | Type::String => {
                f.write_str(self.ty.primitive_name())
            }
        }
    }
}

/// The ID of the struct among [`sample_types`] (a struct in the shared
/// fixture's facts).
pub const SAMPLE_STRUCT: &str = "sym_Point";
/// The ID of the enum among [`sample_types`] (an enum in the shared
/// fixture's facts).
pub const SAMPLE_ENUM: &str = "sym_Color";

/// The seven M2 types and a fixed list of library and user types, for the
/// conversion table and the shared type fixture (types.md §9). The user
/// types are [`SAMPLE_STRUCT`] and [`SAMPLE_ENUM`]; facts decide what they
/// are.
pub fn sample_types() -> Vec<Type> {
    use LibType as L;
    let app = |ty: LibType, args: &[Type]| {
        Type::Lib(LibApp {
            ty,
            args: args.into(),
        })
    };
    let named = |id: &str| SymbolId::new(id).map_or(Type::Error, Type::Named);
    let point = named(SAMPLE_STRUCT);
    let color = named(SAMPLE_ENUM);
    let int_list = app(L::Vector, &[Type::Int]);
    vec![
        Type::Void,
        Type::Bool,
        Type::Char,
        Type::Int,
        Type::Double,
        Type::String,
        Type::Error,
        int_list.clone(),
        app(L::Vector, &[Type::Double]),
        app(L::Vector, &[Type::String]),
        app(L::Vector, &[int_list]),
        app(L::Deque, &[Type::Char]),
        app(L::Set, &[Type::Int]),
        app(L::Set, &[Type::String]),
        app(L::Map, &[Type::String, Type::Int]),
        app(L::Map, &[Type::Int, app(L::Vector, &[Type::Double])]),
        app(L::Queue, &[Type::Int]),
        app(L::Stack, &[Type::String]),
        app(L::PriorityQueue, &[Type::Double]),
        app(L::IfStream, &[]),
        app(L::OfStream, &[]),
        app(L::Exception, &[]),
        app(L::RuntimeError, &[]),
        app(L::InvalidArgument, &[]),
        app(L::OutOfRange, &[]),
        point.clone(),
        color,
        app(L::Vector, &[point]),
    ]
}

/// A type as files and the wire write it, before its names are looked up
/// (types.md §2). Made by [`TypeTerm::name`] and [`TypeTerm::app`], which
/// check its shape and limits; [`Type::from_term`] looks its names up.
///
/// It serialises canonically: a name as a string, a reference as
/// `{"ref": id}`, a template as an object with exactly one key, its name,
/// whose value is its argument, or an array of its two or more arguments.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TypeTerm {
    /// A name: `"int"`, `"std::string"`, `"auto"`, `"std::ifstream"`.
    Name(Box<str>),
    /// A reference to a user type: `{"ref": "sym_x"}`.
    Ref(SymbolId),
    /// A template with its arguments: `{"std::vector": "int"}`,
    /// `{"std::map": ["int", "int"]}`.
    App(Box<str>, Box<[TypeTerm]>),
}

impl TypeTerm {
    /// A name, checked against the name rule only (it is not looked up).
    ///
    /// # Errors
    /// [`TermError::BadName`] unless the name has 1 to 64 ASCII characters:
    /// identifiers joined by `::` or single spaces.
    pub fn name(name: &str) -> Result<TypeTerm, TermError> {
        if is_type_name(name) {
            Ok(TypeTerm::Name(name.into()))
        } else {
            Err(TermError::BadName)
        }
    }

    /// A template with its arguments, checked for shape and limits only.
    ///
    /// # Errors
    /// [`TermError::BadName`] unless `name` follows the name rule and is
    /// qualified (it contains `::`), [`TermError::BadShape`] without
    /// arguments, and [`TermError::TooDeep`] or [`TermError::TooManyNames`]
    /// over the limits.
    pub fn app(name: &str, args: Vec<TypeTerm>) -> Result<TypeTerm, TermError> {
        if !is_type_name(name) || !name.contains("::") {
            return Err(TermError::BadName);
        }
        if args.is_empty() {
            return Err(TermError::BadShape);
        }
        let term = TypeTerm::App(name.into(), args.into_boxed_slice());
        let (depth, names) = term.measure();
        if depth > Type::MAX_DEPTH {
            return Err(TermError::TooDeep);
        }
        if names > Type::MAX_NAMES {
            return Err(TermError::TooManyNames);
        }
        Ok(term)
    }

    /// Every reference in the term, in order.
    pub fn refs(&self) -> impl Iterator<Item = &SymbolId> {
        let mut refs = Vec::new();
        let mut stack = vec![self];
        while let Some(term) = stack.pop() {
            match term {
                TypeTerm::Name(_) => {}
                TypeTerm::Ref(id) => refs.push(id),
                TypeTerm::App(_, args) => stack.extend(args.iter().rev()),
            }
        }
        refs.into_iter()
    }

    /// Replaces each reference that `map` has a new ID for.
    pub fn rename(&mut self, map: &BTreeMap<SymbolId, SymbolId>) {
        let mut stack = vec![self];
        while let Some(term) = stack.pop() {
            match term {
                TypeTerm::Name(_) => {}
                TypeTerm::Ref(id) => {
                    if let Some(new) = map.get(id) {
                        *id = new.clone();
                    }
                }
                TypeTerm::App(_, args) => stack.extend(args.iter_mut()),
            }
        }
    }

    /// Whether the term holds a template anywhere (a format version 2 key).
    pub fn has_app(&self) -> bool {
        matches!(self, TypeTerm::App(..))
    }

    /// `Name(n)` gives `n` itself (`int`, `std::string`); every other term
    /// gives its canonical compact JSON (`{"std::vector":"int"}`,
    /// `{"ref":"sym_x"}`). Equal terms give equal keys, and different terms
    /// different keys.
    pub fn key(&self) -> String {
        match self {
            TypeTerm::Name(name) => name.to_string(),
            TypeTerm::Ref(_) | TypeTerm::App(..) => {
                let mut out = String::new();
                self.write_json(&mut out);
                out
            }
        }
    }

    /// Writes the canonical compact JSON of the term.
    fn write_json(&self, out: &mut String) {
        match self {
            TypeTerm::Name(name) => write_json_string(out, name),
            TypeTerm::Ref(id) => {
                out.push_str("{\"ref\":");
                write_json_string(out, id.as_str());
                out.push('}');
            }
            TypeTerm::App(name, args) => {
                out.push('{');
                write_json_string(out, name);
                out.push(':');
                if let [arg] = &args[..] {
                    arg.write_json(out);
                } else {
                    out.push('[');
                    for (i, arg) in args.iter().enumerate() {
                        if i > 0 {
                            out.push(',');
                        }
                        arg.write_json(out);
                    }
                    out.push(']');
                }
                out.push('}');
            }
        }
    }

    /// The term's template levels and its names and references, without
    /// recursion.
    fn measure(&self) -> (usize, usize) {
        let mut depth = 0;
        let mut names = 0;
        let mut stack = vec![(self, 0)];
        while let Some((term, level)) = stack.pop() {
            names += 1;
            if let TypeTerm::App(_, args) = term {
                depth = depth.max(level + 1);
                stack.extend(args.iter().map(|arg| (arg, level + 1)));
            }
        }
        (depth, names)
    }
}

/// The name rule (types.md §2.2): 1 to 64 ASCII characters, identifiers
/// (`[A-Za-z_][A-Za-z0-9_]*`) joined by `::` or single spaces.
fn is_type_name(name: &str) -> bool {
    if name.is_empty() || name.len() > 64 {
        return false;
    }
    let bytes = name.as_bytes();
    let mut i = 0;
    loop {
        // An identifier.
        match bytes.get(i) {
            Some(b) if b.is_ascii_alphabetic() || *b == b'_' => i += 1,
            _ => return false,
        }
        while bytes
            .get(i)
            .is_some_and(|b| b.is_ascii_alphanumeric() || *b == b'_')
        {
            i += 1;
        }
        // Then the end, or a separator before the next one.
        match bytes.get(i..) {
            Some([]) => return true,
            Some([b':', b':', ..]) => i += 2,
            Some([b' ', ..]) => i += 1,
            _ => return false,
        }
    }
}

/// Writes `text` as a JSON string, escaping what JSON requires.
fn write_json_string(out: &mut String, text: &str) {
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if u32::from(c) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", u32::from(c));
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

impl Serialize for TypeTerm {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            TypeTerm::Name(name) => serializer.serialize_str(name),
            TypeTerm::Ref(id) => {
                let mut map = serializer.serialize_map(Some(1))?;
                map.serialize_entry("ref", id)?;
                map.end()
            }
            TypeTerm::App(name, args) => {
                let mut map = serializer.serialize_map(Some(1))?;
                if let [arg] = &args[..] {
                    map.serialize_entry(&**name, arg)?;
                } else {
                    map.serialize_entry(&**name, &Args(args))?;
                }
                map.end()
            }
        }
    }
}

/// Two or more arguments, serialised as an array.
struct Args<'a>(&'a [TypeTerm]);

impl Serialize for Args<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut seq = serializer.serialize_seq(Some(self.0.len()))?;
        for arg in self.0 {
            seq.serialize_element(arg)?;
        }
        seq.end()
    }
}

impl<'de> Deserialize<'de> for TypeTerm {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        match deserializer.deserialize_any(TermVisitor { list: false })? {
            Value::One(term) => Ok(term),
            Value::Many(_) => Err(de::Error::custom(TermError::BadShape)),
        }
    }
}

/// A template's value: one argument, or an array of them.
enum Value {
    One(TypeTerm),
    Many(Vec<TypeTerm>),
}

impl<'de> Deserialize<'de> for Value {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(TermVisitor { list: true })
    }
}

/// Reads a term; with `list`, also an array of two or more terms.
struct TermVisitor {
    list: bool,
}

impl<'de> Visitor<'de> for TermVisitor {
    type Value = Value;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a type: a name, {\"ref\": id} or a one-key template object")
    }

    fn visit_str<E: de::Error>(self, value: &str) -> Result<Value, E> {
        TypeTerm::name(value).map(Value::One).map_err(E::custom)
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Value, A::Error> {
        let Some(key) = map.next_key::<String>()? else {
            return Err(de::Error::custom(TermError::BadShape));
        };
        let term = if key == "ref" {
            TypeTerm::Ref(map.next_value::<SymbolId>()?)
        } else {
            let args = match map.next_value::<Value>()? {
                Value::One(arg) => vec![arg],
                Value::Many(args) => args,
            };
            TypeTerm::app(&key, args).map_err(de::Error::custom)?
        };
        if map.next_key::<de::IgnoredAny>()?.is_some() {
            return Err(de::Error::custom(TermError::BadShape));
        }
        Ok(Value::One(term))
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Value, A::Error> {
        if !self.list {
            return Err(de::Error::custom(TermError::BadShape));
        }
        let mut args = Vec::new();
        while let Some(arg) = seq.next_element::<TypeTerm>()? {
            if args.len() == Type::MAX_NAMES {
                return Err(de::Error::custom(TermError::TooManyNames));
            }
            args.push(arg);
        }
        // One argument is never written as a one-element array, and an empty
        // array is never written: each type has one spelling.
        if args.len() < 2 {
            return Err(de::Error::custom(TermError::BadShape));
        }
        Ok(Value::Many(args))
    }
}

/// How [`term_form`] writes [`Type::Error`], which has no term: the type of
/// an expression that already has an error.
pub const ERROR_JSON: &str = "error";

/// A [`Type`] serialised in its term form ([`term_form`]): its
/// [`TypeTerm`]'s canonical JSON, or [`ERROR_JSON`] for [`Type::Error`].
/// Writing it allocates nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TermForm<'a>(pub &'a Type);

impl Serialize for TermForm<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self.0 {
            Type::Error => serializer.serialize_str(ERROR_JSON),
            Type::Named(id) => {
                let mut map = serializer.serialize_map(Some(1))?;
                map.serialize_entry("ref", id)?;
                map.end()
            }
            // `Type::lib` keeps a type within `Type::MAX_DEPTH`, so the
            // recursion stays shallow.
            Type::Lib(app) => match app.args() {
                [] => serializer.serialize_str(app.ty().cpp_name()),
                [arg] => {
                    let mut map = serializer.serialize_map(Some(1))?;
                    map.serialize_entry(app.ty().cpp_name(), &TermForm(arg))?;
                    map.end()
                }
                args => {
                    let mut map = serializer.serialize_map(Some(1))?;
                    map.serialize_entry(app.ty().cpp_name(), &TermArgs(args))?;
                    map.end()
                }
            },
            ty @ (Type::Void | Type::Bool | Type::Char | Type::Int | Type::Double | Type::String) => {
                serializer.serialize_str(ty.primitive_name())
            }
        }
    }
}

/// Two or more template arguments in their term form, as an array.
struct TermArgs<'a>(&'a [Type]);

impl Serialize for TermArgs<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut seq = serializer.serialize_seq(Some(self.0.len()))?;
        for arg in self.0 {
            seq.serialize_element(&TermForm(arg))?;
        }
        seq.end()
    }
}

/// A type serialises in its term form ([`term_form`]).
impl Serialize for Type {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        term_form::serialize(self, serializer)
    }
}

/// A type is read in its term form ([`term_form`]).
impl<'de> Deserialize<'de> for Type {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        term_form::deserialize(deserializer)
    }
}

/// The term form of a [`Type`] (types.md §3.1), for `#[serde(with = "…")]`:
/// a type is written as its [`TypeTerm`] (`"int"`, `"std::string"`,
/// `{"std::vector": "int"}`, `{"std::map": ["std::string", "int"]}`,
/// `{"ref": "sym_x"}`), and [`Type::Error`] as [`ERROR_JSON`]. Reading checks
/// the term's shape and limits, looks its names up ([`Type::from_term`]) and
/// accepts [`ERROR_JSON`]; M2's `"string"` is not a type name there.
///
/// `Type`'s own `Serialize` and `Deserialize` are these two functions.
pub mod term_form {
    use serde::de::{self, Deserialize, Deserializer};
    use serde::ser::{Serialize, Serializer};

    use super::{ERROR_JSON, TermForm, Type, TypeTerm};

    /// Writes `ty` in its term form.
    ///
    /// # Errors
    /// Only the serializer's own errors.
    pub fn serialize<S: Serializer>(ty: &Type, serializer: S) -> Result<S::Ok, S::Error> {
        TermForm(ty).serialize(serializer)
    }

    /// Reads a type in its term form.
    ///
    /// # Errors
    /// A term of the wrong shape or over the limits, or a name that is not a
    /// type (`auto`, `string`, an unknown name, a library type with the
    /// wrong arguments).
    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Type, D::Error> {
        match TypeTerm::deserialize(deserializer)? {
            TypeTerm::Name(name) if &*name == ERROR_JSON => Ok(Type::Error),
            term => Type::from_term(&term).map_err(de::Error::custom),
        }
    }
}

/// What the type rules need to know about user types.
pub trait TypeFacts {
    /// The user type defined by the symbol `id`, or `None` when the facts do
    /// not know it (or it is not a user type).
    fn user_type(&self, id: &SymbolId) -> Option<UserType<'_>>;
}

/// Facts that know no user type.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoFacts;

impl TypeFacts for NoFacts {
    fn user_type(&self, _: &SymbolId) -> Option<UserType<'_>> {
        None
    }
}

/// A user type, as [`TypeFacts`] describes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UserType<'a> {
    /// Its name.
    pub name: &'a str,
    /// A struct or an enum.
    pub kind: UserKind,
    /// Ticked *comparable* and every field's type is comparable (types.md
    /// §1.6). Enums are always comparable.
    pub comparable: bool,
    /// Ticked *printable* and every field's type is printable (types.md
    /// §1.6). Enums are always printable.
    pub printable: bool,
}

/// The kind of a user type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UserKind {
    /// A struct.
    Struct,
    /// An enum (`enum class`).
    Enum,
}

/// Classes of types (types.md §1.3): what a catalog input's `check` accepts
/// and what a signature's `where` allows a type variable to be. Catalog
/// files write them in snake case.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TypeClass {
    /// Every type but `void`.
    Any,
    /// `int`, `double`, `char` (in a `check`: what converts to `double`).
    Number,
    /// `int`, `char` (in a `check`: what converts to `int`).
    Integer,
    /// `bool` (in a `check`: what converts to `bool`).
    Bool,
    /// `std::string`, `char`.
    Text,
    /// What a variable can hold: the fundamental types but `void`,
    /// `std::string`, the containers, `std::map`, structs and enums.
    Value,
    /// The six containers and `std::map`.
    Collection,
    /// `std::vector`, `std::deque`.
    Sequence,
    /// `std::vector`, `std::deque`, `std::map`.
    Indexable,
    /// The containers that *add* takes: all six but `std::map`.
    Insertable,
    /// `std::vector`, `std::deque`, `std::set`, `std::map`.
    Searchable,
    /// `std::vector`, `std::deque`, `std::set`, `std::string`.
    Iterable,
    /// `std::set`, `std::map`.
    Keyed,
    /// `std::queue`, `std::stack`, `std::priority_queue`.
    Adaptor,
    /// `std::vector`, `std::deque`, `std::set`.
    Listable,
    /// `std::vector` and `std::deque` of a comparable element.
    Sortable,
    /// `char`, `int`, enums.
    Switchable,
    /// Types with the six comparison operators (types.md §1.3).
    Comparable,
    /// Types that can be printed in one go.
    Printable,
    /// User structs.
    Struct,
    /// User enums.
    Enum,
    /// `std::ifstream`.
    InputFile,
    /// `std::ofstream`.
    OutputFile,
    /// The four standard errors.
    Exception,
}

impl TypeClass {
    /// Every class, in declaration order.
    pub const ALL: [TypeClass; 24] = [
        Self::Any,
        Self::Number,
        Self::Integer,
        Self::Bool,
        Self::Text,
        Self::Value,
        Self::Collection,
        Self::Sequence,
        Self::Indexable,
        Self::Insertable,
        Self::Searchable,
        Self::Iterable,
        Self::Keyed,
        Self::Adaptor,
        Self::Listable,
        Self::Sortable,
        Self::Switchable,
        Self::Comparable,
        Self::Printable,
        Self::Struct,
        Self::Enum,
        Self::InputFile,
        Self::OutputFile,
        Self::Exception,
    ];

    /// The name catalog files write (`any`, `input_file`, …).
    pub const fn name(self) -> &'static str {
        match self {
            Self::Any => "any",
            Self::Number => "number",
            Self::Integer => "integer",
            Self::Bool => "bool",
            Self::Text => "text",
            Self::Value => "value",
            Self::Collection => "collection",
            Self::Sequence => "sequence",
            Self::Indexable => "indexable",
            Self::Insertable => "insertable",
            Self::Searchable => "searchable",
            Self::Iterable => "iterable",
            Self::Keyed => "keyed",
            Self::Adaptor => "adaptor",
            Self::Listable => "listable",
            Self::Sortable => "sortable",
            Self::Switchable => "switchable",
            Self::Comparable => "comparable",
            Self::Printable => "printable",
            Self::Struct => "struct",
            Self::Enum => "enum",
            Self::InputFile => "input_file",
            Self::OutputFile => "output_file",
            Self::Exception => "exception",
        }
    }
}

/// The type functions of catalog patterns (types.md §1.4, §4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TypeFn {
    /// What one item is: `T` of a container, `V` of a map, `char` of a
    /// string.
    Elem,
    /// What *item of* takes: `int` for a vector, deque or string, `K` for a
    /// map.
    Index,
    /// What *contains* and *remove (x)* look for: `T` of a vector, deque or
    /// set, `K` of a map.
    Key,
}

impl TypeFn {
    /// Every type function, in declaration order.
    pub const ALL: [TypeFn; 3] = [Self::Elem, Self::Index, Self::Key];

    /// The name patterns write (`elem`, `index`, `key`).
    pub const fn name(self) -> &'static str {
        match self {
            Self::Elem => "elem",
            Self::Index => "index",
            Self::Key => "key",
        }
    }

    /// The function applied to a type: `Error` for [`Type::Error`], `None`
    /// where it does not apply.
    pub fn apply(self, ty: &Type) -> Option<Type> {
        use LibType as L;
        let (lib, args) = match ty {
            Type::Error => return Some(Type::Error),
            Type::String => {
                return match self {
                    Self::Elem => Some(Type::Char),
                    Self::Index => Some(Type::Int),
                    Self::Key => None,
                };
            }
            Type::Lib(app) => (app.ty, app.args()),
            Type::Void | Type::Bool | Type::Char | Type::Int | Type::Double | Type::Named(_) => return None,
        };
        match (self, lib) {
            (Self::Elem, L::Vector | L::Deque | L::Set | L::Queue | L::Stack | L::PriorityQueue)
            | (Self::Key, L::Vector | L::Deque | L::Set | L::Map)
            | (Self::Index, L::Map) => args.first().cloned(),
            (Self::Elem, L::Map) => args.get(1).cloned(),
            (Self::Index, L::Vector | L::Deque) => Some(Type::Int),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;
    use serde_json::json;

    use super::*;

    fn sym(id: &str) -> SymbolId {
        SymbolId::new(id).unwrap()
    }

    fn lib(ty: LibType, args: Vec<Type>) -> Type {
        Type::lib(ty, args).unwrap()
    }

    fn vector(of: Type) -> Type {
        lib(LibType::Vector, vec![of])
    }

    fn map(k: Type, v: Type) -> Type {
        lib(LibType::Map, vec![k, v])
    }

    fn term(json: serde_json::Value) -> Result<TypeTerm, serde_json::Error> {
        serde_json::from_value(json)
    }

    /// Facts with a comparable, printable struct `sym_Point`, a struct
    /// `sym_Bare` that is neither, and an enum `sym_Color`.
    struct Facts;

    impl TypeFacts for Facts {
        fn user_type(&self, id: &SymbolId) -> Option<UserType<'_>> {
            let (name, kind, ticked) = match id.as_str() {
                "sym_Point" => ("Point", UserKind::Struct, true),
                "sym_Bare" => ("Bare", UserKind::Struct, false),
                "sym_Color" => ("Color", UserKind::Enum, false),
                _ => return None,
            };
            Some(UserType {
                name,
                kind,
                comparable: ticked,
                printable: ticked,
            })
        }
    }

    #[test]
    fn a_type_fits_in_24_bytes() {
        assert!(size_of::<Type>() <= 24, "{}", size_of::<Type>());
    }

    #[test]
    fn library_types_have_consistent_tables() {
        for ty in LibType::ALL {
            assert_eq!(LibType::from_cpp_name(ty.cpp_name()), Some(ty));
            assert_eq!(Header::standard(ty.header().as_str()), Some(ty.header()));
            assert_eq!(ty.label().matches('%').count(), ty.arity(), "{ty:?}");
            assert_eq!(ty.is_value(), ty.arity() > 0);
        }
        assert_eq!(LibType::from_cpp_name("std::list"), None);
        assert_eq!(LibType::from_cpp_name("vector"), None);
        assert_eq!(LibType::Map.arity(), 2);
        assert_eq!(LibType::PriorityQueue.header().as_str(), "<queue>");
        assert_eq!(LibType::OutOfRange.header().as_str(), "<stdexcept>");
        assert_eq!(LibType::Exception.header().as_str(), "<exception>");
    }

    #[test]
    fn lib_checks_arity_values_and_errors() {
        assert_eq!(
            Type::lib(LibType::Vector, vec![]),
            Err(TermError::Arity {
                name: "std::vector",
                expected: 1,
                found: 0
            })
        );
        assert!(matches!(
            Type::lib(LibType::Map, vec![Type::Int]),
            Err(TermError::Arity {
                expected: 2,
                found: 1,
                ..
            })
        ));
        assert!(matches!(
            Type::lib(LibType::IfStream, vec![Type::Int]),
            Err(TermError::Arity { expected: 0, .. })
        ));
        // An error argument gives an error, reported once.
        assert_eq!(Type::lib(LibType::Vector, vec![Type::Error]), Ok(Type::Error));
        assert_eq!(
            Type::lib(LibType::Map, vec![Type::Void, Type::Error]),
            Ok(Type::Error)
        );
        // Arguments are values.
        let stream = lib(LibType::IfStream, vec![]);
        let error = lib(LibType::RuntimeError, vec![]);
        for bad in [Type::Void, stream, error] {
            assert_eq!(Type::lib(LibType::Set, vec![bad]), Err(TermError::NotAValue));
        }
        for good in [Type::Int, Type::String, Type::Named(sym("s")), vector(Type::Int)] {
            assert!(Type::lib(LibType::Set, vec![good]).is_ok());
        }
    }

    #[test]
    fn lib_keeps_the_limits() {
        let mut ty = Type::Int;
        for _ in 0..Type::MAX_DEPTH {
            ty = vector(ty);
        }
        assert_eq!(
            Type::lib(LibType::Vector, vec![ty.clone()]),
            Err(TermError::TooDeep)
        );
        assert_eq!(ty.depth(), 8);
        // 31 names: a full tree of maps four levels deep, then one more name.
        let mut wide = Type::Int;
        for _ in 0..4 {
            wide = map(wide.clone(), wide);
        }
        assert_eq!(wide.names(), 31);
        assert_eq!(
            Type::lib(LibType::Vector, vec![wide.clone()]).map(|t| t.names()),
            Ok(32)
        );
        assert_eq!(
            Type::lib(LibType::Map, vec![Type::Int, wide]),
            Err(TermError::TooManyNames)
        );
    }

    #[test]
    fn terms_round_trip_through_canonical_json() {
        for json in [
            json!("int"),
            json!("std::string"),
            json!("auto"),
            json!("std::ifstream"),
            json!({"ref": "sym_Pt12"}),
            json!({"std::vector": "int"}),
            json!({"std::vector": {"std::vector": "int"}}),
            json!({"std::map": ["std::string", {"std::vector": {"ref": "sym_Pt12"}}]}),
            json!({"std::map": ["int", "int"]}),
        ] {
            let parsed = term(json.clone()).unwrap();
            assert_eq!(serde_json::to_value(&parsed).unwrap(), json);
            assert_eq!(term(serde_json::to_value(&parsed).unwrap()).unwrap(), parsed);
            // The key of a template or reference is its compact JSON.
            if !json.is_string() {
                assert_eq!(parsed.key(), serde_json::to_string(&json).unwrap());
            }
        }
    }

    #[test]
    fn terms_have_one_spelling() {
        for bad in [
            json!({"std::vector": ["int"]}),
            json!({"std::vector": []}),
            json!({"std::map": ["int", "int"], "std::set": "int"}),
            json!({"ref": "sym_x", "name": "x"}),
            json!({}),
            json!(["int", "int"]),
            json!(1),
            json!(null),
            json!({"std::vector": null}),
            json!({"std::vector": 1}),
            json!({"vector": "int"}),
            json!({"ref": "sym-x"}),
            json!({"ref": {"std::vector": "int"}}),
            json!(""),
            json!("std:: string"),
            json!("std::"),
            json!("::int"),
            json!("int  x"),
            json!("int "),
            json!("in-t"),
            json!("1int"),
            json!("ïnt"),
        ] {
            assert!(term(bad.clone()).is_err(), "{bad}");
        }
        assert!(term(json!("unsigned int")).is_ok());
        assert!(term(json!("a".repeat(64))).is_ok());
        assert!(term(json!("a".repeat(65))).is_err());
    }

    /// A full tree of maps `levels` deep: `2^levels - 1` maps and
    /// `2^levels` leaves.
    fn tree(levels: usize, leaf: &serde_json::Value) -> serde_json::Value {
        if levels == 0 {
            return leaf.clone();
        }
        json!({ "std::map": [tree(levels - 1, leaf), tree(levels - 1, leaf)] })
    }

    #[test]
    fn terms_keep_the_limits() {
        let nest = |levels: usize, mut json: serde_json::Value| {
            for _ in 0..levels {
                json = json!({ "std::vector": json });
            }
            json
        };
        assert!(term(nest(8, json!("int"))).is_ok());
        assert_eq!(
            term(nest(9, json!("int"))).unwrap_err().to_string(),
            TermError::TooDeep.to_string()
        );
        // A full tree of maps four levels deep has 15 maps and 16 leaves.
        let int = json!("int");
        let at_limit = term(nest(1, tree(4, &int))).unwrap();
        assert_eq!(at_limit.measure(), (5, 32));
        let mut over = tree(4, &int);
        over["std::map"][0]["std::map"][0]["std::map"][0]["std::map"][0] = json!({"std::vector": "int"});
        assert_eq!(
            term(nest(1, over)).unwrap_err().to_string(),
            TermError::TooManyNames.to_string()
        );
        assert_eq!(
            TypeTerm::app("std::map", vec![at_limit, TypeTerm::Name("int".into())]),
            Err(TermError::TooManyNames)
        );
    }

    #[test]
    fn term_constructors_check_shape_only() {
        assert_eq!(
            TypeTerm::name("std::nothing"),
            Ok(TypeTerm::Name("std::nothing".into()))
        );
        assert_eq!(TypeTerm::name("a::"), Err(TermError::BadName));
        assert_eq!(
            TypeTerm::app("vector", vec![TypeTerm::Name("int".into())]),
            Err(TermError::BadName)
        );
        assert_eq!(TypeTerm::app("std::vector", vec![]), Err(TermError::BadShape));
        let unknown = TypeTerm::app("std::list", vec![TypeTerm::Name("int".into())]).unwrap();
        assert_eq!(Type::from_term(&unknown), Err(TermError::Unknown));
    }

    #[test]
    fn keys_of_names_are_the_names() {
        for name in ["int", "std::string", "auto", "std::ofstream"] {
            assert_eq!(TypeTerm::name(name).unwrap().key(), name);
        }
        assert_eq!(TypeTerm::Ref(sym("sym_x")).key(), r#"{"ref":"sym_x"}"#);
        // Even a term made by hand gives valid JSON.
        assert_eq!(
            TypeTerm::App("a\"b\\".into(), vec![TypeTerm::Name("\n".into())].into()).key(),
            r#"{"a\"b\\":"\u000a"}"#
        );
    }

    #[test]
    fn refs_rename_and_templates() {
        let mut t = term(json!({"std::map": [{"ref": "a"}, {"std::vector": {"ref": "b"}}]})).unwrap();
        assert_eq!(t.refs().map(SymbolId::as_str).collect::<Vec<_>>(), ["a", "b"]);
        assert!(t.has_app());
        t.rename(&BTreeMap::from([(sym("b"), sym("c")), (sym("z"), sym("y"))]));
        assert_eq!(t.refs().map(SymbolId::as_str).collect::<Vec<_>>(), ["a", "c"]);
        assert!(!TypeTerm::Ref(sym("a")).has_app());
        assert_eq!(TypeTerm::name("int").unwrap().refs().count(), 0);
    }

    #[test]
    fn from_term_looks_names_up() {
        let ok = |json: serde_json::Value| Type::from_term(&term(json).unwrap());
        assert_eq!(ok(json!("int")), Ok(Type::Int));
        assert_eq!(ok(json!("std::string")), Ok(Type::String));
        assert_eq!(ok(json!("auto")), Err(TermError::Auto));
        assert_eq!(ok(json!("string")), Err(TermError::Unknown));
        assert_eq!(ok(json!("std::ifstream")), Ok(lib(LibType::IfStream, vec![])));
        assert!(matches!(
            ok(json!("std::vector")),
            Err(TermError::Arity { found: 0, .. })
        ));
        assert!(matches!(
            ok(json!({"std::ifstream": "int"})),
            Err(TermError::Arity { expected: 0, .. })
        ));
        assert_eq!(ok(json!({"std::vector": "void"})), Err(TermError::NotAValue));
        assert_eq!(ok(json!({"std::vector": "auto"})), Err(TermError::Auto));
        assert_eq!(ok(json!({"ref": "sym_x"})), Ok(Type::Named(sym("sym_x"))));
        assert_eq!(
            ok(json!({"std::map": ["std::string", {"std::vector": "int"}]})),
            Ok(map(Type::String, vector(Type::Int)))
        );
        // A term made by hand over the limits is refused before any lookup.
        let mut deep = TypeTerm::Name("int".into());
        for _ in 0..100 {
            deep = TypeTerm::App("std::vector".into(), vec![deep].into());
        }
        assert_eq!(Type::from_term(&deep), Err(TermError::TooDeep));
        assert_eq!(Type::Error.to_term(), None);
    }

    #[test]
    fn headers_of_types() {
        let headers = |ty: &Type| ty.headers().map(Header::as_str).collect::<Vec<_>>();
        assert!(headers(&Type::Int).is_empty());
        assert_eq!(headers(&Type::String), ["<string>"]);
        assert_eq!(
            headers(&map(Type::String, vector(Type::String))),
            ["<map>", "<string>", "<vector>"]
        );
        assert_eq!(
            headers(&lib(LibType::PriorityQueue, vec![Type::Int])),
            ["<queue>"]
        );
        assert!(headers(&Type::Named(sym("s"))).is_empty());
    }

    #[test]
    fn spellings_and_labels() {
        let point = Type::Named(sym("sym_Point"));
        let gone = Type::Named(sym("sym_Gone"));
        let cases = [
            (Type::Int, "int", "int"),
            (Type::String, "std::string", "string"),
            (Type::Void, "void", "void"),
            (Type::Error, "<error>", "unknown"),
            (
                vector(vector(Type::Int)),
                "std::vector<std::vector<int>>",
                "list of list of int",
            ),
            (
                map(Type::String, vector(Type::Int)),
                "std::map<std::string, std::vector<int>>",
                "map from string to list of int",
            ),
            (
                map(map(Type::Int, Type::Int), map(Type::Int, Type::Int)),
                "std::map<std::map<int, int>, std::map<int, int>>",
                "map from (map from int to int) to map from int to int",
            ),
            (vector(point.clone()), "std::vector<Point>", "list of Point"),
            (
                lib(LibType::PriorityQueue, vec![Type::Double]),
                "std::priority_queue<double>",
                "priority queue of double",
            ),
            (lib(LibType::IfStream, vec![]), "std::ifstream", "input file"),
            (
                lib(LibType::OutOfRange, vec![]),
                "std::out_of_range",
                "out-of-range error",
            ),
            (point, "Point", "Point"),
            (gone, "<unknown type>", "unknown type"),
        ];
        for (ty, cpp, label) in cases {
            assert_eq!(ty.cpp(&Facts).to_string(), cpp);
            assert_eq!(ty.label(&Facts).to_string(), label);
        }
    }

    #[test]
    fn class_membership() {
        use TypeClass as C;
        let facts = Facts;
        let point = Type::Named(sym("sym_Point"));
        let bare = Type::Named(sym("sym_Bare"));
        let color = Type::Named(sym("sym_Color"));
        let gone = Type::Named(sym("sym_Gone"));
        let set = |t| lib(LibType::Set, vec![t]);
        let queue = lib(LibType::Queue, vec![Type::Int]);
        let pq = lib(LibType::PriorityQueue, vec![Type::Int]);
        let stream = lib(LibType::OfStream, vec![]);
        let error = lib(LibType::InvalidArgument, vec![]);
        let is = |ty: &Type, class| ty.is_in(class, &facts);
        // Error is in every class, void in none; unknown user types are unknown.
        for class in TypeClass::ALL {
            assert_eq!(is(&Type::Error, class), Some(true));
            assert_eq!(is(&Type::Void, class), Some(false));
            assert_eq!(is(&gone, class), None);
        }
        let table: &[(&Type, C, Option<bool>)] = &[
            (&Type::Char, C::Number, Some(true)),
            (&Type::Bool, C::Number, Some(false)),
            (&Type::Double, C::Integer, Some(false)),
            (&Type::Char, C::Text, Some(true)),
            (&Type::Bool, C::Switchable, Some(false)),
            (&color, C::Switchable, Some(true)),
            (&point, C::Value, Some(true)),
            (&stream, C::Value, Some(false)),
            (&error, C::Any, Some(true)),
            (&queue, C::Collection, Some(true)),
            (&Type::String, C::Collection, Some(false)),
            (&Type::String, C::Iterable, Some(true)),
            (&map(Type::Int, Type::Int), C::Iterable, Some(false)),
            (&map(Type::Int, Type::Int), C::Indexable, Some(true)),
            (&map(Type::Int, Type::Int), C::Insertable, Some(false)),
            (&pq, C::Adaptor, Some(true)),
            (&pq, C::Comparable, Some(false)),
            (&queue, C::Comparable, Some(true)),
            (&set(point.clone()), C::Comparable, Some(true)),
            (&set(bare.clone()), C::Comparable, Some(false)),
            (&set(gone.clone()), C::Comparable, None),
            (&map(bare.clone(), gone.clone()), C::Comparable, Some(false)),
            (&vector(Type::Int), C::Sortable, Some(true)),
            (&vector(bare.clone()), C::Sortable, Some(false)),
            (&vector(gone.clone()), C::Sortable, None),
            (&set(Type::Int), C::Sortable, Some(false)),
            (&vector(Type::Int), C::Printable, Some(false)),
            (&point, C::Printable, Some(true)),
            (&bare, C::Printable, Some(false)),
            (&color, C::Printable, Some(true)),
            (&bare, C::Struct, Some(true)),
            (&color, C::Struct, Some(false)),
            (&color, C::Enum, Some(true)),
            (&stream, C::OutputFile, Some(true)),
            (&stream, C::InputFile, Some(false)),
            (&error, C::Exception, Some(true)),
            (&vector(Type::Int), C::Listable, Some(true)),
            (&set(Type::Int), C::Keyed, Some(true)),
            (&lib(LibType::Deque, vec![Type::Int]), C::Sequence, Some(true)),
            (&set(Type::Int), C::Searchable, Some(true)),
        ];
        for (ty, class, expected) in table {
            assert_eq!(is(ty, *class), *expected, "{ty:?} in {class:?}");
        }
    }

    #[test]
    fn type_functions() {
        use TypeFn as F;
        let m = map(Type::String, vector(Type::Int));
        let cases = [
            (F::Elem, vector(Type::Double), Some(Type::Double)),
            (F::Index, vector(Type::Double), Some(Type::Int)),
            (F::Key, vector(Type::Double), Some(Type::Double)),
            (F::Elem, m.clone(), Some(vector(Type::Int))),
            (F::Index, m.clone(), Some(Type::String)),
            (F::Key, m, Some(Type::String)),
            (F::Elem, lib(LibType::Set, vec![Type::Char]), Some(Type::Char)),
            (F::Index, lib(LibType::Set, vec![Type::Char]), None),
            (F::Elem, lib(LibType::Stack, vec![Type::Int]), Some(Type::Int)),
            (F::Key, lib(LibType::Stack, vec![Type::Int]), None),
            (F::Elem, Type::String, Some(Type::Char)),
            (F::Index, Type::String, Some(Type::Int)),
            (F::Key, Type::String, None),
            (F::Elem, Type::Int, None),
            (F::Elem, lib(LibType::IfStream, vec![]), None),
            (F::Key, Type::Error, Some(Type::Error)),
        ];
        for (f, ty, expected) in cases {
            assert_eq!(f.apply(&ty), expected, "{f:?} of {ty:?}");
        }
    }

    #[test]
    fn class_names_match_serde() {
        for class in TypeClass::ALL {
            assert_eq!(serde_json::to_value(class).unwrap(), json!(class.name()));
        }
        assert_eq!(TypeFn::ALL.map(TypeFn::name), ["elem", "index", "key"]);
    }

    #[test]
    fn samples_are_valid_and_distinct() {
        let samples = sample_types();
        assert_eq!(
            &samples[..7],
            [
                Type::Void,
                Type::Bool,
                Type::Char,
                Type::Int,
                Type::Double,
                Type::String,
                Type::Error
            ]
        );
        assert!(samples.len() >= 25, "{}", samples.len());
        let unique: BTreeSet<&Type> = samples.iter().collect();
        assert_eq!(unique.len(), samples.len());
        for ty in &samples[7..] {
            assert_eq!(Type::from_term(&ty.to_term().unwrap()).as_ref(), Ok(ty));
        }
    }

    #[test]
    #[allow(deprecated)]
    fn the_m2_helpers_stay() {
        assert_eq!(Type::String.cpp_name(), "std::string");
        assert_eq!(Type::Error.cpp_name(), "<error>");
        assert_eq!(vector(Type::Int).cpp_name(), "std::vector");
        assert_eq!(Type::Named(sym("s")).cpp_name(), "<user type>");
        assert_eq!(Type::Int.friendly_name(), "whole number");
        assert_eq!(vector(Type::Int).friendly_name(), "unknown");
        assert_eq!(Type::from_field("std::string"), Some(Type::String));
        assert_eq!(Type::from_field("std::vector"), None);
        assert!(Type::Char.is_numeric());
        assert!(!vector(Type::Int).is_numeric());
        assert_eq!(Type::String.header(), Some("<string>"));
        assert_eq!(vector(Type::String).header(), Some("<vector>"));
        assert_eq!(Type::Named(sym("s")).header(), None);
    }

    #[test]
    fn types_serialise_as_their_terms() {
        for ty in sample_types() {
            assert_eq!(serde_json::to_value(&ty).unwrap(), term_json(&ty));
            assert_eq!(serde_json::from_value::<Type>(term_json(&ty)).unwrap(), ty);
        }
        assert_eq!(serde_json::to_value(Type::String).unwrap(), json!("std::string"));
        assert_eq!(
            serde_json::to_value(vector(Type::Int)).unwrap(),
            json!({"std::vector": "int"})
        );
        for m2 in ["string", "lib", "named"] {
            assert!(serde_json::from_value::<Type>(json!(m2)).is_err(), "{m2}");
        }
    }

    fn term_json(ty: &Type) -> serde_json::Value {
        serde_json::to_value(TermForm(ty)).unwrap()
    }

    fn read_term_form(json: serde_json::Value) -> Result<Type, serde_json::Error> {
        term_form::deserialize(json)
    }

    #[test]
    fn the_term_form_is_the_term_and_error() {
        for ty in sample_types() {
            let expected = ty
                .to_term()
                .map_or(json!(ERROR_JSON), |term| serde_json::to_value(term).unwrap());
            assert_eq!(term_json(&ty), expected, "{ty:?}");
            assert_eq!(read_term_form(term_json(&ty)).unwrap(), ty);
            if let Some(term) = ty.to_term() {
                assert_eq!(
                    serde_json::to_string(&TermForm(&ty)).unwrap(),
                    serde_json::to_string(&term).unwrap()
                );
            }
        }
        assert_eq!(term_json(&Type::String), json!("std::string"));
        assert_eq!(term_json(&Type::Error), json!("error"));
        assert_eq!(term_json(&Type::Void), json!("void"));
        let nested = map(Type::String, vector(Type::Named(sym("sym_Pt12"))));
        assert_eq!(
            serde_json::to_string(&TermForm(&nested)).unwrap(),
            r#"{"std::map":["std::string",{"std::vector":{"ref":"sym_Pt12"}}]}"#
        );
        assert_eq!(
            term_json(&lib(LibType::IfStream, Vec::new())),
            json!("std::ifstream")
        );
    }

    #[test]
    fn the_term_form_reads_only_types() {
        for bad in [
            json!("string"),
            json!("auto"),
            json!("lib"),
            json!("named"),
            json!("std::vector"),
            json!("Error"),
            json!({"std::vector": "void"}),
            json!({"std::vector": "std::ofstream"}),
            json!({"std::vector": ["int"]}),
            json!({"std::vector": []}),
            json!({"std::map": "int"}),
            json!({"std::ifstream": "int"}),
            json!({"std::vector": "int", "std::set": "int"}),
            json!({"ref": "not an id"}),
            json!({"ref": "sym_x", "x": 1}),
            json!({}),
            json!(["int", "int"]),
            json!(1),
            json!(null),
            json!(true),
        ] {
            assert!(read_term_form(bad.clone()).is_err(), "{bad}");
        }
        let nest = |levels: usize| {
            let mut json = json!("int");
            for _ in 0..levels {
                json = json!({ "std::vector": json });
            }
            json
        };
        assert!(read_term_form(nest(Type::MAX_DEPTH)).is_ok());
        assert!(read_term_form(nest(Type::MAX_DEPTH + 1)).is_err());
        // 15 maps and 16 leaves are 31 names; a vector around them is 32.
        let int = json!("int");
        assert!(read_term_form(json!({"std::vector": tree(4, &int)})).is_ok());
        assert!(read_term_form(json!({"std::set": {"std::vector": tree(4, &int)}})).is_err());
    }

    /// A struct with a field that opts in to the term form.
    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Opted {
        #[serde(with = "term_form")]
        ty: Type,
    }

    #[test]
    fn a_field_opts_in_to_the_term_form() {
        let opted = Opted {
            ty: map(Type::String, Type::Int),
        };
        let json = serde_json::to_value(&opted).unwrap();
        assert_eq!(json, json!({"ty": {"std::map": ["std::string", "int"]}}));
        assert_eq!(serde_json::from_value::<Opted>(json).unwrap(), opted);
        assert!(serde_json::from_value::<Opted>(json!({"ty": "string"})).is_err());
    }

    fn arb_type() -> impl Strategy<Value = Type> {
        let leaf = prop_oneof![
            Just(Type::Bool),
            Just(Type::Char),
            Just(Type::Int),
            Just(Type::Double),
            Just(Type::String),
            "[a-z]{1,4}".prop_map(|id| Type::Named(SymbolId::new(&id).unwrap())),
        ];
        leaf.prop_recursive(4, 16, 2, |inner| {
            prop_oneof![
                (
                    prop::sample::select(vec![
                        LibType::Vector,
                        LibType::Deque,
                        LibType::Set,
                        LibType::Queue,
                        LibType::Stack,
                        LibType::PriorityQueue
                    ]),
                    inner.clone()
                )
                    .prop_map(|(ty, arg)| Type::lib(ty, vec![arg]).unwrap_or(Type::Int)),
                (inner.clone(), inner)
                    .prop_map(|(k, v)| Type::lib(LibType::Map, vec![k, v]).unwrap_or(Type::Int)),
            ]
        })
    }

    proptest! {
        #[test]
        fn from_term_and_to_term_round_trip(ty in arb_type()) {
            let term = ty.to_term().unwrap();
            prop_assert_eq!(Type::from_term(&term), Ok(ty.clone()));
            let json = serde_json::to_value(&term).unwrap();
            prop_assert_eq!(serde_json::from_value::<TypeTerm>(json.clone()).unwrap(), term.clone());
            prop_assert_eq!(term.key() == serde_json::to_string(&json).unwrap(), !json.is_string());
            prop_assert_eq!(&term_json(&ty), &json);
            prop_assert_eq!(read_term_form(json).unwrap(), ty);
        }
    }
}
