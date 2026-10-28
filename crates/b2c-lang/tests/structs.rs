//! W2's struct and enum blocks in the analyser (docs/plans/m3-contracts/w2.md
//! §1.5, §2.2, §2.10; package C1): their declarations and symbols, the scope
//! query's members, the fit of their parts with each message, the
//! declaration graph's order with `B2C-E0420`, and *join* refusing structs.
//! The blocks come as the project file holds them, analysed with the
//! catalog's signatures.

// Test code: unwrap/expect/panic, printing to stderr and builders that take
// JSON by value are fine here.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::print_stderr,
    clippy::needless_pass_by_value
)]

mod common;

use b2c_ir::ids::{BlockId, ModuleId, SymbolId};
use b2c_ir::sast::{ExprKind, ItemKind, StmtKind, SymbolKind};
use b2c_ir::scope_info::SymbolInfoKind;
use b2c_ir::types::{LibType, Type, TypeFacts};
use b2c_lang::Analysis;
use common::*;
use serde_json::{Value, json};

/// The symbol of a struct's field or an enum's value.
fn member_sym(owner: &str, name: &str) -> String {
    sym(&format!("{owner}_{name}"))
}

fn symbol(name: &str) -> SymbolId {
    SymbolId::new(&sym(name)).unwrap()
}

/// A user type as a type term.
fn user(name: &str) -> Value {
    json!({"ref": sym(name)})
}

/// A list of a type term.
fn list_of(term: Value) -> Value {
    json!({"std::vector": term})
}

/// *struct* `name` with these `(field, type term)` rows and ticks.
fn structure(name: &str, fields: &[(&str, Value)], comparable: bool, printable: bool) -> Value {
    let rows: Vec<Value> = fields
        .iter()
        .map(|(field, ty)| json!({"sym": member_sym(name, field), "name": field, "type": ty}))
        .collect();
    json!({
        "id": format!("st_{name}"), "type": "type.struct", "v": 1,
        "fields": {"NAME": {"sym": sym(name), "name": name}, "COMPARABLE": comparable, "PRINTABLE": printable},
        "extra": {"fields": rows}
    })
}

/// *enum* `name` with these values.
fn enumeration(name: &str, values: &[&str]) -> Value {
    let rows: Vec<Value> = values
        .iter()
        .map(|value| json!({"sym": member_sym(name, value), "name": value}))
        .collect();
    json!({
        "id": format!("en_{name}"), "type": "type.enum", "v": 1,
        "fields": {"NAME": {"sym": sym(name), "name": name}},
        "extra": {"values": rows}
    })
}

/// *create* `name` of the type term `ty`.
fn create(ty: Value, name: &str, value: Option<Value>) -> Value {
    let mut block = json!({"id": id(), "type": "var.declare", "v": 1,
                           "fields": {"TYPE": ty, "NAME": {"sym": sym(name), "name": name}}});
    if let Some(value) = value {
        block["inputs"] = json!({"VALUE": value});
    }
    block
}

/// *new `ty` with* these parts, with the block ID `block`.
fn new_with(block: &str, ty: &str, parts: Vec<Option<Value>>) -> Value {
    let mut inputs = serde_json::Map::new();
    for (index, part) in parts.iter().enumerate() {
        if let Some(part) = part {
            inputs.insert(format!("FIELD{index}"), part.clone());
        }
    }
    input(
        json!({"id": block, "type": "type.new", "v": 1, "fields": {"TYPE": user(ty)},
                 "extra": {"fieldCount": parts.len()}, "inputs": inputs}),
    )
}

/// *`member` of `object`*, with the block ID `block`.
fn field_of(block: &str, owner: &str, member: &str, object: Value) -> Value {
    input(json!({"id": block, "type": "type.field", "v": 1,
                 "fields": {"MEMBER": {"ref": member_sym(owner, member)}}, "inputs": {"OBJ": object}}))
}

/// *set `member` of `var` to `value`*.
fn set_field(var: &str, owner: &str, member: &str, value: Value) -> Value {
    json!({"id": id(), "type": "type.set_field", "v": 1,
           "fields": {"VAR": {"ref": sym(var)}, "MEMBER": {"ref": member_sym(owner, member)}},
           "inputs": {"VALUE": value}})
}

/// *`enum` `value`*, with the block ID `block`; `chosen` is the symbol of
/// `ENUM`.
fn enum_value(block: &str, chosen: &str, owner: &str, value: &str) -> Value {
    input(json!({"id": block, "type": "type.enum_value", "v": 1,
                 "fields": {"ENUM": {"ref": sym(chosen)}, "VALUE": {"ref": member_sym(owner, value)}}}))
}

