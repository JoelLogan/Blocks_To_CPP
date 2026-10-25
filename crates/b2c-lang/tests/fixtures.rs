//! Writes the shared fixtures that hold the editor's TypeScript copies of the
//! type rules and of typed slots to the analyser's:
//!
//! * `packages/blockly-ext/src/types/fixture.json` (types.md §9): the sample
//!   types, their keys, labels and C++ spellings, every conversion and
//!   assignment between them, class membership and `check` acceptance under
//!   two sets of facts (the second does not know the struct), the type
//!   functions, and pattern bindings;
//! * `packages/blockly-ext/src/types/slot-fixture.json` (slots.md §7):
//!   hand-written `type_block` cases for every rule of slots.md §2, a seeded
//!   sample of part combinations for each catalog block with a pattern, and
//!   the value types the analyser gives the blocks whose output is `any` or
//!   `number`.
//!
//! Both are committed. This test fails when one is stale; to write them
//! again, run from the repository root:
//!
//! ```sh
//! B2C_UPDATE_TYPE_FIXTURES=1 cargo test -p b2c-lang --test fixtures
//! ```
//!
//! **The JSON shapes.** A type is its term (`"int"`, `{"std::vector":
//! "int"}`, `{"ref": "sym_Point"}`) and `Type::Error` is `"error"`, as on the
//! wire (types.md §3.1). A pattern is tagged JSON as `catalog.json` writes it
//! (`{"kind": "var", "name": "C"}`, types.md §4.5). A part is `{"kind":
//! "empty" | "auto" | "fromPlace" | "unknown"}` or `{"kind": "known", "type":
//! …}`; a fit is a conversion's name (`same`, `widening`, `narrowing`,
//! `boolNumber`, `invalid`), `outsideClass` or `unknown`. A case's parts
//! answer as a table: `field(name)` from `fields` (else empty), `input(part)`
//! from `inputs` (else empty), `count(base)` from `counts` (else 0),
//! `param(field, i)` and `member(field, i)` from `params[field][i]` and
//! `members[field][i]` (else empty), `chosenMember(field, owner)` from
//! `chosenMembers[field]` whatever the owner (else unknown), and `result()`
//! from `result` (else empty).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::print_stderr,
    reason = "test helpers fail the test by panicking; the shared g++ helper notes a missing compiler"
)]

mod common;

use std::collections::BTreeMap;

use b2c_ir::ids::SymbolId;
use b2c_ir::sig::{BlockParts, BlockSig, Fit, InputSig, PartName, PartType, TypeEnv, TypePattern, TypeVar};
use b2c_ir::types::{LibType, Type, TypeClass, TypeFacts, TypeFn, UserKind, UserType, sample_types};
use b2c_lang::{Binder, BlockTyping, SlotFit, analyze, assignment, check_accepts, conversion, type_block};
use serde_json::{Map, Value, json};

/// The variable that makes the test write the files instead of comparing.
const UPDATE: &str = "B2C_UPDATE_TYPE_FIXTURES";

/// The type fixture, relative to the repository root.
const TYPES_PATH: &str = "packages/blockly-ext/src/types/fixture.json";

/// The slot fixture, relative to the repository root.
const SLOTS_PATH: &str = "packages/blockly-ext/src/types/slot-fixture.json";

/// The most sampled part combinations per catalog block (slots.md §7).
const SAMPLES_PER_BLOCK: usize = 50;

// --- Facts ---------------------------------------------------------------------

/// Facts from a table, as the fixture writes them.
#[derive(Debug, Clone, Default)]
struct Facts(BTreeMap<String, (String, UserKind, bool, bool)>);

impl TypeFacts for Facts {
    fn user_type(&self, id: &SymbolId) -> Option<UserType<'_>> {
        self.0
            .get(id.as_str())
            .map(|(name, kind, comparable, printable)| UserType {
                name,
                kind: *kind,
                comparable: *comparable,
                printable: *printable,
            })
    }
}

impl Facts {
    fn json(&self) -> Value {
        let users: Map<String, Value> = self
            .0
            .iter()
            .map(|(id, (name, kind, comparable, printable))| {
                let kind = match kind {
                    UserKind::Struct => "struct",
                    UserKind::Enum => "enum",
                };
                (
                    id.clone(),
                    json!({"name": name, "kind": kind, "comparable": comparable, "printable": printable}),
                )
            })
            .collect();
        Value::Object(users)
    }
}

/// The two sets of facts: `known` knows the sample struct (comparable, not
/// printable) and the sample enum; `partial` knows only the enum, so the
/// struct's class membership is not known.
fn facts() -> Vec<(&'static str, Facts)> {
    let point = (
        String::from(b2c_ir::types::SAMPLE_STRUCT),
        (String::from("Point"), UserKind::Struct, true, false),
    );
    let color = (
        String::from(b2c_ir::types::SAMPLE_ENUM),
        (String::from("Color"), UserKind::Enum, true, true),
    );
    vec![
        ("known", Facts(BTreeMap::from([point, color.clone()]))),
        ("partial", Facts(BTreeMap::from([color]))),
    ]
}

// --- JSON of types, patterns, parts and fits ------------------------------------

/// A type as the wire writes it (types.md §3.1).
fn type_json(ty: &Type) -> Value {
    ty.to_term()
        .map_or_else(|| json!("error"), |term| serde_json::to_value(term).unwrap())
}

fn optional_type_json(ty: Option<&Type>) -> Value {
    ty.map_or(Value::Null, type_json)
}

