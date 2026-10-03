//! Property tests: the analyser never panics, is deterministic, does not
//! depend on the order of top-level blocks, and keeps its invariants when it
//! reports no errors: no `Type::Error`, no dangling symbol IDs, code
//! generation needs no error placeholders, and g++ accepts the generated C++
//! (spec §7.5.3: generated code that fails to compile after a clean analysis
//! is a Blocks2Cpp bug).

// Test code: unwrap/expect/panic and printing to stderr are fine here.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::print_stderr)]

mod common;

use b2c_codegen::{CodegenOptions, generate};
use b2c_ir::diag::{DiagSource, Part, Severity};
use b2c_ir::source_map::FileKind;
use b2c_lang::{Analysis, analyze};
use b2c_model::Document;
use common::{doc, render_all};
use proptest::prelude::*;
use serde_json::{Value, json};

/// Names in the symbol pool: some valid, some invalid, some clashing.
const NAMES: &[&str] = &["a", "b", "c", "f", "g", "int", "x y", "", "abs"];
const OPS: &[&str] = &[
    "+", "-", "*", "/", "%", "==", "!=", "<", "<=", ">", ">=", "&&", "||", "!", "and", "or", "not", "(", ")",
    ",", "?", ":", "<<", "=", "[", ".", ";", "++",
];
const TYPES: &[&str] = &[
    "int",
    "double",
    "bool",
    "char",
    "std::string",
    "auto",
    "void",
    "long",
    "",
];

fn sym(index: usize) -> String {
    format!("s{index}")
}

fn pool() -> impl Strategy<Value = usize> {
    0..NAMES.len() + 2 // the last two are never declared
}

fn decl() -> impl Strategy<Value = Value> {
    pool().prop_map(|i| json!({"sym": sym(i), "name": NAMES.get(i).copied().unwrap_or("zz")}))
}

fn reference() -> impl Strategy<Value = Value> {
    pool().prop_map(|i| json!({"ref": sym(i)}))
}

fn token() -> impl Strategy<Value = Value> {
    prop_oneof![
        4 => prop::sample::select(OPS).prop_map(|o| json!({"op": o})),
        3 => "[0-9]{1,3}|[0-9.eEx']{1,5}".prop_map(|n| json!({"num": n})),
        1 => ".{0,4}".prop_map(|s| json!({"str": s})),
        1 => ".{0,2}".prop_map(|s| json!({"chr": s})),
        3 => pool().prop_map(|i| json!({"ref": sym(i)})),
        1 => prop::sample::select(&["true", "false", "nullptr"][..]).prop_map(|k| json!({"kw": k})),
        1 => "[a-z]{1,3}".prop_map(|t| json!({"text": t})),
    ]
}

fn slot() -> impl Strategy<Value = Value> {
    (prop::collection::vec(token(), 0..8), prop::bool::weighted(0.1))
        .prop_map(|(tokens, draft)| json!({"expr": tokens, "draft": draft}))
}

/// A generated block (every one starts with ID "x"; [`build`] renumbers them).
#[allow(clippy::needless_pass_by_value)] // called with `json!` temporaries
fn block(ty: &str, fields: Value, inputs: Value, extra: Value, disabled: bool) -> Value {
    json!({"id": "x", "type": ty, "v": 1, "fields": fields, "inputs": inputs, "extra": extra, "disabled": disabled})
}

fn numbered(prefix: &str, items: Vec<Value>) -> Value {
    Value::Object(
        items
            .into_iter()
            .enumerate()
            .map(|(i, v)| (format!("{prefix}{i}"), v))
            .collect(),
    )
}

