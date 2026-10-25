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
//! Until the analyser types blocks by their signatures (package 3 of
//! types.md §8.2), [`type_block`] answers that nothing is known: nothing is
//! bound, nothing is expected, every fit is [`SlotFit::Unknown`] and the
//! output is left to the M2 rules. Every block keeps its M2 typing.

use std::collections::BTreeMap;

use b2c_ir::sig::{BlockParts, BlockSig, TypeVar};
use b2c_ir::types::{Type, TypeFacts};

use crate::typing::Conversion;

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
/// For now it answers that nothing is known (see the module docs), whatever
/// its arguments.
pub fn type_block(
    sig: &BlockSig,
    parts: &dyn BlockParts,
    place: Option<&Type>,
    facts: &dyn TypeFacts,
) -> BlockTyping {
    let _ = (sig, parts, place, facts);
    BlockTyping {
        bound: BTreeMap::new(),
        inputs: BTreeMap::new(),
        fields: BTreeMap::new(),
        declares: BTreeMap::new(),
        output: None,
        place: SlotFit::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use b2c_ir::ids::SymbolId;
    use b2c_ir::sig::{InputSig, PartName, PartType, TypePattern};
    use b2c_ir::types::{LibType, NoFacts, TypeClass, UserKind, UserType};

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

    fn vector(of: Type) -> Type {
        Type::lib(LibType::Vector, vec![of]).unwrap()
    }

    /// Facts that know a struct `sym_Point`; any other `Named` is unknown.
    struct Facts;

    impl TypeFacts for Facts {
        fn user_type(&self, id: &SymbolId) -> Option<UserType<'_>> {
            (id.as_str() == "sym_Point").then_some(UserType {
                name: "Point",
                kind: UserKind::Struct,
                comparable: true,
                printable: true,
            })
        }
    }

    /// A block's parts from a table.
    #[derive(Default)]
    struct Parts {
        fields: BTreeMap<&'static str, PartType>,
        inputs: BTreeMap<&'static str, PartType>,
        params: Vec<PartType>,
        result: Option<PartType>,
    }

    impl BlockParts for Parts {
        fn field(&self, name: &str) -> PartType {
            self.fields.get(name).cloned().unwrap_or(PartType::Empty)
        }
        fn input(&self, part: &str) -> PartType {
            self.inputs.get(part).cloned().unwrap_or(PartType::Empty)
        }
        fn count(&self, base: &str) -> usize {
            self.inputs.keys().filter(|name| name.starts_with(base)).count()
        }
        fn param(&self, _: &str, index: usize) -> PartType {
            self.params.get(index).cloned().unwrap_or(PartType::Empty)
        }
        fn member(&self, field: &str, index: usize) -> PartType {
            self.param(field, index)
        }
        fn chosen_member(&self, _: &str, _: &Type) -> PartType {
            PartType::Unknown
        }
        fn result(&self) -> PartType {
            self.result.clone().unwrap_or(PartType::Empty)
        }
    }

    /// One case of the test table: a signature, the block's parts and its
    /// place.
    struct Case {
        name: &'static str,
        sig: BlockSig,
        parts: Parts,
        place: Option<Type>,
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
            ..BlockSig::default()
        }
    }

    /// Two inputs of `T` with `where = { T = "integer" }`: the
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

    /// `call [FUNC] (ARG0) (ARG1) …` and `return (VALUE)`.
    fn call_and_return() -> [BlockSig; 2] {
        [
            BlockSig {
                inputs: BTreeMap::from([("ARG".into(), input(TypeClass::Any, Some("param:FUNC"), false))]),
                order: vec![PartName::Input("ARG".into())],
                ..BlockSig::default()
            },
            BlockSig {
                inputs: BTreeMap::from([("VALUE".into(), input(TypeClass::Any, Some("result"), false))]),
                order: vec![PartName::Input("VALUE".into())],
                ..BlockSig::default()
            },
        ]
    }

    fn known(ty: Type) -> PartType {
        PartType::Known(ty)
    }

    /// The test table: the cases slots.md §9 lists for each step of the rule.
    fn cases() -> Vec<Case> {
        let mut cases = variable_cases();
        cases.extend(collection_cases());
        cases.extend(other_cases());
        cases
    }

    /// `create` and `set`: binding from fields, `auto`, empty binders and
    /// `assign`.
    fn variable_cases() -> Vec<Case> {
        vec![
            Case {
                name: "a type field binds T, the value converts",
                sig: declare(),
                parts: Parts {
                    fields: BTreeMap::from([("TYPE", known(Type::Double))]),
                    inputs: BTreeMap::from([("VALUE", known(Type::Int))]),
                    ..Parts::default()
                },
                place: None,
            },
            Case {
                name: "auto binds nothing; the value binds T",
                sig: declare(),
                parts: Parts {
                    fields: BTreeMap::from([("TYPE", PartType::Auto)]),
                    inputs: BTreeMap::from([("VALUE", known(Type::String))]),
                    ..Parts::default()
                },
                place: None,
            },
            Case {
                name: "an empty binder makes the expectation Error, not open",
                sig: set(),
                parts: Parts {
                    inputs: BTreeMap::from([("VALUE", PartType::FromPlace)]),
                    ..Parts::default()
                },
                place: None,
            },
            Case {
                name: "assign: a char into a string, and it never binds",
                sig: set(),
                parts: Parts {
                    fields: BTreeMap::from([("VAR", known(Type::String))]),
                    inputs: BTreeMap::from([("VALUE", known(Type::Char))]),
                    ..Parts::default()
                },
                place: None,
            },
        ]
    }

    /// Classes, shapes, unknown membership and places.
    fn collection_cases() -> Vec<Case> {
        let point = Type::Named(SymbolId::new("sym_Point").unwrap());
        let gone = Type::Named(SymbolId::new("sym_Gone").unwrap());
        vec![
            Case {
                name: "a class mismatch and a shape mismatch",
                sig: item(),
                parts: Parts {
                    fields: BTreeMap::from([(
                        "LIST",
                        known(Type::lib(LibType::Set, vec![Type::Int]).unwrap()),
                    )]),
                    inputs: BTreeMap::from([("INDEX", known(Type::Int))]),
                    ..Parts::default()
                },
                place: None,
            },
            Case {
                name: "unknown membership binds Error",
                sig: item(),
                parts: Parts {
                    fields: BTreeMap::from([("LIST", known(vector(gone.clone())))]),
                    inputs: BTreeMap::from([("INDEX", known(Type::Int))]),
                    ..Parts::default()
                },
                place: None,
            },
            Case {
                name: "a place-typed block with a place",
                sig: list_of(),
                parts: Parts {
                    inputs: BTreeMap::from([("ITEM0", known(Type::Int)), ("ITEM1", known(Type::Double))]),
                    ..Parts::default()
                },
                place: Some(vector(point.clone())),
            },
            Case {
                name: "a place-typed block without a place, and outside its class",
                sig: list_of(),
                parts: Parts::default(),
                place: Some(Type::Int),
            },
        ]
    }

    /// The guarantees' counterexample, `param:` and `result`.
    fn other_cases() -> Vec<Case> {
        let [call, ret] = call_and_return();
        vec![
            Case {
                name: "the first binding wins; a later narrowing warns",
                sig: two_integers(),
                parts: Parts {
                    inputs: BTreeMap::from([("A", known(Type::Int)), ("B", known(Type::Double))]),
                    ..Parts::default()
                },
                place: None,
            },
            Case {
                name: "an unknown binder never turns a fit into an error",
                sig: two_integers(),
                parts: Parts {
                    inputs: BTreeMap::from([("A", PartType::Unknown), ("B", known(Type::Double))]),
                    ..Parts::default()
                },
                place: None,
            },
            Case {
                name: "param: by index, also out of range",
                sig: call,
                parts: Parts {
                    inputs: BTreeMap::from([("ARG0", known(Type::Int)), ("ARG1", known(Type::String))]),
                    params: vec![known(Type::Double)],
                    ..Parts::default()
                },
                place: None,
            },
            Case {
                name: "result of a void function",
                sig: ret,
                parts: Parts {
                    inputs: BTreeMap::from([("VALUE", known(Type::Int))]),
                    result: Some(PartType::Empty),
                    ..Parts::default()
                },
                place: None,
            },
        ]
    }

    /// The stub's answer: nothing known, so nothing is ever refused and every
    /// block keeps its M2 typing. The cases' expected typings per slots.md §2
    /// replace this check with the analyser's own typing (package 3).
    #[test]
    fn every_case_is_typed_as_unknown_for_now() {
        for case in cases() {
            let typing = type_block(&case.sig, &case.parts, case.place.as_ref(), &Facts);
            assert!(typing.bound.is_empty(), "{}", case.name);
            assert!(
                typing.inputs.values().all(|input| input.fit == SlotFit::Unknown),
                "{}",
                case.name
            );
            assert!(
                typing.fields.values().all(|fit| *fit == SlotFit::Unknown),
                "{}",
                case.name
            );
            assert_eq!(typing.output, None, "{}", case.name);
            assert_eq!(typing.place, SlotFit::Unknown, "{}", case.name);
            // Deterministic, also with other facts.
            assert_eq!(
                typing,
                type_block(&case.sig, &case.parts, case.place.as_ref(), &NoFacts)
            );
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
    }
}
