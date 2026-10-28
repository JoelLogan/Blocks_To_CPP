//! The generated benchmark document (docs/spec/09-quality-and-delivery.md
//! §9.2 "Benchmarks"): one module whose `program.main` holds repeated units
//! of ordinary blocks, so every stage of the pipeline has real work at any
//! size.
//!
//! The same generator exists in TypeScript for the webview benchmarks
//! (`apps/desktop/e2e/bench/document.ts`), so the native and the webview
//! numbers are about the same programs. Both describe the shape below, and
//! both check [`parity_digest`] against the same constants, so a change to
//! one that is not made to the other fails a test.
//!
//! # Shape
//!
//! * `program.main` (`b0`, at x 40, y 40) holds `units` units, then
//!   `fillers` prints, where `units = (main − 1) / 8` and
//!   `fillers = (main − 1) % 8`, `main` being the requested block count
//!   (minus the two blocks of the drag handle, when asked for).
//! * Unit `u` is 8 blocks: `int value_u = u;`, then
//!   `for (int i_u = 0; i_u < 10; ...)` holding an `if (i_u % 2 == 0)`
//!   whose branches change `value_u` by `i_u` and by `-1`, then
//!   `print "value u: ", value_u + 1` (a `math.arithmetic` with a
//!   `var.get` in it).
//! * Every block is one that every block catalog since 1.0.0 reads the
//!   same way (the second change is a plain *change* by `-1`, not *change
//!   … down by* 1, whose menu came with catalog 1.2.0): the webview A/B
//!   runs open the head's documents in the base's build too.
//! * With [`Units::Collections`] (W2, docs/plans/m3-contracts/w2.md §1.2;
//!   native only), unit `u` is 8 blocks of the collection blocks instead:
//!   `std::vector<int> scores_u{};`, a `for (int i_u …)` adding `i_u` to
//!   it, a `for (const auto& item_u : scores_u)` setting item 0 to
//!   `item_u`, `print "count u: ", length of scores_u` and `sort scores_u`
//!   largest first. The document is then format version 2 and names catalog
//!   1.3.0, which no A/B base before W2 reads, so the webview benchmarks
//!   (whose documents every catalog since 1.0.0 must read) and the parity
//!   digests keep the plain shape.
//! * With [`Units::Language`] (W2's integration step, w2.md §5.3; native
//!   only), the module also has 8 blocks of definitions after `main`: an
//!   enum `Color`, a struct `Point` (an `int` and a `Color`), a global
//!   `counter` and a global constant `LIMIT`, and two overloads of `area`,
//!   one with a default value (each a *define* holding a *return*). Unit
//!   `u` is then 8 blocks of the kind `u % 5`: 0, the collection unit
//!   above; 1, text, math and errors (a *try* reading a number from
//!   trimmed text and printing it kept between 0 and 10, its catch part
//!   printing the error's message); 2, structs, enums and globals (a
//!   `Point` made with *new … with*, its `x` set to an absolute value and
//!   printed, `counter` changed); 3, control (a *switch* on `counter` whose
//!   first case goes on into the next, with a default part, and a *do …
//!   then repeat* holding a *check at compile time* on `LIMIT` and a
//!   print); 4, functions, text and files (the two overloads called and
//!   printed, upper-case text, and whether a file of that name exists).
//! * A filler `f` is `print "filler f"`.
//! * The drag handle (webview drag benchmark only) is a separate
//!   `func.define dragMe` at x 800, y 40 holding one print: a top-level
//!   block the benchmark can drag back and forth without changing the
//!   program.
//!
//! Block IDs are `b<n>` in document order, symbol IDs `s_value_<u>`,
//! `s_i_<u>` and `s_drag` (`s_scores_<u>`, `s_i_<u>` and `s_item_<u>` in
//! collection units; `s_n_<u>`, `s_e_<u>`, `s_p_<u>` and `s_word_<u>` in
//! the other language units, and fixed IDs for the definitions).

use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

/// Blocks per unit.
pub(crate) const UNIT_BLOCKS: usize = 8;

/// Blocks of the drag handle (`func.define` and its print).
pub(crate) const HANDLE_BLOCKS: usize = 2;

/// Blocks of the definitions of [`Units::Language`].
pub(crate) const DEFINITION_BLOCKS: usize = 8;