/// A pattern as `catalog.json` writes it (types.md §4.5).
fn pattern_json(pattern: &TypePattern) -> Value {
    match pattern {
        TypePattern::Any => json!({"kind": "any"}),
        TypePattern::Number => json!({"kind": "number"}),
        TypePattern::Type(ty) => json!({"kind": "type", "type": type_json(ty)}),
        TypePattern::Var(var) => json!({"kind": "var", "name": var.as_str()}),
        TypePattern::Lib(ty, args) => json!({
            "kind": "lib",
            "name": ty.cpp_name(),
            "args": args.iter().map(pattern_json).collect::<Vec<_>>(),
        }),
        TypePattern::Fn(function, arg) => {
            json!({"kind": "fn", "fn": function.name(), "of": pattern_json(arg)})
        }
        TypePattern::Field(name) => json!({"kind": "field", "field": name}),
        TypePattern::Symbol(name) => json!({"kind": "symbol", "field": name}),
        TypePattern::Param(name) => json!({"kind": "param", "field": name}),
        TypePattern::Member(name) => json!({"kind": "member", "field": name}),
        TypePattern::Result => json!({"kind": "result"}),
    }
}

fn part_json(part: &PartType) -> Value {
    match part {
        PartType::Empty => json!({"kind": "empty"}),
        PartType::Auto => json!({"kind": "auto"}),
        PartType::FromPlace => json!({"kind": "fromPlace"}),
        PartType::Unknown => json!({"kind": "unknown"}),
        PartType::Known(ty) => json!({"kind": "known", "type": type_json(ty)}),
    }
}

fn fit_name(fit: SlotFit) -> &'static str {
    match fit {
        SlotFit::Unknown => "unknown",
        SlotFit::OutsideClass => "outsideClass",
        SlotFit::Invalid => "invalid",
        SlotFit::Converts(conversion) => conversion.name(),
    }
}

fn binding_fit_name(fit: Fit) -> &'static str {
    match fit {
        Fit::Fits => "fits",
        Fit::Unknown => "unknown",
        Fit::OutsideClass => "outsideClass",
        Fit::BadShape => "badShape",
    }
}

fn typing_json(typing: &BlockTyping) -> Value {
    let bound: Map<String, Value> = typing
        .bound
        .iter()
        .map(|(var, (ty, binder))| {
            let binder = match binder {
                Binder::Field(name) => json!({"kind": "field", "name": name}),
                Binder::Input(name) => json!({"kind": "input", "name": name}),
                Binder::Place => json!({"kind": "place"}),
            };
            (
                var.as_str().to_owned(),
                json!({"type": type_json(ty), "binder": binder}),
            )
        })
        .collect();
    let inputs: Map<String, Value> = typing
        .inputs
        .iter()
        .map(|(part, input)| {
            (
                part.clone(),
                json!({"expects": optional_type_json(input.expects.as_ref()), "fit": fit_name(input.fit)}),
            )
        })
        .collect();
    let fields: Map<String, Value> = typing
        .fields
        .iter()
        .map(|(name, fit)| (name.clone(), json!(fit_name(*fit))))
        .collect();
    let declares: Map<String, Value> = typing
        .declares
        .iter()
        .map(|(name, ty)| (name.clone(), type_json(ty)))
        .collect();
    json!({
        "bound": bound,
        "inputs": inputs,
        "fields": fields,
        "declares": declares,
        "output": optional_type_json(typing.output.as_ref()),
        "place": fit_name(typing.place),
    })
}

/// A signature as `catalog.json` writes a block's (types.md §4.5), with the
/// keys typing reads: `typeVars`, `output`, `order`, `placeVar`, `members`,
/// and each field's `kind` and `type` and each input's `check`, `type`,
/// `assign` and `repeat`.
fn sig_json(id: &str, sig: &BlockSig) -> Value {
    let type_vars: Map<String, Value> = sig
        .vars
        .iter()
        .map(|(var, class)| (var.as_str().to_owned(), json!(class.name())))
        .collect();
    let order: Vec<Value> = sig
        .order
        .iter()
        .map(|part| match part {
            PartName::Field(name) => json!({"field": name}),
            PartName::Input(name) => json!({"input": name}),
        })
        .collect();
    let members: Map<String, Value> = sig
        .members
        .iter()
        .map(|(field, var)| (field.clone(), json!(var.as_str())))
        .collect();
    let mut fields: Vec<Value> = sig
        .fields
        .iter()
        .map(|(name, pattern)| {
            let kind = if sig.symbol_decls.contains(name) {
                "symbol_decl"
            } else if sig.symbol_refs.contains(name) {
                "symbol_ref"
            } else {
                "type"
            };
            json!({"name": name, "kind": kind, "type": pattern_json(pattern)})
        })
        .collect();
    fields.extend(
        sig.members
            .keys()
            .map(|name| json!({"name": name, "kind": "symbol_ref"})),
    );
    let inputs: Vec<Value> = sig
        .inputs
        .iter()
        .map(|(name, input)| {
            let repeat = if sig.repeated.contains(name) {
                json!({"count": format!("{}Count", name.to_lowercase()), "plus": 0})
            } else {
                Value::Null
            };
            json!({
                "name": name,
                "check": input.check.name(),
                "type": input.pattern.as_ref().map_or(Value::Null, pattern_json),
                "assign": input.assign,
                "repeat": repeat,
            })
        })
        .collect();
    json!({
        "id": id,
        "typeVars": type_vars,
        "output": sig.output.as_ref().map_or(Value::Null, pattern_json),
        "order": order,
        "placeVar": sig.place_var.as_ref().map(TypeVar::as_str),
        "members": members,
        "field": fields,
        "input": inputs,
    })
}

