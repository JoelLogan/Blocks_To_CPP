//! The pipeline DTOs (generated files, source maps, static types and symbol
//! records) are byte-identical in JSON to the shared `b2c-ir` types, so the
//! WebAssembly core, the CLI and the backend send what `packages/ipc-types`
//! declares (`docs/spec/06-compiler-pipeline.md` §6.5, §6.6, §6.9).

// Test code: unwrap/expect/panic are fine here.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::fmt::Debug;

use b2c_ipc::pipeline::{
    FileKind, GeneratedFile, MappedRange, PassMode, SourceMap, SymbolInfo, SymbolInfoKind, TypeJson,
};
use b2c_ir::source_map::{self as ir, FileMap, Position};
use b2c_ir::types::sample_types;
use b2c_ir::{BlockId, LibType, ModuleId, Part, SymbolId, TermForm, Type};
use proptest::collection::vec;
use proptest::prelude::*;
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::json;

/// Serialises the shared value and its DTO, compares the JSON byte for byte
/// (compact and pretty), and reads each one's JSON back as the other.
fn assert_identical<S, D>(shared: &S)
where
    S: Serialize + DeserializeOwned + PartialEq + Debug,
    D: Serialize + DeserializeOwned + PartialEq + Debug + for<'a> From<&'a S>,
{
    let dto = D::from(shared);
    let shared_json = serde_json::to_string(shared).unwrap();
    let dto_json = serde_json::to_string(&dto).unwrap();
    assert_eq!(dto_json, shared_json);
    assert_eq!(
        serde_json::to_string_pretty(&dto).unwrap(),
        serde_json::to_string_pretty(shared).unwrap()
    );
    assert_eq!(serde_json::from_str::<D>(&shared_json).unwrap(), dto);
    assert_eq!(&serde_json::from_str::<S>(&dto_json).unwrap(), shared);
}

fn module() -> ModuleId {
    ModuleId::new("mod_main").unwrap()
}

fn block(id: &str) -> BlockId {
    BlockId::new(id).unwrap()
}

fn sym(id: &str) -> SymbolId {
    SymbolId::new(id).unwrap()
}

const TYPES: [Type; 7] = [
    Type::Void,
    Type::Bool,
    Type::Char,
    Type::Int,
    Type::Double,
    Type::String,
    Type::Error,
];

const MODES: [b2c_ir::sast::PassMode; 3] = [
    b2c_ir::sast::PassMode::Copy,
    b2c_ir::sast::PassMode::Editable,
    b2c_ir::sast::PassMode::ReadOnly,
];

fn parts() -> Vec<Part> {
    vec![
        Part::Whole,
        Part::Field { name: "NAME".into() },
        Part::Input { name: "COND0".into() },
        Part::Tokens {
            input: "EXPR".into(),
            start: 2,
            end: 5,
        },
    ]
}

#[test]
fn generated_files() {
    for kind in [ir::FileKind::Source, ir::FileKind::Header] {
        for module in [None, Some(module())] {
            let file = ir::GeneratedFile {
                path: "geo/b2c_support.hpp".into(),
                kind,
                module,
                contents: "// “Quoted” <b>text</b> \u{202e} \\ \"\n#include <string>\n\tint x = 1;\n".into(),
            };
            assert_identical::<_, GeneratedFile>(&file);
        }
    }
    assert_eq!(FileKind::VALUES, ["source", "header"]);
}

#[test]
fn source_maps() {
    let ranges = parts()
        .into_iter()
        .enumerate()
        .map(|(i, part)| {
            let line = u32::try_from(i).unwrap() + 1;
            ir::MappedRange {
                start: Position { line, column: 1 },
                end: Position {
                    line: line + 2,
                    column: u32::MAX,
                },
                module: module(),
                block: block(&format!("b{i:03}")),
                part,
            }
        })
        .collect::<Vec<_>>();
    for range in &ranges {
        assert_identical::<_, MappedRange>(range);
    }
    let map = ir::SourceMap {
        version: 1,
        files: vec![
            FileMap {
                path: "main.cpp".into(),
                module: None,
                ranges,
            },
            FileMap {
                path: "empty.hpp".into(),
                module: Some(module()),
                ranges: Vec::new(),
            },
        ],
    };
    assert_identical::<_, SourceMap>(&map);
    assert_identical::<_, SourceMap>(&ir::SourceMap::default());
}