/// What a shape's units are made of (see the module comment).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Units {
    /// Blocks every catalog since 1.0.0 reads (the webview documents too).
    Plain,
    /// W2's collection blocks.
    Collections,
    /// W2's blocks of every kind, with definitions after `main`.
    Language,
}

/// The most blocks a generated document may have: the project limit
/// (05 §5.6).
pub(crate) const MAX_BLOCKS: usize = 100_000;

/// What to generate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Shape {
    /// How many blocks the document has in all.
    pub(crate) blocks: usize,
    /// Whether it has the drag handle.
    pub(crate) drag_handle: bool,
    /// What its units are made of.
    pub(crate) units: Units,
}

impl Shape {
    /// The blocks outside `main`: the handle and the definitions, when the
    /// shape has them.
    pub(crate) const fn outside_main(self) -> usize {
        let handle = if self.drag_handle { HANDLE_BLOCKS } else { 0 };
        let definitions = if matches!(self.units, Units::Language) {
            DEFINITION_BLOCKS
        } else {
            0
        };
        handle + definitions
    }

    /// The fewest blocks this shape can have (`program.main`, plus the
    /// blocks outside it).
    pub(crate) const fn min_blocks(self) -> usize {
        1 + self.outside_main()
    }
}

/// A shape that cannot be generated.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub(crate) enum GenerateError {
    /// Fewer blocks than the shape needs.
    #[error("a benchmark document needs at least {min} blocks, not {blocks}")]
    TooFew {
        /// The requested count.
        blocks: usize,
        /// The fewest possible.
        min: usize,
    },
    /// More blocks than a project may have.
    #[error("a benchmark document may have at most {MAX_BLOCKS} blocks, not {blocks}")]
    TooMany {
        /// The requested count.
        blocks: usize,
    },
}

/// Hands out block IDs in document order.
struct Ids(usize);

impl Ids {
    fn next(&mut self) -> String {
        let id = format!("b{}", self.0);
        self.0 += 1;
        id
    }
}

fn num(value: usize) -> Value {
    json!({ "expr": [{ "num": value.to_string() }] })
}

fn print(ids: &mut Ids, text: &str) -> Value {
    json!({
        "id": ids.next(),
        "type": "io.print",
        "v": 1,
        "extra": { "itemCount": 1 },
        "fields": { "NEWLINE": true, "SEP": "none", "STREAM": "out" },
        "inputs": { "ITEM0": { "expr": [{ "str": text }] } },
    })
}

/// Unit `unit`: 8 blocks in three statements of `main` (see the module
/// comment). IDs are handed out in document order.
fn unit(ids: &mut Ids, unit: usize) -> [Value; 3] {
    let value = format!("s_value_{unit}");
    let counter = format!("s_i_{unit}");
    let declare = json!({
        "id": ids.next(),
        "type": "var.declare",
        "v": 1,
        "fields": {
            "CONST": false,
            "NAME": { "sym": value, "name": format!("value_{unit}") },
            "TYPE": "int",
        },
        "inputs": { "VALUE": num(unit) },
    });
    let loop_id = ids.next();
    let if_id = ids.next();
    let change = json!({
        "id": ids.next(),
        "type": "var.change",
        "v": 1,
        "fields": { "VAR": { "ref": value } },
        "inputs": { "BY": { "expr": [{ "ref": counter }] } },
    });
    let back = json!({
        "id": ids.next(),
        "type": "var.change",
        "v": 1,
        "fields": { "VAR": { "ref": value } },
        "inputs": { "BY": { "expr": [{ "op": "-" }, { "num": "1" }] } },
    });
    let branch = json!({
        "id": if_id,
        "type": "control.if",
        "v": 1,
        "extra": { "elseIfCount": 0, "hasElse": true },
        "inputs": {
            "COND0": { "expr": [
                { "ref": counter }, { "op": "%" }, { "num": "2" }, { "op": "==" }, { "num": "0" },
            ] },
        },
        "statements": { "DO0": [change], "ELSE": [back] },
    });
    let for_loop = json!({
        "id": loop_id,
        "type": "control.for_range",
        "v": 1,
        "fields": {
            "DIRECTION": "to",
            "VAR": { "sym": counter, "name": format!("i_{unit}") },
        },
        "inputs": { "FROM": num(0), "TO": num(10) },
        "statements": { "BODY": [branch] },
    });
    let print_id = ids.next();
    let sum_id = ids.next();
    let get = json!({
        "id": ids.next(),
        "type": "var.get",
        "v": 1,
        "fields": { "VAR": { "ref": value } },
    });
    let report = json!({
        "id": print_id,
        "type": "io.print",
        "v": 1,
        "extra": { "itemCount": 2 },
        "fields": { "NEWLINE": true, "SEP": "none", "STREAM": "out" },
        "inputs": {
            "ITEM0": { "expr": [{ "str": format!("value {unit}: ") }] },
            "ITEM1": { "block": {
                "id": sum_id,
                "type": "math.arithmetic",
                "v": 1,
                "fields": { "OP": "add" },
                "inputs": { "A": { "block": get }, "B": num(1) },
            } },
        },
    });
    [declare, for_loop, report]
}