/// A value input: a slot or a reporter block.
fn value_input() -> impl Strategy<Value = Value> {
    let leaf = prop_oneof![
        3 => slot(),
        1 => prop::sample::select(&["0", "1", "2.5", "-3", "x", "99999999999", "", "0x1F", "1e3"][..])
            .prop_map(|v| json!({"block": block("math.number", json!({"VALUE": v}), json!({}), json!({}), false)})),
        1 => ".{0,3}".prop_map(|v| json!({"block": block("text.literal", json!({"VALUE": v}), json!({}), json!({}), false)})),
        1 => ".{0,2}".prop_map(|v| json!({"block": block("text.char", json!({"VALUE": v}), json!({}), json!({}), false)})),
        1 => prop::sample::select(&["true", "false", "maybe"][..])
            .prop_map(|v| json!({"block": block("logic.boolean", json!({"VALUE": v}), json!({}), json!({}), false)})),
        2 => (reference(), any::<bool>())
            .prop_map(|(r, d)| json!({"block": block("var.get", json!({"VAR": r}), json!({}), json!({}), d)})),
        1 => Just(json!({"block": block("pack.unknown", json!({}), json!({}), json!({}), false)})),
    ];
    leaf.prop_recursive(3, 24, 4, |inner| {
        let ops = prop::sample::select(&["add", "sub", "mul", "div", "mod", "lt", "le", "gt", "ge", "eq", "ne", "pow"][..]);
        prop_oneof![
            (ops, inner.clone(), inner.clone(), any::<bool>()).prop_map(|(op, a, b, arith)| {
                let ty = if arith { "math.arithmetic" } else { "math.compare" };
                json!({"block": block(ty, json!({"OP": op}), json!({"A": a, "B": b}), json!({}), false)})
            }),
            (prop::sample::select(&["and", "or"][..]), prop::collection::vec(inner.clone(), 0..4)).prop_map(|(op, items)| {
                let n = items.len();
                json!({"block": block("logic.operation", json!({"OP": op}), numbered("ITEM", items), json!({"itemCount": n}), false)})
            }),
            inner.clone().prop_map(|a| json!({"block": block("logic.not", json!({}), json!({"A": a}), json!({}), false)})),
            (inner.clone(), inner.clone(), inner.clone()).prop_map(|(c, t, e)| {
                json!({"block": block("logic.ternary", json!({}), json!({"COND": c, "THEN": t, "ELSE": e}), json!({}), false)})
            }),
            prop::collection::vec(inner.clone(), 0..4).prop_map(|items| {
                let n = items.len();
                json!({"block": block("text.join", json!({}), numbered("ITEM", items), json!({"itemCount": n}), false)})
            }),
            (reference(), prop::collection::vec(inner.clone(), 0..3)).prop_map(|(f, args)| {
                let n = args.len();
                json!({"block": block("func.call", json!({"FUNC": f}), numbered("ARG", args), json!({"argCount": n}), false)})
            }),
            (inner.clone(), inner.clone()).prop_map(|(l, h)| {
                json!({"block": block("math.random_int", json!({}), json!({"LOW": l, "HIGH": h}), json!({}), false)})
            }),
            (prop::sample::select(&["int", "double", "bool"][..]), inner).prop_map(|(to, v)| {
                json!({"block": block("math.convert", json!({"TO": to}), json!({"VALUE": v}), json!({}), false)})
            }),
        ]
    })
}

fn maybe(input: Option<Value>, name: &str) -> Value {
    input.map_or_else(|| json!({}), |v| json!({name: v}))
}

