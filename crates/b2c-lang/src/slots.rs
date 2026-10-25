//! Typed slots (ADR-0018; the shapes are those of
//! `docs/plans/m3-contracts/slots.md` §1): what each part of a block expects,
//! worked out in one pass from the block's catalog signature
//! ([`b2c_ir::sig`]).
//!
//! [`type_block`] types one block: it binds the signature's type variables
//! from the block's parts, in catalog order, then from its place, resolves
//! every pattern and checks how each part fits. It lives here, not in
//! `b2c-ir`, because checking a fit needs [`conversion`](crate::conversion).
//!
//! The rule is normative for both copies, this one and the editor's
//! TypeScript copy (`packages/blockly-ext/src/types/slots.ts`); the slot
//! fixture that a test of this crate writes holds the two together.

use std::collections::BTreeMap;

use b2c_ir::sig::{
    BlockParts, BlockSig, Fit, InputSig, PartName, PartType, Resolution, TypeEnv, TypePattern, TypeVar,
};
use b2c_ir::types::{Type, TypeFacts};
use b2c_model::limits::MAX_VARIADIC_PARTS;

use crate::typing::{Conversion, assignment, check_accepts, conversion};

/// How a part fits what its block expects of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotFit {
    /// Nothing to check: an empty, unknown or erroneous part, a value typed
    /// from its place, an expectation that does not resolve, or class
    /// membership that is not known.
    Unknown,
    /// It fits by this conversion (never [`Conversion::Invalid`]);
    /// [`Conversion::Narrowing`] and [`Conversion::BoolNumber`] warn.
    Converts(Conversion),
    /// Its type is outside the class the part requires (`Fit::OutsideClass`,
    /// or its `check`).
    OutsideClass,
    /// It cannot fit: a wrong shape for the pattern (`Fit::BadShape`), or
    /// [`Conversion::Invalid`].
    Invalid,
}

impl SlotFit {
    /// [`SlotFit::OutsideClass`] and [`SlotFit::Invalid`]: what the analyser
    /// reports and the editor refuses.
    pub const fn is_error(self) -> bool {
        matches!(self, Self::OutsideClass | Self::Invalid)
    }
}

/// A value input part's expected type and how its value fits it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputTyping {
    /// `None`: nothing gives this input a type (`Resolution::Open`).
    /// `Some(Type::Error)`: what would give it one is missing or already
    /// wrong (`Resolution::Missing`).
    pub expects: Option<Type>,
    /// How its value fits.
    pub fit: SlotFit,
}

/// Which part bound a variable, for messages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Binder {
    /// A field.
    Field(String),
    /// A value input part.
    Input(String),
    /// The block's place (a place-typed block).
    Place,
}

/// The result of typing one block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockTyping {
    /// Each bound variable, its type (`Error` when an unknown or failed part
    /// bound it) and its binder.
    pub bound: BTreeMap<TypeVar, (Type, Binder)>,
    /// Every value input part the block has, by part name.
    pub inputs: BTreeMap<String, InputTyping>,
    /// Every `type` and `symbol_ref` field that has a pattern or `members`.
    pub fields: BTreeMap<String, SlotFit>,
    /// What each `symbol_decl` field with a pattern declares (`Type::Error`
    /// if it does not resolve).
    pub declares: BTreeMap<String, Type>,
    /// The block's value type: `None` for `any` and `number` outputs, which
    /// keep their M2 rules; `Some(Type::Error)` for an output that does not
    /// resolve.
    pub output: Option<Type>,
    /// A place-typed block: how its place's type fits its output variable
    /// ([`SlotFit::Unknown`] without one).
    pub place: SlotFit,
}

/// Types one block (slots.md §2). Pure and deterministic, never panics,
/// linear in the block's parts.
///
/// 1. **Bind from fields**, in catalog order: each `type` or `symbol_ref`
///    field with a pattern, as [`TypeEnv::bind`] does.
/// 2. **Bind from value inputs**, in catalog order, a repeated input part by
///    part. An `assign` input never binds.
/// 3. **Bind from the place**, for a place-typed block.
/// 4. **Resolve and check** every pattern: what each input expects and how
///    each part fits, what each declaration declares, and the output.
///
/// The first part that binds a variable decides it, and a later part that
/// disagrees is the mismatch. A part whose binding fails, an unknown part
/// and one whose class membership is not known bind their variables to
/// `Error`, so replacing a part by an unknown one never turns another
/// part's fit into an error.
pub fn type_block(
    sig: &BlockSig,
    parts: &dyn BlockParts,
    place: Option<&Type>,
    facts: &dyn TypeFacts,
) -> BlockTyping {
    let mut env = TypeEnv::new(sig);
    // Steps 1 and 2.
    let failed = bind_parts(sig, parts, facts, &mut env);

    // Step 3: the place.
    let place_fit = match (&sig.place_var, place) {
        (Some(var), Some(place)) if env.get(var).is_none() => match env.bind_place(Some(place), facts) {
            Fit::OutsideClass => SlotFit::OutsideClass,
            Fit::Fits if *place != Type::Error => SlotFit::Converts(Conversion::Same),
            Fit::Fits | Fit::Unknown | Fit::BadShape => SlotFit::Unknown,
        },
        _ => SlotFit::Unknown,
    };

    // Step 4: resolve and check.
    let resolver = Resolver {
        env: &env,
        parts,
        failed: &failed,
        facts,
    };
    let (fields, declares) = resolver.fields(sig);
    let output = match &sig.output {
        None | Some(TypePattern::Any | TypePattern::Number) => None,
        Some(pattern) => Some(resolver.expects(pattern, None).unwrap_or(Type::Error)),
    };
    let bound = sig
        .vars
        .keys()
        .filter_map(|var| {
            let (ty, by) = env.get(var)?;
            let binder = match by {
                None => Binder::Place,
                Some(PartName::Field(name)) => Binder::Field(name.clone()),
                Some(PartName::Input(name)) => Binder::Input(name.clone()),
            };
            Some((var.clone(), (ty.clone(), binder)))
        })
        .collect();
    BlockTyping {
        bound,
        inputs: resolver.inputs(sig),
        fields,
        declares,
        output,
        place: place_fit,
    }
}