/// Collection unit `unit`: 8 blocks in five statements of `main` (see the
/// module comment). IDs are handed out in document order.
fn collection_unit(ids: &mut Ids, unit: usize) -> [Value; 5] {
    let scores = format!("s_scores_{unit}");
    let counter = format!("s_i_{unit}");
    let item = format!("s_item_{unit}");
    let list = || json!({ "ref": scores });
    let declare = json!({
        "id": ids.next(),
        "type": "var.declare",
        "v": 1,
        "fields": {
            "CONST": false,
            "NAME": { "sym": scores, "name": format!("scores_{unit}") },
            "TYPE": { "std::vector": "int" },
        },
    });
    let loop_id = ids.next();
    let fill = json!({
        "id": loop_id,
        "type": "control.for_range",
        "v": 1,
        "fields": {
            "DIRECTION": "to",
            "VAR": { "sym": counter, "name": format!("i_{unit}") },
        },
        "inputs": { "FROM": num(0), "TO": num(10) },
        "statements": { "BODY": [{
            "id": ids.next(),
            "type": "coll.add",
            "v": 1,
            "fields": { "LIST": list() },
            "inputs": { "VALUE": { "expr": [{ "ref": counter }] } },
        }] },
    });
    let each_id = ids.next();
    let each = json!({
        "id": each_id,
        "type": "coll.for_each",
        "v": 1,
        "fields": {
            "ITEM": { "sym": item, "name": format!("item_{unit}") },
            "MODE": "read_only",
        },
        "inputs": { "LIST": { "expr": [{ "ref": scores }] } },
        "statements": { "BODY": [{
            "id": ids.next(),
            "type": "coll.set_item",
            "v": 1,
            "fields": { "LIST": list() },
            "inputs": { "INDEX": num(0), "VALUE": { "expr": [{ "ref": item }] } },
        }] },
    });
    let print_id = ids.next();
    let report = json!({
        "id": print_id,
        "type": "io.print",
        "v": 1,
        "extra": { "itemCount": 2 },
        "fields": { "NEWLINE": true, "SEP": "none", "STREAM": "out" },
        "inputs": {
            "ITEM0": { "expr": [{ "str": format!("count {unit}: ") }] },
            "ITEM1": { "block": {
                "id": ids.next(),
                "type": "coll.length",
                "v": 1,
                "fields": { "LIST": list() },
            } },
        },
    });
    let sort = json!({
        "id": ids.next(),
        "type": "coll.sort",
        "v": 1,
        "fields": { "LIST": list(), "ORDER": "descending" },
    });
    [declare, fill, each, report, sort]
}

/// A block with fields and inputs (its ID is the caller's).
fn block(id: String, block_type: &str, fields: Value, inputs: Value) -> Value {
    let mut map = Map::new();
    map.insert(String::from("id"), Value::String(id));
    map.insert(String::from("type"), Value::String(block_type.to_owned()));
    map.insert(String::from("v"), json!(1));
    map.insert(String::from("fields"), fields);
    map.insert(String::from("inputs"), inputs);
    Value::Object(map)
}

