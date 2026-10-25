//! Catalog signatures (ADR-0017 §5, ADR-0018; the shapes are those of
//! `docs/plans/m3-contracts/types.md` §4).
//!
//! A catalog entry may declare type variables with their classes (`where`)
//! and type patterns on its output and parts (`type`). `b2c-catalog` builds a
//! [`BlockSig`] for each block into a [`SigTable`]; `b2c-lang` and the
//! editor's copy read it. [`TypeEnv`] binds a block's variables from its
//! parts and resolves its patterns; checking how a part fits needs the
//! analyser's conversion rule, so that step (`type_block`) lives in
//! `b2c-lang`.
//!
//! The grammar of a pattern, written in C++ syntax in TOML strings:
//!
//! ```ebnf
//! pattern = "any" | "number"                      (* M2: the analyser decides *)
//!         | var                                   (* T, K, V, C, E, S, T2: [A-Z][0-9]? *)
//!         | name                                  (* int, std::string, std::ifstream *)
//!         | name "<" pattern { "," pattern } ">"  (* std::vector<T>, std::map<K, V> *)
//!         | ( "elem" | "index" | "key" ) "(" pattern ")"
//!         | "field:" NAME | "symbol" | "symbol:" NAME
//!         | "param:" NAME | "member:" NAME | "result" ;
//! ```

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use crate::headers::Header;
use crate::types::{LibType, NoFacts, TermError, Type, TypeClass, TypeFacts, TypeFn, TypeTerm};

/// A type variable: an upper-case letter, optionally followed by a digit
/// (`T`, `K`, `T2`).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TypeVar(Box<str>);

impl TypeVar {
    /// The variable with this name, if it is one (`[A-Z][0-9]?`).
    pub fn new(name: &str) -> Option<TypeVar> {
        is_var(name).then(|| TypeVar(name.into()))
    }

    /// The variable's name.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for TypeVar {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

fn is_var(name: &str) -> bool {
    match name.as_bytes() {
        [letter] => letter.is_ascii_uppercase(),
        [letter, digit] => letter.is_ascii_uppercase() && digit.is_ascii_digit(),
        _ => false,
    }
}

/// Why a pattern does not parse.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PatternError {
    /// Longer than [`TypePattern::MAX_LEN`] bytes.
    #[error("a type pattern has at most 128 bytes")]
    TooLong,
    /// Nested more than [`TypePattern::MAX_DEPTH`] levels deep.
    #[error("a type pattern nests at most 8 levels deep")]
    TooDeep,
    /// Not in the grammar, at this byte offset.
    #[error("not a type pattern (at byte {at})")]
    Syntax {
        /// The byte offset of the problem.
        at: usize,
    },
    /// A type name that is not a type, or a library type with the wrong
    /// arguments.
    #[error("{0}")]
    Type(TermError),
}

/// A type pattern (types.md §4.2).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TypePattern {
    /// `any`: the analyser decides (M2).
    Any,
    /// `number`: `int` or `double` by the operands; outputs only (M2).
    Number,
    /// A fixed type: a name, or a template whose arguments are all fixed
    /// types (`std::vector<int>` parses to this).
    Type(Type),
    /// A type variable.
    Var(TypeVar),
    /// A library template with at least one argument that is not a fixed
    /// type: `std::vector<T>`, `std::map<K, V>`.
    Lib(LibType, Vec<TypePattern>),
    /// A type function applied to a pattern: `elem(C)`.
    Fn(TypeFn, Box<TypePattern>),
    /// `field:NAME`: the type chosen in the type field `NAME` (M2).
    Field(String),
    /// `symbol` (M2) or `symbol:NAME`: the type of the symbol in the block's
    /// one symbol field, or in the field `NAME`.
    Symbol(Option<String>),
    /// `param:NAME`: on a repeated input, the type of parameter `i` of the
    /// function in the field `NAME`.
    Param(String),
    /// `member:NAME`: on a repeated input, the type of field `i` of the
    /// struct in the field `NAME`.
    Member(String),
    /// `result`: what a `return` gives back where the block is.
    Result,
}

impl TypePattern {
    /// The longest pattern, in bytes.
    pub const MAX_LEN: usize = 128;
    /// The deepest nesting of templates and type functions.
    pub const MAX_DEPTH: usize = 8;

    /// Parses a pattern.
    ///
    /// # Errors
    /// [`PatternError::TooLong`] over 128 bytes, [`PatternError::TooDeep`]
    /// over 8 levels, [`PatternError::Syntax`] outside the grammar and
    /// [`PatternError::Type`] for an unknown type, a library type with the
    /// wrong number of arguments or an argument that is not a value type.
    pub fn parse(text: &str) -> Result<TypePattern, PatternError> {
        if text.len() > Self::MAX_LEN {
            return Err(PatternError::TooLong);
        }
        let mut parser = Parser {
            text: text.as_bytes(),
            at: 0,
        };
        let pattern = parser.pattern(0)?;
        parser.spaces();
        if parser.at == text.len() {
            Ok(pattern)
        } else {
            Err(PatternError::Syntax { at: parser.at })
        }
    }

    /// Every type variable in the pattern, in order (with repeats).
    pub fn vars(&self) -> impl Iterator<Item = &TypeVar> {
        let mut vars = Vec::new();
        self.collect_vars(&mut vars, true);
        vars.into_iter()
    }

    /// The variables a part with this pattern can bind: those outside every
    /// type function, each once, in order (types.md §4.3, rule 1).
    pub fn binding_vars(&self) -> Vec<&TypeVar> {
        let mut vars = Vec::new();
        self.collect_vars(&mut vars, false);
        let mut seen = BTreeSet::new();
        vars.retain(|var| seen.insert(*var));
        vars
    }