#[test]
fn types_and_pass_modes() {
    for ty in &TYPES {
        let dto = TypeJson::from(ty);
        assert_eq!(dto.as_json(), &serde_json::to_value(ty).unwrap());
        assert_eq!(
            serde_json::from_value::<Type>(dto.as_json().clone()).unwrap(),
            *ty
        );
        assert_eq!(
            serde_json::from_value::<TypeJson>(dto.as_json().clone()).unwrap(),
            dto
        );
    }
    assert_eq!(
        TYPES.map(|ty| TypeJson::from(&ty).as_json().clone()),
        ["void", "bool", "char", "int", "double", "string", "error"].map(|name| json!(name))
    );
    for mode in MODES {
        let dto = PassMode::from(mode);
        assert_eq!(
            serde_json::to_value(dto).unwrap(),
            serde_json::to_value(mode).unwrap()
        );
    }
    assert_eq!(PassMode::VALUES, ["copy", "editable", "read_only"]);
}

/// Library and user types, which `b2c_ir::Type` does not serialise until the
/// switch to the term form (types.md §8.2), get their term form, and every
/// sample type reads back.
#[test]
fn every_sample_type_has_its_json() {
    for ty in sample_types() {
        let dto = TypeJson::from(&ty);
        let expected =
            serde_json::to_value(&ty).unwrap_or_else(|_| serde_json::to_value(TermForm(&ty)).unwrap());
        assert_eq!(dto.as_json(), &expected, "{ty:?}");
        assert_eq!(serde_json::from_value::<TypeJson>(expected).unwrap(), dto);
    }
    let scores = Type::lib(
        LibType::Map,
        vec![Type::String, Type::lib(LibType::Vector, vec![Type::Int]).unwrap()],
    )
    .unwrap();
    assert_eq!(
        serde_json::to_string(&TypeJson::from(&scores)).unwrap(),
        r#"{"std::map":["std::string",{"std::vector":"int"}]}"#
    );
    assert_eq!(
        TypeJson::from(&Type::Named(sym("sym_Pt12"))).as_json(),
        &json!({"ref": "sym_Pt12"})
    );
}

#[test]
fn only_types_read_as_types() {
    for good in [
        json!("int"),
        json!("string"),
        json!("std::string"),
        json!("error"),
        json!("std::ifstream"),
        json!({"std::vector": "int"}),
        json!({"std::map": ["std::string", {"ref": "sym_x"}]}),
    ] {
        assert!(serde_json::from_value::<TypeJson>(good.clone()).is_ok(), "{good}");
    }
    for bad in [
        json!("auto"),
        json!("Student"),
        json!(""),
        json!({"std::vector": "void"}),
        json!({"std::vector": ["int"]}),
        json!({"std::vector": "int", "std::set": "int"}),
        json!({"__proto__": "int"}),
        json!({"ref": "not an id"}),
        json!(["int"]),
        json!(3),
        json!(null),
    ] {
        assert!(serde_json::from_value::<TypeJson>(bad.clone()).is_err(), "{bad}");
    }
}

/// Every kind of symbol, with every type and pass mode.
#[test]
fn symbol_infos() {
    let mut kinds = user_type_kinds();
    kinds.extend([
        b2c_ir::SymbolInfoKind::Variable { is_const: false },
        b2c_ir::SymbolInfoKind::Variable { is_const: true },
        b2c_ir::SymbolInfoKind::LoopVariable,
        b2c_ir::SymbolInfoKind::Function {
            params: Vec::new(),
            returns: Type::Void,
        },
        b2c_ir::SymbolInfoKind::Function {
            params: vec![sym("p_w"), sym("p_h")],
            returns: Type::Double,
        },
    ]);
    kinds.extend(MODES.map(|mode| b2c_ir::SymbolInfoKind::Parameter { mode }));
    for kind in kinds {
        for ty in &TYPES {
            for shared in [false, true] {
                let info = b2c_ir::SymbolInfo {
                    id: sym("s_guess"),
                    name: "my “score” <b>".into(),
                    kind: kind.clone(),
                    ty: ty.clone(),
                    module: module(),
                    decl_block: block("b003"),
                    shared,
                };
                assert_identical::<_, SymbolInfo>(&info);
            }
        }
    }
}