/// A print of `items`, each an input.
fn print_items(id: String, items: Vec<Value>) -> Value {
    let count = items.len();
    let inputs: Map<String, Value> = items
        .into_iter()
        .enumerate()
        .map(|(index, item)| (format!("ITEM{index}"), item))
        .collect();
    let mut printed = block(
        id,
        "io.print",
        json!({ "NEWLINE": true, "SEP": "none", "STREAM": "out" }),
        Value::Object(inputs),
    );
    if let Value::Object(map) = &mut printed {
        map.insert(String::from("extra"), json!({ "itemCount": count }));
    }
    printed
}

fn text_input(text: &str) -> Value {
    json!({ "expr": [{ "str": text }] })
}

fn ref_input(symbol: &str) -> Value {
    json!({ "expr": [{ "ref": symbol }] })
}

fn nested(value: Value) -> Value {
    let mut map = Map::new();
    map.insert(String::from("block"), value);
    Value::Object(map)
}

/// Language unit `unit` of kind 1: text, math and errors, 8 blocks in one
/// statement of `main`.
fn text_unit(ids: &mut Ids, unit: usize) -> [Value; 1] {
    let number = format!("s_n_{unit}");
    let error = format!("s_e_{unit}");
    let try_id = ids.next();
    let declare_id = ids.next();
    let to_number = {
        let id = ids.next();
        let trim = block(
            ids.next(),
            "text.trim",
            json!({}),
            json!({ "TEXT": text_input(&format!(" {unit} ")) }),
        );
        block(
            id,
            "text.to_number",
            json!({ "TO": "int" }),
            json!({ "TEXT": nested(trim) }),
        )
    };
    let declare = block(
        declare_id,
        "var.declare",
        json!({ "CONST": false, "NAME": { "sym": number, "name": format!("n_{unit}") }, "TYPE": "int" }),
        json!({ "VALUE": nested(to_number) }),
    );
    let print_id = ids.next();
    let clamp = block(
        ids.next(),
        "math.clamp",
        json!({}),
        json!({ "VALUE": ref_input(&number), "LOW": num(0), "HIGH": num(10) }),
    );
    let show = print_items(print_id, vec![nested(clamp)]);
    let caught_id = ids.next();
    let message = block(
        ids.next(),
        "error.message",
        json!({ "ERROR": { "ref": error } }),
        json!({}),
    );
    let caught = print_items(caught_id, vec![nested(message)]);
    [json!({
        "id": try_id,
        "type": "error.try",
        "v": 1,
        "extra": { "catchCount": 1 },
        "fields": { "ERROR0": "std::exception", "NAME0": { "sym": error, "name": format!("e_{unit}") } },
        "statements": { "TRY": [declare, show], "CATCH0": [caught] },
    })]
}

/// Language unit `unit` of kind 2: structs, enums and globals, 8 blocks in
/// four statements of `main`.
fn struct_unit(ids: &mut Ids, unit: usize) -> [Value; 4] {
    let point = format!("s_p_{unit}");
    let declare_id = ids.next();
    let new_id = ids.next();
    let green = block(
        ids.next(),
        "type.enum_value",
        json!({ "ENUM": { "ref": "s_Color" }, "VALUE": { "ref": "s_Color_Green" } }),
        json!({}),
    );
    let made = json!({
        "id": new_id,
        "type": "type.new",
        "v": 1,
        "extra": { "fieldCount": 2 },
        "fields": { "TYPE": { "ref": "s_Point" } },
        "inputs": { "FIELD0": num(unit), "FIELD1": nested(green) },
    });
    let declare = block(
        declare_id,
        "var.declare",
        json!({ "CONST": false, "NAME": { "sym": point, "name": format!("p_{unit}") }, "TYPE": { "ref": "s_Point" } }),
        json!({ "VALUE": nested(made) }),
    );
    let set_id = ids.next();
    let absolute = block(
        ids.next(),
        "math.abs",
        json!({}),
        json!({ "VALUE": { "expr": [{ "op": "-" }, { "num": unit.to_string() }] } }),
    );
    let set = block(
        set_id,
        "type.set_field",
        json!({ "MEMBER": { "ref": "s_Point_x" }, "VAR": { "ref": point } }),
        json!({ "VALUE": nested(absolute) }),
    );
    let print_id = ids.next();
    let field = block(
        ids.next(),
        "type.field",
        json!({ "MEMBER": { "ref": "s_Point_x" } }),
        json!({ "OBJ": ref_input(&point) }),
    );
    let show = print_items(print_id, vec![nested(field)]);
    let count = block(
        ids.next(),
        "var.change",
        json!({ "VAR": { "ref": "s_counter" }, "OP": "add" }),
        json!({ "BY": num(1) }),
    );
    [declare, set, show, count]
}