/// The rendered diagnostics, one per line.
fn reported(a: &Analysis) -> Vec<String> {
    a.diagnostics.iter().map(render).collect()
}

/// The static type of block `id`.
fn type_of(a: &Analysis, block: &str) -> Type {
    a.block_types()
        .get(&BlockId::new(block).unwrap())
        .cloned()
        .unwrap_or_else(|| panic!("no type for {block}"))
}

fn student() -> Value {
    structure(
        "Student",
        &[("name", json!("std::string")), ("mark", json!("int"))],
        false,
        false,
    )
}

fn action() -> Value {
    enumeration("Action", &["Deposit", "Withdraw", "Quit"])
}

// --- Declarations ------------------------------------------------------------------

#[test]
fn structs_and_enums_declare_their_rows_in_order() {
    let a = run(vec![student(), action(), main(Vec::new())]);
    assert_clean(&a);
    let symbols = &a.program.symbols;
    let SymbolKind::Struct { fields, .. } = &symbols.get(&symbol("Student")).unwrap().kind else {
        panic!("not a struct")
    };
    let names: Vec<(&str, &Type)> = fields
        .iter()
        .map(|field| {
            let field = symbols.get(field).unwrap();
            assert_eq!(
                field.kind,
                SymbolKind::Field {
                    owner: symbol("Student")
                }
            );
            (field.name.as_str(), &field.ty)
        })
        .collect();
    assert_eq!(names, [("name", &Type::String), ("mark", &Type::Int)]);
    let SymbolKind::Enum { values } = &symbols.get(&symbol("Action")).unwrap().kind else {
        panic!("not an enum")
    };
    let values: Vec<&str> = values
        .iter()
        .map(|value| symbols.get(value).unwrap().name.as_str())
        .collect();
    assert_eq!(values, ["Deposit", "Withdraw", "Quit"]);
    // Both are items of their module, the struct and the enum in name order.
    let kinds: Vec<&str> = a.program.modules[0]
        .items
        .iter()
        .map(|item| match item.kind {
            ItemKind::Struct(_) => "struct",
            ItemKind::Enum(_) => "enum",
            ItemKind::Main(_) => "main",
            _ => "other",
        })
        .collect();
    // Items come in block ID order; the declarations in the graph's.
    assert_eq!(kinds, ["enum", "main", "struct"]);
    assert_eq!(a.program.layout.modules[0].source_order, [0, 2]);
}

#[test]
fn the_scope_query_lists_user_types_with_their_members_everywhere() {
    let body = vec![create(json!("int"), "n", None)];
    let a = run(vec![student(), action(), main(body)]);
    assert_clean(&a);
    let main_id = BlockId::new("main").unwrap();
    let scope = a.scope_at(&main_id, Some("BODY")).unwrap();
    let found: Vec<(&str, &SymbolInfoKind)> = scope
        .iter()
        .map(|info| (info.name.as_str(), &info.kind))
        .collect();
    let names: Vec<&str> = found.iter().map(|(name, _)| *name).collect();
    for name in ["Student", "name", "mark", "Action", "Deposit", "Withdraw", "Quit"] {
        assert!(names.contains(&name), "{name} in {names:?}");
    }
    assert!(found.contains(&(
        "mark",
        &SymbolInfoKind::Field {
            owner: symbol("Student")
        }
    )));
    // The definitions' own blocks answer with them too.
    let at_struct = a.scope_at(&BlockId::new("st_Student").unwrap(), None).unwrap();
    assert!(at_struct.iter().any(|info| info.name == "Action"));
    // A parked block sees them.
    let parked = a.parked_scope(&ModuleId::new("mod_0").unwrap());
    assert!(parked.iter().any(|info| info.name == "Student"));
}