/// The kinds of user types (milestone M3). Their records carry M2 types here:
/// `b2c_ir::Type` does not serialise user types until the switch to the term
/// form (types.md §8.2).
fn user_type_kinds() -> Vec<b2c_ir::SymbolInfoKind> {
    vec![
        b2c_ir::SymbolInfoKind::Struct {
            fields: vec![sym("f_x"), sym("f_y")],
            comparable: true,
            printable: false,
        },
        b2c_ir::SymbolInfoKind::Struct {
            fields: Vec::new(),
            comparable: false,
            printable: true,
        },
        b2c_ir::SymbolInfoKind::Field { owner: sym("s_pt") },
        b2c_ir::SymbolInfoKind::Enum {
            values: vec![sym("e_red"), sym("e_green")],
        },
        b2c_ir::SymbolInfoKind::Enumerator { owner: sym("s_col") },
    ]
}

#[test]
fn user_type_kinds_have_their_json_shape() {
    let record = |kind: b2c_ir::SymbolInfoKind| {
        let info = b2c_ir::SymbolInfo {
            id: sym("s1"),
            name: "Point".into(),
            kind,
            ty: Type::Int,
            module: module(),
            decl_block: block("b1"),
            shared: false,
        };
        serde_json::to_value(SymbolInfo::from(&info)).unwrap()
    };
    let records: Vec<serde_json::Value> = user_type_kinds().into_iter().map(record).collect();
    let [structure, empty, field, colour, red] = <[serde_json::Value; 5]>::try_from(records).unwrap();
    assert_eq!(
        structure,
        json!({"id": "s1", "name": "Point", "kind": "struct", "fields": ["f_x", "f_y"],
               "comparable": true, "printable": false, "type": "int", "module": "mod_main",
               "declBlock": "b1"})
    );
    assert_eq!(empty["fields"], json!([]));
    assert_eq!(field["kind"], "field");
    assert_eq!(field["owner"], "s_pt");
    assert_eq!(colour["kind"], "enum");
    assert_eq!(colour["values"], json!(["e_red", "e_green"]));
    assert_eq!(red["kind"], "enumerator");
    assert_eq!(red["owner"], "s_col");
    // A user type's own record has the type itself: its term.
    let point = b2c_ir::SymbolInfo {
        id: sym("s_pt"),
        name: "Point".into(),
        kind: b2c_ir::SymbolInfoKind::Struct {
            fields: Vec::new(),
            comparable: false,
            printable: false,
        },
        ty: Type::Named(sym("s_pt")),
        module: module(),
        decl_block: block("b1"),
        shared: true,
    };
    let json = serde_json::to_value(SymbolInfo::from(&point)).unwrap();
    assert_eq!(json["type"], json!({"ref": "s_pt"}));
    assert_eq!(json["shared"], true);
    assert_eq!(
        serde_json::from_value::<SymbolInfo>(json).unwrap(),
        SymbolInfo::from(&point)
    );
}

/// The exact text of the spec example (06 §6.5), key order included.
#[test]
fn the_spec_example() {
    let info = SymbolInfo {
        id: "s_guess".into(),
        name: "guess".into(),
        kind: SymbolInfoKind::Variable { is_const: false },
        ty: TypeJson::from(&Type::Int),
        module: "mod_main".into(),
        decl_block: "b003".into(),
        shared: false,
    };
    assert_eq!(
        serde_json::to_string(&info).unwrap(),
        r#"{"id":"s_guess","name":"guess","kind":"variable","isConst":false,"type":"int","module":"mod_main","declBlock":"b003"}"#
    );
}

// ---------------------------------------------------------------------------
// Arbitrary values
// ---------------------------------------------------------------------------

fn id() -> impl Strategy<Value = String> {
    "[A-Za-z0-9_]{1,32}"
}