/// Language unit `unit` of kind 3: control, 8 blocks in two statements of
/// `main`.
fn control_unit(ids: &mut Ids, unit: usize) -> [Value; 2] {
    let switch_id = ids.next();
    let zero = print_items(ids.next(), vec![text_input(&format!("zero {unit}"))]);
    let on = json!({ "id": ids.next(), "type": "control.fallthrough", "v": 1 });
    let one = print_items(ids.next(), vec![text_input(&format!("one {unit}"))]);
    let other = print_items(ids.next(), vec![text_input(&format!("other {unit}"))]);
    let switch = json!({
        "id": switch_id,
        "type": "control.switch",
        "v": 1,
        "extra": { "caseCount": 2, "hasDefault": true },
        "inputs": { "VALUE": ref_input("s_counter"), "CASE0": num(0), "CASE1": num(1) },
        "statements": { "DO0": [zero, on], "DO1": [one], "DEFAULT": [other] },
    });
    let loop_id = ids.next();
    let check = block(
        ids.next(),
        "program.check",
        json!({ "MESSAGE": "the limit is too small" }),
        json!({ "COND": { "expr": [{ "ref": "s_LIMIT" }, { "op": ">" }, { "num": "0" }] } }),
    );
    let again = print_items(ids.next(), vec![text_input(&format!("again {unit}"))]);
    let repeat = json!({
        "id": loop_id,
        "type": "control.do_while",
        "v": 1,
        "fields": { "MODE": "while" },
        "inputs": { "COND": { "expr": [{ "kw": "false" }] } },
        "statements": { "BODY": [check, again] },
    });
    [switch, repeat]
}

/// Language unit `unit` of kind 4: functions (an overload and a default
/// value), text and files, 8 blocks in four statements of `main`.
fn function_unit(ids: &mut Ids, unit: usize) -> [Value; 4] {
    let word = format!("s_word_{unit}");
    let call = |ids: &mut Ids, function: &str, argument: Value| {
        let print_id = ids.next();
        let called = json!({
            "id": ids.next(),
            "type": "func.call",
            "v": 1,
            "extra": { "argCount": 1 },
            "fields": { "FUNC": { "ref": function } },
            "inputs": { "ARG0": argument },
        });
        print_items(print_id, vec![nested(called)])
    };
    let whole = call(ids, "s_area_int", num(unit));
    let decimal = call(ids, "s_area_double", json!({ "expr": [{ "num": "1.5" }] }));
    let declare_id = ids.next();
    let upper = block(
        ids.next(),
        "text.case",
        json!({ "CASE": "upper" }),
        json!({ "TEXT": text_input(&format!("w{unit}.txt")) }),
    );
    let declare = block(
        declare_id,
        "var.declare",
        json!({ "CONST": false, "NAME": { "sym": word, "name": format!("word_{unit}") }, "TYPE": "std::string" }),
        json!({ "VALUE": nested(upper) }),
    );
    let print_id = ids.next();
    let exists = block(
        ids.next(),
        "file.exists",
        json!({}),
        json!({ "PATH": ref_input(&word) }),
    );
    let show = print_items(print_id, vec![nested(exists)]);
    [whole, decimal, declare, show]
}

/// Language unit `unit`, of the kind `unit % 5` (see the module comment).
fn language_unit(ids: &mut Ids, unit: usize) -> Vec<Value> {
    match unit % 5 {
        0 => collection_unit(ids, unit).into(),
        1 => text_unit(ids, unit).into(),
        2 => struct_unit(ids, unit).into(),
        3 => control_unit(ids, unit).into(),
        _ => function_unit(ids, unit).into(),
    }
}