// --- The type fixture ----------------------------------------------------------------

fn var(name: &str) -> TypeVar {
    TypeVar::new(name).unwrap()
}

fn pattern(text: &str) -> TypePattern {
    TypePattern::parse(text).unwrap()
}

/// Parts with nothing to answer (a binding reads none).
struct NoParts;

impl BlockParts for NoParts {
    fn field(&self, _: &str) -> PartType {
        PartType::Empty
    }
    fn input(&self, _: &str) -> PartType {
        PartType::Empty
    }
    fn count(&self, _: &str) -> usize {
        0
    }
    fn param(&self, _: &str, _: usize) -> PartType {
        PartType::Empty
    }
    fn member(&self, _: &str, _: usize) -> PartType {
        PartType::Empty
    }
    fn chosen_member(&self, _: &str, _: &Type) -> PartType {
        PartType::Unknown
    }
    fn result(&self) -> PartType {
        PartType::Empty
    }
}

/// The pattern bindings of the type fixture: a pattern, its variables'
/// classes, and what binding each sample type gives, with the type
/// functions resolved over the bound variable.
fn bindings(samples: &[Type], facts: &[(&'static str, Facts)]) -> Vec<Value> {
    let patterns: [(&str, &[(&str, TypeClass)]); 6] = [
        ("T", &[("T", TypeClass::Value)]),
        ("C", &[("C", TypeClass::Indexable)]),
        ("T", &[("T", TypeClass::Integer)]),
        ("std::vector<T>", &[("T", TypeClass::Number)]),
        (
            "std::map<K, V>",
            &[("K", TypeClass::Comparable), ("V", TypeClass::Any)],
        ),
        ("std::set<T>", &[("T", TypeClass::Struct)]),
    ];
    let derived = [
        "elem(C)",
        "index(C)",
        "key(C)",
        "std::vector<T>",
        "std::map<V, K>",
    ];
    let mut cases = Vec::new();
    for (text, vars) in patterns {
        let pattern = pattern(text);
        let sig = BlockSig {
            vars: vars.iter().map(|(name, class)| (var(name), *class)).collect(),
            ..BlockSig::default()
        };
        for (facts_name, facts) in facts {
            for ty in samples {
                let mut env = TypeEnv::new(&sig);
                let fit = env.bind(
                    &pattern,
                    &PartType::Known(ty.clone()),
                    PartName::Field(String::from("F")),
                    facts,
                );
                let bound: Map<String, Value> = vars
                    .iter()
                    .filter_map(|(name, _)| {
                        let (ty, _) = env.get(&var(name))?;
                        Some(((*name).to_owned(), type_json(ty)))
                    })
                    .collect();
                let resolved: Map<String, Value> = derived
                    .iter()
                    .filter(|derived| pattern_vars_cover(derived, vars))
                    .map(|derived| {
                        let resolution = match env.resolve(&self::pattern(derived), &NoParts, None) {
                            b2c_ir::sig::Resolution::Type(ty) => type_json(&ty),
                            b2c_ir::sig::Resolution::Missing => json!("missing"),
                            b2c_ir::sig::Resolution::Open => json!("open"),
                        };
                        ((*derived).to_owned(), resolution)
                    })
                    .collect();
                cases.push(json!({
                    "pattern": pattern_json(&pattern),
                    "vars": vars.iter().map(|(name, class)| ((*name).to_owned(), json!(class.name()))).collect::<Map<_, _>>(),
                    "facts": facts_name,
                    "type": type_json(ty),
                    "fit": binding_fit_name(fit),
                    "bound": bound,
                    "resolved": resolved,
                }));
            }
        }
    }
    cases
}

/// Whether every variable of `pattern` is one of `vars`.
fn pattern_vars_cover(text: &str, vars: &[(&str, TypeClass)]) -> bool {
    pattern(text)
        .vars()
        .all(|v| vars.iter().any(|(name, _)| *name == v.as_str()))
}

fn types_fixture() -> Value {
    let samples = sample_types();
    let facts = facts();
    let samples_json: Vec<Value> = samples
        .iter()
        .map(|ty| {
            let key = ty.to_term().map(|term| term.key());
            let mut labels = Map::new();
            let mut spellings = Map::new();
            for (name, facts) in &facts {
                labels.insert((*name).to_owned(), json!(ty.label(facts).to_string()));
                spellings.insert((*name).to_owned(), json!(ty.cpp(facts).to_string()));
            }
            json!({"type": type_json(ty), "key": key, "label": labels, "cpp": spellings})
        })
        .collect();
    let matrix = |rule: fn(&Type, &Type) -> b2c_lang::Conversion| -> Vec<Vec<&'static str>> {
        samples
            .iter()
            .map(|from| samples.iter().map(|to| rule(from, to).name()).collect())
            .collect()
    };
    let by_class = |answer: &dyn Fn(TypeClass, &Type, &Facts) -> Option<bool>| -> Value {
        let mut out = Map::new();
        for (name, facts) in &facts {
            let classes: Map<String, Value> = TypeClass::ALL
                .iter()
                .map(|class| {
                    let row: Vec<Value> = samples
                        .iter()
                        .map(|ty| answer(*class, ty, facts).map_or(Value::Null, Value::Bool))
                        .collect();
                    (class.name().to_owned(), Value::Array(row))
                })
                .collect();
            out.insert((*name).to_owned(), Value::Object(classes));
        }
        Value::Object(out)
    };
    let type_fns: Map<String, Value> = TypeFn::ALL
        .iter()
        .map(|function| {
            let row: Vec<Value> = samples
                .iter()
                .map(|ty| optional_type_json(function.apply(ty).as_ref()))
                .collect();
            (function.name().to_owned(), Value::Array(row))
        })
        .collect();
    let lib_types: Vec<Value> = LibType::ALL
        .iter()
        .map(|ty| {
            json!({
                "name": ty.cpp_name(),
                "arity": ty.arity(),
                "label": ty.label(),
                "header": ty.header().as_str(),
            })
        })
        .collect();
    json!({
        "description": "Written by crates/b2c-lang/tests/fixtures.rs: the analyser's type rules over sample types (types.md §9). Rows of conversions, assignments, classes, checks and typeFns follow the order of samples.",
        "facts": facts.iter().map(|(name, facts)| ((*name).to_owned(), facts.json())).collect::<Map<_, _>>(),
        "libTypes": lib_types,
        "samples": samples_json,
        "conversions": matrix(conversion),
        "assignments": matrix(assignment),
        "classes": by_class(&|class, ty, facts| ty.is_in(class, facts)),
        "checks": by_class(&|class, ty, facts| check_accepts(class, ty, facts)),
        "typeFns": type_fns,
        "bindings": bindings(&samples, &facts),
    })
}

// --- The slot fixture ------------------------------------------------------------------

/// A block's parts from a table, as the fixture writes them.
#[derive(Debug, Clone, Default)]
struct Parts {
    fields: BTreeMap<String, PartType>,
    inputs: BTreeMap<String, PartType>,
    counts: BTreeMap<String, usize>,
    params: BTreeMap<String, Vec<PartType>>,
    members: BTreeMap<String, Vec<PartType>>,
    chosen_members: BTreeMap<String, PartType>,
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
        self.counts.get(base).copied().unwrap_or(0)
    }
    fn param(&self, field: &str, index: usize) -> PartType {
        self.params
            .get(field)
            .and_then(|list| list.get(index))
            .cloned()
            .unwrap_or(PartType::Empty)
    }
    fn member(&self, field: &str, index: usize) -> PartType {
        self.members
            .get(field)
            .and_then(|list| list.get(index))
            .cloned()
            .unwrap_or(PartType::Empty)
    }
    fn chosen_member(&self, field: &str, _: &Type) -> PartType {
        self.chosen_members
            .get(field)
            .cloned()
            .unwrap_or(PartType::Unknown)
    }
    fn result(&self) -> PartType {
        self.result.clone().unwrap_or(PartType::Empty)
    }
}