/// Steps 1 and 2: binds the variables from the fields, then from the value
/// inputs, each in catalog order. Returns the binding failures by part
/// name.
fn bind_parts(
    sig: &BlockSig,
    parts: &dyn BlockParts,
    facts: &dyn TypeFacts,
    env: &mut TypeEnv,
) -> BTreeMap<String, SlotFit> {
    let mut failed = BTreeMap::new();
    let mut record = |part: &str, fit: Fit| {
        let fit = match fit {
            Fit::OutsideClass => SlotFit::OutsideClass,
            Fit::BadShape => SlotFit::Invalid,
            Fit::Fits | Fit::Unknown => return,
        };
        failed.insert(part.to_owned(), fit);
    };
    for name in sig.order.iter().filter_map(|part| match part {
        PartName::Field(name) => Some(name),
        PartName::Input(_) => None,
    }) {
        let Some(pattern) = sig.fields.get(name) else {
            continue;
        };
        if sig.symbol_decls.contains(name) || sig.members.contains_key(name) {
            continue;
        }
        let fit = env.bind(pattern, &parts.field(name), PartName::Field(name.clone()), facts);
        record(name, fit);
    }
    for base in sig.order.iter().filter_map(|part| match part {
        PartName::Input(name) => Some(name),
        PartName::Field(_) => None,
    }) {
        let Some(InputSig {
            pattern: Some(pattern),
            assign: false,
            ..
        }) = sig.inputs.get(base)
        else {
            continue;
        };
        for (part, _) in input_parts(sig, parts, base) {
            let fit = env.bind(pattern, &parts.input(&part), PartName::Input(part.clone()), facts);
            record(&part, fit);
        }
    }
    failed
}

/// Step 4: what each part expects and how it fits, once every variable that
/// can be bound is.
struct Resolver<'a> {
    env: &'a TypeEnv,
    parts: &'a dyn BlockParts,
    failed: &'a BTreeMap<String, SlotFit>,
    facts: &'a dyn TypeFacts,
}

impl Resolver<'_> {
    /// A resolved pattern: `Some(t)` for `Type(t)`, `Some(Error)` for
    /// `Missing`, `None` for `Open`.
    fn expects(&self, pattern: &TypePattern, index: Option<usize>) -> Option<Type> {
        match self.env.resolve(pattern, self.parts, index) {
            Resolution::Type(ty) => Some(ty),
            Resolution::Missing => Some(Type::Error),
            Resolution::Open => None,
        }
    }

    /// Every value input part's expectation and fit.
    fn inputs(&self, sig: &BlockSig) -> BTreeMap<String, InputTyping> {
        let mut inputs = BTreeMap::new();
        for (base, input) in &sig.inputs {
            for (part, index) in input_parts(sig, self.parts, base) {
                let expects = input
                    .pattern
                    .as_ref()
                    .and_then(|pattern| self.expects(pattern, index));
                let fit = match known(&self.parts.input(&part)) {
                    None => SlotFit::Unknown,
                    Some(ty) => match self.failed.get(&part) {
                        Some(failure) => *failure,
                        None => self.input_fit(input, &ty, expects.as_ref()),
                    },
                };
                inputs.insert(part, InputTyping { expects, fit });
            }
        }
        inputs
    }

    /// How a value of a known type fits an input that did not fail to bind:
    /// its `check` class, then the conversion (or assignment) to what it
    /// expects.
    fn input_fit(&self, input: &InputSig, ty: &Type, expects: Option<&Type>) -> SlotFit {
        match check_accepts(input.check, ty, self.facts) {
            Some(false) => SlotFit::OutsideClass,
            None => SlotFit::Unknown,
            Some(true) => match expects {
                None => SlotFit::Unknown,
                Some(expects) => {
                    let conversion = if input.assign {
                        assignment(ty, expects)
                    } else {
                        conversion(ty, expects)
                    };
                    if conversion.is_error() {
                        SlotFit::Invalid
                    } else {
                        SlotFit::Converts(conversion)
                    }
                }
            },
        }
    }

    /// The fit of every `type` and `symbol_ref` field with a pattern or
    /// `members`, and what each `symbol_decl` field with a pattern declares.
    fn fields(&self, sig: &BlockSig) -> (BTreeMap<String, SlotFit>, BTreeMap<String, Type>) {
        let mut fields = BTreeMap::new();
        let mut declares = BTreeMap::new();
        for (name, pattern) in &sig.fields {
            if sig.symbol_decls.contains(name) {
                declares.insert(name.clone(), self.expects(pattern, None).unwrap_or(Type::Error));
                continue;
            }
            if sig.members.contains_key(name) {
                continue;
            }
            let fit = match known(&self.parts.field(name)) {
                None => SlotFit::Unknown,
                Some(ty) => match self.failed.get(name) {
                    Some(failure) => *failure,
                    None if sig.symbol_refs.contains(name) => match self.expects(pattern, None) {
                        // An earlier part bound the variable to another type:
                        // a reference is never converted.
                        Some(expected) if expected != Type::Error && expected != ty => SlotFit::Invalid,
                        _ => SlotFit::Converts(Conversion::Same),
                    },
                    None => SlotFit::Converts(Conversion::Same),
                },
            };
            fields.insert(name.clone(), fit);
        }
        for (name, var) in &sig.members {
            let fit = match self.env.get(var) {
                Some((owner, _)) if *owner != Type::Error => {
                    match known(&self.parts.chosen_member(name, owner)) {
                        Some(_) => SlotFit::Converts(Conversion::Same),
                        None => SlotFit::Unknown,
                    }
                }
                _ => SlotFit::Unknown,
            };
            fields.insert(name.clone(), fit);
        }
        (fields, declares)
    }
}

/// The parts of the value input `base`: the input itself, or each part of a
/// repeated input (`ITEM0`, `ITEM1`, …) with its index. An input is repeated
/// when the signature says so, or when the parts count parts of it (an input
/// with one part has no count), so a signature built without `repeated`
/// still types every part.
pub(crate) fn input_parts(
    sig: &BlockSig,
    parts: &dyn BlockParts,
    base: &str,
) -> Vec<(String, Option<usize>)> {
    if sig.repeated.contains(base) || parts.count(base) > 0 {
        (0..parts.count(base).min(MAX_VARIADIC_PARTS))
            .map(|i| (format!("{base}{i}"), Some(i)))
            .collect()
    } else {
        vec![(base.to_owned(), None)]
    }
}