/// The definitions of [`Units::Language`] ([`DEFINITION_BLOCKS`] blocks, at
/// x 800 from y 40), in document order.
fn definitions(ids: &mut Ids) -> Vec<Value> {
    let place = |index: usize| (800, 40 + 200 * index);
    let mut top = Vec::new();
    let (x, y) = place(0);
    top.push(json!({
        "id": ids.next(), "type": "type.enum", "v": 1, "x": x, "y": y,
        "extra": { "values": [
            { "sym": "s_Color_Red", "name": "Red" },
            { "sym": "s_Color_Green", "name": "Green" },
        ] },
        "fields": { "NAME": { "sym": "s_Color", "name": "Color" }, "SHARED": false },
    }));
    let (x, y) = place(1);
    top.push(json!({
        "id": ids.next(), "type": "type.struct", "v": 1, "x": x, "y": y,
        "extra": { "fields": [
            { "sym": "s_Point_x", "name": "x", "type": "int" },
            { "sym": "s_Point_color", "name": "color", "type": { "ref": "s_Color" } },
        ] },
        "fields": {
            "NAME": { "sym": "s_Point", "name": "Point" },
            "COMPARABLE": false, "PRINTABLE": false, "SHARED": false,
        },
    }));
    for (index, (symbol, name, kind, value)) in [
        ("s_counter", "counter", "variable", 0),
        ("s_LIMIT", "LIMIT", "constant", 3),
    ]
    .into_iter()
    .enumerate()
    {
        let (x, y) = place(2 + index);
        top.push(json!({
            "id": ids.next(), "type": "var.global", "v": 1, "x": x, "y": y,
            "fields": {
                "KIND": kind, "TYPE": "int", "NAME": { "sym": symbol, "name": name }, "SHARED": false,
            },
            "inputs": { "VALUE": num(value) },
        }));
    }
    // area(int width, int height = 2) and area(double radius).
    let (x, y) = place(4);
    let id = ids.next();
    top.push(json!({
        "id": id, "type": "func.define", "v": 1, "x": x, "y": y,
        "extra": { "params": [
            { "sym": "s_area_width", "name": "width", "type": "int", "mode": "copy" },
            { "sym": "s_area_height", "name": "height", "type": "int", "mode": "copy" },
        ] },
        "fields": { "NAME": { "sym": "s_area_int", "name": "area" }, "RETURNS": "int" },
        "inputs": { "DEFAULT1": num(2) },
        "statements": { "BODY": [block(ids.next(), "func.return", json!({}), json!({
            "VALUE": { "expr": [{ "ref": "s_area_width" }, { "op": "*" }, { "ref": "s_area_height" }] },
        }))] },
    }));
    let (x, y) = place(5);
    let id = ids.next();
    top.push(json!({
        "id": id, "type": "func.define", "v": 1, "x": x, "y": y,
        "extra": { "params": [
            { "sym": "s_area_radius", "name": "radius", "type": "double", "mode": "copy" },
        ] },
        "fields": { "NAME": { "sym": "s_area_double", "name": "area" }, "RETURNS": "double" },
        "statements": { "BODY": [block(ids.next(), "func.return", json!({}), json!({
            "VALUE": { "expr": [{ "ref": "s_area_radius" }, { "op": "*" }, { "ref": "s_area_radius" }] },
        }))] },
    }));
    top
}

/// The project settings every generated document has (those of the
/// examples).
fn project(blocks: usize) -> Value {
    json!({
        "id": format!("prj_bench_{blocks}"),
        "name": format!("Benchmark {blocks} blocks"),
        "language": { "standard": "c++20" },
        "options": {
            "showAdvanced": false,
            "manualMemory": false,
            "preferPlainStd": false,
            "formattingStyle": "stream",
            "checkedIndexing": true,
        },
        "build": { "configurations": {
            "debug": {
                "optimization": "none",
                "debugInfo": true,
                "sanitizers": ["address", "undefined"],
                "warnings": "helpful",
                "hardening": true,
            },
            "release": {
                "optimization": "speed",
                "debugInfo": false,
                "sanitizers": [],
                "warnings": "helpful",
                "hardening": true,
            },
        } },
        "run": { "workingDirectory": "project" },
    })
}