#[test]
fn names_of_structs_enums_and_rows_are_checked() {
    let mut twice = structure(
        "Pair",
        &[("a", json!("int")), ("b", json!("int")), ("Pair", json!("int"))],
        false,
        false,
    );
    twice["extra"]["fields"][1]["name"] = json!("a");
    let mut same_symbol = twice.clone();
    same_symbol["id"] = json!("st_Pair2");
    same_symbol["fields"]["NAME"] = json!({"sym": "sym_Other", "name": "Pair"});
    same_symbol["extra"]["fields"] = json!([]);
    let mut values = enumeration("Mode", &["On", "Off", "int"]);
    values["extra"]["values"][1]["name"] = json!("On");
    let mut clash = structure("area", &[], false, false);
    clash["fields"]["NAME"]["sym"] = json!("sym_area_struct");
    let function = func("area", "void", &[], Vec::new());
    let a = run(vec![
        twice,
        same_symbol,
        values,
        clash,
        function,
        main(Vec::new()),
    ]);
    assert_eq!(
        reported(&a),
        [
            "error B2C-E0210 at st_Pair2 field NAME: There is already a struct or enum called `Pair`. Rename one \
             of them.",
            "error B2C-E0210 at st_area field NAME: There is already a function called `area`, and C++ would not \
             know which of the two the name means. Rename the struct or the function.",
            "error B2C-E0210 at en_Mode field values[1]: Two values of `Mode` are called `On`. Rename one of them.",
            "error B2C-E0220 at en_Mode field values[2]: `int` can't be used as a name: `int` is a C++ keyword.",
            "error B2C-E0210 at st_Pair field fields[1]: Two fields of `Pair` are called `a`. Rename one of them.",
            "error B2C-E0210 at st_Pair field fields[2]: A field can't have the name of its struct (`Pair`). \
             Rename the field.",
        ]
    );
}

#[test]
fn locals_inputs_and_fields_named_like_a_type_are_refused() {
    // The generated C++ names a user type as it is (`Student s{};`), so
    // where a variable, an input or a field has its name, C++ finds that
    // instead of the type: `int Student = 1; Student s{};` does not
    // compile, nor does `void show(int Student, Student s)` or
    // `struct Car { Color Color; };`.
    let local = create(json!("int"), "Student", Some(num("1")));
    let local_id = local["id"].as_str().unwrap().to_owned();
    let mut show = func("show", "void", &[], Vec::new());
    show["extra"]["params"] = json!([
        {"sym": sym("Student_input"), "name": "Student", "type": "int", "mode": "copy"},
        {"sym": sym("s"), "name": "s", "type": user("Student"), "mode": "copy"},
    ]);
    let mut program = main(vec![local, create(user("Student"), "s2", None)]);
    program["fields"] = json!({"ARGS": true, "ARGV": {"sym": sym("Color_args"), "name": "Color"}});
    let a = run(vec![
        student(),
        enumeration("Color", &["Red", "Green"]),
        structure("Car", &[("Color", user("Color"))], false, false),
        show,
        program,
    ]);
    let refused: Vec<String> = reported(&a)
        .into_iter()
        .filter(|line| line.contains("B2C-E0210"))
        .collect();
    assert_eq!(
        refused,
        [
            "error B2C-E0210 at st_Car field fields[0]: A field can't have the name of a struct or an enum: inside \
             `Car`, C++ would find the field `Color` instead of the enum. Rename the field."
                .to_owned(),
            "error B2C-E0210 at fn_show field params[0]: `Student` is the name of a struct, so C++ would not find \
             the struct by that name where `Student` exists. Choose another name."
                .to_owned(),
            "error B2C-E0210 at main field ARGV: `Color` is the name of an enum, so C++ would not find the enum by \
             that name where `Color` exists. Choose another name."
                .to_owned(),
            format!(
                "error B2C-E0210 at {local_id} field NAME: `Student` is the name of a struct, so C++ would not \
                 find the struct by that name where `Student` exists. Choose another name."
            ),
        ]
    );
    // Each points at the type it would hide.
    let notes: Vec<_> = a
        .diagnostics
        .iter()
        .filter(|d| d.code.0 == "B2C-E0210")
        .map(|d| d.related.first().map(|r| r.message.clone()))
        .collect();
    assert_eq!(
        notes,
        [
            Some(String::from("the enum")),
            Some(String::from("the struct")),
            Some(String::from("the enum")),
            Some(String::from("the struct")),
        ]
    );
}

#[test]
fn field_types_may_name_user_types_and_lists_of_them() {
    let school = structure(
        "School",
        &[
            ("pupils", list_of(user("Student"))),
            ("head", user("Student")),
            ("last", user("Action")),
        ],
        false,
        false,
    );
    let broken = structure(
        "Broken",
        &[
            ("gone", user("Missing")),
            ("nothing", json!("void")),
            ("odd", json!(42)),
        ],
        false,
        false,
    );
    let a = run(vec![school, broken, student(), action(), main(Vec::new())]);
    assert_eq!(
        reported(&a),
        [
            "error B2C-E0201 at st_Broken field fields[0]: This type refers to a struct or enum that doesn't \
             exist (any more). Choose another type.",
            "error B2C-E0430 at st_Broken field fields[1]: A variable can't have the type 'nothing' (void). \
             Choose another type.",
            "error B2C-E0430 at st_Broken field fields[2]: `42` is not a type that a field can have.",
        ]
    );
    let school = a
        .program
        .symbols
        .get(&SymbolId::new(&member_sym("School", "pupils")).unwrap());
    assert_eq!(
        school.unwrap().ty,
        Type::lib(LibType::Vector, vec![Type::Named(symbol("Student"))]).unwrap()
    );
}

