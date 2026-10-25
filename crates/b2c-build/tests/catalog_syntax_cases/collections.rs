//! The catalog snapshot cases of `catalog/core/collections.toml` (W2, package
//! A; docs/plans/m3-contracts/w2.md §1.2).
//!
//! Every case is a format-version-2 project, since its collections have
//! template types. A collection is created without a value (it starts
//! empty) or from *list of*, and its length is printed at the end, so that
//! the program reads it. The curated variants cover each form the code
//! generator chooses by type (set, map, queue, stack, priority queue, text),
//! each option that changes the form, the casts of brace items (slots.md
//! §3.1) and loops whose body does not read their names.

use serde_json::{Value, json};

use super::{Blocks, Case, declaration, nested, reference, tokens};

const LIST: &str = "s_xs";

/// A type term: `std::vector<int>` and so on.
fn list_of(item: Value) -> Value {
    lib("std::vector", item)
}

/// A library template's type term: `std::set<int>` and so on.
fn lib(name: &str, args: Value) -> Value {
    Value::Object(serde_json::Map::from_iter([(String::from(name), args)]))
}

/// Tokens of one number.
fn num(text: &str) -> Value {
    tokens(&json!([{ "num": text }]))
}

/// Tokens of one text.
fn text(value: &str) -> Value {
    tokens(&json!([{ "str": value }]))
}

/// Tokens of one reference.
fn get(symbol: &str) -> Value {
    tokens(&json!([reference(symbol)]))
}

/// *create* `name` of type `ty` without a value: an empty collection.
fn create_empty(b: &Blocks, symbol: &str, name: &str, ty: &Value) -> Value {
    let mut block = b.block(
        "var.declare",
        &json!({ "fields": { "TYPE": ty, "NAME": declaration(symbol, name) } }),
    );
    block.as_object_mut().unwrap().remove("inputs");
    block
}

/// *create* `name` of type `ty` set to `value` (an input).
fn create_with(b: &Blocks, symbol: &str, name: &str, ty: &Value, value: &Value) -> Value {
    b.block(
        "var.declare",
        &json!({
            "fields": { "TYPE": ty, "NAME": declaration(symbol, name) },
            "inputs": { "VALUE": value },
        }),
    )
}

/// *list of* with these item inputs.
fn list_of_items(b: &Blocks, items: &[Value]) -> Value {
    let inputs: serde_json::Map<String, Value> = items
        .iter()
        .enumerate()
        .map(|(index, item)| (format!("ITEM{index}"), item.clone()))
        .collect();
    b.block(
        "coll.list_of",
        &json!({ "extra": { "itemCount": items.len() }, "inputs": inputs }),
    )
}

/// The list of `int` `xs`, holding 1, 2 and 3.
fn create_xs(b: &Blocks) -> Value {
    let items = list_of_items(b, &[num("1"), num("2"), num("3")]);
    create_with(b, LIST, "xs", &list_of(json!("int")), &nested(&items))
}

/// A print of *length of* `symbol`.
fn print_length(b: &Blocks, symbol: &str) -> Value {
    let length = b.block("coll.length", &json!({ "fields": { "LIST": reference(symbol) } }));
    b.print(&nested(&length))
}

/// A collection block naming `symbol` in `LIST`, with these inputs.
fn on(b: &Blocks, kind: &str, symbol: &str, inputs: &Value) -> Value {
    b.block(
        kind,
        &json!({ "fields": { "LIST": reference(symbol) }, "inputs": inputs }),
    )
}

/// `main` with `steps` between the creation of `xs` (1, 2, 3) and a print
/// of its length.
fn around_xs(b: &Blocks, steps: Vec<Value>) -> Vec<Value> {
    let mut body = vec![create_xs(b)];
    body.extend(steps);
    body.push(print_length(b, LIST));
    vec![b.main(&body)]
}

/// `main` with the empty collection `c` of type `ty`, `steps`, then a print
/// of its length.
fn around_empty(b: &Blocks, ty: &Value, steps: Vec<Value>) -> Vec<Value> {
    let mut body = vec![create_empty(b, "s_c", "c", ty)];
    body.extend(steps);
    body.push(print_length(b, "s_c"));
    vec![b.main(&body)]
}

/// A default case of a statement block, placed as `around_xs` places it.
fn on_xs(b: &Blocks, kind: &'static str, inputs: &Value, placement: &'static str) -> Case {
    Case::new(kind, placement, around_xs(b, vec![on(b, kind, LIST, inputs)])).with_format_version(2)
}