/// A statement block, possibly with nested statement lists.
fn statement() -> impl Strategy<Value = Value> {
    let d = || prop::bool::weighted(0.1);
    let leaf = prop_oneof![
        3 => (prop::sample::select(TYPES), decl(), prop::option::of(value_input()), any::<bool>(), d()).prop_map(
            |(ty, name, value, constant, dis)| block("var.declare", json!({"TYPE": ty, "NAME": name, "CONST": constant}),
                maybe(value, "VALUE"), json!({}), dis)),
        2 => (reference(), value_input(), d()).prop_map(|(r, v, dis)| block("var.set", json!({"VAR": r}), json!({"VALUE": v}), json!({}), dis)),
        1 => (reference(), value_input()).prop_map(|(r, v)| block("var.change", json!({"VAR": r}), json!({"BY": v}), json!({}), false)),
        1 => (reference(), prop::sample::select(&["add", "sub", "mul", "div", "mod", "pow"][..]), value_input())
            .prop_map(|(r, op, v)| block("var.update", json!({"VAR": r, "OP": op}), json!({"VALUE": v}), json!({}), false)),
        2 => (prop::collection::vec(value_input(), 0..3), prop::sample::select(&["none", "space", "comma"][..]))
            .prop_map(|(items, sep)| { let n = items.len();
                block("io.print", json!({"SEP": sep}), numbered("ITEM", items), json!({"itemCount": n}), false) }),
        1 => (reference(), prop::option::of(value_input()))
            .prop_map(|(r, p)| block("io.ask", json!({"VAR": r}), maybe(p, "PROMPT"), json!({}), false)),
        1 => (reference(), prop::collection::vec(value_input(), 0..3)).prop_map(|(f, args)| { let n = args.len();
                block("func.call_stmt", json!({"FUNC": f}), numbered("ARG", args), json!({"argCount": n}), false) }),
        1 => prop::option::of(value_input()).prop_map(|v| block("func.return", json!({}), maybe(v, "VALUE"), json!({}), false)),
        1 => value_input().prop_map(|v| block("program.exit", json!({}), json!({"CODE": v}), json!({}), false)),
        1 => Just(block("control.break", json!({}), json!({}), json!({}), false)),
        1 => Just(block("control.continue", json!({}), json!({}), json!({}), false)),
        1 => Just(block("math.number", json!({}), json!({}), json!({}), false)),
    ];
    leaf.prop_recursive(3, 32, 4, |inner| {
        let body = || prop::collection::vec(inner.clone(), 0..4);
        prop_oneof![
            (
                prop::collection::vec((value_input(), body()), 1..3),
                prop::option::of(body()),
                any::<bool>()
            )
                .prop_map(|(branches, else_body, dis)| {
                    let n = branches.len() - 1;
                    let (conds, bodies): (Vec<Value>, Vec<Value>) =
                        branches.into_iter().map(|(c, b)| (c, Value::Array(b))).unzip();
                    let mut statements = numbered("DO", bodies);
                    let has_else = else_body.is_some();
                    if let Some(e) = else_body {
                        statements["ELSE"] = Value::Array(e);
                    }
                    let mut b = block(
                        "control.if",
                        json!({}),
                        numbered("COND", conds),
                        json!({"elseIfCount": n, "hasElse": has_else}),
                        dis,
                    );
                    b["statements"] = statements;
                    b
                }),
            (
                prop::sample::select(&["while", "until"][..]),
                value_input(),
                body()
            )
                .prop_map(|(mode, c, b)| {
                    let mut blk = block(
                        "control.while",
                        json!({"MODE": mode}),
                        json!({"COND": c}),
                        json!({}),
                        false,
                    );
                    blk["statements"] = json!({"BODY": b});
                    blk
                }),
            (value_input(), body()).prop_map(|(t, b)| {
                let mut blk = block("control.repeat", json!({}), json!({"TIMES": t}), json!({}), false);
                blk["statements"] = json!({"BODY": b});
                blk
            }),
            (
                decl(),
                value_input(),
                value_input(),
                prop::option::of(value_input()),
                prop::sample::select(&["to", "through", "down_to"][..]),
                body()
            )
                .prop_map(|(var, from, to, step, dir, b)| {
                    let mut inputs = json!({"FROM": from, "TO": to});
                    if let Some(step) = step {
                        inputs["STEP"] = step;
                    }
                    let mut blk = block(
                        "control.for_range",
                        json!({"VAR": var, "DIRECTION": dir}),
                        inputs,
                        json!({}),
                        false,
                    );
                    blk["statements"] = json!({"BODY": b});
                    blk
                }),
            body().prop_map(|b| {
                let mut blk = block("control.forever", json!({}), json!({}), json!({}), false);
                blk["statements"] = json!({"BODY": b});
                blk
            }),
        ]
    })
}

fn function() -> impl Strategy<Value = Value> {
    let param = (
        decl(),
        prop::sample::select(TYPES),
        prop::sample::select(&["copy", "editable", "read_only", "x"][..]),
    )
        .prop_map(|(d, ty, mode)| json!({"sym": d["sym"], "name": d["name"], "type": ty, "mode": mode}));
    (
        decl(),
        prop::sample::select(TYPES),
        prop::collection::vec(param, 0..3),
        prop::collection::vec(statement(), 0..5),
        prop::bool::weighted(0.1),
    )
        .prop_map(|(name, ret, params, body, disabled)| {
            let mut b = block(
                "func.define",
                json!({"NAME": name, "RETURNS": ret}),
                json!({}),
                json!({"params": params}),
                disabled,
            );
            b["statements"] = json!({"BODY": body});
            b
        })
}