fn text() -> impl Strategy<Value = String> {
    any::<String>()
}

fn ty() -> impl Strategy<Value = Type> {
    prop::sample::select(TYPES.to_vec())
}

fn part() -> impl Strategy<Value = Part> {
    prop_oneof![
        Just(Part::Whole),
        text().prop_map(|name| Part::Field { name }),
        text().prop_map(|name| Part::Input { name }),
        (text(), any::<u32>(), any::<u32>()).prop_map(|(input, start, end)| Part::Tokens {
            input,
            start,
            end
        }),
    ]
}

fn position() -> impl Strategy<Value = Position> {
    (any::<u32>(), any::<u32>()).prop_map(|(line, column)| Position { line, column })
}

fn range() -> impl Strategy<Value = ir::MappedRange> {
    (position(), position(), id(), id(), part()).prop_map(|(start, end, m, b, part)| ir::MappedRange {
        start,
        end,
        module: ModuleId::new(&m).unwrap(),
        block: BlockId::new(&b).unwrap(),
        part,
    })
}

fn source_map() -> impl Strategy<Value = ir::SourceMap> {
    let file =
        (text(), prop::option::of(id()), vec(range(), 0..4)).prop_map(|(path, module, ranges)| FileMap {
            path,
            module: module.map(|m| ModuleId::new(&m).unwrap()),
            ranges,
        });
    (any::<u32>(), vec(file, 0..3)).prop_map(|(version, files)| ir::SourceMap { version, files })
}

fn generated_file() -> impl Strategy<Value = ir::GeneratedFile> {
    let kind = prop_oneof![Just(ir::FileKind::Source), Just(ir::FileKind::Header)];
    (text(), kind, prop::option::of(id()), text()).prop_map(|(path, kind, module, contents)| {
        ir::GeneratedFile {
            path,
            kind,
            module: module.map(|m| ModuleId::new(&m).unwrap()),
            contents,
        }
    })
}

fn symbol_info() -> impl Strategy<Value = b2c_ir::SymbolInfo> {
    let kind = prop_oneof![
        any::<bool>().prop_map(|is_const| b2c_ir::SymbolInfoKind::Variable { is_const }),
        prop::sample::select(MODES.to_vec()).prop_map(|mode| b2c_ir::SymbolInfoKind::Parameter { mode }),
        Just(b2c_ir::SymbolInfoKind::LoopVariable),
        (vec(id(), 0..4), ty()).prop_map(|(params, returns)| b2c_ir::SymbolInfoKind::Function {
            params: params.iter().map(|p| sym(p)).collect(),
            returns,
        }),
        (vec(id(), 0..4), any::<bool>(), any::<bool>()).prop_map(|(fields, comparable, printable)| {
            b2c_ir::SymbolInfoKind::Struct {
                fields: fields.iter().map(|f| sym(f)).collect(),
                comparable,
                printable,
            }
        }),
        id().prop_map(|owner| b2c_ir::SymbolInfoKind::Field { owner: sym(&owner) }),
        vec(id(), 0..4).prop_map(|values| b2c_ir::SymbolInfoKind::Enum {
            values: values.iter().map(|v| sym(v)).collect(),
        }),
        id().prop_map(|owner| b2c_ir::SymbolInfoKind::Enumerator { owner: sym(&owner) }),
    ];
    (id(), text(), kind, ty(), id(), id(), any::<bool>()).prop_map(|(s, name, kind, ty, m, b, shared)| {
        b2c_ir::SymbolInfo {
            id: sym(&s),
            name,
            kind,
            ty,
            module: ModuleId::new(&m).unwrap(),
            decl_block: block(&b),
            shared,
        }
    })
}

proptest! {
    #[test]
    fn arbitrary_source_maps_are_identical(map in source_map()) {
        assert_identical::<_, SourceMap>(&map);
    }

    #[test]
    fn arbitrary_generated_files_are_identical(file in generated_file()) {
        assert_identical::<_, GeneratedFile>(&file);
    }

    #[test]
    fn arbitrary_symbol_infos_are_identical(info in symbol_info()) {
        assert_identical::<_, SymbolInfo>(&info);
    }
}