impl Parts {
    fn field(mut self, name: &str, part: PartType) -> Self {
        self.fields.insert(name.to_owned(), part);
        self
    }

    fn input(mut self, name: &str, part: PartType) -> Self {
        self.inputs.insert(name.to_owned(), part);
        self
    }

    fn count(mut self, base: &str, count: usize) -> Self {
        self.counts.insert(base.to_owned(), count);
        self
    }

    fn json(&self) -> Value {
        let map = |parts: &BTreeMap<String, PartType>| -> Map<String, Value> {
            parts
                .iter()
                .map(|(name, part)| (name.clone(), part_json(part)))
                .collect()
        };
        let lists = |parts: &BTreeMap<String, Vec<PartType>>| -> Map<String, Value> {
            parts
                .iter()
                .map(|(name, list)| (name.clone(), Value::Array(list.iter().map(part_json).collect())))
                .collect()
        };
        json!({
            "fields": map(&self.fields),
            "inputs": map(&self.inputs),
            "counts": self.counts,
            "params": lists(&self.params),
            "members": lists(&self.members),
            "chosenMembers": map(&self.chosen_members),
            "result": self.result.as_ref().map_or(Value::Null, part_json),
        })
    }
}

fn known(ty: Type) -> PartType {
    PartType::Known(ty)
}

fn vector(of: Type) -> Type {
    Type::lib(LibType::Vector, vec![of]).unwrap()
}

fn named(id: &str) -> Type {
    Type::Named(SymbolId::new(id).unwrap())
}

fn input_sig(check: TypeClass, text: Option<&str>, assign: bool) -> InputSig {
    InputSig {
        check,
        pattern: text.map(pattern),
        assign,
    }
}