/// A curated variant on a collection of type `ty`.
fn on_empty(
    b: &Blocks,
    kind: &'static str,
    variant: &str,
    ty: &Value,
    steps: Vec<Value>,
    placement: &'static str,
) -> Case {
    Case::variant(kind, variant, placement, around_empty(b, ty, steps)).with_format_version(2)
}

/// A value block printed after `setup` in `main`.
fn printed_after(b: &Blocks, setup: Vec<Value>, value: &Value) -> Vec<Value> {
    let mut body = setup;
    body.push(b.print(&nested(value)));
    vec![b.main(&body)]
}

/// Every block of the file in its default configuration, and the curated
/// variants.
pub(super) fn cases(b: &Blocks) -> Vec<Case> {
    let mut cases = additions(b);
    cases.extend(removals(b));
    cases.extend(item_values(b));
    cases.extend(whole_values(b));
    cases.extend(list_of_cases(b));
    cases.extend(loops(b));
    cases.extend(pair_loops(b));
    cases
}

/// *add*, *set item*.
fn additions(b: &Blocks) -> Vec<Case> {
    let int = json!("int");
    let set = lib("std::set", int.clone());
    let map = lib("std::map", json!(["std::string", "int"]));
    let queue = lib("std::queue", int.clone());
    let deque = lib("std::deque", int.clone());
    let grid = list_of(list_of(int.clone()));
    let placement = "between create xs (a list of int: 1, 2, 3) and a print of its length, in main";
    let variant = "between create c (empty) and a print of its length, in main";
    vec![
        // *add*: the start value of the list's item type (0).
        on_xs(b, "coll.add", &json!({ "VALUE": num("0") }), placement),
        on_empty(
            b,
            "coll.add",
            "set",
            &set,
            vec![on(b, "coll.add", "s_c", &json!({ "VALUE": num("4") }))],
            variant,
        ),
        on_empty(
            b,
            "coll.add",
            "queue",
            &queue,
            vec![on(b, "coll.add", "s_c", &json!({ "VALUE": num("4") }))],
            variant,
        ),
        on_empty(
            b,
            "coll.add",
            "deque",
            &deque,
            vec![on(b, "coll.add", "s_c", &json!({ "VALUE": num("4") }))],
            variant,
        ),
        on_empty(
            b,
            "coll.add",
            "list-of-lists",
            &grid,
            vec![on(
                b,
                "coll.add",
                "s_c",
                &json!({ "VALUE": nested(&list_of_items(b, &[num("1"), num("2")])) }),
            )],
            "adds a list of 1 and 2 to c, a list of lists, between create c (empty) and a print of its \
             length, in main",
        ),
        // *set item*: position 0, the start value 0.
        on_xs(
            b,
            "coll.set_item",
            &json!({ "INDEX": num("0"), "VALUE": num("0") }),
            placement,
        ),
        on_empty(
            b,
            "coll.set_item",
            "map",
            &map,
            vec![on(
                b,
                "coll.set_item",
                "s_c",
                &json!({ "INDEX": text("Ann"), "VALUE": num("30") }),
            )],
            "sets the value for Ann in c, a map from string to int, between create c (empty) and a print \
             of its length, in main",
        ),
        Case::variant(
            "coll.set_item",
            "position",
            "sets the item of xs at the int i (0) to i, between create xs (1, 2, 3) and create i and a print \
             of its length, in main",
            around_xs(
                b,
                vec![
                    create_with(b, "s_i", "i", &int, &num("0")),
                    on(
                        b,
                        "coll.set_item",
                        LIST,
                        &json!({ "INDEX": get("s_i"), "VALUE": get("s_i") }),
                    ),
                ],
            ),
        )
        .with_format_version(2),
    ]
}

