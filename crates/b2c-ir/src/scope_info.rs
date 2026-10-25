//! What the editor learns about a symbol (spec §6.5): [`SymbolInfo`].
//!
//! The analyser (`b2c_lang::Analysis::symbols_in_scope`) answers scope
//! queries with these records; the WebAssembly core and the IPC layer send
//! them to the editor unchanged, so their JSON is the same everywhere:
//!
//! ```json
//! { "id": "s_guess", "name": "guess", "kind": "variable", "isConst": false,
//!   "type": "int", "module": "mod_main", "declBlock": "b003" }
//! ```
//!
//! `kind` is `variable` (with `isConst`), `parameter` (with `mode`: `copy`,
//! `editable` or `read_only`), `loopVariable`, or `function` (with `params`,
//! the parameter symbol IDs in order, and `returns`). Types are written as
//! their terms (`"int"`, `"std::string"`, `{"std::vector": "int"}`,
//! `{"ref": "sym_x"}`), and `"error"` for an expression that already has an
//! error (`crate::types::term_form`).
//!
//! Milestone M3 adds the kinds `struct` (with `fields`, `comparable` and
//! `printable`), `field` (with `owner`), `enum` (with `values`) and
//! `enumerator` (with `owner`), and the key `shared`, written only when it
//! is `true` (ADR-0019). No stage makes them yet.

use serde::{Deserialize, Serialize};

use crate::ids::{BlockId, ModuleId, SymbolId};
use crate::sast::{PassMode, Symbol, SymbolKind};
use crate::types::Type;

/// A symbol as the editor sees it: enough to fill a dropdown, label a getter
/// block or pick the type of a reporter.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SymbolInfo {
    /// The symbol's ID (what blocks store).
    pub id: SymbolId,
    /// The name the user gave it, as written in its declaring block.
    pub name: String,
    /// What it is, with the details of that kind.
    #[serde(flatten)]
    pub kind: SymbolInfoKind,
    /// Its type; for a function, the type it gives back.
    #[serde(rename = "type")]
    pub ty: Type,
    /// The module that declares it.
    pub module: ModuleId,
    /// The block that declares it (`var.declare`, `control.for_range`, or the
    /// `func.define` of a function and of its parameters).
    pub decl_block: BlockId,
    /// Whether its definition goes into its module's header (ADR-0019).
    /// Always `false` in a single-module project; serialised only when
    /// `true`.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub shared: bool,
}

/// The kind of a [`SymbolInfo`], serialised as its `kind` key next to the
/// other keys of the record.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum SymbolInfoKind {
    /// A local variable.
    Variable {
        /// Whether it was declared `const`.
        is_const: bool,
    },
    /// A function parameter.
    Parameter {
        /// How the argument is passed.
        mode: PassMode,
    },
    /// The counter of a `for` loop.
    LoopVariable,
    /// A user function.
    Function {
        /// Its parameters' symbols, in order.
        params: Vec<SymbolId>,
        /// The type it gives back (`void` for nothing).
        returns: Type,
    },
    /// A user struct.
    Struct {
        /// Its fields' symbols, in order.
        fields: Vec<SymbolId>,
        /// Whether it is comparable: ticked, and every field's type is
        /// comparable (the effective flag of `UserType`).
        comparable: bool,
        /// Whether it is printable: ticked, and every field's type is
        /// printable (the effective flag of `UserType`).
        printable: bool,
    },
    /// A field of a struct.
    Field {
        /// The struct.
        owner: SymbolId,
    },
    /// A user enum.
    Enum {
        /// Its enumerators' symbols, in order.
        values: Vec<SymbolId>,
    },
    /// A value of an enum.
    Enumerator {
        /// The enum.
        owner: SymbolId,
    },
}