    fn collect_vars<'a>(&'a self, vars: &mut Vec<&'a TypeVar>, in_functions: bool) {
        match self {
            TypePattern::Var(var) => vars.push(var),
            TypePattern::Lib(_, args) => {
                for arg in args {
                    arg.collect_vars(vars, in_functions);
                }
            }
            TypePattern::Fn(_, arg) if in_functions => arg.collect_vars(vars, in_functions),
            TypePattern::Any
            | TypePattern::Number
            | TypePattern::Type(_)
            | TypePattern::Fn(..)
            | TypePattern::Field(_)
            | TypePattern::Symbol(_)
            | TypePattern::Param(_)
            | TypePattern::Member(_)
            | TypePattern::Result => {}
        }
    }

    /// Whether the pattern is a fixed type, which one catalog `default` can
    /// fit (types.md §4.3).
    pub fn is_fixed(&self) -> bool {
        matches!(self, TypePattern::Type(_))
    }
}

impl fmt::Display for TypePattern {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TypePattern::Any => f.write_str("any"),
            TypePattern::Number => f.write_str("number"),
            TypePattern::Type(ty) => write!(f, "{}", ty.cpp(&NoFacts)),
            TypePattern::Var(var) => write!(f, "{var}"),
            TypePattern::Lib(ty, args) => {
                write!(f, "{}<", ty.cpp_name())?;
                for (i, arg) in args.iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{arg}")?;
                }
                f.write_str(">")
            }
            TypePattern::Fn(function, arg) => write!(f, "{}({arg})", function.name()),
            TypePattern::Field(name) => write!(f, "field:{name}"),
            TypePattern::Symbol(None) => f.write_str("symbol"),
            TypePattern::Symbol(Some(name)) => write!(f, "symbol:{name}"),
            TypePattern::Param(name) => write!(f, "param:{name}"),
            TypePattern::Member(name) => write!(f, "member:{name}"),
            TypePattern::Result => f.write_str("result"),
        }
    }
}

/// A recursive-descent parser over a pattern's bytes.
struct Parser<'a> {
    text: &'a [u8],
    at: usize,
}

impl Parser<'_> {
    fn spaces(&mut self) {
        while self.text.get(self.at) == Some(&b' ') {
            self.at += 1;
        }
    }

    /// Takes `byte` (after spaces) if it is next.
    fn eat(&mut self, byte: u8) -> bool {
        self.spaces();
        if self.text.get(self.at) == Some(&byte) {
            self.at += 1;
            true
        } else {
            false
        }
    }

    fn expect(&mut self, byte: u8) -> Result<(), PatternError> {
        if self.eat(byte) {
            Ok(())
        } else {
            Err(PatternError::Syntax { at: self.at })
        }
    }

    /// A word: letters, digits, `_` and `:`.
    fn word(&mut self) -> &str {
        self.spaces();
        let start = self.at;
        while self
            .text
            .get(self.at)
            .is_some_and(|b| b.is_ascii_alphanumeric() || *b == b'_' || *b == b':')
        {
            self.at += 1;
        }
        // Only ASCII bytes were taken.
        std::str::from_utf8(self.text.get(start..self.at).unwrap_or_default()).unwrap_or_default()
    }

    fn pattern(&mut self, depth: usize) -> Result<TypePattern, PatternError> {
        let start = self.at;
        let word = self.word().to_owned();
        let syntax = PatternError::Syntax { at: start };
        if !word.contains("::")
            && let Some((form, name)) = word.split_once(':')
        {
            if !is_part_name(name) {
                return Err(syntax);
            }
            let name = name.to_owned();
            return match form {
                "field" => Ok(TypePattern::Field(name)),
                "symbol" => Ok(TypePattern::Symbol(Some(name))),
                "param" => Ok(TypePattern::Param(name)),
                "member" => Ok(TypePattern::Member(name)),
                _ => Err(syntax),
            };
        }
        let function = TypeFn::ALL.into_iter().find(|f| f.name() == word);
        match word.as_str() {
            "" => Err(syntax),
            "any" => Ok(TypePattern::Any),
            "number" => Ok(TypePattern::Number),
            "symbol" => Ok(TypePattern::Symbol(None)),
            "result" => Ok(TypePattern::Result),
            _ if is_var(&word) => Ok(TypePattern::Var(TypeVar(word.into()))),
            _ if function.is_some() && self.eat(b'(') => {
                let depth = nested(depth)?;
                let arg = self.pattern(depth)?;
                self.expect(b')')?;
                Ok(TypePattern::Fn(function.unwrap_or(TypeFn::Elem), Box::new(arg)))
            }
            _ if self.eat(b'<') => {
                let depth = nested(depth)?;
                let ty = LibType::from_cpp_name(&word).ok_or(PatternError::Type(TermError::Unknown))?;
                let mut args = vec![self.pattern(depth)?];
                while self.eat(b',') {
                    args.push(self.pattern(depth)?);
                }
                self.expect(b'>')?;
                template(ty, args)
            }
            _ => match TypeTerm::name(&word).and_then(|term| Type::from_term(&term)) {
                Ok(ty) => Ok(TypePattern::Type(ty)),
                Err(TermError::BadName) => Err(syntax),
                Err(error) => Err(PatternError::Type(error)),
            },
        }
    }
}

/// One more level of nesting, within the limit.
fn nested(depth: usize) -> Result<usize, PatternError> {
    if depth >= TypePattern::MAX_DEPTH {
        Err(PatternError::TooDeep)
    } else {
        Ok(depth + 1)
    }
}

/// A template pattern: a fixed type when every argument is one.
fn template(ty: LibType, args: Vec<TypePattern>) -> Result<TypePattern, PatternError> {
    if args.len() != ty.arity() {
        return Err(PatternError::Type(TermError::Arity {
            name: ty.cpp_name(),
            expected: ty.arity(),
            found: args.len(),
        }));
    }
    if args.iter().all(TypePattern::is_fixed) {
        let types = args
            .into_iter()
            .filter_map(|arg| match arg {
                TypePattern::Type(ty) => Some(ty),
                _ => None,
            })
            .collect();
        return Type::lib(ty, types)
            .map(TypePattern::Type)
            .map_err(PatternError::Type);
    }
    // A fixed argument must still be a value type.
    let bad = args.iter().any(|arg| {
        matches!(arg, TypePattern::Type(arg_ty) if Type::lib(LibType::Vector, vec![arg_ty.clone()]) == Err(TermError::NotAValue))
    });
    if bad {
        return Err(PatternError::Type(TermError::NotAValue));
    }
    Ok(TypePattern::Lib(ty, args))
}