fn program() -> impl Strategy<Value = Vec<Value>> {
    (
        prop::collection::vec(statement(), 0..6),
        prop::collection::vec(function(), 0..3),
        prop::option::of(statement()),
    )
        .prop_map(|(body, functions, loose)| {
            let mut blocks =
                vec![json!({"id": "x", "type": "program.main", "v": 1, "statements": {"BODY": body}})];
            blocks.extend(functions);
            blocks.extend(loose);
            blocks
        })
}

/// Gives every block a unique ID (generated blocks all start as "x").
fn renumber(value: &mut Value, next: &mut usize) {
    match value {
        Value::Object(map) => {
            if map.contains_key("type") && map.contains_key("v") {
                *next += 1;
                map.insert(String::from("id"), json!(format!("b{next}")));
            }
            for child in map.values_mut() {
                renumber(child, next);
            }
        }
        Value::Array(items) => {
            for child in items {
                renumber(child, next);
            }
        }
        _ => {}
    }
}

fn build(mut blocks: Vec<Value>) -> Document {
    let mut next = 0;
    for block in &mut blocks {
        renumber(block, &mut next);
    }
    doc(blocks)
}

/// Finds `"ty": "error"` anywhere in the serialised program.
fn has_error_type(value: &Value) -> bool {
    match value {
        Value::Object(map) => map
            .iter()
            .any(|(k, v)| (k == "ty" && v == "error") || has_error_type(v)),
        Value::Array(items) => items.iter().any(has_error_type),
        _ => false,
    }
}

/// Every symbol ID used by statements and expressions.
fn referenced_symbols(value: &Value, out: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            for (k, v) in map {
                if let (true, Value::String(s)) =
                    (["symbol", "target", "var", "function"].contains(&k.as_str()), v)
                {
                    out.push(s.clone());
                }
                referenced_symbols(v, out);
            }
        }
        Value::Array(items) => items.iter().for_each(|v| referenced_symbols(v, out)),
        _ => {}
    }
}

/// Whether the catalog check would report a block the analyser skips
/// silently: an unknown type, or a reporter where a statement belongs.
fn has_unchecked_blocks(document: &Document) -> bool {
    fn statement_list(blocks: &[b2c_model::Block]) -> bool {
        blocks.iter().any(|b| b.block_type == "math.number" || block(b))
    }
    fn block(b: &b2c_model::Block) -> bool {
        b.block_type == "pack.unknown"
            || b.statements.values().any(|list| statement_list(list))
            || b.inputs
                .values()
                .any(|i| matches!(i, b2c_model::Input::Block(nested) if block(&nested.block)))
    }
    document
        .modules
        .iter()
        .any(|m| m.workspace.blocks.iter().any(block))
}

fn check_invariants(document: &Document, a: &Analysis) -> Result<(), TestCaseError> {
    // Deterministic.
    prop_assert_eq!(&analyze(document), a);
    // Every diagnostic is ours, and slot ranges are well formed.
    for d in &a.diagnostics {
        prop_assert_eq!(d.source, DiagSource::Analyser);
        prop_assert!(d.code.0.starts_with("B2C-"));
        prop_assert!(!d.message.is_empty());
        if let Part::Tokens { start, end, .. } = &d.primary.part {
            prop_assert!(start <= end && *end as usize <= b2c_model::limits::MAX_EXPR_TOKENS);
        }
    }
    let program = serde_json::to_value(&a.program).expect("serialise");
    if !a.diagnostics.iter().any(|d| d.severity == Severity::Error) && !has_unchecked_blocks(document) {
        prop_assert!(
            !has_error_type(&program),
            "error type without an error:\n{}",
            render_all(a)
        );
        let mut used = Vec::new();
        referenced_symbols(&program["modules"], &mut used);
        for sym in used {
            let id = b2c_ir::ids::SymbolId::new(&sym).expect("id");
            prop_assert!(a.program.symbols.get(&id).is_some(), "dangling {}", sym);
        }
        let project = generate(&a.program, &CodegenOptions::default());
        for file in &project.files {
            prop_assert!(
                !file.contents.contains("/* error */"),
                "the generator needed error placeholders:\n{}",
                file.contents
            );
        }
    }
    Ok(())
}