/// The hand-written signatures of the slot fixture, by an ID of their own.
#[allow(clippy::too_many_lines, reason = "a table")]
fn hand_written_sigs() -> Vec<(&'static str, BlockSig)> {
    let mut sigs: Vec<(&'static str, BlockSig)> = common::m2_sigs()
        .iter()
        .filter_map(|(id, sig)| match id {
            "var.declare" => Some(("create", sig.clone())),
            "var.set" => Some(("set", sig.clone())),
            "func.call" => Some(("call", sig.clone())),
            "func.return" => Some(("return", sig.clone())),
            _ => None,
        })
        .collect();
    sigs.extend([
        (
            "item of",
            BlockSig {
                vars: BTreeMap::from([(var("C"), TypeClass::Indexable)]),
                output: Some(pattern("elem(C)")),
                fields: BTreeMap::from([(String::from("LIST"), pattern("C"))]),
                inputs: BTreeMap::from([(
                    String::from("INDEX"),
                    input_sig(TypeClass::Any, Some("index(C)"), false),
                )]),
                order: vec![PartName::Field("LIST".into()), PartName::Input("INDEX".into())],
                symbol_refs: [String::from("LIST")].into(),
                ..BlockSig::default()
            },
        ),
        (
            "two lists",
            BlockSig {
                vars: BTreeMap::from([(var("T"), TypeClass::Number)]),
                output: Some(pattern("T")),
                fields: BTreeMap::from([
                    (String::from("LIST"), pattern("std::vector<T>")),
                    (String::from("OTHER"), pattern("std::vector<T>")),
                ]),
                order: vec![PartName::Field("LIST".into()), PartName::Field("OTHER".into())],
                symbol_refs: [String::from("LIST"), String::from("OTHER")].into(),
                ..BlockSig::default()
            },
        ),
        (
            "list of",
            BlockSig {
                vars: BTreeMap::from([(var("C"), TypeClass::Listable)]),
                output: Some(pattern("C")),
                inputs: BTreeMap::from([(
                    String::from("ITEM"),
                    input_sig(TypeClass::Any, Some("elem(C)"), false),
                )]),
                order: vec![PartName::Input("ITEM".into())],
                place_var: Some(var("C")),
                repeated: [String::from("ITEM")].into(),
                ..BlockSig::default()
            },
        ),
        (
            "two integers",
            BlockSig {
                vars: BTreeMap::from([(var("T"), TypeClass::Integer)]),
                inputs: BTreeMap::from([
                    (String::from("A"), input_sig(TypeClass::Any, Some("T"), false)),
                    (String::from("B"), input_sig(TypeClass::Any, Some("T"), false)),
                ]),
                order: vec![PartName::Input("A".into()), PartName::Input("B".into())],
                ..BlockSig::default()
            },
        ),
        (
            "texts",
            BlockSig {
                vars: BTreeMap::from([(var("T"), TypeClass::Text)]),
                inputs: BTreeMap::from([
                    (String::from("A"), input_sig(TypeClass::Any, Some("T"), false)),
                    (String::from("B"), input_sig(TypeClass::Any, Some("T"), true)),
                    (String::from("C"), input_sig(TypeClass::Any, Some("T"), false)),
                ]),
                order: vec![
                    PartName::Input("A".into()),
                    PartName::Input("B".into()),
                    PartName::Input("C".into()),
                ],
                ..BlockSig::default()
            },
        ),
        (
            "new with",
            BlockSig {
                vars: BTreeMap::from([(var("S"), TypeClass::Struct)]),
                output: Some(pattern("S")),
                fields: BTreeMap::from([(String::from("TYPE"), pattern("S"))]),
                inputs: BTreeMap::from([(
                    String::from("FIELD"),
                    input_sig(TypeClass::Any, Some("member:TYPE"), false),
                )]),
                order: vec![PartName::Field("TYPE".into()), PartName::Input("FIELD".into())],
                repeated: [String::from("FIELD")].into(),
                ..BlockSig::default()
            },
        ),
        (
            "member",
            BlockSig {
                vars: BTreeMap::from([(var("S"), TypeClass::Struct)]),
                output: Some(pattern("symbol:MEMBER")),
                fields: BTreeMap::from([(String::from("OBJ"), pattern("S"))]),
                order: vec![PartName::Field("OBJ".into())],
                members: BTreeMap::from([(String::from("MEMBER"), var("S"))]),
                symbol_refs: [String::from("OBJ")].into(),
                ..BlockSig::default()
            },
        ),
        (
            "checks",
            BlockSig {
                inputs: BTreeMap::from([
                    (String::from("BOOL"), input_sig(TypeClass::Bool, None, false)),
                    (String::from("INT"), input_sig(TypeClass::Integer, None, false)),
                    (String::from("NUM"), input_sig(TypeClass::Number, None, false)),
                    (
                        String::from("COLL"),
                        input_sig(TypeClass::Collection, None, false),
                    ),
                ]),
                ..BlockSig::default()
            },
        ),
    ]);
    sigs
}

/// One hand-written case: a signature by ID, its parts, its place and the
/// facts by name.
struct Case {
    name: &'static str,
    sig: &'static str,
    parts: Parts,
    place: Option<Type>,
    facts: &'static str,
}

#[allow(clippy::too_many_lines, reason = "a table")]
fn hand_written_cases() -> Vec<Case> {
    let point = named(b2c_ir::types::SAMPLE_STRUCT);
    let set = Type::lib(LibType::Set, vec![Type::Int]).unwrap();
    let case = |name, sig, parts, place, facts| Case {
        name,
        sig,
        parts,
        place,
        facts,
    };
    vec![
        case(
            "a type field binds T, the value converts",
            "create",
            Parts::default()
                .field("TYPE", known(Type::Double))
                .input("VALUE", known(Type::Int)),
            None,
            "known",
        ),
        case(
            "auto binds nothing; the value binds T",
            "create",
            Parts::default()
                .field("TYPE", PartType::Auto)
                .input("VALUE", known(Type::String)),
            None,
            "known",
        ),
        case(
            "auto and a value typed from its place: nothing gives a type",
            "create",
            Parts::default()
                .field("TYPE", PartType::Auto)
                .input("VALUE", PartType::FromPlace),
            None,
            "known",
        ),
        case(
            "a type outside the variable's class binds Error",
            "create",
            Parts::default()
                .field("TYPE", known(Type::lib(LibType::IfStream, vec![]).unwrap()))
                .input("VALUE", known(Type::Int)),
            None,
            "known",
        ),
        case(
            "an empty binder makes the expectation Error, not open",
            "set",
            Parts::default().input("VALUE", PartType::FromPlace),
            None,
            "known",
        ),
        case(
            "assign: a char into a string, and it never binds",
            "set",
            Parts::default()
                .field("VAR", known(Type::String))
                .input("VALUE", known(Type::Char)),
            None,
            "known",
        ),
        case(
            "text into an int variable does not convert",
            "set",
            Parts::default()
                .field("VAR", known(Type::Int))
                .input("VALUE", known(Type::String)),
            None,
            "known",
        ),
        case(
            "an unknown reference binds Error",
            "set",
            Parts::default()
                .field("VAR", PartType::Unknown)
                .input("VALUE", known(Type::String)),
            None,
            "known",
        ),
        case(
            "a class mismatch binds Error, which then fits",
            "item of",
            Parts::default()
                .field("LIST", known(set.clone()))
                .input("INDEX", known(Type::Int)),
            None,
            "known",
        ),
        case(
            "a list binds C; the index and the output follow it",
            "item of",
            Parts::default()
                .field("LIST", known(vector(Type::String)))
                .input("INDEX", known(Type::Double)),
            None,
            "known",
        ),
        case(
            "unknown membership binds Error",
            "item of",
            Parts::default()
                .field("LIST", known(point.clone()))
                .input("INDEX", known(Type::String)),
            None,
            "partial",
        ),
        case(
            "a shape mismatch is invalid",
            "two lists",
            Parts::default()
                .field("LIST", known(set))
                .field("OTHER", known(vector(Type::Int))),
            None,
            "known",
        ),
        case(
            "a reference never converts to what an earlier part bound",
            "two lists",
            Parts::default()
                .field("LIST", known(vector(Type::Int)))
                .field("OTHER", known(vector(Type::Double))),
            None,
            "known",
        ),
        case(
            "a place-typed block with a place",
            "list of",
            Parts::default()
                .count("ITEM", 2)
                .input("ITEM0", known(point.clone()))
                .input("ITEM1", known(Type::Double)),
            Some(vector(point.clone())),
            "known",
        ),
        case(
            "a place-typed block without a place",
            "list of",
            Parts::default().count("ITEM", 1).input("ITEM0", known(Type::Int)),
            None,
            "known",
        ),
        case(
            "a place-typed block outside its class",
            "list of",
            Parts::default().count("ITEM", 1).input("ITEM0", known(Type::Int)),
            Some(Type::Int),
            "known",
        ),
        case(
            "a place whose membership is not known",
            "list of",
            Parts::default(),
            Some(point.clone()),
            "partial",
        ),
        case(
            "the first binding wins; a later narrowing warns",
            "two integers",
            Parts::default()
                .input("A", known(Type::Int))
                .input("B", known(Type::Double)),
            None,
            "known",
        ),
        case(
            "the counterexample: an unknown binder never turns a fit into an error",
            "two integers",
            Parts::default()
                .input("A", PartType::Unknown)
                .input("B", known(Type::Double)),
            None,
            "known",
        ),
        case(
            "the second counterexample: an assigned char among strings does not bind",
            "texts",
            Parts::default()
                .input("A", PartType::Unknown)
                .input("B", known(Type::Char))
                .input("C", known(Type::String)),
            None,
            "known",
        ),
        case(
            "param: by index, also out of range",
            "call",
            Parts {
                params: BTreeMap::from([(String::from("FUNC"), vec![known(Type::Double)])]),
                ..Parts::default()
            }
            .field("FUNC", known(Type::Int))
            .count("ARG", 2)
            .input("ARG0", known(Type::Int))
            .input("ARG1", known(Type::String)),
            None,
            "known",
        ),
        case(
            "result of a function",
            "return",
            Parts {
                result: Some(known(Type::Double)),
                ..Parts::default()
            }
            .input("VALUE", known(Type::String)),
            None,
            "known",
        ),
        case(
            "result of a void function",
            "return",
            Parts::default().input("VALUE", known(Type::Int)),
            None,
            "known",
        ),
        case(
            "member: a struct's fields by index",
            "new with",
            Parts {
                members: BTreeMap::from([(
                    String::from("TYPE"),
                    vec![known(Type::Int), known(Type::String)],
                )]),
                ..Parts::default()
            }
            .field("TYPE", known(point.clone()))
            .count("FIELD", 3)
            .input("FIELD0", known(Type::Double))
            .input("FIELD1", known(Type::Int))
            .input("FIELD2", known(Type::Int)),
            None,
            "known",
        ),
        case(
            "a members field fits through its owner",
            "member",
            Parts {
                chosen_members: BTreeMap::from([(String::from("MEMBER"), known(Type::Int))]),
                ..Parts::default()
            }
            .field("OBJ", known(point.clone())),
            None,
            "known",
        ),
        case(
            "a members field whose owner is not known",
            "member",
            Parts {
                chosen_members: BTreeMap::from([(String::from("MEMBER"), known(Type::Int))]),
                ..Parts::default()
            }
            .field("OBJ", known(point)),
            None,
            "partial",
        ),
        case(
            "checks: bool, integer and number by conversion, the others by membership",
            "checks",
            Parts::default()
                .input("BOOL", known(Type::Int))
                .input("INT", known(Type::Double))
                .input("NUM", known(Type::String))
                .input("COLL", known(Type::Int)),
            None,
            "known",
        ),
    ]
}

/// A tiny deterministic generator (xorshift), so the samples never change
/// between runs or platforms.
struct Rng(u64);

impl Rng {
    fn below(&mut self, n: usize) -> usize {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        usize::try_from(self.0 % u64::try_from(n.max(1)).unwrap()).unwrap()
    }

    fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len())]
    }
}