/// A field or input name in a pattern: `[A-Za-z_][A-Za-z0-9_]*`, 1 to 64
/// characters.
fn is_part_name(name: &str) -> bool {
    let mut bytes = name.bytes();
    name.len() <= 64
        && bytes.next().is_some_and(|b| b.is_ascii_alphabetic() || b == b'_')
        && bytes.all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

/// A part of a block that has a pattern, by its catalog name.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PartName {
    /// A field.
    Field(String),
    /// A value input (a repeated input part by part: `ITEM2`).
    Input(String),
}

/// What a value input expects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputSig {
    /// The class its value must be in (`check`, with M2's meaning for
    /// `bool`, `integer` and `number`: what converts).
    pub check: TypeClass,
    /// Its `type` pattern, if any.
    pub pattern: Option<TypePattern>,
    /// `assign = true`: assigned, not initialised (`char` fits
    /// `std::string`); never binds.
    pub assign: bool,
}

/// A block's signature (types.md §4.4).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BlockSig {
    /// Each type variable and its class (`where`).
    pub vars: BTreeMap<TypeVar, TypeClass>,
    /// The output's pattern; `None` for a statement.
    pub output: Option<TypePattern>,
    /// The patterns of `type`, `symbol_ref` and `symbol_decl` fields.
    pub fields: BTreeMap<String, TypePattern>,
    /// The value inputs, a repeated input under its base name.
    pub inputs: BTreeMap<String, InputSig>,
    /// The parts that have a pattern, in catalog order: the order they bind
    /// in.
    pub order: Vec<PartName>,
    /// The output's variable when no part binds it: the block takes its type
    /// from its place.
    pub place_var: Option<TypeVar>,
    /// `symbol_ref` fields that list the members of the user type bound to a
    /// variable.
    pub members: BTreeMap<String, TypeVar>,
    /// The fields of `fields` that are `symbol_ref` fields; the others not in
    /// `symbol_decls` are `type` fields. Typing tells the kinds apart
    /// (slots.md §2): a reference is never converted, and a declaration
    /// never binds.
    pub symbol_refs: BTreeSet<String>,
    /// The fields of `fields` that are `symbol_decl` fields: they declare a
    /// symbol of their resolved pattern and bind nothing.
    pub symbol_decls: BTreeSet<String>,
    /// The inputs of `inputs` that are repeated (`ITEM0`, `ITEM1`, …); the
    /// others have one part, named as the input.
    pub repeated: BTreeSet<String>,
    /// The headers every form of the block needs.
    pub headers: Vec<Header>,
    /// The headers a dropdown option adds, by field, then option.
    pub option_headers: BTreeMap<String, BTreeMap<String, Vec<Header>>>,
}

/// Every block's signature, by block type ID. Built by `b2c-catalog` when
/// the catalog loads.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SigTable {
    sigs: BTreeMap<String, BlockSig>,
}

impl SigTable {
    /// A table without signatures, for tests: no block is typed by patterns,
    /// and the blocks the analyser always types by them (M2's `create`, `set`,
    /// calls and `return`) check nothing of their values' types.
    pub const fn empty() -> SigTable {
        SigTable {
            sigs: BTreeMap::new(),
        }
    }

    /// The signature of a block type, if it has one.
    pub fn get(&self, block_type: &str) -> Option<&BlockSig> {
        self.sigs.get(block_type)
    }

    /// Adds or replaces the signature of a block type.
    pub fn insert(&mut self, block_type: impl Into<String>, sig: BlockSig) {
        self.sigs.insert(block_type.into(), sig);
    }

    /// Every block type with its signature, sorted by block type.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &BlockSig)> {
        self.sigs.iter().map(|(id, sig)| (id.as_str(), sig))
    }

    /// The number of signatures.
    pub fn len(&self) -> usize {
        self.sigs.len()
    }

    /// Whether the table has no signature.
    pub fn is_empty(&self) -> bool {
        self.sigs.is_empty()
    }
}

/// What one part of a block is when the block is typed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PartType {
    /// Nothing chosen or connected, or a disabled block.
    Empty,
    /// `auto` in a type field.
    Auto,
    /// A value that takes its type from its place: typed after the block.
    FromPlace,
    /// A known type. `Type::Error` binds the variable to `Error` and fits
    /// everything.
    Known(Type),
    /// Not known to the caller (the editor, for something the analysis has
    /// not seen).
    Unknown,
}

/// The parts of the block being typed. A repeated input is asked part by
/// part (`ITEM2`).
pub trait BlockParts {
    /// A field.
    fn field(&self, name: &str) -> PartType;
    /// A value input part.
    fn input(&self, part: &str) -> PartType;
    /// How many parts the repeated input `base` has now.
    fn count(&self, base: &str) -> usize;
    /// `param:F`: parameter `index` of the function chosen in field `field`.
    fn param(&self, field: &str, index: usize) -> PartType;
    /// `member:F`: field `index` of the struct chosen in field `field`.
    fn member(&self, field: &str, index: usize) -> PartType;
    /// A `members` field: the chosen member's type when it is a member of
    /// `owner`.
    fn chosen_member(&self, field: &str, owner: &Type) -> PartType;
    /// `result`: what a `return` gives back where the block is.
    fn result(&self) -> PartType;
}

/// How a binding went.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Fit {
    /// It fits, or the part binds nothing.
    Fits,
    /// Class membership is not known (a user type the facts do not know).
    Unknown,
    /// The type is outside the variable's class.
    OutsideClass,
    /// The type does not have the pattern's shape (`std::set` for
    /// `std::vector<T>`).
    BadShape,
}