/// Whether the analysis is clean enough that the generated C++ must compile.
fn must_compile(document: &Document, a: &Analysis) -> bool {
    !a.diagnostics.iter().any(|d| d.severity == Severity::Error) && !has_unchecked_blocks(document)
}

/// Checks that g++ accepts every source file generated for the program.
fn compiles(a: &Analysis) -> Result<(), TestCaseError> {
    let project = generate(&a.program, &CodegenOptions::default());
    for file in project.files.iter().filter(|f| f.kind == FileKind::Source) {
        if let Err(messages) = common::gxx::syntax_check(&file.contents, false) {
            return Err(TestCaseError::fail(format!(
                "g++ rejected a program the analyser accepted:\n{messages}\n--- diagnostics ---\n{}\n--- source ---\n{}",
                render_all(a),
                file.contents
            )));
        }
    }
    Ok(())
}

// --- Well-typed programs ---------------------------------------------------------
//
// A type-directed generator: every program it makes is valid, so the analyser
// must report no errors (no false positives), and the invariants are then
// checked on large, non-trivial programs.

#[derive(Debug, Clone, Copy)]
enum T {
    Int,
    Double,
    Bool,
    Char,
    Str,
}

const ALL_TYPES: [T; 5] = [T::Int, T::Double, T::Bool, T::Char, T::Str];

/// The variable of each type declared at the top of `main`.
fn var_of(t: T) -> &'static str {
    match t {
        T::Int => "i",
        T::Double => "d",
        T::Bool => "b",
        T::Char => "c",
        T::Str => "s",
    }
}

fn any_type() -> impl Strategy<Value = T> {
    prop::sample::select(&ALL_TYPES[..])
}

fn leaf_expr(t: T) -> BoxedStrategy<Value> {
    use common::{boolean, chr, e, get, num, text};
    match t {
        T::Int => prop_oneof![
            (0u32..1000).prop_map(|n| num(&n.to_string())),
            Just(get("i")),
            Just(e("i + 1")),
            Just(e("sq(i) % 7 - -2")),
            Just(e("p ? i : 3 * (i - 1)")),
        ]
        .boxed(),
        T::Double => prop_oneof![
            Just(num("2.5")),
            Just(get("d")),
            Just(e("d * 2.0 + i")),
            Just(e("-d / 3"))
        ]
        .boxed(),
        T::Bool => prop_oneof![
            any::<bool>().prop_map(boolean),
            Just(get("b")),
            Just(e("i < 10 && b")),
            Just(e("s == \"x\" or not b")),
            Just(e("c != 'z'")),
        ]
        .boxed(),
        T::Char => prop_oneof![Just(chr("q")), Just(get("c")), Just(e("'#'"))].boxed(),
        T::Str => prop_oneof!["[a-zA-Z0-9 ,.!?]{0,8}".prop_map(|t| text(&t)), Just(get("s"))].boxed(),
    }
}

fn typed_expr(t: T, depth: u32) -> BoxedStrategy<Value> {
    use common::{arith, call, compare, convert, join, logic, not, random_int, ternary};
    let leaf = leaf_expr(t);
    if depth == 0 {
        return leaf;
    }
    let sub = move |t: T| typed_expr(t, depth - 1);
    match t {
        T::Int => prop_oneof![
            2 => leaf,
            1 => (prop::sample::select(&["add", "sub", "mul", "div", "mod"][..]), sub(T::Int), sub(T::Int))
                .prop_map(|(op, a, b)| arith(op, a, b)),
            1 => (sub(T::Int), sub(T::Int)).prop_map(|(l, h)| random_int(l, h)),
            1 => sub(T::Double).prop_map(|v| convert("int", v)),
            1 => sub(T::Int).prop_map(|v| call("sq", vec![v])),
            1 => (sub(T::Bool), sub(T::Int), sub(T::Int)).prop_map(|(c, a, b)| ternary(c, a, b)),
        ]
        .boxed(),
        T::Double => prop_oneof![
            2 => leaf,
            1 => (prop::sample::select(&["add", "sub", "mul"][..]), sub(T::Double), sub(T::Double))
                .prop_map(|(op, a, b)| arith(op, a, b)),
            1 => sub(T::Int).prop_map(|v| convert("double", v)),
        ]
        .boxed(),
        T::Bool => prop_oneof![
            2 => leaf,
            1 => (prop::sample::select(&["lt", "le", "gt", "ge", "eq", "ne"][..]), sub(T::Int), sub(T::Int))
                .prop_map(|(op, a, b)| compare(op, a, b)),
            1 => (prop::sample::select(&["eq", "ne", "lt"][..]), sub(T::Str), sub(T::Str))
                .prop_map(|(op, a, b)| compare(op, a, b)),
            1 => (prop::sample::select(&["and", "or"][..]), prop::collection::vec(sub(T::Bool), 2..4))
                .prop_map(|(op, items)| logic(op, items)),
            1 => sub(T::Bool).prop_map(not),
        ]
        .boxed(),
        T::Char => prop_oneof![2 => leaf, 1 => (sub(T::Bool), sub(T::Char), sub(T::Char)).prop_map(|(c, a, b)| ternary(c, a, b))]
            .boxed(),
        T::Str => prop_oneof![
            2 => leaf,
            1 => prop::collection::vec(any_type().prop_flat_map(sub), 2..4).prop_map(join),
            1 => (sub(T::Bool), sub(T::Str), sub(T::Str)).prop_map(|(c, a, b)| ternary(c, a, b)),
        ]
        .boxed(),
    }
}