/// The benchmark document of `shape` (see the module comment).
///
/// # Errors
/// [`GenerateError`] when the shape has too few or too many blocks.
pub(crate) fn generate(shape: Shape) -> Result<Value, GenerateError> {
    let min = shape.min_blocks();
    if shape.blocks < min {
        return Err(GenerateError::TooFew {
            blocks: shape.blocks,
            min,
        });
    }
    if shape.blocks > MAX_BLOCKS {
        return Err(GenerateError::TooMany { blocks: shape.blocks });
    }
    let in_main = shape.blocks - shape.outside_main();
    let units = (in_main - 1) / UNIT_BLOCKS;
    let fillers = (in_main - 1) % UNIT_BLOCKS;

    let mut ids = Ids(0);
    let main_id = ids.next();
    let mut body = Vec::with_capacity(units * 3 + fillers);
    for index in 0..units {
        match shape.units {
            Units::Plain => body.extend(unit(&mut ids, index)),
            Units::Collections => body.extend(collection_unit(&mut ids, index)),
            Units::Language => body.extend(language_unit(&mut ids, index)),
        }
    }
    for index in 0..fillers {
        body.push(print(&mut ids, &format!("filler {index}")));
    }
    let mut top = vec![json!({
        "id": main_id,
        "type": "program.main",
        "v": 1,
        "x": 40,
        "y": 40,
        "statements": { "BODY": body },
    })];
    if shape.drag_handle {
        let define_id = ids.next();
        top.push(json!({
            "id": define_id,
            "type": "func.define",
            "v": 1,
            "x": 800,
            "y": 40,
            "extra": { "params": [] },
            "fields": { "NAME": { "sym": "s_drag", "name": "dragMe" }, "RETURNS": "void" },
            "statements": { "BODY": [print(&mut ids, "drag me")] },
        }));
    }
    if shape.units == Units::Language {
        top.extend(definitions(&mut ids));
    }
    // A collection's type is a template term: format version 2 (types.md
    // §2.3), with W2's catalog.
    let (format_version, catalog) = if shape.units == Units::Plain {
        (1, "1.0.0")
    } else {
        (2, "1.3.0")
    };
    Ok(json!({
        "format": "blocks2cpp/project",
        "formatVersion": format_version,
        "generator": { "app": "0.1.0", "catalog": catalog },
        "project": project(shape.blocks),
        "modules": [{
            "id": "mod_main",
            "name": "main",
            "workspace": { "blocks": top },
        }],
    }))
}

/// Writes `value` as compact JSON with every object's keys in byte order,
/// whatever order the map keeps them in.
fn write_sorted(value: &Value, out: &mut String) {
    match value {
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            out.push('{');
            for (index, key) in keys.into_iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                out.push_str(&Value::String(key.clone()).to_string());
                out.push(':');
                if let Some(item) = map.get(key) {
                    write_sorted(item, out);
                }
            }
            out.push('}');
        }
        Value::Array(items) => {
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                write_sorted(item, out);
            }
            out.push(']');
        }
        other => out.push_str(&other.to_string()),
    }
}

/// The SHA-256 (64 lower-case hex digits) of the document as compact JSON
/// with sorted keys: the TypeScript generator must give the same digest for
/// the same shape.
pub(crate) fn parity_digest(document: &Value) -> String {
    let mut text = String::new();
    write_sorted(document, &mut text);
    b2c_model::hex(&Sha256::digest(text.as_bytes()))
}

/// How many blocks a document has, counted independently of the generator:
/// every block node at any depth (top-level blocks, statement lists, value
/// inputs and stacks).
pub(crate) fn count_blocks(document: &Value) -> usize {
    let mut pending: Vec<&Value> = Vec::new();
    if let Some(modules) = document.get("modules").and_then(Value::as_array) {
        for module in modules {
            if let Some(blocks) = module.pointer("/workspace/blocks").and_then(Value::as_array) {
                pending.extend(blocks);
            }
        }
    }
    let mut count = 0;
    while let Some(block) = pending.pop() {
        count += 1;
        let children = |key: &str| block.get(key).and_then(Value::as_object).map(Map::values);
        if let Some(lists) = children("statements") {
            pending.extend(lists.filter_map(Value::as_array).flatten());
        }
        if let Some(inputs) = children("inputs") {
            pending.extend(inputs.filter_map(|input| input.get("block")));
        }
        if let Some(stack) = block.get("stack").and_then(Value::as_array) {
            pending.extend(stack);
        }
    }
    count
}