impl SymbolInfo {
    /// The record for a symbol of the program's symbol table.
    ///
    /// `name` is the user's name for it. It usually equals `symbol.name`, but
    /// differs when the name is not a valid identifier: the table then holds a
    /// generated placeholder, while the editor must show what the user wrote.
    pub fn new(id: SymbolId, name: impl Into<String>, symbol: &Symbol) -> Self {
        let kind = match &symbol.kind {
            SymbolKind::Variable { is_const } => SymbolInfoKind::Variable { is_const: *is_const },
            SymbolKind::Parameter { mode } => SymbolInfoKind::Parameter { mode: *mode },
            SymbolKind::LoopVariable => SymbolInfoKind::LoopVariable,
            SymbolKind::Function { params } => SymbolInfoKind::Function {
                params: params.clone(),
                returns: symbol.ty.clone(),
            },
            SymbolKind::Struct {
                fields,
                comparable,
                printable,
            } => SymbolInfoKind::Struct {
                fields: fields.clone(),
                comparable: *comparable,
                printable: *printable,
            },
            SymbolKind::Field { owner } => SymbolInfoKind::Field { owner: owner.clone() },
            SymbolKind::Enum { values } => SymbolInfoKind::Enum {
                values: values.clone(),
            },
            SymbolKind::Enumerator { owner } => SymbolInfoKind::Enumerator { owner: owner.clone() },
        };
        Self {
            id,
            name: name.into(),
            kind,
            ty: symbol.ty.clone(),
            module: symbol.origin.module.clone(),
            decl_block: symbol.origin.block.clone(),
            shared: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::*;
    use crate::diag::Part;
    use crate::sast::Origin;
    use crate::text::Ident;

    fn sym(id: &str) -> SymbolId {
        SymbolId::new(id).expect("id")
    }

    fn symbol(name: &str, kind: SymbolKind, ty: Type, block: &str, part: Part) -> Symbol {
        Symbol {
            name: Ident::new(name).expect("name"),
            kind,
            ty,
            origin: Origin {
                module: ModuleId::new("mod_main").expect("id"),
                block: BlockId::new(block).expect("id"),
                part,
            },
        }
    }

    fn field(name: &str) -> Part {
        Part::Field {
            name: name.to_owned(),
        }
    }

    /// Serialises, checks the JSON, and checks that it reads back unchanged.
    fn round_trip(info: &SymbolInfo, expected: &Value) {
        let json = serde_json::to_value(info).expect("serialise");
        assert_eq!(&json, expected);
        let back: SymbolInfo = serde_json::from_value(json).expect("deserialise");
        assert_eq!(&back, info);
    }

    #[test]
    fn variables_match_the_spec_example() {
        let s = symbol(
            "guess",
            SymbolKind::Variable { is_const: false },
            Type::Int,
            "b003",
            field("NAME"),
        );
        let info = SymbolInfo::new(sym("s_guess"), "guess", &s);
        round_trip(
            &info,
            &json!({"id": "s_guess", "name": "guess", "kind": "variable", "isConst": false,
                    "type": "int", "module": "mod_main", "declBlock": "b003"}),
        );
        // The exact text of spec §6.5, key order included.
        assert_eq!(
            serde_json::to_string(&info).expect("serialise"),
            r#"{"id":"s_guess","name":"guess","kind":"variable","isConst":false,"type":"int","module":"mod_main","declBlock":"b003"}"#
        );
    }

    #[test]
    fn every_kind_has_its_json_shape() {
        let param = symbol(
            "text",
            SymbolKind::Parameter {
                mode: PassMode::ReadOnly,
            },
            Type::String,
            "fn_f",
            field("params[0]"),
        );
        round_trip(
            &SymbolInfo::new(sym("p_text"), "text", &param),
            &json!({"id": "p_text", "name": "text", "kind": "parameter", "mode": "read_only",
                    "type": "std::string", "module": "mod_main", "declBlock": "fn_f"}),
        );
        let counter = symbol("i", SymbolKind::LoopVariable, Type::Int, "b_for", field("VAR"));
        round_trip(
            &SymbolInfo::new(sym("s_i"), "i", &counter),
            &json!({"id": "s_i", "name": "i", "kind": "loopVariable", "type": "int",
                    "module": "mod_main", "declBlock": "b_for"}),
        );
        let function = symbol(
            "area",
            SymbolKind::Function {
                params: vec![sym("p_w"), sym("p_h")],
            },
            Type::Double,
            "fn_area",
            field("NAME"),
        );
        round_trip(
            &SymbolInfo::new(sym("f_area"), "area", &function),
            &json!({"id": "f_area", "name": "area", "kind": "function", "params": ["p_w", "p_h"],
                    "returns": "double", "type": "double", "module": "mod_main", "declBlock": "fn_area"}),
        );
        let void = symbol(
            "stop",
            SymbolKind::Function { params: Vec::new() },
            Type::Void,
            "fn_stop",
            field("NAME"),
        );
        let info = SymbolInfo::new(sym("f_stop"), "stop", &void);
        assert_eq!(
            info.kind,
            SymbolInfoKind::Function {
                params: Vec::new(),
                returns: Type::Void
            }
        );
        assert_eq!(serde_json::to_value(&info).expect("json")["returns"], "void");
    }

    #[test]
    fn the_users_name_wins_over_a_placeholder() {
        let mut s = symbol(
            "placeholder",
            SymbolKind::Variable { is_const: true },
            Type::Error,
            "b1",
            field("NAME"),
        );
        s.name = Ident::generated("b2c_invalid_name1").expect("name");
        let info = SymbolInfo::new(sym("s1"), "my score", &s);
        assert_eq!(info.name, "my score");
        assert_eq!(info.kind, SymbolInfoKind::Variable { is_const: true });
        assert_eq!(serde_json::to_value(&info).expect("json")["type"], "error");
    }

    #[test]
    fn pass_modes_and_types_read_back() {
        for (mode, text) in [
            (PassMode::Copy, "copy"),
            (PassMode::Editable, "editable"),
            (PassMode::ReadOnly, "read_only"),
        ] {
            assert_eq!(serde_json::to_value(mode).expect("json"), json!(text));
            assert_eq!(
                serde_json::from_value::<PassMode>(json!(text)).expect("mode"),
                mode
            );
        }
        assert!(serde_json::from_value::<PassMode>(json!("readOnly")).is_err());
        for (ty, text) in [
            (Type::Void, "void"),
            (Type::Bool, "bool"),
            (Type::Char, "char"),
            (Type::Int, "int"),
            (Type::Double, "double"),
            (Type::String, "std::string"),
            (Type::Error, "error"),
        ] {
            assert_eq!(serde_json::to_value(&ty).expect("json"), json!(text));
        }
    }

    #[test]
    fn user_type_kinds_have_their_json_shape() {
        let module = ModuleId::new("mod_main").expect("id");
        let make = |kind: SymbolKind, ty: Type| {
            SymbolInfo::new(
                sym("s1"),
                "Point",
                &symbol("Point", kind, ty, "b1", field("NAME")),
            )
        };
        let point = Type::Named(sym("s1"));
        let structure = make(
            SymbolKind::Struct {
                fields: vec![sym("f_x"), sym("f_y")],
                comparable: true,
                printable: false,
            },
            point.clone(),
        );
        assert_eq!(
            structure.kind,
            SymbolInfoKind::Struct {
                fields: vec![sym("f_x"), sym("f_y")],
                comparable: true,
                printable: false
            }
        );
        round_trip(
            &structure,
            &json!({"id": "s1", "name": "Point", "kind": "struct", "fields": ["f_x", "f_y"],
                    "comparable": true, "printable": false, "type": {"ref": "s1"},
                    "module": "mod_main", "declBlock": "b1"}),
        );
        let mut shared = make(SymbolKind::Field { owner: sym("s_pt") }, Type::Double);
        assert!(!shared.shared);
        shared.shared = true;
        round_trip(
            &shared,
            &json!({"id": "s1", "name": "Point", "kind": "field", "owner": "s_pt", "type": "double",
                    "module": "mod_main", "declBlock": "b1", "shared": true}),
        );
        let mut colour = make(
            SymbolKind::Enum {
                values: vec![sym("e_red")],
            },
            point.clone(),
        );
        colour.ty = Type::Int;
        assert_eq!(
            serde_json::to_value(&colour).expect("json")["values"],
            json!(["e_red"])
        );
        let mut red = make(SymbolKind::Enumerator { owner: sym("s_col") }, point);
        red.ty = Type::Int;
        assert_eq!(serde_json::to_value(&red).expect("json")["kind"], "enumerator");
        assert_eq!(red.module, module);
    }

    #[test]
    fn malformed_records_are_rejected() {
        let good = json!({"id": "s1", "name": "x", "kind": "variable", "isConst": false,
                          "type": "int", "module": "m", "declBlock": "b"});
        assert!(serde_json::from_value::<SymbolInfo>(good.clone()).is_ok());
        let mut bad_id = good.clone();
        bad_id["id"] = json!("s-1");
        assert!(serde_json::from_value::<SymbolInfo>(bad_id).is_err());
        let mut bad_kind = good.clone();
        bad_kind["kind"] = json!("class");
        assert!(serde_json::from_value::<SymbolInfo>(bad_kind).is_err());
        let mut missing = good;
        missing.as_object_mut().expect("object").remove("isConst");
        assert!(serde_json::from_value::<SymbolInfo>(missing).is_err());
    }
}