/// A resolved pattern.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolution {
    /// The pattern's type.
    Type(Type),
    /// Something it depends on is empty, unknown or already wrong: `Error`
    /// fits.
    Missing,
    /// Nothing could give it a type (no pattern, or only `auto` and
    /// place-typed values).
    Open,
}

/// The type variables of one block being typed: their classes, what is bound
/// and by which part (types.md §4.3, rules 1 to 3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeEnv {
    classes: BTreeMap<TypeVar, TypeClass>,
    place_var: Option<TypeVar>,
    members: BTreeMap<String, TypeVar>,
    /// Each bound variable, its type and its binder (`None`: the place).
    bound: BTreeMap<TypeVar, (Type, Option<PartName>)>,
    /// Unbound variables that an empty part could have bound.
    empty: BTreeSet<TypeVar>,
}

impl TypeEnv {
    /// The variables of `sig`, all unbound.
    pub fn new(sig: &BlockSig) -> TypeEnv {
        TypeEnv {
            classes: sig.vars.clone(),
            place_var: sig.place_var.clone(),
            members: sig.members.clone(),
            bound: BTreeMap::new(),
            empty: BTreeSet::new(),
        }
    }

    /// Unifies `pattern` with `part` (rules 1 and 2) and binds the pattern's
    /// variables that are outside every type function and still unbound: a
    /// variable already bound is left alone (the first binding wins; checking
    /// the part against it is the caller's).
    ///
    /// * `Known(t)`: a bare variable takes `t` when `t` is in its class; a
    ///   template takes its arguments from a type of the same template. A
    ///   concrete type and a type function bind nothing. When the binding
    ///   fails ([`Fit::OutsideClass`], [`Fit::BadShape`]) or membership is
    ///   not known ([`Fit::Unknown`]), the variables are bound to `Error`.
    /// * `Known(Error)` and `Unknown` bind the variables to `Error`
    ///   ([`Fit::Fits`] and [`Fit::Unknown`]).
    /// * `Empty`, `Auto` and `FromPlace` bind nothing ([`Fit::Fits`]); an
    ///   empty part makes its variables resolve to [`Resolution::Missing`]
    ///   while nothing binds them.
    ///
    /// The caller never passes an `assign` input or a `members` field: they
    /// never bind.
    pub fn bind(
        &mut self,
        pattern: &TypePattern,
        part: &PartType,
        by: PartName,
        facts: &dyn TypeFacts,
    ) -> Fit {
        let ty = match part {
            PartType::Empty => {
                for var in pattern.binding_vars() {
                    if !self.bound.contains_key(var) {
                        self.empty.insert(var.clone());
                    }
                }
                return Fit::Fits;
            }
            PartType::Auto | PartType::FromPlace => return Fit::Fits,
            PartType::Unknown => {
                self.bind_error(pattern, &by);
                return Fit::Unknown;
            }
            PartType::Known(Type::Error) => {
                self.bind_error(pattern, &by);
                return Fit::Fits;
            }
            PartType::Known(ty) => ty,
        };
        let mut found = BTreeMap::new();
        let fit = self.unify(pattern, ty, facts, &mut found);
        if fit == Fit::Fits {
            let by = Some(by);
            for (var, ty) in found {
                self.bound.entry(var).or_insert_with(|| (ty, by.clone()));
            }
        } else {
            self.bind_error(pattern, &by);
        }
        fit
    }

    /// Binds the place-typed block's variable to its place's type (rule 3).
    /// `None` leaves it unbound. A place outside the variable's class binds
    /// `Error` ([`Fit::OutsideClass`]), and so does a place whose membership
    /// is not known ([`Fit::Unknown`]).
    pub fn bind_place(&mut self, place: Option<&Type>, facts: &dyn TypeFacts) -> Fit {
        let (Some(var), Some(place)) = (self.place_var.clone(), place) else {
            return Fit::Fits;
        };
        if self.bound.contains_key(&var) {
            return Fit::Fits;
        }
        let (ty, fit) = match place.is_in(self.class(&var), facts) {
            Some(true) => (place.clone(), Fit::Fits),
            Some(false) => (Type::Error, Fit::OutsideClass),
            None => (Type::Error, Fit::Unknown),
        };
        self.bound.insert(var, (ty, None));
        fit
    }

    /// Resolves a pattern against what is bound. `index` is the part's index
    /// for `param:` and `member:`.
    ///
    /// An unbound variable is [`Resolution::Missing`] when an empty part
    /// could have bound it, and [`Resolution::Open`] otherwise; a type
    /// function that does not apply is `Missing`; `field:`, `symbol`,
    /// `param:`, `member:` and `result` are the part's type when it is
    /// known, and `Missing` otherwise. `symbol` without a field name (M2),
    /// which the catalog names when it builds the signature, is `Missing`.
    pub fn resolve(&self, pattern: &TypePattern, parts: &dyn BlockParts, index: Option<usize>) -> Resolution {
        fn known(part: PartType) -> Resolution {
            match part {
                PartType::Known(ty) => Resolution::Type(ty),
                PartType::Empty | PartType::Auto | PartType::FromPlace | PartType::Unknown => {
                    Resolution::Missing
                }
            }
        }
        match pattern {
            TypePattern::Any | TypePattern::Number => Resolution::Open,
            TypePattern::Type(ty) => Resolution::Type(ty.clone()),
            TypePattern::Var(var) => match self.bound.get(var) {
                Some((ty, _)) => Resolution::Type(ty.clone()),
                None if self.empty.contains(var) => Resolution::Missing,
                None => Resolution::Open,
            },
            TypePattern::Lib(ty, args) => {
                let mut types = Vec::new();
                let (mut missing, mut open) = (false, false);
                for arg in args {
                    match self.resolve(arg, parts, index) {
                        Resolution::Type(ty) => types.push(ty),
                        Resolution::Missing => missing = true,
                        Resolution::Open => open = true,
                    }
                }
                if missing {
                    Resolution::Missing
                } else if open {
                    Resolution::Open
                } else {
                    Type::lib(*ty, types).map_or(Resolution::Missing, Resolution::Type)
                }
            }
            TypePattern::Fn(function, arg) => match self.resolve(arg, parts, index) {
                Resolution::Type(ty) => function.apply(&ty).map_or(Resolution::Missing, Resolution::Type),
                other => other,
            },
            TypePattern::Field(name) => known(parts.field(name)),
            TypePattern::Symbol(None) => Resolution::Missing,
            TypePattern::Symbol(Some(name)) => match self.members.get(name) {
                Some(var) => match self.bound.get(var) {
                    Some((owner, _)) if *owner != Type::Error => known(parts.chosen_member(name, owner)),
                    _ => Resolution::Missing,
                },
                None => known(parts.field(name)),
            },
            TypePattern::Param(field) => index.map_or(Resolution::Missing, |i| known(parts.param(field, i))),
            TypePattern::Member(field) => {
                index.map_or(Resolution::Missing, |i| known(parts.member(field, i)))
            }
            TypePattern::Result => known(parts.result()),
        }
    }

