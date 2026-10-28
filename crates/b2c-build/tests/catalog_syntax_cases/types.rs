//! The catalog snapshot cases of `catalog/core/types.toml` (W2, package C1;
//! docs/plans/m3-contracts/w2.md §1.5).
//!
//! Each case defines a struct `Point` (fields `x`, an int, and `y`, a
//! double) or an enum `Colour` (`Red`, `Green`) on the canvas and uses it in
//! `main`. The curated variants cover the forms the code generator chooses:
//! the operators of a comparable and printable struct, a struct holding an
//! enum and a list of itself, *new … with* with an empty part (`{}` in
//! C++17) and a part cast to its field's type, and an enum joined into text
//! (`b2c::to_text`).

use serde_json::{Value, json};

use super::{Blocks, Case, declaration, nested, reference, tokens};

/// The struct's symbol.
const POINT: &str = "s_Point";
/// The enum's symbol.
const COLOUR: &str = "s_Colour";
/// The variable the cases make (`p`).
const P: &str = "s_p";

/// The symbol of a struct's field or an enum's value.
fn member(owner: &str, name: &str) -> String {
    format!("{owner}_{name}")
}

/// A user type as a type term.
fn user(symbol: &str) -> Value {
    json!({ "ref": symbol })
}

/// Tokens of one number.
fn num(text: &str) -> Value {
    tokens(&json!([{ "num": text }]))
}

/// Tokens of one reference.
fn get(symbol: &str) -> Value {
    tokens(&json!([reference(symbol)]))
}

/// The struct `Point` with fields `x` (int) and `y` (double), and any more
/// `(name, type term)` rows, ticked as given.
fn point(b: &Blocks, more: &[(&str, Value)], comparable: bool, printable: bool) -> Value {
    let mut rows = vec![
        json!({ "sym": member(POINT, "x"), "name": "x", "type": "int" }),
        json!({ "sym": member(POINT, "y"), "name": "y", "type": "double" }),
    ];
    rows.extend(
        more.iter()
            .map(|(name, ty)| json!({ "sym": member(POINT, name), "name": name, "type": ty })),
    );
    b.block(
        "type.struct",
        &json!({
            "x": 400, "y": 40,
            "fields": { "NAME": declaration(POINT, "Point"), "COMPARABLE": comparable, "PRINTABLE": printable },
            "extra": { "fields": rows },
        }),
    )
}

/// The enum `Colour` with the values `Red` and `Green`.
fn colour(b: &Blocks) -> Value {
    b.block(
        "type.enum",
        &json!({
            "x": 400, "y": 400,
            "fields": { "NAME": declaration(COLOUR, "Colour") },
            "extra": { "values": [
                { "sym": member(COLOUR, "Red"), "name": "Red" },
                { "sym": member(COLOUR, "Green"), "name": "Green" },
            ] },
        }),
    )
}

/// *new Point with* these parts (`None` for an empty part).
fn new_point(b: &Blocks, parts: &[Option<Value>]) -> Value {
    let inputs: serde_json::Map<String, Value> = parts
        .iter()
        .enumerate()
        .filter_map(|(index, part)| part.clone().map(|part| (format!("FIELD{index}"), part)))
        .collect();
    b.block(
        "type.new",
        &json!({ "fields": { "TYPE": user(POINT) }, "extra": { "fieldCount": parts.len() }, "inputs": inputs }),
    )
}

/// *create* `p` as a `Point` set to `value` (a block).
fn create_p(b: &Blocks, value: &Value) -> Value {
    b.block(
        "var.declare",
        &json!({
            "fields": { "TYPE": user(POINT), "NAME": declaration(P, "p") },
            "inputs": { "VALUE": nested(value) },
        }),
    )
}

/// *create* `p` as a `Point` made from 1 and 2.5.
fn create_point(b: &Blocks) -> Value {
    create_p(b, &new_point(b, &[Some(num("1")), Some(num("2.5"))]))
}

/// *`field` of p*.
fn field_of_p(b: &Blocks, field: &str) -> Value {
    b.block(
        "type.field",
        &json!({ "fields": { "MEMBER": reference(&member(POINT, field)) }, "inputs": { "OBJ": get(P) } }),
    )
}

/// *Colour `value`*.
fn colour_value(b: &Blocks, value: &str) -> Value {
    b.block(
        "type.enum_value",
        &json!({ "fields": { "ENUM": reference(COLOUR), "VALUE": reference(&member(COLOUR, value)) } }),
    )
}

/// The cases of the file's blocks.
pub(super) fn cases(b: &Blocks) -> Vec<Case> {
    let mut cases = struct_cases(b);
    cases.extend(struct_variants(b));
    cases.extend(enum_cases(b));
    cases
}