// --- Ticks ---------------------------------------------------------------------------

#[test]
fn ticks_hold_only_when_nested_structs_qualify() {
    let inner = structure("Inner", &[("x", json!("int"))], false, true);
    let outer = structure("Outer", &[("inner", user("Inner"))], true, true);
    let keys = structure(
        "Keys",
        &[("seen", json!({"std::set": {"ref": sym("Inner")}}))],
        false,
        false,
    );
    let a = run(vec![inner, outer, keys, main(Vec::new())]);
    assert_eq!(
        reported(&a),
        [
            "error B2C-E0313 at st_Outer field COMPARABLE: `Outer` is ticked 'comparable', but its field `inner` \
             holds `Inner`, which is not comparable. Tick 'comparable' on `Inner` too, or untick it here.",
            "error B2C-E0313 at st_Keys field fields[0]: A set keeps its items in order, so they must be \
             comparable, and `Inner` is not. Tick 'comparable' on `Inner`, or use a list.",
        ]
    );
    let user_type = a.program.symbols.user_type(&symbol("Outer")).unwrap();
    assert_eq!((user_type.comparable, user_type.printable), (false, true));
}

// --- Cycles and order ----------------------------------------------------------------

#[test]
fn structs_that_hold_themselves_are_reported_and_lists_are_fine() {
    let node = structure("Node", &[("next", user("Node"))], false, false);
    let tree = structure("Tree", &[("children", list_of(user("Tree")))], false, false);
    let a = run(vec![node, tree, main(Vec::new())]);
    assert_eq!(
        reported(&a),
        [
            "error B2C-E0420 at st_Node field fields[0]: `Node` holds a `Node` in its field `next`, so each `Node` \
          would have to hold a whole copy of itself, which C++ does not allow. Make the field a list (a list \
          keeps its items elsewhere), or remove it."
        ]
    );
}

#[test]
fn a_struct_is_printed_after_the_types_it_holds() {
    let a_struct = structure("Alpha", &[("zone", user("Zone"))], false, false);
    let zone = structure("Zone", &[("colour", user("Colour"))], false, false);
    let colour = enumeration("Colour", &["Red"]);
    let helper = func("helper", "void", &[], Vec::new());
    let a = run(vec![a_struct, zone, colour, helper, main(Vec::new())]);
    assert_clean(&a);
    let module = &a.program.modules[0];
    let order: Vec<String> = a.program.layout.modules[0]
        .source_order
        .iter()
        .map(|&index| module.items[index].origin.block.as_str().to_owned())
        .collect();
    assert_eq!(order, ["en_Colour", "st_Zone", "st_Alpha", "fn_helper"]);
}

// --- The value blocks -------------------------------------------------------------------

#[test]
fn new_field_and_enum_value_give_their_types() {
    let body = vec![
        create(
            user("Student"),
            "s",
            Some(new_with(
                "new",
                "Student",
                vec![Some(text("Ann")), Some(num("74"))],
            )),
        ),
        print(vec![field_of("mark", "Student", "mark", get("s"))]),
        create(
            user("Action"),
            "act",
            Some(enum_value("quit", "Action", "Action", "Quit")),
        ),
        set_field("s", "Student", "mark", num("80")),
    ];
    let a = run(vec![student(), action(), main(body)]);
    assert_clean(&a);
    assert_eq!(type_of(&a, "new"), Type::Named(symbol("Student")));
    assert_eq!(type_of(&a, "mark"), Type::Int);
    assert_eq!(type_of(&a, "quit"), Type::Named(symbol("Action")));
    let main_json = main_json(&a);
    let text = main_json.to_string();
    assert!(text.contains("\"set_field\""), "{text}");
    let Some(ItemKind::Main(def)) = a.program.modules[0]
        .items
        .iter()
        .map(|item| &item.kind)
        .find(|kind| matches!(kind, ItemKind::Main(_)))
    else {
        panic!("no main")
    };
    let StmtKind::VarDecl(decl) = &def.body.stmts[0].kind else {
        panic!()
    };
    let ExprKind::New { ty, fields } = &decl.init.as_ref().unwrap().kind else {
        panic!()
    };
    assert_eq!(ty, &symbol("Student"));
    assert_eq!(fields.len(), 2);
}