    /// A bound variable's type and the part that bound it (`None` when the
    /// place did).
    pub fn get(&self, var: &TypeVar) -> Option<(&Type, Option<&PartName>)> {
        self.bound.get(var).map(|(ty, by)| (ty, by.as_ref()))
    }

    /// A variable's class (`any` for a variable the signature does not
    /// declare).
    fn class(&self, var: &TypeVar) -> TypeClass {
        self.classes.get(var).copied().unwrap_or(TypeClass::Any)
    }

    /// Binds the pattern's unbound variables to `Error`.
    fn bind_error(&mut self, pattern: &TypePattern, by: &PartName) {
        for var in pattern.binding_vars() {
            self.bound
                .entry(var.clone())
                .or_insert_with(|| (Type::Error, Some(by.clone())));
        }
    }

    /// Unifies a pattern with a known type that is not `Error`, collecting
    /// the new bindings in `found`; the worst fit of its parts.
    fn unify(
        &self,
        pattern: &TypePattern,
        ty: &Type,
        facts: &dyn TypeFacts,
        found: &mut BTreeMap<TypeVar, Type>,
    ) -> Fit {
        match pattern {
            TypePattern::Var(var) => {
                if self.bound.contains_key(var) {
                    return Fit::Fits;
                }
                if let Some(earlier) = found.get(var) {
                    return if earlier == ty { Fit::Fits } else { Fit::BadShape };
                }
                match ty.is_in(self.class(var), facts) {
                    Some(true) => {
                        found.insert(var.clone(), ty.clone());
                        Fit::Fits
                    }
                    Some(false) => Fit::OutsideClass,
                    None => Fit::Unknown,
                }
            }
            TypePattern::Lib(lib, args) => match ty {
                Type::Lib(app) if app.ty() == *lib && app.args().len() == args.len() => args
                    .iter()
                    .zip(app.args())
                    .map(|(arg, ty)| self.unify_argument(arg, ty, facts, found))
                    .max()
                    .unwrap_or(Fit::Fits),
                _ => Fit::BadShape,
            },
            TypePattern::Any
            | TypePattern::Number
            | TypePattern::Type(_)
            | TypePattern::Fn(..)
            | TypePattern::Field(_)
            | TypePattern::Symbol(_)
            | TypePattern::Param(_)
            | TypePattern::Member(_)
            | TypePattern::Result => Fit::Fits,
        }
    }