fn any_typed_expr() -> BoxedStrategy<Value> {
    any_type().prop_flat_map(|t| typed_expr(t, 2)).boxed()
}

fn typed_stmt(depth: u32) -> BoxedStrategy<Value> {
    use common::{ask, call_stmt, change, get, print, set, update};
    let leaf = prop_oneof![
        3 => any_type().prop_flat_map(|t| typed_expr(t, 2).prop_map(move |v| set(var_of(t), v))),
        1 => typed_expr(T::Int, 2).prop_map(|v| change("i", v)),
        1 => typed_expr(T::Double, 1).prop_map(|v| update("d", "mul", v)),
        2 => prop::collection::vec(any_typed_expr(), 1..4).prop_map(print),
        1 => any_type().prop_map(|t| ask(var_of(t), Some(common::text("? ")))),
        1 => typed_expr(T::Str, 1).prop_map(|v| call_stmt("show", vec![v])),
        1 => Just(call_stmt("bump", vec![get("i")])),
        1 => typed_expr(T::Int, 1).prop_map(|v| call_stmt("sq", vec![v])),
    ]
    .boxed();
    if depth == 0 {
        return leaf;
    }
    let body = move || prop::collection::vec(typed_stmt(depth - 1), 0..3);
    // Loop bodies may end with `continue` or `break`.
    let loop_body = move || {
        (body(), 0..3usize).prop_map(|(mut stmts, end)| {
            match end {
                1 => stmts.push(common::cont()),
                2 => stmts.push(common::brk()),
                _ => {}
            }
            stmts
        })
    };
    prop_oneof![
        3 => leaf,
        1 => (prop::collection::vec((typed_expr(T::Bool, 1), body()), 1..3), prop::option::of(body()))
            .prop_map(|(branches, else_body)| common::if_else(branches, else_body)),
        1 => (prop::sample::select(&["while", "until"][..]), typed_expr(T::Bool, 1), loop_body())
            .prop_map(|(mode, c, b)| common::while_loop(mode, c, b)),
        1 => (typed_expr(T::Int, 1), loop_body()).prop_map(|(n, b)| common::repeat(n, b)),
        1 => (typed_expr(T::Int, 1), typed_expr(T::Int, 1), prop::sample::select(&["to", "through", "down_to"][..]), loop_body())
            .prop_map(|(from, to, dir, b)| common::for_range("k", from, dir, to, None, b)),
        1 => body().prop_map(|mut b| { b.push(common::brk()); common::forever(b) }),
    ]
    .boxed()
}

/// Gives every `for` counter a unique symbol and name.
fn unique_counters(value: &mut Value, next: &mut usize) {
    match value {
        Value::Object(map) => {
            if map.get("type").and_then(Value::as_str) == Some("control.for_range") {
                *next += 1;
                map["fields"]["VAR"] = json!({"sym": format!("ctr{next}"), "name": format!("k{next}")});
            }
            map.values_mut().for_each(|child| unique_counters(child, next));
        }
        Value::Array(items) => items.iter_mut().for_each(|child| unique_counters(child, next)),
        _ => {}
    }
}