/// A seeded sample of part combinations for a catalog block with a pattern.
fn sampled_parts(sig: &BlockSig, rng: &mut Rng, samples: &[Type]) -> Parts {
    let known_or = |rng: &mut Rng, others: &[PartType]| -> PartType {
        if rng.below(3) == 0 {
            rng.pick(others).clone()
        } else {
            PartType::Known(rng.pick(samples).clone())
        }
    };
    let mut parts = Parts::default();
    for name in sig.fields.keys().chain(sig.members.keys()) {
        let part = if sig.symbol_refs.contains(name) || sig.members.contains_key(name) {
            known_or(rng, &[PartType::Empty, PartType::Unknown])
        } else {
            known_or(rng, &[PartType::Empty, PartType::Auto])
        };
        parts.fields.insert(name.clone(), part);
        parts
            .chosen_members
            .insert(name.clone(), known_or(rng, &[PartType::Unknown]));
        let list = (0..3)
            .map(|_| known_or(rng, &[PartType::Empty]))
            .collect::<Vec<_>>();
        parts.params.insert(name.clone(), list.clone());
        parts.members.insert(name.clone(), list);
    }
    for base in sig.inputs.keys() {
        let names = if sig.repeated.contains(base) {
            let count = rng.below(4);
            parts.counts.insert(base.clone(), count);
            (0..count).map(|i| format!("{base}{i}")).collect()
        } else {
            vec![base.clone()]
        };
        for name in names {
            let part = known_or(rng, &[PartType::Empty, PartType::FromPlace, PartType::Unknown]);
            parts.inputs.insert(name, part);
        }
    }
    parts.result = Some(known_or(rng, &[PartType::Empty, PartType::Unknown]));
    parts
}

