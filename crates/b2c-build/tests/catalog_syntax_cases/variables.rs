//! The catalog snapshot cases of `catalog/core/variables.toml`, with
//! package C2's globals (w2.md §2.7).

use serde_json::{Map, Value, json};

use super::{Blocks, Case, F, X, declaration, nested, reference, tokens};

/// *create*, *set*, *change* and *get* on the `int` x, and variables and
/// parameters of library types.
pub(super) fn cases(b: &Blocks) -> Vec<Case> {
    vec![
        Case::new(
            "var.declare",
            "creates x, before a print of x, in main",
            vec![b.main(&[b.create_x(&json!({})), b.print_x()])],
        ),
        Case::new(
            "var.set",
            "sets the int x to 0, after create x and before a print of x, in main",
            b.with_x(b.block(
                "var.set",
                &json!({
                    "fields": { "VAR": reference(X) },
                    "inputs": { "VALUE": tokens(&json!([{ "num": "0" }])) },
                }),
            )),
        ),
        Case::new(
            "var.change",
            "changes the int x, after create x and before a print of x, in main",
            b.with_x(b.block("var.change", &json!({ "fields": { "VAR": reference(X) } }))),
        ),
        Case::new(
            "var.get",
            "the int x, printed after create x in main",
            vec![b.main(&[
                b.create_x(&json!({})),
                b.print(&nested(
                    &b.block("var.get", &json!({ "fields": { "VAR": reference(X) } })),
                )),
            ])],
        ),
        library_types(b),
        Case::new(
            "var.global",
            "the global variable count (the toolbox's: an int set to 0) and the global constant kMax = 3, \
             both printed in main",
            vec![
                b.main(&[
                    b.print(&tokens(&json!([reference("s_count")]))),
                    b.print(&tokens(&json!([reference("s_kMax")]))),
                ]),
                b.block(
                    "var.global",
                    &json!({ "x": 400, "y": 40, "fields": { "NAME": declaration("s_count", "count") } }),
                ),
                b.block(
                    "var.global",
                    &json!({
                        "x": 400, "y": 120,
                        "fields": { "KIND": "constant", "NAME": declaration("s_kMax", "kMax") },
                        "inputs": { "VALUE": tokens(&json!([{ "num": "3" }])) },
                    }),
                ),
            ],
        ),
    ]
}

/// Variables and parameters of library types, from a format-version-2
/// project: `f` takes a list of `int` read-only and one editable, a map by
/// copy and one editable, and copies each into the editable one; `main`
/// creates the four and calls `f`.
fn library_types(b: &Blocks) -> Case {
    let list = json!({ "std::vector": "int" });
    let map = json!({ "std::map": ["std::string", { "std::set": "char" }] });
    // Without the toolbox's start value 0: a list starts empty.
    let create = |symbol: &str, name: &str, ty: &Value| {
        let mut block = b.block(
            "var.declare",
            &json!({ "fields": { "TYPE": ty, "NAME": declaration(symbol, name) } }),
        );
        block.as_object_mut().unwrap().remove("inputs");
        block
    };
    let row = |symbol: &str, name: &str, ty: &Value, mode: &str| json!({ "sym": symbol, "name": name, "type": ty, "mode": mode });
    let set = |to: &str, from: &str| {
        b.block(
            "var.set",
            &json!({
                "fields": { "VAR": reference(to) },
                "inputs": { "VALUE": tokens(&json!([reference(from)])) },
            }),
        )
    };
    let define = b.define_f(&json!({
        "extra": { "params": [
            row("s_from", "from", &list, "read_only"),
            row("s_to", "to", &list, "editable"),
            row("s_names", "names", &map, "copy"),
            row("s_out", "out", &map, "editable"),
        ] },
        "statements": { "BODY": [set("s_to", "s_from"), set("s_out", "s_names")] },
    }));
    let args: Map<String, Value> = ["s_a", "s_b", "s_m", "s_n"]
        .iter()
        .enumerate()
        .map(|(index, symbol)| (format!("ARG{index}"), tokens(&json!([reference(symbol)]))))
        .collect();
    let call = b.block(
        "func.call_stmt",
        &json!({ "fields": { "FUNC": reference(F) }, "extra": { "argCount": 4 }, "inputs": args }),
    );
    Case::variant(
        "var.declare",
        "library-types",
        "creates the lists a and b and the maps m and n, and calls f(a, b, m, n), in main; f copies its \
         read-only list and its copied map into its editable ones (format version 2)",
        vec![
            b.main(&[
                create("s_a", "a", &list),
                create("s_b", "b", &list),
                create("s_m", "m", &map),
                create("s_n", "n", &map),
                call,
            ]),
            define,
        ],
    )
    .with_format_version(2)
}