/// *clear*, *remove …*, *sort*.
fn removals(b: &Blocks) -> Vec<Case> {
    let int = json!("int");
    let set = lib("std::set", int.clone());
    let map = lib("std::map", json!(["std::string", "int"]));
    let queue = lib("std::queue", int.clone());
    let stack = lib("std::stack", int.clone());
    let deque = lib("std::deque", int);
    let placement = "between create xs (a list of int: 1, 2, 3) and a print of its length, in main";
    let variant = "between create c (empty) and a print of its length, in main";
    vec![
        on_xs(b, "coll.clear", &json!({}), placement),
        on_empty(
            b,
            "coll.clear",
            "stack",
            &stack,
            vec![on(b, "coll.clear", "s_c", &json!({}))],
            variant,
        ),
        on_xs(b, "coll.remove_at", &json!({ "INDEX": num("0") }), placement),
        Case::new(
            "coll.remove_value",
            "removes 4 from c, a set of int, between create c (empty) and a print of its length, in main",
            around_empty(
                b,
                &set,
                vec![on(b, "coll.remove_value", "s_c", &json!({ "VALUE": num("4") }))],
            ),
        )
        .with_format_version(2),
        on_empty(
            b,
            "coll.remove_value",
            "map",
            &map,
            vec![on(
                b,
                "coll.remove_value",
                "s_c",
                &json!({ "VALUE": text("Ann") }),
            )],
            "removes the key Ann from c, a map from string to int, between create c (empty) and a print of \
             its length, in main",
        ),
        Case::new(
            "coll.remove_next",
            "removes the next item of c, a queue of int holding 4, between create c and a print of its \
             length, in main",
            around_empty(
                b,
                &queue,
                vec![
                    on(b, "coll.add", "s_c", &json!({ "VALUE": num("4") })),
                    plain_on(b, "coll.remove_next", "s_c"),
                ],
            ),
        )
        .with_format_version(2),
        on_xs(b, "coll.sort", &json!({}), placement),
        Case::variant(
            "coll.sort",
            "descending",
            placement,
            around_xs(
                b,
                vec![b.block(
                    "coll.sort",
                    &json!({ "fields": { "LIST": reference(LIST), "ORDER": "descending" } }),
                )],
            ),
        )
        .with_format_version(2),
        on_empty(
            b,
            "coll.sort",
            "deque",
            &deque,
            vec![plain_on(b, "coll.sort", "s_c")],
            variant,
        ),
    ]
}

/// A collection block naming `symbol` in `LIST`, without inputs.
fn plain_on(b: &Blocks, kind: &str, symbol: &str) -> Value {
    b.block(kind, &json!({ "fields": { "LIST": reference(symbol) } }))
}

/// *item of*.
fn item_values(b: &Blocks) -> Vec<Case> {
    let int = json!("int");
    let map = lib("std::map", json!(["std::string", "int"]));
    let printed = "printed after create xs (a list of int: 1, 2, 3), in main";
    let item = |index: Value| on(b, "coll.item", LIST, &json!({ "INDEX": index }));
    vec![
        Case::new(
            "coll.item",
            printed,
            printed_after(b, vec![create_xs(b)], &item(num("0"))),
        )
        .with_format_version(2),
        Case::variant(
            "coll.item",
            "position",
            "the item of xs at the int i (1), printed after create xs (1, 2, 3) and create i, in main",
            printed_after(
                b,
                vec![create_xs(b), create_with(b, "s_i", "i", &int, &num("1"))],
                &item(get("s_i")),
            ),
        )
        .with_format_version(2),
        Case::variant(
            "coll.item",
            "map",
            "the value for Ann of c, a map from string to int, printed after create c (empty) and set item, \
             in main",
            printed_after(
                b,
                vec![
                    create_empty(b, "s_c", "c", &map),
                    on(
                        b,
                        "coll.set_item",
                        "s_c",
                        &json!({ "INDEX": text("Ann"), "VALUE": num("30") }),
                    ),
                ],
                &on(b, "coll.item", "s_c", &json!({ "INDEX": text("Ann") })),
            ),
        )
        .with_format_version(2),
    ]
}