#[test]
fn parts_of_new_are_checked_against_the_fields() {
    let body = vec![
        create(
            user("Student"),
            "s",
            Some(new_with("new", "Student", vec![None, Some(text("x"))])),
        ),
        create(
            user("Student"),
            "t",
            Some(new_with("short", "Student", vec![Some(text("Ann"))])),
        ),
    ];
    let a = run(vec![student(), main(body)]);
    let lines = reported(&a);
    assert_eq!(lines.len(), 2, "{lines:#?}");
    assert!(lines[0].starts_with("error B2C-E0301"), "{lines:#?}");
    assert!(lines[0].contains("The `mark` field of `Student`"), "{lines:#?}");
    assert_eq!(
        lines[1],
        "error B2C-E0306 at short: `Student` has 2 fields, but this block has 1 part. Choose the struct again so \
         that the block has one part for each field."
    );
}

#[test]
fn members_must_belong_to_the_chosen_type() {
    let point = structure("Point", &[("x", json!("int"))], false, false);
    let body = vec![
        create(
            user("Student"),
            "s",
            Some(new_with("new", "Student", vec![None, None])),
        ),
        create(json!("int"), "n", None),
        print(vec![field_of("wrong", "Point", "x", get("s"))]),
        print(vec![field_of("number", "Student", "mark", get("n"))]),
        create(
            user("Action"),
            "act",
            Some(enum_value("first", "Action", "Action", "Deposit")),
        ),
        create(
            user("Action"),
            "bad",
            Some(enum_value("variable", "act", "Action", "Quit")),
        ),
        create(
            user("Action"),
            "other",
            Some(enum_value("struct", "Student", "Action", "Quit")),
        ),
    ];
    let a = run(vec![student(), point, action(), main(body)]);
    let lines = reported(&a);
    assert_eq!(
        lines[0], "error B2C-E0207 at wrong field MEMBER: `x` is not a field of `Student`.",
        "{lines:#?}"
    );
    assert!(
        lines[1].starts_with("error B2C-E0301")
            && lines[1].ends_with(": `n` is a whole number, but this block needs a struct."),
        "{lines:#?}"
    );
    assert!(
        lines.contains(&String::from(
            "error B2C-E0207 at variable field ENUM: `act` is not an enum: choose an enum here, not a variable or \
             value of one."
        )),
        "{lines:#?}"
    );
    assert!(
        lines.contains(&String::from(
            "error B2C-E0207 at struct field ENUM: `Student` is a struct, not an enum: choose an enum here."
        )),
        "{lines:#?}"
    );
}

#[test]
fn set_field_needs_a_changeable_struct_variable() {
    let body = vec![
        create(
            user("Student"),
            "s",
            Some(new_with("new", "Student", vec![None, None])),
        ),
        set_field("s", "Student", "mark", text("high")),
    ];
    let reader = json!({
        "id": "fn_show", "type": "func.define", "v": 1,
        "fields": {"NAME": {"sym": sym("show"), "name": "show"}, "RETURNS": "void"},
        "extra": {"params": [{"sym": sym("p"), "name": "p", "type": user("Student"), "mode": "read_only"}]},
        "statements": {"BODY": [set_field("p", "Student", "mark", num("1"))]}
    });
    let a = run(vec![student(), reader, main(body)]);
    let lines = reported(&a);
    assert_eq!(lines.len(), 2, "{lines:#?}");
    assert!(
        lines.iter().any(|line| line.starts_with("error B2C-E0308")),
        "{lines:#?}"
    );
    assert!(
        lines.iter().any(|line| line.contains("The `mark` field of `s`")),
        "{lines:#?}"
    );
}

// --- join ----------------------------------------------------------------------------------

#[test]
fn join_refuses_structs_and_print_takes_printable_ones() {
    let shown = structure("Shown", &[("x", json!("int"))], false, true);
    let body = vec![
        create(user("Shown"), "s", None),
        create(user("Action"), "act", None),
        print(vec![get("s"), get("act")]),
        print(vec![join(vec![text("s = "), get("s")])]),
        print(vec![join(vec![text("act = "), get("act")])]),
    ];
    let a = run(vec![shown, action(), main(body)]);
    let lines = reported(&a);
    assert_eq!(lines.len(), 1, "{lines:#?}");
    assert!(
        lines[0].ends_with("`Shown` can't be joined into text in one go. Join its fields instead."),
        "{lines:#?}"
    );
}