/// The cases of the struct blocks.
fn struct_cases(b: &Blocks) -> Vec<Case> {
    vec![
        Case::new(
            "type.struct",
            "a struct Point with an int x and a double y, beside an empty main",
            vec![b.main(&[]), point(b, &[], false, false)],
        ),
        Case::new(
            "type.new",
            "makes the Point p from 1 and 2.5 (a struct with an int x and a double y), then main prints p's x",
            vec![
                b.main(&[create_point(b), b.print(&nested(&field_of_p(b, "x")))]),
                point(b, &[], false, false),
            ],
        ),
        Case::variant(
            "type.new",
            "empty-and-cast",
            "makes the Point p with an empty x and a y from the int n (so it is cast), after creating n = 3; \
             main prints p's y",
            vec![
                b.main(&[
                    b.block(
                        "var.declare",
                        &json!({ "fields": { "TYPE": "int", "NAME": declaration("s_n", "n") },
                                 "inputs": { "VALUE": num("3") } }),
                    ),
                    create_p(b, &new_point(b, &[None, Some(get("s_n"))])),
                    b.print(&nested(&field_of_p(b, "y"))),
                ]),
                point(b, &[], false, false),
            ],
        ),
        Case::new(
            "type.field",
            "printed: the y of the Point p, made from 1 and 2.5",
            vec![
                b.main(&[create_point(b), b.print(&nested(&field_of_p(b, "y")))]),
                point(b, &[], false, false),
            ],
        ),
        Case::new(
            "type.set_field",
            "sets the x of the Point p, made from 1 and 2.5, to 7, then main prints p's x",
            vec![
                b.main(&[
                    create_point(b),
                    b.block(
                        "type.set_field",
                        &json!({
                            "fields": { "MEMBER": reference(&member(POINT, "x")), "VAR": reference(P) },
                            "inputs": { "VALUE": num("7") },
                        }),
                    ),
                    b.print(&nested(&field_of_p(b, "x"))),
                ]),
                point(b, &[], false, false),
            ],
        ),
    ]
}

/// The curated variants of the struct block: its operators, and a struct
/// holding an enum and a list of itself.
fn struct_variants(b: &Blocks) -> Vec<Case> {
    let compare = |op: &str| {
        b.block(
            "math.compare",
            &json!({ "fields": { "OP": op }, "inputs": { "A": get(P), "B": get(P) } }),
        )
    };
    vec![
        Case::variant(
            "type.struct",
            "comparable-printable",
            "a struct Point with an int x and a double y, ticked comparable and printable; main makes a \
             Point p from 1 and 2.5, prints it, and prints whether p is less than itself",
            vec![
                b.main(&[
                    create_point(b),
                    b.print(&get(P)),
                    b.print(&nested(&compare("lt"))),
                ]),
                point(b, &[], true, true),
            ],
        ),
        Case::variant(
            "type.struct",
            "defaults",
            "a struct Point with an int x that starts at 7 and a double y without a default; main creates \
             the Point p without a value and prints its x",
            vec![
                b.main(&[
                    {
                        // Without the toolbox's starting value.
                        let mut create = b.block(
                            "var.declare",
                            &json!({ "fields": { "TYPE": user(POINT), "NAME": declaration(P, "p") } }),
                        );
                        create["inputs"] = json!({});
                        create
                    },
                    b.print(&nested(&field_of_p(b, "x"))),
                ]),
                {
                    let mut point = point(b, &[], false, false);
                    point["inputs"] = json!({ "DEFAULT0": num("7") });
                    point
                },
            ],
        ),
        Case::variant(
            "type.struct",
            "nested",
            "a struct Point with an int x, a double y, a Colour colour and a list of Point near, and the \
             enum Colour (Red, Green); main makes a Point p from 1 and 2.5, its other parts empty, and \
             prints its colour",
            vec![
                b.main(&[
                    create_p(b, &new_point(b, &[Some(num("1")), Some(num("2.5")), None, None])),
                    b.print(&nested(&field_of_p(b, "colour"))),
                ]),
                point(
                    b,
                    &[
                        ("colour", user(COLOUR)),
                        ("near", json!({ "std::vector": user(POINT) })),
                    ],
                    false,
                    false,
                ),
                colour(b),
            ],
        )
        .with_format_version(2),
    ]
}

/// The cases of the enum blocks.
fn enum_cases(b: &Blocks) -> Vec<Case> {
    vec![
        Case::new(
            "type.enum",
            "an enum Colour with the values Red and Green, beside an empty main",
            vec![b.main(&[]), colour(b)],
        ),
        Case::new(
            "type.enum_value",
            "printed: Colour Green, of the enum Colour (Red, Green)",
            vec![b.main(&[b.print(&nested(&colour_value(b, "Green")))]), colour(b)],
        ),
        Case::variant(
            "type.enum_value",
            "joined",
            "printed: the join of \"colour \" and Colour Red, of the enum Colour (Red, Green)",
            vec![
                b.main(&[b.print(&nested(&b.block(
                    "text.join",
                    &json!({ "extra": { "itemCount": 2 },
                             "inputs": { "ITEM0": tokens(&json!([{ "str": "colour " }])),
                                         "ITEM1": nested(&colour_value(b, "Red")) } }),
                )))]),
                colour(b),
            ],
        ),
    ]
}