/// *length of*, *is empty*, *contains*, *next item of*.
fn whole_values(b: &Blocks) -> Vec<Case> {
    let int = json!("int");
    let map = lib("std::map", json!(["std::string", "int"]));
    let printed = "printed after create xs (a list of int: 1, 2, 3), in main";
    vec![
        Case::new(
            "coll.length",
            printed,
            printed_after(b, vec![create_xs(b)], &plain_on(b, "coll.length", LIST)),
        )
        .with_format_version(2),
        Case::new(
            "coll.is_empty",
            printed,
            printed_after(b, vec![create_xs(b)], &plain_on(b, "coll.is_empty", LIST)),
        )
        .with_format_version(2),
        Case::new(
            "coll.contains",
            printed,
            printed_after(
                b,
                vec![create_xs(b)],
                &on(b, "coll.contains", LIST, &json!({ "VALUE": num("2") })),
            ),
        )
        .with_format_version(2),
        Case::variant(
            "coll.contains",
            "set",
            "whether c, a set of int, holds 2, printed after create c (empty), in main",
            printed_after(
                b,
                vec![create_empty(b, "s_c", "c", &lib("std::set", int.clone()))],
                &on(b, "coll.contains", "s_c", &json!({ "VALUE": num("2") })),
            ),
        )
        .with_format_version(2),
        Case::variant(
            "coll.contains",
            "map",
            "whether c, a map from string to int, has the key Ann, printed after create c (empty), in main",
            printed_after(
                b,
                vec![create_empty(b, "s_c", "c", &map)],
                &on(b, "coll.contains", "s_c", &json!({ "VALUE": text("Ann") })),
            ),
        )
        .with_format_version(2),
        Case::variant(
            "coll.contains",
            "text",
            "whether c, a list of string, holds the text Ann, printed after create c (empty), in main",
            printed_after(
                b,
                vec![create_empty(b, "s_c", "c", &list_of(json!("std::string")))],
                &on(b, "coll.contains", "s_c", &json!({ "VALUE": text("Ann") })),
            ),
        )
        .with_format_version(2),
        Case::variant(
            "coll.contains",
            "list-of",
            "whether c, a list of lists of int, holds a list of 1 and 2, printed after create c (empty), in \
             main",
            printed_after(
                b,
                vec![create_empty(b, "s_c", "c", &list_of(list_of(int.clone())))],
                &on(
                    b,
                    "coll.contains",
                    "s_c",
                    &json!({ "VALUE": nested(&list_of_items(b, &[num("1"), num("2")])) }),
                ),
            ),
        )
        .with_format_version(2),
        Case::new(
            "coll.next",
            "the next item of c, a queue of int holding 4, printed after create c and add, in main",
            next_case(b, "std::queue"),
        )
        .with_format_version(2),
        Case::variant(
            "coll.next",
            "stack",
            "the next item of c, a stack of int holding 4, printed after create c and add, in main",
            next_case(b, "std::stack"),
        )
        .with_format_version(2),
        Case::variant(
            "coll.next",
            "priority-queue",
            "the next item of c, a priority queue of int holding 4, printed after create c and add, in main",
            next_case(b, "std::priority_queue"),
        )
        .with_format_version(2),
    ]
}

/// *next item of* a `kind` of `int` holding 4.
fn next_case(b: &Blocks, kind: &str) -> Vec<Value> {
    printed_after(
        b,
        vec![
            create_empty(b, "s_c", "c", &lib(kind, json!("int"))),
            on(b, "coll.add", "s_c", &json!({ "VALUE": num("4") })),
        ],
        &plain_on(b, "coll.next", "s_c"),
    )
}

/// *list of* in *create* (its place), with the casts of slots.md §3.1.
fn list_of_cases(b: &Blocks) -> Vec<Case> {
    let in_create = |ty: Value, items: &[Value]| {
        let list = list_of_items(b, items);
        around_list(b, &ty, &list, Vec::new())
    };
    let int = json!("int");
    vec![
        // As the toolbox gives it, landed in *create*: two items with the
        // start value of the place's item type.
        Case::new(
            "coll.list_of",
            "the value of create c (a list of int), before a print of its length, in main",
            in_create(list_of(int.clone()), &[num("0"), num("0")]),
        )
        .with_format_version(2),
        Case::variant(
            "coll.list_of",
            "empty",
            "the value of create c (a list of int) with no items, before a print of its length, in main",
            in_create(list_of(int.clone()), &[]),
        )
        .with_format_version(2),
        Case::variant(
            "coll.list_of",
            "literals",
            "the value of create c (a list of double) from an int and a char literal, which fit as \
             written, and a double, before a print of its length, in main",
            in_create(
                list_of(json!("double")),
                &[num("1"), tokens(&json!([{ "chr": "a" }])), num("2.5")],
            ),
        )
        .with_format_version(2),
        Case::variant(
            "coll.list_of",
            "casts",
            "the value of create c (a list of double) from the int i, cast, and 2.5, after create i (1) and \
             before a print of its length, in main",
            around_list(
                b,
                &list_of(json!("double")),
                &list_of_items(b, &[get("s_i"), num("2.5")]),
                vec![create_with(b, "s_i", "i", &int, &num("1"))],
            ),
        )
        .with_format_version(2),
        Case::variant(
            "coll.list_of",
            "set",
            "the value of create c (a set of string), before a print of its length, in main",
            in_create(lib("std::set", json!("std::string")), &[text("b"), text("a")]),
        )
        .with_format_version(2),
        Case::variant(
            "coll.list_of",
            "nested",
            "the value of create c (a list of lists of int), holding two lists, before a print of its \
             length, in main",
            around_list(
                b,
                &list_of(list_of(int.clone())),
                &list_of_items(
                    b,
                    &[
                        nested(&list_of_items(b, &[num("1"), num("2")])),
                        nested(&list_of_items(b, &[])),
                    ],
                ),
                Vec::new(),
            ),
        )
        .with_format_version(2),
    ]
}