/// A catalog block as `catalog.json` writes it (types.md §4.5): its
/// definition in its serde shape, with what its signature adds (`order`,
/// `placeVar`, `members` and `optionHeaders`, each left out while empty).
fn export_json(def: &b2c_catalog::schema::BlockDef, sig: &BlockSig) -> Value {
    let mut json = serde_json::to_value(def).unwrap();
    let object = json.as_object_mut().unwrap();
    let order: Vec<Value> = sig
        .order
        .iter()
        .map(|part| match part {
            PartName::Field(name) => json!({"field": name}),
            PartName::Input(name) => json!({"input": name}),
        })
        .collect();
    if !order.is_empty() {
        object.insert("order".into(), Value::from(order));
    }
    if let Some(var) = &sig.place_var {
        object.insert("placeVar".into(), json!(var.as_str()));
    }
    if !sig.members.is_empty() {
        let members: Map<String, Value> = sig
            .members
            .iter()
            .map(|(field, var)| (field.clone(), json!(var.as_str())))
            .collect();
        object.insert("members".into(), Value::Object(members));
    }
    if !sig.option_headers.is_empty() {
        let headers: Map<String, Value> = sig
            .option_headers
            .iter()
            .map(|(field, options)| {
                let options: Map<String, Value> = options
                    .iter()
                    .map(|(option, headers)| {
                        let names: Vec<&str> = headers.iter().map(|header| header.as_str()).collect();
                        (option.clone(), json!(names))
                    })
                    .collect();
                (field.clone(), Value::Object(options))
            })
            .collect();
        object.insert("optionHeaders".into(), Value::Object(headers));
    }
    json
}