/// The type of a part that has one to check: `None` for an empty, `auto`,
/// place-typed, unknown or erroneous part, which fits [`SlotFit::Unknown`].
fn known(part: &PartType) -> Option<Type> {
    match part {
        PartType::Known(ty) if *ty != Type::Error => Some(ty.clone()),
        PartType::Known(_) | PartType::Empty | PartType::Auto | PartType::FromPlace | PartType::Unknown => {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use b2c_ir::ids::SymbolId;
    use b2c_ir::sig::{InputSig, PartName, PartType, TypePattern};
    use b2c_ir::types::{LibType, NoFacts, TypeClass, UserKind, UserType};
    use proptest::prelude::*;

    use super::*;

    fn var(name: &str) -> TypeVar {
        TypeVar::new(name).unwrap()
    }

    fn pattern(text: &str) -> TypePattern {
        TypePattern::parse(text).unwrap()
    }

    fn input(check: TypeClass, pattern_text: Option<&str>, assign: bool) -> InputSig {
        InputSig {
            check,
            pattern: pattern_text.map(pattern),
            assign,
        }
    }

    fn names(list: &[&str]) -> BTreeSet<String> {
        list.iter().map(|name| (*name).to_owned()).collect()
    }

    fn vector(of: Type) -> Type {
        Type::lib(LibType::Vector, vec![of]).unwrap()
    }

    fn named(id: &str) -> Type {
        Type::Named(SymbolId::new(id).unwrap())
    }

    /// Facts that know a struct `sym_Point` and an enum `sym_Color`; any
    /// other `Named` is unknown.
    struct Facts;

    impl TypeFacts for Facts {
        fn user_type(&self, id: &SymbolId) -> Option<UserType<'_>> {
            let (name, kind) = match id.as_str() {
                "sym_Point" => ("Point", UserKind::Struct),
                "sym_Color" => ("Color", UserKind::Enum),
                _ => return None,
            };
            Some(UserType {
                name,
                kind,
                comparable: true,
                printable: true,
            })
        }
    }

    /// A block's parts from a table.
    #[derive(Debug, Clone, Default)]
    struct Parts {
        fields: BTreeMap<String, PartType>,
        inputs: BTreeMap<String, PartType>,
        params: Vec<PartType>,
        /// The chosen member's type, for any owner.
        member: Option<PartType>,
        result: Option<PartType>,
    }

    impl Parts {
        fn with_field(mut self, name: &str, part: PartType) -> Self {
            self.fields.insert(name.to_owned(), part);
            self
        }

        fn with_input(mut self, name: &str, part: PartType) -> Self {
            self.inputs.insert(name.to_owned(), part);
            self
        }
    }

    impl BlockParts for Parts {
        fn field(&self, name: &str) -> PartType {
            self.fields.get(name).cloned().unwrap_or(PartType::Empty)
        }
        fn input(&self, part: &str) -> PartType {
            self.inputs.get(part).cloned().unwrap_or(PartType::Empty)
        }
        fn count(&self, base: &str) -> usize {
            self.inputs
                .keys()
                .filter(|name| {
                    name.strip_prefix(base)
                        .is_some_and(|index| !index.is_empty() && index.bytes().all(|b| b.is_ascii_digit()))
                })
                .count()
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

    /// `create [TYPE] [NAME] = (VALUE)`: `where = { T = "value" }`.
    fn declare() -> BlockSig {
        BlockSig {
            vars: BTreeMap::from([(var("T"), TypeClass::Value)]),
            fields: BTreeMap::from([("TYPE".into(), pattern("T")), ("NAME".into(), pattern("T"))]),
            inputs: BTreeMap::from([("VALUE".into(), input(TypeClass::Any, Some("T"), false))]),
            order: vec![
                PartName::Field("TYPE".into()),
                PartName::Field("NAME".into()),
                PartName::Input("VALUE".into()),
            ],
            symbol_decls: names(&["NAME"]),
            ..BlockSig::default()
        }
    }

    /// `set [VAR] to (VALUE)`: the value is assigned.
    fn set() -> BlockSig {
        BlockSig {
            vars: BTreeMap::from([(var("T"), TypeClass::Value)]),
            fields: BTreeMap::from([("VAR".into(), pattern("T"))]),
            inputs: BTreeMap::from([("VALUE".into(), input(TypeClass::Any, Some("T"), true))]),
            order: vec![PartName::Field("VAR".into()), PartName::Input("VALUE".into())],
            symbol_refs: names(&["VAR"]),
            ..BlockSig::default()
        }
    }

    /// `item (INDEX) of [LIST]`: `where = { C = "indexable" }`.
    fn item() -> BlockSig {
        BlockSig {
            vars: BTreeMap::from([(var("C"), TypeClass::Indexable)]),
            output: Some(pattern("elem(C)")),
            fields: BTreeMap::from([("LIST".into(), pattern("C"))]),
            inputs: BTreeMap::from([("INDEX".into(), input(TypeClass::Any, Some("index(C)"), false))]),
            order: vec![PartName::Field("LIST".into()), PartName::Input("INDEX".into())],
            symbol_refs: names(&["LIST"]),
            ..BlockSig::default()
        }
    }

    /// `sum of [LIST] and [OTHER]`: two references of `std::vector<T>`.
    fn two_lists() -> BlockSig {
        BlockSig {
            vars: BTreeMap::from([(var("T"), TypeClass::Number)]),
            output: Some(pattern("T")),
            fields: BTreeMap::from([
                ("LIST".into(), pattern("std::vector<T>")),
                ("OTHER".into(), pattern("std::vector<T>")),
            ]),
            order: vec![PartName::Field("LIST".into()), PartName::Field("OTHER".into())],
            symbol_refs: names(&["LIST", "OTHER"]),
            ..BlockSig::default()
        }
    }

    /// `list of (ITEM0) (ITEM1) …`: place-typed, `where = { C = "listable" }`.
    fn list_of() -> BlockSig {
        BlockSig {
            vars: BTreeMap::from([(var("C"), TypeClass::Listable)]),
            output: Some(pattern("C")),
            inputs: BTreeMap::from([("ITEM".into(), input(TypeClass::Any, Some("elem(C)"), false))]),
            order: vec![PartName::Input("ITEM".into())],
            place_var: Some(var("C")),
            repeated: names(&["ITEM"]),
            ..BlockSig::default()
        }
    }

    /// Two inputs of `T` with `where = { T = "integer" }`: the first
    /// counterexample of slots.md §2's guarantees.
    fn two_integers() -> BlockSig {
        BlockSig {
            vars: BTreeMap::from([(var("T"), TypeClass::Integer)]),
            inputs: BTreeMap::from([
                ("A".into(), input(TypeClass::Any, Some("T"), false)),
                ("B".into(), input(TypeClass::Any, Some("T"), false)),
            ]),
            order: vec![PartName::Input("A".into()), PartName::Input("B".into())],
            ..BlockSig::default()
        }
    }

    /// Three inputs of `T` with `where = { T = "text" }` and `assign` on the
    /// second: the second counterexample (a `char` among strings must not
    /// bind).
    fn texts() -> BlockSig {
        BlockSig {
            vars: BTreeMap::from([(var("T"), TypeClass::Text)]),
            inputs: BTreeMap::from([
                ("A".into(), input(TypeClass::Any, Some("T"), false)),
                ("B".into(), input(TypeClass::Any, Some("T"), true)),
                ("C".into(), input(TypeClass::Any, Some("T"), false)),
            ]),
            order: vec![
                PartName::Input("A".into()),
                PartName::Input("B".into()),
                PartName::Input("C".into()),
            ],
            ..BlockSig::default()
        }
    }

    /// `call [FUNC] (ARG0) (ARG1) …`.
    fn call() -> BlockSig {
        BlockSig {
            inputs: BTreeMap::from([("ARG".into(), input(TypeClass::Any, Some("param:FUNC"), false))]),
            order: vec![PartName::Input("ARG".into())],
            repeated: names(&["ARG"]),
            ..BlockSig::default()
        }
    }

    /// `return (VALUE)`.
    fn ret() -> BlockSig {
        BlockSig {
            inputs: BTreeMap::from([("VALUE".into(), input(TypeClass::Any, Some("result"), false))]),
            order: vec![PartName::Input("VALUE".into())],
            ..BlockSig::default()
        }
    }

    /// Inputs with a `check` and no pattern.
    fn checks() -> BlockSig {
        BlockSig {
            inputs: BTreeMap::from([
                ("BOOL".into(), input(TypeClass::Bool, None, false)),
                ("INT".into(), input(TypeClass::Integer, None, false)),
                ("NUM".into(), input(TypeClass::Number, None, false)),
                ("COLL".into(), input(TypeClass::Collection, None, false)),
            ]),
            ..BlockSig::default()
        }
    }

    /// `[OBJ] . [MEMBER]`: `where = { S = "struct" }`, a `members` field.
    fn member() -> BlockSig {
        BlockSig {
            vars: BTreeMap::from([(var("S"), TypeClass::Struct)]),
            output: Some(pattern("symbol:MEMBER")),
            fields: BTreeMap::from([("OBJ".into(), pattern("S"))]),
            order: vec![PartName::Field("OBJ".into())],
            members: BTreeMap::from([("MEMBER".into(), var("S"))]),
            symbol_refs: names(&["OBJ"]),
            ..BlockSig::default()
        }
    }

    fn known(ty: Type) -> PartType {
        PartType::Known(ty)
    }

    fn converts(conversion: Conversion) -> SlotFit {
        SlotFit::Converts(conversion)
    }

    fn by_field(name: &str) -> Binder {
        Binder::Field(name.to_owned())
    }

    fn by_input(name: &str) -> Binder {
        Binder::Input(name.to_owned())
    }

    /// An expected typing, built part by part.
    #[derive(Default)]
    struct Expect {
        bound: Vec<(&'static str, Type, Binder)>,
        inputs: Vec<(&'static str, Option<Type>, SlotFit)>,
        fields: Vec<(&'static str, SlotFit)>,
        declares: Vec<(&'static str, Type)>,
        output: Option<Type>,
        place: Option<SlotFit>,
    }

    impl Expect {
        fn typing(self) -> BlockTyping {
            BlockTyping {
                bound: self
                    .bound
                    .into_iter()
                    .map(|(name, ty, binder)| (var(name), (ty, binder)))
                    .collect(),
                inputs: self
                    .inputs
                    .into_iter()
                    .map(|(name, expects, fit)| (name.to_owned(), InputTyping { expects, fit }))
                    .collect(),
                fields: self
                    .fields
                    .into_iter()
                    .map(|(name, fit)| (name.to_owned(), fit))
                    .collect(),
                declares: self
                    .declares
                    .into_iter()
                    .map(|(name, ty)| (name.to_owned(), ty))
                    .collect(),
                output: self.output,
                place: self.place.unwrap_or(SlotFit::Unknown),
            }
        }
    }

    /// One case of the test table: a signature, the block's parts, its place
    /// and the typing slots.md §2 gives.
    struct Case {
        name: &'static str,
        sig: BlockSig,
        parts: Parts,
        place: Option<Type>,
        expect: Expect,
    }

    /// The test table: the cases slots.md §9 lists for each step of the rule.
    fn cases() -> Vec<Case> {
        let mut cases = variable_cases();
        cases.extend(collection_cases());
        cases.extend(place_cases());
        cases.extend(other_cases());
        cases
    }

    /// `create` and `set`: binding from fields, `auto`, empty binders and
    /// `assign`.
    #[allow(clippy::too_many_lines, reason = "a table")]
    fn variable_cases() -> Vec<Case> {
        vec![
            Case {
                name: "a type field binds T, the value converts",
                sig: declare(),
                parts: Parts::default()
                    .with_field("TYPE", known(Type::Double))
                    .with_input("VALUE", known(Type::Int)),
                place: None,
                expect: Expect {
                    bound: vec![("T", Type::Double, by_field("TYPE"))],
                    inputs: vec![("VALUE", Some(Type::Double), converts(Conversion::Widening))],
                    fields: vec![("TYPE", converts(Conversion::Same))],
                    declares: vec![("NAME", Type::Double)],
                    ..Expect::default()
                },
            },
            Case {
                name: "auto binds nothing; the value binds T",
                sig: declare(),
                parts: Parts::default()
                    .with_field("TYPE", PartType::Auto)
                    .with_input("VALUE", known(Type::String)),
                place: None,
                expect: Expect {
                    bound: vec![("T", Type::String, by_input("VALUE"))],
                    inputs: vec![("VALUE", Some(Type::String), converts(Conversion::Same))],
                    fields: vec![("TYPE", SlotFit::Unknown)],
                    declares: vec![("NAME", Type::String)],
                    ..Expect::default()
                },
            },
            Case {
                name: "auto and a value typed from its place: nothing gives a type",
                sig: declare(),
                parts: Parts::default()
                    .with_field("TYPE", PartType::Auto)
                    .with_input("VALUE", PartType::FromPlace),
                place: None,
                expect: Expect {
                    inputs: vec![("VALUE", None, SlotFit::Unknown)],
                    fields: vec![("TYPE", SlotFit::Unknown)],
                    declares: vec![("NAME", Type::Error)],
                    ..Expect::default()
                },
            },
            Case {
                name: "a type outside the variable's class binds Error",
                sig: declare(),
                parts: Parts::default()
                    .with_field("TYPE", known(Type::lib(LibType::IfStream, vec![]).unwrap()))
                    .with_input("VALUE", known(Type::Int)),
                place: None,
                expect: Expect {
                    bound: vec![("T", Type::Error, by_field("TYPE"))],
                    inputs: vec![("VALUE", Some(Type::Error), converts(Conversion::Same))],
                    fields: vec![("TYPE", SlotFit::OutsideClass)],
                    declares: vec![("NAME", Type::Error)],
                    ..Expect::default()
                },
            },
            Case {
                name: "an empty binder makes the expectation Error, not open",
                sig: set(),
                parts: Parts::default().with_input("VALUE", PartType::FromPlace),
                place: None,
                expect: Expect {
                    inputs: vec![("VALUE", Some(Type::Error), SlotFit::Unknown)],
                    fields: vec![("VAR", SlotFit::Unknown)],
                    ..Expect::default()
                },
            },
            Case {
                name: "assign: a char into a string, and it never binds",
                sig: set(),
                parts: Parts::default()
                    .with_field("VAR", known(Type::String))
                    .with_input("VALUE", known(Type::Char)),
                place: None,
                expect: Expect {
                    bound: vec![("T", Type::String, by_field("VAR"))],
                    inputs: vec![("VALUE", Some(Type::String), converts(Conversion::Widening))],
                    fields: vec![("VAR", converts(Conversion::Same))],
                    ..Expect::default()
                },
            },
            Case {
                name: "an assigned value with nothing to bind it leaves T open",
                sig: set(),
                parts: Parts::default()
                    .with_field("VAR", PartType::Auto)
                    .with_input("VALUE", known(Type::Char)),
                place: None,
                expect: Expect {
                    inputs: vec![("VALUE", None, SlotFit::Unknown)],
                    fields: vec![("VAR", SlotFit::Unknown)],
                    ..Expect::default()
                },
            },
            Case {
                name: "text into an int variable does not convert",
                sig: set(),
                parts: Parts::default()
                    .with_field("VAR", known(Type::Int))
                    .with_input("VALUE", known(Type::String)),
                place: None,
                expect: Expect {
                    bound: vec![("T", Type::Int, by_field("VAR"))],
                    inputs: vec![("VALUE", Some(Type::Int), SlotFit::Invalid)],
                    fields: vec![("VAR", converts(Conversion::Same))],
                    ..Expect::default()
                },
            },
        ]
    }

    /// Classes, shapes, unknown membership, references and members.
    #[allow(clippy::too_many_lines, reason = "a table")]
    fn collection_cases() -> Vec<Case> {
        let set = Type::lib(LibType::Set, vec![Type::Int]).unwrap();
        vec![
            Case {
                name: "a class mismatch binds Error, which then fits",
                sig: item(),
                parts: Parts::default()
                    .with_field("LIST", known(set.clone()))
                    .with_input("INDEX", known(Type::Int)),
                place: None,
                expect: Expect {
                    bound: vec![("C", Type::Error, by_field("LIST"))],
                    inputs: vec![("INDEX", Some(Type::Error), converts(Conversion::Same))],
                    fields: vec![("LIST", SlotFit::OutsideClass)],
                    output: Some(Type::Error),
                    ..Expect::default()
                },
            },
            Case {
                name: "a list binds C; the index and the output follow it",
                sig: item(),
                parts: Parts::default()
                    .with_field("LIST", known(vector(Type::String)))
                    .with_input("INDEX", known(Type::Double)),
                place: None,
                expect: Expect {
                    bound: vec![("C", vector(Type::String), by_field("LIST"))],
                    inputs: vec![("INDEX", Some(Type::Int), converts(Conversion::Narrowing))],
                    fields: vec![("LIST", converts(Conversion::Same))],
                    output: Some(Type::String),
                    ..Expect::default()
                },
            },
            Case {
                name: "unknown membership binds Error",
                sig: item(),
                parts: Parts::default()
                    .with_field("LIST", known(named("sym_Gone")))
                    .with_input("INDEX", known(Type::String)),
                place: None,
                expect: Expect {
                    bound: vec![("C", Type::Error, by_field("LIST"))],
                    inputs: vec![("INDEX", Some(Type::Error), converts(Conversion::Same))],
                    fields: vec![("LIST", converts(Conversion::Same))],
                    output: Some(Type::Error),
                    ..Expect::default()
                },
            },
            Case {
                name: "an unknown reference binds Error",
                sig: item(),
                parts: Parts::default()
                    .with_field("LIST", PartType::Unknown)
                    .with_input("INDEX", known(Type::String)),
                place: None,
                expect: Expect {
                    bound: vec![("C", Type::Error, by_field("LIST"))],
                    inputs: vec![("INDEX", Some(Type::Error), converts(Conversion::Same))],
                    fields: vec![("LIST", SlotFit::Unknown)],
                    output: Some(Type::Error),
                    ..Expect::default()
                },
            },
            Case {
                name: "a shape mismatch is invalid",
                sig: two_lists(),
                parts: Parts::default()
                    .with_field("LIST", known(set))
                    .with_field("OTHER", known(vector(Type::Int))),
                place: None,
                expect: Expect {
                    bound: vec![("T", Type::Error, by_field("LIST"))],
                    fields: vec![("LIST", SlotFit::Invalid), ("OTHER", converts(Conversion::Same))],
                    output: Some(Type::Error),
                    ..Expect::default()
                },
            },
            Case {
                name: "a reference never converts to what an earlier part bound",
                sig: two_lists(),
                parts: Parts::default()
                    .with_field("LIST", known(vector(Type::Int)))
                    .with_field("OTHER", known(vector(Type::Double))),
                place: None,
                expect: Expect {
                    bound: vec![("T", Type::Int, by_field("LIST"))],
                    fields: vec![("LIST", converts(Conversion::Same)), ("OTHER", SlotFit::Invalid)],
                    output: Some(Type::Int),
                    ..Expect::default()
                },
            },
            Case {
                name: "a members field fits through its owner",
                sig: member(),
                parts: Parts {
                    member: Some(known(Type::Int)),
                    ..Parts::default()
                }
                .with_field("OBJ", known(named("sym_Point"))),
                place: None,
                expect: Expect {
                    bound: vec![("S", named("sym_Point"), by_field("OBJ"))],
                    fields: vec![
                        ("MEMBER", converts(Conversion::Same)),
                        ("OBJ", converts(Conversion::Same)),
                    ],
                    output: Some(Type::Int),
                    ..Expect::default()
                },
            },
            Case {
                name: "a members field whose owner is not a struct",
                sig: member(),
                parts: Parts {
                    member: Some(known(Type::Int)),
                    ..Parts::default()
                }
                .with_field("OBJ", known(named("sym_Color"))),
                place: None,
                expect: Expect {
                    bound: vec![("S", Type::Error, by_field("OBJ"))],
                    fields: vec![("MEMBER", SlotFit::Unknown), ("OBJ", SlotFit::OutsideClass)],
                    output: Some(Type::Error),
                    ..Expect::default()
                },
            },
        ]
    }

    /// Place-typed blocks.
    fn place_cases() -> Vec<Case> {
        let point = named("sym_Point");
        vec![
            Case {
                name: "a place-typed block with a place",
                sig: list_of(),
                parts: Parts::default()
                    .with_input("ITEM0", known(point.clone()))
                    .with_input("ITEM1", known(Type::Double)),
                place: Some(vector(point.clone())),
                expect: Expect {
                    bound: vec![("C", vector(point.clone()), Binder::Place)],
                    inputs: vec![
                        ("ITEM0", Some(point.clone()), converts(Conversion::Same)),
                        ("ITEM1", Some(point.clone()), SlotFit::Invalid),
                    ],
                    output: Some(vector(point)),
                    place: Some(converts(Conversion::Same)),
                    ..Expect::default()
                },
            },
            Case {
                name: "a place-typed block without a place",
                sig: list_of(),
                parts: Parts::default().with_input("ITEM0", known(Type::Int)),
                place: None,
                expect: Expect {
                    inputs: vec![("ITEM0", None, SlotFit::Unknown)],
                    output: Some(Type::Error),
                    ..Expect::default()
                },
            },
            Case {
                name: "a place-typed block outside its class",
                sig: list_of(),
                parts: Parts::default().with_input("ITEM0", known(Type::Int)),
                place: Some(Type::Int),
                expect: Expect {
                    bound: vec![("C", Type::Error, Binder::Place)],
                    inputs: vec![("ITEM0", Some(Type::Error), converts(Conversion::Same))],
                    output: Some(Type::Error),
                    place: Some(SlotFit::OutsideClass),
                    ..Expect::default()
                },
            },
            Case {
                name: "a place whose membership is not known",
                sig: list_of(),
                parts: Parts::default(),
                place: Some(named("sym_Gone")),
                expect: Expect {
                    bound: vec![("C", Type::Error, Binder::Place)],
                    output: Some(Type::Error),
                    ..Expect::default()
                },
            },
            Case {
                name: "an erroneous place",
                sig: list_of(),
                parts: Parts::default(),
                place: Some(Type::Error),
                expect: Expect {
                    bound: vec![("C", Type::Error, Binder::Place)],
                    output: Some(Type::Error),
                    ..Expect::default()
                },
            },
        ]
    }

    /// The guarantees' counterexamples, `param:`, `result` and `check`.
    #[allow(clippy::too_many_lines, reason = "a table")]
    fn other_cases() -> Vec<Case> {
        vec![
            Case {
                name: "the first binding wins; a later narrowing warns",
                sig: two_integers(),
                parts: Parts::default()
                    .with_input("A", known(Type::Int))
                    .with_input("B", known(Type::Double)),
                place: None,
                expect: Expect {
                    bound: vec![("T", Type::Int, by_input("A"))],
                    inputs: vec![
                        ("A", Some(Type::Int), converts(Conversion::Same)),
                        ("B", Some(Type::Int), converts(Conversion::Narrowing)),
                    ],
                    ..Expect::default()
                },
            },
            Case {
                name: "an unknown binder never turns a fit into an error",
                sig: two_integers(),
                parts: Parts::default()
                    .with_input("A", PartType::Unknown)
                    .with_input("B", known(Type::Double)),
                place: None,
                expect: Expect {
                    bound: vec![("T", Type::Error, by_input("A"))],
                    inputs: vec![
                        ("A", Some(Type::Error), SlotFit::Unknown),
                        ("B", Some(Type::Error), converts(Conversion::Same)),
                    ],
                    ..Expect::default()
                },
            },
            Case {
                name: "an assigned char among strings does not bind",
                sig: texts(),
                parts: Parts::default()
                    .with_input("A", PartType::Unknown)
                    .with_input("B", known(Type::Char))
                    .with_input("C", known(Type::String)),
                place: None,
                expect: Expect {
                    bound: vec![("T", Type::Error, by_input("A"))],
                    inputs: vec![
                        ("A", Some(Type::Error), SlotFit::Unknown),
                        ("B", Some(Type::Error), converts(Conversion::Same)),
                        ("C", Some(Type::Error), converts(Conversion::Same)),
                    ],
                    ..Expect::default()
                },
            },
            Case {
                name: "param: by index, also out of range",
                sig: call(),
                parts: Parts {
                    params: vec![known(Type::Double)],
                    ..Parts::default()
                }
                .with_input("ARG0", known(Type::Int))
                .with_input("ARG1", known(Type::String)),
                place: None,
                expect: Expect {
                    inputs: vec![
                        ("ARG0", Some(Type::Double), converts(Conversion::Widening)),
                        ("ARG1", Some(Type::Error), converts(Conversion::Same)),
                    ],
                    ..Expect::default()
                },
            },
            Case {
                name: "result of a function",
                sig: ret(),
                parts: Parts {
                    result: Some(known(Type::Double)),
                    ..Parts::default()
                }
                .with_input("VALUE", known(Type::String)),
                place: None,
                expect: Expect {
                    inputs: vec![("VALUE", Some(Type::Double), SlotFit::Invalid)],
                    ..Expect::default()
                },
            },
            Case {
                name: "result of a void function",
                sig: ret(),
                parts: Parts {
                    result: Some(PartType::Empty),
                    ..Parts::default()
                }
                .with_input("VALUE", known(Type::Int)),
                place: None,
                expect: Expect {
                    inputs: vec![("VALUE", Some(Type::Error), converts(Conversion::Same))],
                    ..Expect::default()
                },
            },
            Case {
                name: "checks: bool, integer and number by conversion, the others by membership",
                sig: checks(),
                parts: Parts::default()
                    .with_input("BOOL", known(Type::Int))
                    .with_input("INT", known(Type::Double))
                    .with_input("NUM", known(Type::String))
                    .with_input("COLL", known(Type::Int)),
                place: None,
                expect: Expect {
                    inputs: vec![
                        ("BOOL", None, SlotFit::Unknown),
                        ("COLL", None, SlotFit::OutsideClass),
                        ("INT", None, SlotFit::Unknown),
                        ("NUM", None, SlotFit::OutsideClass),
                    ],
                    ..Expect::default()
                },
            },
            Case {
                name: "a check whose membership is not known",
                sig: checks(),
                parts: Parts::default().with_input("COLL", known(named("sym_Gone"))),
                place: None,
                expect: Expect {
                    inputs: vec![
                        ("BOOL", None, SlotFit::Unknown),
                        ("COLL", None, SlotFit::Unknown),
                        ("INT", None, SlotFit::Unknown),
                        ("NUM", None, SlotFit::Unknown),
                    ],
                    ..Expect::default()
                },
            },
        ]
    }

    #[test]
    fn every_case_is_typed_as_the_rule_says() {
        for case in cases() {
            let typing = type_block(&case.sig, &case.parts, case.place.as_ref(), &Facts);
            assert_eq!(typing, case.expect.typing(), "{}", case.name);
        }
    }

    #[test]
    fn errors_are_class_and_invalid_fits() {
        assert!(SlotFit::OutsideClass.is_error());
        assert!(SlotFit::Invalid.is_error());
        assert!(!SlotFit::Unknown.is_error());
        for conversion in Conversion::ALL {
            assert!(!SlotFit::Converts(conversion).is_error());
        }
        assert_ne!(Binder::Place, Binder::Field("TYPE".into()));
        // Without facts, a user type's membership is not known.
        let typing = type_block(
            &item(),
            &Parts::default().with_field("LIST", known(named("sym_Point"))),
            None,
            &NoFacts,
        );
        assert_eq!(typing.fields["LIST"], converts(Conversion::Same));
        assert_eq!(typing.output, Some(Type::Error));
    }

    // --- Properties ------------------------------------------------------------

    fn arb_type() -> impl Strategy<Value = Type> {
        let leaf = prop_oneof![
            Just(Type::Void),
            Just(Type::Bool),
            Just(Type::Char),
            Just(Type::Int),
            Just(Type::Double),
            Just(Type::String),
            Just(Type::Error),
            Just(named("sym_Point")),
            Just(named("sym_Color")),
            Just(named("sym_Gone")),
        ];
        leaf.prop_recursive(3, 12, 2, |inner| {
            (prop::sample::select(LibType::ALL.to_vec()), inner.clone(), inner).prop_map(|(ty, a, b)| {
                let args = match ty.arity() {
                    0 => vec![],
                    1 => vec![a],
                    _ => vec![a, b],
                };
                Type::lib(ty, args).unwrap_or(Type::Error)
            })
        })
    }

    fn arb_part() -> impl Strategy<Value = PartType> {
        prop_oneof![
            1 => Just(PartType::Empty),
            1 => Just(PartType::Auto),
            1 => Just(PartType::FromPlace),
            1 => Just(PartType::Unknown),
            6 => arb_type().prop_map(PartType::Known),
        ]
    }

    const PATTERNS: [&str; 17] = [
        "T",
        "K",
        "C",
        "int",
        "std::string",
        "std::vector<T>",
        "std::map<K, T>",
        "std::set<K>",
        "elem(C)",
        "index(C)",
        "key(C)",
        "elem(std::vector<T>)",
        "param:F0",
        "member:F0",
        "result",
        "any",
        "field:F1",
    ];

    fn arb_pattern() -> impl Strategy<Value = TypePattern> {
        prop::sample::select(PATTERNS.to_vec()).prop_map(pattern)
    }

    fn arb_class() -> impl Strategy<Value = TypeClass> {
        prop::sample::select(TypeClass::ALL.to_vec())
    }

    prop_compose! {
        /// A signature over the variables `T`, `K` and `C` with up to three
        /// fields `F0` to `F2` of any kind and up to three inputs `I0` to
        /// `I2` (the last one repeated), in either order.
        fn arb_sig()(
            classes in prop::array::uniform3(arb_class()),
            fields in prop::collection::vec((arb_pattern(), 0_u8..4), 0..=3),
            inputs in prop::collection::vec(
                (arb_class(), prop::option::of(arb_pattern()), any::<bool>()),
                0..=3,
            ),
            output in prop::option::of(arb_pattern()),
            place in any::<bool>(),
            fields_first in any::<bool>(),
        ) -> BlockSig {
            let mut sig = BlockSig {
                vars: [("T", classes[0]), ("K", classes[1]), ("C", classes[2])]
                    .into_iter()
                    .map(|(name, class)| (var(name), class))
                    .collect(),
                output,
                place_var: place.then(|| var("C")),
                ..BlockSig::default()
            };
            let mut field_order = Vec::new();
            for (i, (pattern, kind)) in fields.into_iter().enumerate() {
                let name = format!("F{i}");
                match kind {
                    0 => {}
                    1 => {
                        sig.symbol_refs.insert(name.clone());
                    }
                    2 => {
                        sig.symbol_decls.insert(name.clone());
                    }
                    _ => {
                        sig.members.insert(name.clone(), var("T"));
                        continue;
                    }
                }
                sig.fields.insert(name.clone(), pattern);
                field_order.push(PartName::Field(name));
            }
            let mut input_order = Vec::new();
            let count = inputs.len();
            for (i, (check, pattern, assign)) in inputs.into_iter().enumerate() {
                let name = format!("I{i}");
                if i + 1 == count {
                    sig.repeated.insert(name.clone());
                }
                if pattern.is_some() {
                    input_order.push(PartName::Input(name.clone()));
                }
                sig.inputs.insert(name, InputSig { check, pattern, assign });
            }
            sig.order = if fields_first {
                field_order.into_iter().chain(input_order).collect()
            } else {
                input_order.into_iter().chain(field_order).collect()
            };
            sig
        }
    }

    prop_compose! {
        /// Parts for [`arb_sig`]'s blocks: fields `F0` to `F2`, inputs `I0`
        /// to `I2` and two parts of each repeated input.
        fn arb_parts()(
            fields in prop::collection::vec(arb_part(), 3),
            inputs in prop::collection::vec(arb_part(), 9),
            params in prop::collection::vec(arb_part(), 0..3),
            member in arb_part(),
            result in arb_part(),
        ) -> Parts {
            let mut parts = Parts {
                params,
                member: Some(member),
                result: Some(result),
                ..Parts::default()
            };
            for (i, part) in fields.into_iter().enumerate() {
                parts.fields.insert(format!("F{i}"), part);
            }
            let mut inputs = inputs.into_iter();
            for base in ["I0", "I1", "I2"] {
                for suffix in ["", "0", "1"] {
                    if let Some(part) = inputs.next() {
                        parts.inputs.insert(format!("{base}{suffix}"), part);
                    }
                }
            }
            parts
        }
    }

    /// Every fit of a typing, by part.
    fn fits(typing: &BlockTyping) -> BTreeMap<String, SlotFit> {
        typing
            .inputs
            .iter()
            .map(|(name, input)| (name.clone(), input.fit))
            .chain(typing.fields.iter().map(|(name, fit)| (name.clone(), *fit)))
            .collect()
    }

    /// Step 4's fit of a value input that bound nothing, from what it
    /// expects (slots.md §2).
    fn fit_of(part: &PartType, expects: Option<&Type>, input: &InputSig) -> SlotFit {
        let PartType::Known(ty) = part else {
            return SlotFit::Unknown;
        };
        if *ty == Type::Error {
            return SlotFit::Unknown;
        }
        match check_accepts(input.check, ty, &Facts) {
            Some(false) => SlotFit::OutsideClass,
            None => SlotFit::Unknown,
            Some(true) => match expects {
                None => SlotFit::Unknown,
                Some(expects) => {
                    let conversion = if input.assign {
                        assignment(ty, expects)
                    } else {
                        conversion(ty, expects)
                    };
                    if conversion.is_error() {
                        SlotFit::Invalid
                    } else {
                        SlotFit::Converts(conversion)
                    }
                }
            },
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(512))]

        /// Never panics, and gives the same answer twice.
        #[test]
        fn typing_is_deterministic(
            sig in arb_sig(),
            parts in arb_parts(),
            place in prop::option::of(arb_type()),
        ) {
            let first = type_block(&sig, &parts, place.as_ref(), &Facts);
            prop_assert_eq!(&first, &type_block(&sig, &parts, place.as_ref(), &Facts));
            prop_assert_eq!(first.inputs.len(), sig.inputs.keys().map(|base| input_parts(&sig, &parts, base).len()).sum::<usize>());
        }

        /// Replacing any part by `Unknown` never turns another part's fit
        /// from a non-error into an error (slots.md §2's guarantees).
        #[test]
        fn unknown_is_never_worse_than_known(
            sig in arb_sig(),
            parts in arb_parts(),
            place in prop::option::of(arb_type()),
            which in 0_usize..12,
        ) {
            let before = type_block(&sig, &parts, place.as_ref(), &Facts);
            let mut names: Vec<(bool, String)> = parts.fields.keys().map(|n| (true, n.clone())).collect();
            names.extend(parts.inputs.keys().map(|n| (false, n.clone())));
            let (is_field, name) = names[which % names.len()].clone();
            let mut changed = parts.clone();
            if is_field {
                changed.fields.insert(name.clone(), PartType::Unknown);
            } else {
                changed.inputs.insert(name.clone(), PartType::Unknown);
            }
            let after = fits(&type_block(&sig, &changed, place.as_ref(), &Facts));
            for (part, fit) in fits(&before) {
                if part == name || fit.is_error() {
                    continue;
                }
                let now = after.get(&part).copied().unwrap_or(SlotFit::Unknown);
                prop_assert!(!now.is_error(), "{part}: {fit:?} became {now:?} when {name} became unknown");
            }
        }

        /// A candidate typed in place: the fit of a value in an input that
        /// binds nothing (an `assign` input, or one whose pattern binds no
        /// variable: `elem(C)`, `param:F`, `result`) is how it fits what the
        /// input expects while it is not there (as a value typed from its
        /// place), which is what the editor's candidate check computes
        /// before the drop and the analyser after it.
        #[test]
        fn a_candidate_fits_what_its_input_expects_without_it(
            sig in arb_sig(),
            parts in arb_parts(),
            candidate in arb_part(),
        ) {
            // An empty input is not a candidate (and makes what it would
            // bind missing rather than open).
            let Some((base, input)) = sig.inputs.iter().next().filter(|_| candidate != PartType::Empty) else {
                return Ok(());
            };
            let binds = input
                .pattern
                .as_ref()
                .is_some_and(|pattern| !input.assign && !pattern.binding_vars().is_empty());
            if binds {
                return Ok(());
            }
            let part = if sig.repeated.contains(base) { format!("{base}0") } else { base.clone() };
            let mut with = parts.clone();
            with.inputs.insert(part.clone(), candidate.clone());
            let typing = type_block(&sig, &with, None, &Facts);
            let Some(fitted) = typing.inputs.get(&part) else {
                return Ok(());
            };
            let mut without = parts;
            without.inputs.insert(part.clone(), PartType::FromPlace);
            let expects = type_block(&sig, &without, None, &Facts).inputs[&part].expects.clone();
            prop_assert_eq!(&fitted.expects, &expects);
            prop_assert_eq!(fitted.fit, fit_of(&candidate, expects.as_ref(), input));
        }
    }
}