/// `main` with `setup`, *create* `c` of type `ty` from `list`, and a print of
/// its length.
fn around_list(b: &Blocks, ty: &Value, list: &Value, setup: Vec<Value>) -> Vec<Value> {
    let mut body = setup;
    body.push(create_with(b, "s_c", "c", ty, &nested(list)));
    body.push(print_length(b, "s_c"));
    vec![b.main(&body)]
}

/// *for each*.
fn loops(b: &Blocks) -> Vec<Case> {
    let each = |mode: &str, name: &str, list: &Value, body: Vec<Value>| {
        b.block(
            "coll.for_each",
            &json!({
                "fields": { "ITEM": declaration("s_item", name), "MODE": mode },
                "inputs": { "LIST": list },
                "statements": { "BODY": body },
            }),
        )
    };
    let print_item = || b.print(&get("s_item"));
    let change_item = || b.block("var.change", &json!({ "fields": { "VAR": reference("s_item") } }));
    vec![
        Case::new(
            "coll.for_each",
            "prints each item of xs (a list of int: 1, 2, 3), between create xs and a print of its length, \
             in main",
            around_xs(b, vec![each("read_only", "item", &get(LIST), vec![print_item()])]),
        )
        .with_format_version(2),
        Case::variant(
            "coll.for_each",
            "unused",
            "prints a text for each item of xs, which the body never reads, between create xs and a print \
             of its length, in main",
            around_xs(
                b,
                vec![each("read_only", "item", &get(LIST), vec![b.print(&text("*"))])],
            ),
        )
        .with_format_version(2),
        Case::variant(
            "coll.for_each",
            "copy",
            "changes and prints a copy of each item of xs, between create xs and a print of its length, in \
             main",
            around_xs(
                b,
                vec![each(
                    "copy",
                    "item",
                    &get(LIST),
                    vec![change_item(), print_item()],
                )],
            ),
        )
        .with_format_version(2),
        Case::variant(
            "coll.for_each",
            "editable",
            "changes each item of xs, between create xs and a print of its length, in main",
            around_xs(b, vec![each("editable", "item", &get(LIST), vec![change_item()])]),
        )
        .with_format_version(2),
        Case::variant(
            "coll.for_each",
            "text",
            "prints each letter of the text hello, the only step of main",
            vec![b.main(&[each("read_only", "letter", &text("hello"), vec![print_item()])])],
        ),
        Case::variant(
            "coll.for_each",
            "set",
            "prints each item of c (an empty set of string), between create c and a print of its length, \
             in main",
            around_empty(
                b,
                &lib("std::set", json!("std::string")),
                vec![each("read_only", "item", &get("s_c"), vec![print_item()])],
            ),
        )
        .with_format_version(2),
    ]
}

/// *for each key value*.
fn pair_loops(b: &Blocks) -> Vec<Case> {
    let map = lib("std::map", json!(["std::string", "int"]));
    let pair = |body: Vec<Value>| {
        b.block(
            "coll.for_each_pair",
            &json!({
                "fields": {
                    "MAP": reference("s_c"),
                    "KEY": declaration("s_key", "key"),
                    "VALUE": declaration("s_value", "value"),
                },
                "statements": { "BODY": body },
            }),
        )
    };
    vec![
        Case::new(
            "coll.for_each_pair",
            "prints each key and value of c (an empty map from string to int), between create c and a \
             print of its length, in main",
            around_empty(
                b,
                &map,
                vec![pair(vec![print_two(b, &get("s_key"), &get("s_value"))])],
            ),
        )
        .with_format_version(2),
        Case::variant(
            "coll.for_each_pair",
            "unused",
            "prints a text for each key of c (an empty map), which the body never reads, between create c \
             and a print of its length, in main",
            around_empty(b, &map, vec![pair(vec![b.print(&text("*"))])]),
        )
        .with_format_version(2),
    ]
}

/// A print of two values.
fn print_two(b: &Blocks, first: &Value, second: &Value) -> Value {
    b.block(
        "io.print",
        &json!({ "extra": { "itemCount": 2 }, "inputs": { "ITEM0": first, "ITEM1": second } }),
    )
}