/// For each catalog block with a pattern, a seeded sample of part
/// combinations with its signature as `catalog.json` writes it.
fn catalog_samples(facts: &[(&'static str, Facts)]) -> Vec<Value> {
    let catalog = b2c_catalog::core_catalog();
    let samples = sample_types();
    let mut rng = Rng(0x5eed_b10c_2026);
    let mut out = Vec::new();
    for (id, sig) in catalog.sigs().iter() {
        let typed = !sig.order.is_empty() || !sig.members.is_empty() || sig.place_var.is_some();
        if !typed {
            continue;
        }
        let def = catalog
            .blocks
            .get(id)
            .map_or_else(|| sig_json(id, sig), |def| export_json(def, sig));
        let mut cases = Vec::new();
        for i in 0..SAMPLES_PER_BLOCK {
            let parts = sampled_parts(sig, &mut rng, &samples);
            let place = (rng.below(3) == 0).then(|| rng.pick(&samples).clone());
            let (facts_name, facts) = &facts[i % facts.len()];
            let typing = type_block(sig, &parts, place.as_ref(), facts);
            cases.push(json!({
                "parts": parts.json(),
                "place": optional_type_json(place.as_ref()),
                "facts": facts_name,
                "typing": typing_json(&typing),
            }));
        }
        out.push(json!({"block": id, "def": def, "cases": cases}));
    }
    out
}

/// The value types the analyser gives the blocks whose output is `any` or
/// `number`, each in a small document: the block printed, with variables of
/// the operand types in its inputs.
fn m2_output_types() -> Vec<Value> {
    let operand_types: [(&str, Value); 6] = [
        ("int", json!("int")),
        ("double", json!("double")),
        ("char", json!("char")),
        ("bool", json!("bool")),
        ("std::string", json!("std::string")),
        ("list", json!({"std::vector": "int"})),
    ];
    let declarations: Vec<Value> = operand_types
        .iter()
        .enumerate()
        .map(|(i, (_, term))| {
            let mut block = common::declare("int", &format!("v{i}"), None);
            block["fields"]["TYPE"] = term.clone();
            block
        })
        .collect();
    let mut out = Vec::new();
    let mut record = |block: &str, fields: Value, inputs: Vec<(&str, usize)>, value: Value| {
        let mut body = declarations.clone();
        body.push(common::print(vec![value]));
        let document = common::doc(vec![common::main(body)]);
        let analysis = analyze(&document);
        let id = b2c_ir::ids::BlockId::new("probe").unwrap();
        let ty = analysis.block_types().get(&id).cloned().unwrap_or(Type::Error);
        let inputs: Map<String, Value> = inputs
            .into_iter()
            .map(|(name, i)| (name.to_owned(), operand_types[i].1.clone()))
            .collect();
        out.push(json!({"block": block, "fields": fields, "inputs": inputs, "type": type_json(&ty)}));
    };
    for text in ["0", "42", "-7", "1.5", "1e3", "0x1F", "2147483648"] {
        let value =
            json!({"block": {"id": "probe", "type": "math.number", "v": 1, "fields": {"VALUE": text}}});
        record("math.number", json!({"VALUE": text}), vec![], value);
    }
    for op in ["add", "sub", "mul", "div", "mod"] {
        for a in 0..operand_types.len() {
            for b in 0..operand_types.len() {
                let value = json!({"block": {"id": "probe", "type": "math.arithmetic", "v": 1,
                    "fields": {"OP": op},
                    "inputs": {"A": common::get(&format!("v{a}")), "B": common::get(&format!("v{b}"))}}});
                record(
                    "math.arithmetic",
                    json!({"OP": op}),
                    vec![("A", a), ("B", b)],
                    value,
                );
            }
        }
    }
    for a in 0..operand_types.len() {
        for b in 0..operand_types.len() {
            let value = json!({"block": {"id": "probe", "type": "logic.ternary", "v": 1,
                "inputs": {"COND": common::boolean(true), "THEN": common::get(&format!("v{a}")),
                           "ELSE": common::get(&format!("v{b}"))}}});
            record("logic.ternary", json!({}), vec![("THEN", a), ("ELSE", b)], value);
        }
    }
    out
}

fn slots_fixture() -> Value {
    let facts = facts();
    let sigs: BTreeMap<&str, BlockSig> = hand_written_sigs().into_iter().collect();
    let cases: Vec<Value> = hand_written_cases()
        .into_iter()
        .map(|case| {
            let sig = &sigs[case.sig];
            let facts = &facts.iter().find(|(name, _)| *name == case.facts).unwrap().1;
            let typing = type_block(sig, &case.parts, case.place.as_ref(), facts);
            json!({
                "name": case.name,
                "sig": case.sig,
                "parts": case.parts.json(),
                "place": optional_type_json(case.place.as_ref()),
                "facts": case.facts,
                "typing": typing_json(&typing),
            })
        })
        .collect();
    let sigs_json: Map<String, Value> = sigs
        .iter()
        .map(|(id, sig)| ((*id).to_owned(), sig_json(id, sig)))
        .collect();
    json!({
        "description": "Written by crates/b2c-lang/tests/fixtures.rs: type_block's answers (slots.md §7). See that file for the JSON shapes and how a case's parts answer.",
        "facts": facts.iter().map(|(name, facts)| ((*name).to_owned(), facts.json())).collect::<Map<_, _>>(),
        "sigs": sigs_json,
        "cases": cases,
        "catalog": catalog_samples(&facts),
        "outputs": m2_output_types(),
    })
}

// --- Writing and comparing --------------------------------------------------------------

fn text(value: &Value) -> String {
    let mut text = serde_json::to_string_pretty(value).unwrap();
    text.push('\n');
    text
}

/// Writes the file under `B2C_UPDATE_TYPE_FIXTURES=1`, else fails when the
/// committed one differs.
fn check(path: &str, value: &Value) {
    let text = text(value);
    let full = common::repo_root().join(path);
    if std::env::var_os(UPDATE).is_some_and(|value| value == "1") {
        std::fs::write(&full, &text).unwrap();
        return;
    }
    let committed = std::fs::read_to_string(&full).unwrap_or_default();
    if committed != text {
        let line = committed
            .lines()
            .zip(text.lines())
            .position(|(old, new)| old != new)
            .unwrap_or_else(|| committed.lines().count().min(text.lines().count()));
        panic!(
            "{path} is stale (it differs from line {}). Write it again with `{UPDATE}=1 cargo test -p b2c-lang --test fixtures` and commit the result.",
            line + 1
        );
    }
}

#[test]
fn the_type_fixture_is_current() {
    check(TYPES_PATH, &types_fixture());
}

#[test]
fn the_slot_fixture_is_current() {
    check(SLOTS_PATH, &slots_fixture());
}

#[test]
fn the_fixtures_cover_what_they_promise() {
    let types = types_fixture();
    let samples = sample_types().len();
    assert_eq!(types["samples"].as_array().unwrap().len(), samples);
    assert_eq!(types["conversions"].as_array().unwrap().len(), samples);
    // One set of facts does not know the struct, so some answers are null.
    let partial = &types["classes"]["partial"]["comparable"];
    assert!(partial.as_array().unwrap().iter().any(Value::is_null));
    let slots = slots_fixture();
    let names: Vec<&str> = slots["cases"]
        .as_array()
        .unwrap()
        .iter()
        .map(|case| case["name"].as_str().unwrap())
        .collect();
    assert!(names.iter().any(|name| name.contains("counterexample")));
    assert!(names.iter().any(|name| name.contains("members field")));
    for block in slots["catalog"].as_array().unwrap() {
        assert!(block["cases"].as_array().unwrap().len() <= SAMPLES_PER_BLOCK);
    }
    // Every M2 output rule the editor copies is sampled.
    let blocks: std::collections::BTreeSet<&str> = slots["outputs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|output| output["block"].as_str().unwrap())
        .collect();
    assert_eq!(
        blocks,
        ["logic.ternary", "math.arithmetic", "math.number"]
            .into_iter()
            .collect()
    );
}
