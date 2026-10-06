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
//!   whose branches change `value_u` by `i_u` and subtract 1 from it, then
//!   `print "value u: ", value_u + 1` (a `math.arithmetic` with a
//!   `var.get` in it).
//! * A filler `f` is `print "filler f"`.
//! * The drag handle (webview drag benchmark only) is a separate
//!   `func.define dragMe` at x 800, y 40 holding one print: a top-level
//!   block the benchmark can drag back and forth without changing the
//!   program.
//!
//! Block IDs are `b<n>` in document order, symbol IDs `s_value_<u>`,
//! `s_i_<u>` and `s_drag`.

use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

/// Blocks per unit.
pub(crate) const UNIT_BLOCKS: usize = 8;

/// Blocks of the drag handle (`func.define` and its print).
pub(crate) const HANDLE_BLOCKS: usize = 2;

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
}

impl Shape {
    /// The fewest blocks this shape can have (`program.main`, plus the
    /// handle when asked for).
    pub(crate) const fn min_blocks(self) -> usize {
        if self.drag_handle { 1 + HANDLE_BLOCKS } else { 1 }
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
    let update = json!({
        "id": ids.next(),
        "type": "var.update",
        "v": 1,
        "fields": { "OP": "sub", "VAR": { "ref": value } },
        "inputs": { "VALUE": num(1) },
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
        "statements": { "DO0": [change], "ELSE": [update] },
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
    let in_main = shape.blocks - if shape.drag_handle { HANDLE_BLOCKS } else { 0 };
    let units = (in_main - 1) / UNIT_BLOCKS;
    let fillers = (in_main - 1) % UNIT_BLOCKS;

    let mut ids = Ids(0);
    let main_id = ids.next();
    let mut body = Vec::with_capacity(units * 3 + fillers);
    for index in 0..units {
        body.extend(unit(&mut ids, index));
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
    Ok(json!({
        "format": "blocks2cpp/project",
        "formatVersion": 1,
        "generator": { "app": "0.1.0", "catalog": "1.0.0" },
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