fn typed_program() -> impl Strategy<Value = Vec<Value>> {
    use common::{boolean, change, chr, declare, e, func, get, main, num, print, ret, text};
    prop::collection::vec(typed_stmt(2), 0..8).prop_map(|stmts| {
        let mut body = vec![
            declare("int", "i", Some(num("1"))),
            declare("double", "d", Some(num("0.5"))),
            declare("bool", "b", Some(boolean(false))),
            declare("char", "c", Some(chr("a"))),
            declare("std::string", "s", Some(text("x"))),
            declare("bool", "p", Some(e("i > 0"))),
        ];
        body.extend(stmts);
        let mut blocks = vec![
            main(body),
            func("sq", "int", &[("n", "int", "copy")], vec![ret(Some(e("n * n")))]),
            func(
                "show",
                "void",
                &[("t", "std::string", "read_only")],
                vec![print(vec![get("t")])],
            ),
            func(
                "bump",
                "void",
                &[("acc", "int", "editable")],
                vec![change("acc", num("1"))],
            ),
        ];
        let mut next = 0;
        for block in &mut blocks {
            unique_counters(block, &mut next);
        }
        blocks
    })
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 256, ..ProptestConfig::default() })]

    #[test]
    fn well_typed_programs_have_no_errors(blocks in typed_program()) {
        let document = build(blocks);
        let a = analyze(&document);
        prop_assert!(
            !a.diagnostics.iter().any(|d| d.severity == Severity::Error),
            "false positive:\n{}",
            render_all(&a)
        );
        check_invariants(&document, &a)?;
    }
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 256, ..ProptestConfig::default() })]

    #[test]
    fn arbitrary_slot_tokens(tokens in prop::collection::vec(token(), 0..30), draft in prop::bool::weighted(0.1)) {
        let slot = json!({"expr": tokens, "draft": draft});
        let blocks = vec![
            json!({"id": "x", "type": "program.main", "v": 1, "statements": {"BODY": [
                {"id": "x", "type": "var.declare", "v": 1, "fields": {"TYPE": "int", "NAME": {"sym": "s0", "name": "a"}},
                 "inputs": {"VALUE": {"expr": [{"num": "1"}]}}},
                {"id": "x", "type": "io.print", "v": 1, "inputs": {"ITEM0": slot}},
                {"id": "x", "type": "var.declare", "v": 1, "fields": {"TYPE": "auto", "NAME": {"sym": "s1", "name": "b"}},
                 "inputs": {"VALUE": slot}},
                {"id": "x", "type": "control.if", "v": 1, "inputs": {"COND0": slot}},
                {"id": "x", "type": "func.call_stmt", "v": 1, "fields": {"FUNC": {"ref": "s3"}},
                 "extra": {"argCount": 1}, "inputs": {"ARG0": slot}}
            ]}}),
            json!({"id": "x", "type": "func.define", "v": 1,
                   "fields": {"NAME": {"sym": "s3", "name": "f"}, "RETURNS": "int"},
                   "extra": {"params": [{"sym": "s4", "name": "p", "type": "int", "mode": "copy"}]},
                   "statements": {"BODY": [{"id": "x", "type": "func.return", "v": 1, "inputs": {"VALUE": slot}}]}}),
        ];
        let document = build(blocks);
        let a = analyze(&document);
        check_invariants(&document, &a)?;
    }

    #[test]
    fn random_block_trees(blocks in program()) {
        let document = build(blocks);
        let a = analyze(&document);
        check_invariants(&document, &a)?;
        // The order of top-level blocks in the file does not matter.
        let mut reversed = document.clone();
        reversed.modules[0].workspace.blocks.reverse();
        prop_assert_eq!(analyze(&reversed), a);
    }
}

proptest! {
    // Each case runs g++, so there are few of them.
    #![proptest_config(ProptestConfig { cases: 24, ..ProptestConfig::default() })]

    #[test]
    fn programs_without_errors_compile(blocks in prop_oneof![program(), typed_program()]) {
        if !common::gxx::available() {
            return Ok(());
        }
        let document = build(blocks);
        let a = analyze(&document);
        if must_compile(&document, &a) {
            compiles(&a)?;
        }
    }
}