    /// Unifies a template argument: a fixed type there must be the same
    /// type, since library types convert only to themselves.
    fn unify_argument(
        &self,
        pattern: &TypePattern,
        ty: &Type,
        facts: &dyn TypeFacts,
        found: &mut BTreeMap<TypeVar, Type>,
    ) -> Fit {
        match pattern {
            TypePattern::Type(fixed) if fixed != ty => Fit::BadShape,
            _ => self.unify(pattern, ty, facts, found),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::SymbolId;
    use crate::types::{UserKind, UserType};

    fn var(name: &str) -> TypeVar {
        TypeVar::new(name).unwrap()
    }

    fn pattern(text: &str) -> TypePattern {
        TypePattern::parse(text).unwrap()
    }

    fn vector(of: Type) -> Type {
        Type::lib(LibType::Vector, vec![of]).unwrap()
    }

    fn input(name: &str) -> PartName {
        PartName::Input(name.to_owned())
    }

    /// Facts that know a struct `sym_Point` that is not comparable.
    struct Facts;

    impl TypeFacts for Facts {
        fn user_type(&self, id: &SymbolId) -> Option<UserType<'_>> {
            (id.as_str() == "sym_Point").then_some(UserType {
                name: "Point",
                kind: UserKind::Struct,
                comparable: false,
                printable: false,
            })
        }
    }

    /// Parts from a table: fields and inputs by name, parameters and
    /// members by index, the result and the members' answer.
    #[derive(Default)]
    struct Parts {
        fields: BTreeMap<String, PartType>,
        params: Vec<PartType>,
        result: Option<PartType>,
        member: Option<PartType>,
    }

    impl BlockParts for Parts {
        fn field(&self, name: &str) -> PartType {
            self.fields.get(name).cloned().unwrap_or(PartType::Empty)
        }
        fn input(&self, _: &str) -> PartType {
            PartType::Empty
        }
        fn count(&self, _: &str) -> usize {
            self.params.len()
        }
        fn param(&self, _: &str, index: usize) -> PartType {
            self.params.get(index).cloned().unwrap_or(PartType::Empty)
        }
        fn member(&self, field: &str, index: usize) -> PartType {
            self.param(field, index)
        }
        fn chosen_member(&self, _: &str, _: &Type) -> PartType {
            self.member.clone().unwrap_or(PartType::Unknown)
        }
        fn result(&self) -> PartType {
            self.result.clone().unwrap_or(PartType::Empty)
        }
    }

    fn sig(vars: &[(&str, TypeClass)]) -> BlockSig {
        BlockSig {
            vars: vars.iter().map(|(name, class)| (var(name), *class)).collect(),
            ..BlockSig::default()
        }
    }

    #[test]
    fn variables() {
        for good in ["T", "K", "V", "T2", "S0"] {
            assert_eq!(var(good).as_str(), good);
            assert_eq!(var(good).to_string(), good);
        }
        for bad in ["", "t", "TT", "T22", "2", "T_", "Ä"] {
            assert_eq!(TypeVar::new(bad), None, "{bad}");
        }
    }

    #[test]
    fn patterns_parse_and_print() {
        let cases = [
            ("any", TypePattern::Any),
            ("number", TypePattern::Number),
            ("int", TypePattern::Type(Type::Int)),
            ("std::string", TypePattern::Type(Type::String)),
            (
                "std::ifstream",
                TypePattern::Type(Type::lib(LibType::IfStream, vec![]).unwrap()),
            ),
            ("std::vector<int>", TypePattern::Type(vector(Type::Int))),
            ("T2", TypePattern::Var(var("T2"))),
            (
                "std::vector<T>",
                TypePattern::Lib(LibType::Vector, vec![TypePattern::Var(var("T"))]),
            ),
            (
                "std::map<K, V>",
                TypePattern::Lib(
                    LibType::Map,
                    vec![TypePattern::Var(var("K")), TypePattern::Var(var("V"))],
                ),
            ),
            (
                "std::map<std::string, std::vector<T>>",
                TypePattern::Lib(
                    LibType::Map,
                    vec![
                        TypePattern::Type(Type::String),
                        TypePattern::Lib(LibType::Vector, vec![TypePattern::Var(var("T"))]),
                    ],
                ),
            ),
            (
                "elem(C)",
                TypePattern::Fn(TypeFn::Elem, Box::new(TypePattern::Var(var("C")))),
            ),
            (
                "index(std::vector<T>)",
                TypePattern::Fn(
                    TypeFn::Index,
                    Box::new(TypePattern::Lib(
                        LibType::Vector,
                        vec![TypePattern::Var(var("T"))],
                    )),
                ),
            ),
            ("field:TYPE", TypePattern::Field("TYPE".into())),
            ("symbol", TypePattern::Symbol(None)),
            ("symbol:MEMBER", TypePattern::Symbol(Some("MEMBER".into()))),
            ("param:FUNC", TypePattern::Param("FUNC".into())),
            ("member:TYPE", TypePattern::Member("TYPE".into())),
            ("result", TypePattern::Result),
        ];
        for (text, expected) in cases {
            let parsed = pattern(text);
            assert_eq!(parsed, expected, "{text}");
            assert_eq!(parsed.to_string(), text);
            assert_eq!(pattern(&parsed.to_string()), parsed);
        }
        // Spaces around punctuation are allowed and printed canonically.
        assert_eq!(pattern(" std::map< K ,V > ").to_string(), "std::map<K, V>");
        assert_eq!(pattern("elem ( C )").to_string(), "elem(C)");
    }

    #[test]
    fn patterns_refuse_bad_input() {
        let syntax = [
            "",
            "  ",
            "T T",
            "std::vector<",
            "std::vector<T",
            "std::vector<T>>",
            "elem(C",
            "elem()",
            "field:",
            "field:A-B",
            "param: X",
            "<T>",
            "T,",
            "(T)",
            "symbol:",
            "std::vector<T>x",
            "int*",
            "fields:X",
            "field:1X",
            "::int",
            "std::",
            "a::b::",
        ];
        for text in syntax {
            assert!(
                matches!(TypePattern::parse(text), Err(PatternError::Syntax { .. })),
                "{text}"
            );
        }
        let typed = [
            ("string", TermError::Unknown),
            ("elem", TermError::Unknown),
            ("std::list<T>", TermError::Unknown),
            ("auto", TermError::Auto),
            ("std::vector<void>", TermError::NotAValue),
            ("std::map<std::ifstream, V>", TermError::NotAValue),
        ];
        for (text, error) in typed {
            assert_eq!(TypePattern::parse(text), Err(PatternError::Type(error)), "{text}");
        }
        assert!(matches!(
            TypePattern::parse("std::map<K>"),
            Err(PatternError::Type(TermError::Arity {
                expected: 2,
                found: 1,
                ..
            }))
        ));
        assert!(matches!(
            TypePattern::parse("std::vector"),
            Err(PatternError::Type(TermError::Arity { found: 0, .. }))
        ));
        let mut deep = String::from("T");
        for _ in 0..8 {
            deep = format!("elem({deep})");
        }
        assert!(TypePattern::parse(&deep).is_ok());
        assert_eq!(
            TypePattern::parse(&format!("key({deep})")),
            Err(PatternError::TooDeep)
        );
        assert_eq!(TypePattern::parse(&" ".repeat(129)), Err(PatternError::TooLong));
    }

    #[test]
    fn variables_of_patterns() {
        let p = pattern("std::map<K, std::vector<elem(C)>>");
        assert_eq!(p.vars().map(TypeVar::as_str).collect::<Vec<_>>(), ["K", "C"]);
        assert_eq!(
            p.binding_vars()
                .into_iter()
                .map(TypeVar::as_str)
                .collect::<Vec<_>>(),
            ["K"]
        );
        assert_eq!(pattern("std::map<T, T>").binding_vars().len(), 1);
        assert!(pattern("std::vector<int>").is_fixed());
        assert!(!pattern("std::vector<T>").is_fixed());
        assert!(!pattern("result").is_fixed());
    }

    #[test]
    fn binds_from_a_template_and_keeps_the_first_binding() {
        let mut env = TypeEnv::new(&sig(&[("T", TypeClass::Value)]));
        let list = pattern("std::vector<T>");
        let fit = env.bind(&list, &PartType::Known(vector(Type::Int)), input("A"), &Facts);
        assert_eq!(fit, Fit::Fits);
        assert_eq!(env.get(&var("T")), Some((&Type::Int, Some(&input("A")))));
        // A later part that disagrees does not rebind.
        let fit = env.bind(&pattern("T"), &PartType::Known(Type::Double), input("B"), &Facts);
        assert_eq!(fit, Fit::Fits);
        assert_eq!(env.get(&var("T")).map(|(ty, _)| ty), Some(&Type::Int));
        let parts = Parts::default();
        assert_eq!(
            env.resolve(&list, &parts, None),
            Resolution::Type(vector(Type::Int))
        );
        assert_eq!(
            env.resolve(&pattern("elem(std::vector<T>)"), &parts, None),
            Resolution::Type(Type::Int)
        );
    }

    #[test]
    fn functions_bind_nothing() {
        let mut env = TypeEnv::new(&sig(&[("C", TypeClass::Indexable)]));
        let fit = env.bind(
            &pattern("index(C)"),
            &PartType::Known(Type::Int),
            input("I"),
            &Facts,
        );
        assert_eq!(fit, Fit::Fits);
        assert_eq!(env.get(&var("C")), None);
        assert_eq!(
            env.resolve(&pattern("elem(C)"), &Parts::default(), None),
            Resolution::Open
        );
        // Bound later, the function resolves; on a type it does not apply to, Missing.
        env.bind(
            &pattern("C"),
            &PartType::Known(vector(Type::Char)),
            input("L"),
            &Facts,
        );
        assert_eq!(
            env.resolve(&pattern("index(C)"), &Parts::default(), None),
            Resolution::Type(Type::Int)
        );
        let mut env = TypeEnv::new(&sig(&[("C", TypeClass::Any)]));
        env.bind(&pattern("C"), &PartType::Known(Type::Int), input("L"), &Facts);
        assert_eq!(
            env.resolve(&pattern("elem(C)"), &Parts::default(), None),
            Resolution::Missing
        );
    }

    #[test]
    fn outside_class_and_bad_shape_are_told_apart_and_bind_error() {
        let mut env = TypeEnv::new(&sig(&[("T", TypeClass::Comparable)]));
        let set = pattern("std::vector<T>");
        let point = Type::Named(SymbolId::new("sym_Point").unwrap());
        assert_eq!(
            env.bind(&set, &PartType::Known(vector(point)), input("A"), &Facts),
            Fit::OutsideClass
        );
        assert_eq!(env.get(&var("T")).map(|(ty, _)| ty), Some(&Type::Error));

        let mut env = TypeEnv::new(&sig(&[("T", TypeClass::Comparable)]));
        let other = Type::lib(LibType::Set, vec![Type::Int]).unwrap();
        assert_eq!(
            env.bind(&set, &PartType::Known(other), input("A"), &Facts),
            Fit::BadShape
        );
        assert_eq!(env.get(&var("T")).map(|(ty, _)| ty), Some(&Type::Error));

        // A fixed argument must match exactly.
        let mut env = TypeEnv::new(&sig(&[("V", TypeClass::Value)]));
        let map = Type::lib(LibType::Map, vec![Type::Int, Type::Int]).unwrap();
        assert_eq!(
            env.bind(
                &pattern("std::map<std::string, V>"),
                &PartType::Known(map),
                input("M"),
                &Facts
            ),
            Fit::BadShape
        );
        // And when it does, the other argument binds.
        let mut env = TypeEnv::new(&sig(&[("V", TypeClass::Value)]));
        let map = Type::lib(LibType::Map, vec![Type::String, Type::Int]).unwrap();
        assert_eq!(
            env.bind(
                &pattern("std::map<std::string, V>"),
                &PartType::Known(map),
                input("M"),
                &Facts
            ),
            Fit::Fits
        );
        assert_eq!(env.get(&var("V")).map(|(ty, _)| ty), Some(&Type::Int));

        // A repeated variable must be bound to the same type within a pattern.
        let mut env = TypeEnv::new(&sig(&[("T", TypeClass::Value)]));
        let mixed = Type::lib(LibType::Map, vec![Type::Int, Type::Double]).unwrap();
        assert_eq!(
            env.bind(
                &pattern("std::map<T, T>"),
                &PartType::Known(mixed),
                input("M"),
                &Facts
            ),
            Fit::BadShape
        );
    }

    #[test]
    fn unknown_and_error_parts_bind_error() {
        let gone = Type::Named(SymbolId::new("sym_Gone").unwrap());
        for (part, expected) in [
            (PartType::Unknown, Fit::Unknown),
            (PartType::Known(Type::Error), Fit::Fits),
            (PartType::Known(gone), Fit::Unknown),
        ] {
            let mut env = TypeEnv::new(&sig(&[("T", TypeClass::Comparable)]));
            assert_eq!(
                env.bind(&pattern("T"), &part, input("A"), &Facts),
                expected,
                "{part:?}"
            );
            assert_eq!(env.get(&var("T")), Some((&Type::Error, Some(&input("A")))));
            assert_eq!(
                env.resolve(&pattern("std::vector<T>"), &Parts::default(), None),
                Resolution::Type(Type::Error)
            );
        }
    }

    #[test]
    fn empty_auto_and_place_typed_parts_bind_nothing() {
        let t = pattern("T");
        let mut env = TypeEnv::new(&sig(&[("T", TypeClass::Value)]));
        for part in [PartType::Auto, PartType::FromPlace] {
            assert_eq!(env.bind(&t, &part, input("A"), &Facts), Fit::Fits);
        }
        assert_eq!(env.resolve(&t, &Parts::default(), None), Resolution::Open);
        assert_eq!(env.bind(&t, &PartType::Empty, input("B"), &Facts), Fit::Fits);
        assert_eq!(env.get(&var("T")), None);
        assert_eq!(env.resolve(&t, &Parts::default(), None), Resolution::Missing);
        assert_eq!(
            env.resolve(&pattern("std::map<T, U>"), &Parts::default(), None),
            Resolution::Missing
        );
        assert_eq!(
            env.resolve(&pattern("std::map<int, U>"), &Parts::default(), None),
            Resolution::Open
        );
        // A later part still binds.
        env.bind(&t, &PartType::Known(Type::Char), input("C"), &Facts);
        assert_eq!(
            env.resolve(&t, &Parts::default(), None),
            Resolution::Type(Type::Char)
        );
    }

    #[test]
    fn the_place_binds_a_place_typed_block() {
        let mut list_sig = sig(&[("C", TypeClass::Listable)]);
        list_sig.place_var = Some(var("C"));
        let mut env = TypeEnv::new(&list_sig);
        assert_eq!(env.bind_place(None, &Facts), Fit::Fits);
        assert_eq!(env.get(&var("C")), None);
        assert_eq!(env.bind_place(Some(&vector(Type::Int)), &Facts), Fit::Fits);
        assert_eq!(env.get(&var("C")), Some((&vector(Type::Int), None)));
        let mut env = TypeEnv::new(&list_sig);
        assert_eq!(env.bind_place(Some(&Type::Int), &Facts), Fit::OutsideClass);
        assert_eq!(env.get(&var("C")).map(|(ty, _)| ty), Some(&Type::Error));
        let mut env = TypeEnv::new(&list_sig);
        let gone = Type::Named(SymbolId::new("sym_Gone").unwrap());
        assert_eq!(env.bind_place(Some(&gone), &Facts), Fit::Unknown);
        // A block that is not place-typed ignores its place.
        let mut env = TypeEnv::new(&sig(&[("C", TypeClass::Listable)]));
        assert_eq!(env.bind_place(Some(&Type::Int), &Facts), Fit::Fits);
        assert_eq!(env.get(&var("C")), None);
    }

    #[test]
    fn resolves_part_forms_with_an_index() {
        let env = TypeEnv::new(&BlockSig::default());
        let parts = Parts {
            fields: BTreeMap::from([
                ("TYPE".to_owned(), PartType::Known(Type::Double)),
                ("A".to_owned(), PartType::Auto),
            ]),
            params: vec![
                PartType::Known(Type::Int),
                PartType::Known(Type::String),
                PartType::Unknown,
            ],
            result: Some(PartType::Known(Type::Bool)),
            member: None,
        };
        assert_eq!(
            env.resolve(&pattern("field:TYPE"), &parts, None),
            Resolution::Type(Type::Double)
        );
        assert_eq!(
            env.resolve(&pattern("field:A"), &parts, None),
            Resolution::Missing
        );
        assert_eq!(
            env.resolve(&pattern("symbol:TYPE"), &parts, None),
            Resolution::Type(Type::Double)
        );
        assert_eq!(env.resolve(&pattern("symbol"), &parts, None), Resolution::Missing);
        assert_eq!(
            env.resolve(&pattern("param:FUNC"), &parts, Some(1)),
            Resolution::Type(Type::String)
        );
        assert_eq!(
            env.resolve(&pattern("param:FUNC"), &parts, Some(2)),
            Resolution::Missing
        );
        assert_eq!(
            env.resolve(&pattern("param:FUNC"), &parts, Some(7)),
            Resolution::Missing
        );
        assert_eq!(
            env.resolve(&pattern("param:FUNC"), &parts, None),
            Resolution::Missing
        );
        assert_eq!(
            env.resolve(&pattern("member:TYPE"), &parts, Some(0)),
            Resolution::Type(Type::Int)
        );
        assert_eq!(
            env.resolve(&pattern("result"), &parts, None),
            Resolution::Type(Type::Bool)
        );
        assert_eq!(env.resolve(&pattern("any"), &parts, None), Resolution::Open);
        assert_eq!(
            env.resolve(&pattern("std::vector<int>"), &parts, None),
            Resolution::Type(vector(Type::Int))
        );
        let void = Parts::default();
        assert_eq!(env.resolve(&pattern("result"), &void, None), Resolution::Missing);
    }

    #[test]
    fn members_fields_resolve_through_their_owner() {
        let mut member_sig = sig(&[("S", TypeClass::Struct)]);
        member_sig.members.insert("MEMBER".into(), var("S"));
        let point = Type::Named(SymbolId::new("sym_Point").unwrap());
        let parts = Parts {
            member: Some(PartType::Known(Type::Double)),
            ..Parts::default()
        };
        let mut env = TypeEnv::new(&member_sig);
        assert_eq!(
            env.resolve(&pattern("symbol:MEMBER"), &parts, None),
            Resolution::Missing
        );
        env.bind(&pattern("S"), &PartType::Known(point), input("OBJ"), &Facts);
        assert_eq!(
            env.resolve(&pattern("symbol:MEMBER"), &parts, None),
            Resolution::Type(Type::Double)
        );
        let mut env = TypeEnv::new(&member_sig);
        env.bind(&pattern("S"), &PartType::Unknown, input("OBJ"), &Facts);
        assert_eq!(
            env.resolve(&pattern("symbol:MEMBER"), &parts, None),
            Resolution::Missing
        );
    }

    #[test]
    fn sig_tables() {
        let mut table = SigTable::empty();
        assert!(table.is_empty());
        assert_eq!(table.get("var.declare"), None);
        table.insert("var.declare", sig(&[("T", TypeClass::Value)]));
        assert_eq!(table.len(), 1);
        assert!(table.get("var.declare").is_some());
        assert_eq!(
            table.iter().map(|(id, _)| id).collect::<Vec<_>>(),
            ["var.declare"]
        );
        assert!(
            Fit::BadShape > Fit::OutsideClass && Fit::OutsideClass > Fit::Unknown && Fit::Unknown > Fit::Fits
        );
    }
}
