//! Builders for test documents.
//!
//! Statement helpers return a block as JSON; reporter helpers and [`e`]
//! return an *input* (`{"block": …}` or `{"expr": …}`), ready to plug into
//! another block. Symbols are named `sym_<name>`, so the slot tokenizer can
//! refer to a variable `x` as `sym_x`.

#![allow(dead_code, clippy::needless_pass_by_value)] // shared by several test crates

pub(crate) mod gxx;

use std::cell::Cell;
use std::path::{Path, PathBuf};

use b2c_ir::diag::{Diagnostic, Part, Severity};
use b2c_lang::{Analysis, analyze};
use b2c_model::{Block, Document, Input};
use serde_json::{Value, json};

thread_local! {
    static NEXT_ID: Cell<usize> = const { Cell::new(0) };
}

/// A fresh block ID (unique within a test thread).
pub(crate) fn id() -> String {
    NEXT_ID.with(|n| {
        n.set(n.get() + 1);
        format!("b{:04}", n.get())
    })
}

/// Restarts block IDs (for snapshot tests that need stable IDs).
pub(crate) fn reset_ids() {
    NEXT_ID.with(|n| n.set(0));
}

pub(crate) fn sym(name: &str) -> String {
    format!("sym_{name}")
}

fn decl(name: &str) -> Value {
    json!({"sym": sym(name), "name": name})
}

fn reference(name: &str) -> Value {
    json!({"ref": sym(name)})
}

fn numbered(prefix: &str, items: Vec<Value>) -> serde_json::Map<String, Value> {
    items
        .into_iter()
        .enumerate()
        .map(|(i, v)| (format!("{prefix}{i}"), v))
        .collect()
}

// --- Documents -----------------------------------------------------------------

/// A document with one module holding these top-level blocks.
pub(crate) fn doc(blocks: Vec<Value>) -> Document {
    doc_modules(vec![("main", blocks)])
}

/// A document with several modules.
pub(crate) fn doc_modules(modules: Vec<(&str, Vec<Value>)>) -> Document {
    let modules: Vec<Value> = modules
        .into_iter()
        .enumerate()
        .map(|(i, (name, blocks))| json!({"id": format!("mod_{i}"), "name": name, "workspace": {"blocks": blocks}}))
        .collect();
    serde_json::from_value(json!({
        "format": "blocks2cpp/project",
        "formatVersion": 1,
        "generator": {"app": "0.1.0", "catalog": "1.0.0"},
        "project": {"id": "prj_test", "name": "Test", "language": {"standard": "c++20"}},
        "modules": modules
    }))
    .expect("test document")
}

// --- Project files ---------------------------------------------------------------

/// The repository root.
pub(crate) fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The `.b2c` files of a folder of the repository, sorted by name.
pub(crate) fn project_files(folder: &str) -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = std::fs::read_dir(repo_root().join(folder))
        .unwrap_or_else(|e| panic!("{folder}: {e}"))
        .map(|entry| entry.expect("folder entry").path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "b2c"))
        .collect();
    paths.sort();
    paths
}

/// A file's name, for messages.
pub(crate) fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// A document as the build analyses it: loaded by the real loader and
/// completed by the resolve stage, which must accept it.
pub(crate) fn resolved(bytes: &[u8], what: &str) -> Document {
    let loaded = b2c_model::load(bytes).unwrap_or_else(|e| panic!("{what} does not load: {e:?}"));
    let (document, diagnostics) = b2c_catalog::resolve(&loaded, b2c_catalog::core_catalog());
    assert!(diagnostics.is_empty(), "{what} does not resolve: {diagnostics:?}");
    document
}

/// Every example project (`examples/*.b2c`): its file name and the resolved document.
pub(crate) fn resolved_examples() -> Vec<(String, Document)> {
    let examples: Vec<(String, Document)> = project_files("examples")
        .into_iter()
        .map(|path| {
            let name = file_name(&path);
            let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{name}: {e}"));
            let document = resolved(&bytes, &name);
            (name, document)
        })
        .collect();
    assert!(
        examples.len() >= 15,
        "expected the M1 examples, found {}",
        examples.len()
    );
    examples
}

/// One example, resolved.
pub(crate) fn resolved_example(name: &str) -> Document {
    let path = repo_root().join("examples").join(name);
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{name}: {e}"));
    resolved(&bytes, name)
}

/// Every block of a document with the index of its module and whether it
/// is a top-level block, parents before children.
pub(crate) fn all_blocks(document: &Document) -> Vec<(usize, bool, &Block)> {
    let mut found = Vec::new();
    for (index, module) in document.modules.iter().enumerate() {
        let mut stack: Vec<(&Block, bool)> =
            module.workspace.blocks.iter().rev().map(|b| (b, true)).collect();
        while let Some((block, top)) = stack.pop() {
            found.push((index, top, block));
            let mut children: Vec<&Block> = Vec::new();
            for list in block.statements.values() {
                children.extend(list);
            }
            for input in block.inputs.values() {
                if let Input::Block(nested) = input {
                    children.push(&nested.block);
                }
            }
            stack.extend(children.into_iter().rev().map(|b| (b, false)));
        }
    }
    found
}

/// Analyses a program whose `main` holds these statements.
pub(crate) fn run_main(body: Vec<Value>) -> Analysis {
    analyze(&doc(vec![main(body)]))
}

/// Analyses a program with these top-level blocks.
pub(crate) fn run(blocks: Vec<Value>) -> Analysis {
    analyze(&doc(blocks))
}

// --- Assertions ----------------------------------------------------------------

/// Renders a diagnostic on one line, for failure messages and snapshots.
pub(crate) fn render(d: &Diagnostic) -> String {
    let part = match &d.primary.part {
        Part::Whole => String::new(),
        Part::Field { name } => format!(" field {name}"),
        Part::Input { name } => format!(" input {name}"),
        Part::Tokens { input, start, end } => format!(" {input}[{start}..{end}]"),
    };
    let block = d.primary.block.as_ref().map_or("project", |b| b.as_str());
    let severity = match d.severity {
        Severity::Error => "error",
        Severity::Warning => "warning",
        Severity::Info => "info",
    };
    format!("{severity} {} at {block}{part}: {}", d.code.0, d.message)
}

pub(crate) fn render_all(a: &Analysis) -> String {
    a.diagnostics.iter().map(render).collect::<Vec<_>>().join("\n")
}

/// The diagnostic codes, in order.
pub(crate) fn codes(a: &Analysis) -> Vec<String> {
    a.diagnostics.iter().map(|d| d.code.0.clone()).collect()
}

/// Asserts that there are no diagnostics at all.
#[track_caller]
pub(crate) fn assert_clean(a: &Analysis) {
    assert!(
        a.diagnostics.is_empty(),
        "unexpected diagnostics:\n{}",
        render_all(a)
    );
}

/// Asserts the exact list of codes (in order).
#[track_caller]
pub(crate) fn assert_codes(a: &Analysis, expected: &[&str]) {
    let actual = codes(a);
    assert_eq!(actual, expected, "diagnostics:\n{}", render_all(a));
}

/// The only diagnostic with this code.
#[track_caller]
pub(crate) fn only<'a>(a: &'a Analysis, code: &str) -> &'a Diagnostic {
    let found: Vec<&Diagnostic> = a.diagnostics.iter().filter(|d| d.code.0 == code).collect();
    assert_eq!(found.len(), 1, "expected one {code}:\n{}", render_all(a));
    found[0]
}

/// The statements of `main` (first item named main) as JSON, for structural checks.
pub(crate) fn main_json(a: &Analysis) -> Value {
    let program = serde_json::to_value(&a.program).expect("serialise");
    for item in program["modules"][0]["items"].as_array().expect("items") {
        if let Some(main) = item["kind"].get("main") {
            return main["body"]["stmts"].clone();
        }
    }
    panic!("no main in program")
}

// --- Expression slots ----------------------------------------------------------

/// Tokenises a compact notation into slot tokens: identifiers are refs to
/// `sym_<name>`, `$word` is a text token, `true`/`false`/other keywords in
/// `KEYWORDS` are keyword tokens, `and`/`or`/`not` are operators, `"…"` is a
/// string, `'c'` a character.
pub(crate) fn tokens(src: &str) -> Vec<Value> {
    const KEYWORDS: &[&str] = &["true", "false", "nullptr", "this"];
    const TWO_CHAR: &[&str] = &[
        "==", "!=", "<=", ">=", "&&", "||", "<<", ">>", "++", "--", "+=", "->", "::",
    ];
    let chars: Vec<char> = src.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
        } else if c.is_ascii_digit() || (c == '.' && chars.get(i + 1).is_some_and(char::is_ascii_digit)) {
            let start = i;
            while i < chars.len()
                && (chars[i].is_ascii_alphanumeric()
                    || chars[i] == '.'
                    || chars[i] == '\''
                    || ((chars[i] == '-' || chars[i] == '+') && matches!(chars[i - 1], 'e' | 'E')))
            {
                i += 1;
            }
            out.push(json!({"num": chars[start..i].iter().collect::<String>()}));
        } else if c.is_ascii_alphabetic() || c == '$' || c == '_' {
            let start = i;
            i += 1;
            while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            let word: String = chars[start..i].iter().collect();
            out.push(if KEYWORDS.contains(&word.as_str()) {
                json!({"kw": word})
            } else if ["and", "or", "not"].contains(&word.as_str()) {
                json!({"op": word})
            } else if let Some(text) = word.strip_prefix('$') {
                json!({"text": text})
            } else {
                json!({"ref": sym(&word)})
            });
        } else if c == '"' {
            let start = i + 1;
            i = start;
            while chars[i] != '"' {
                i += 1;
            }
            out.push(json!({"str": chars[start..i].iter().collect::<String>()}));
            i += 1;
        } else if c == '\'' {
            let start = i + 1;
            i = start;
            while chars[i] != '\'' {
                i += 1;
            }
            out.push(json!({"chr": chars[start..i].iter().collect::<String>()}));
            i += 1;
        } else {
            let two: String = chars[i..(i + 2).min(chars.len())].iter().collect();
            if TWO_CHAR.contains(&two.as_str()) {
                out.push(json!({"op": two}));
                i += 2;
            } else {
                out.push(json!({"op": c.to_string()}));
                i += 1;
            }
        }
    }
    out
}

/// An expression-slot input.
pub(crate) fn e(src: &str) -> Value {
    json!({"expr": tokens(src)})
}

/// A draft expression-slot input.
pub(crate) fn draft(src: &str) -> Value {
    json!({"expr": tokens(src), "draft": true})
}

/// An input holding a nested block.
pub(crate) fn input(block: Value) -> Value {
    json!({"block": block})
}

// --- Top-level blocks ----------------------------------------------------------

/// `when program starts` with ID `main`.
pub(crate) fn main(body: Vec<Value>) -> Value {
    json!({"id": "main", "type": "program.main", "v": 1, "statements": {"BODY": body}})
}

/// `define <name>(params) returns <ret>`; params are `(name, type, mode)`.
pub(crate) fn func(name: &str, ret: &str, params: &[(&str, &str, &str)], body: Vec<Value>) -> Value {
    let params: Vec<Value> = params
        .iter()
        .map(|(p, ty, mode)| json!({"sym": sym(p), "name": p, "type": ty, "mode": mode}))
        .collect();
    json!({
        "id": format!("fn_{name}"), "type": "func.define", "v": 1,
        "fields": {"NAME": decl(name), "RETURNS": ret},
        "extra": {"params": params},
        "statements": {"BODY": body}
    })
}

// --- Statements ----------------------------------------------------------------

pub(crate) fn declare(ty: &str, name: &str, value: Option<Value>) -> Value {
    let mut block =
        json!({"id": id(), "type": "var.declare", "v": 1, "fields": {"TYPE": ty, "NAME": decl(name)}});
    if let Some(value) = value {
        block["inputs"] = json!({"VALUE": value});
    }
    block
}

pub(crate) fn declare_const(ty: &str, name: &str, value: Value) -> Value {
    let mut block = declare(ty, name, Some(value));
    block["fields"]["CONST"] = json!(true);
    block
}

pub(crate) fn set(name: &str, value: Value) -> Value {
    json!({"id": id(), "type": "var.set", "v": 1, "fields": {"VAR": reference(name)}, "inputs": {"VALUE": value}})
}

pub(crate) fn change(name: &str, by: Value) -> Value {
    json!({"id": id(), "type": "var.change", "v": 1, "fields": {"VAR": reference(name)}, "inputs": {"BY": by}})
}

pub(crate) fn update(name: &str, op: &str, value: Value) -> Value {
    json!({"id": id(), "type": "var.update", "v": 1,
           "fields": {"VAR": reference(name), "OP": op}, "inputs": {"VALUE": value}})
}

/// `if` with `else if` branches and an optional `else`.
pub(crate) fn if_else(branches: Vec<(Value, Vec<Value>)>, else_body: Option<Vec<Value>>) -> Value {
    let count = branches.len().saturating_sub(1);
    let (conds, bodies): (Vec<Value>, Vec<Value>) =
        branches.into_iter().map(|(c, b)| (c, Value::Array(b))).unzip();
    let mut statements = numbered("DO", bodies);
    let has_else = else_body.is_some();
    if let Some(body) = else_body {
        statements.insert(String::from("ELSE"), Value::Array(body));
    }
    json!({"id": id(), "type": "control.if", "v": 1,
           "extra": {"elseIfCount": count, "hasElse": has_else},
           "inputs": numbered("COND", conds), "statements": statements})
}

pub(crate) fn if_then(cond: Value, body: Vec<Value>) -> Value {
    if_else(vec![(cond, body)], None)
}

pub(crate) fn while_loop(mode: &str, cond: Value, body: Vec<Value>) -> Value {
    json!({"id": id(), "type": "control.while", "v": 1, "fields": {"MODE": mode},
           "inputs": {"COND": cond}, "statements": {"BODY": body}})
}

pub(crate) fn repeat(times: Value, body: Vec<Value>) -> Value {
    json!({"id": id(), "type": "control.repeat", "v": 1, "inputs": {"TIMES": times}, "statements": {"BODY": body}})
}

pub(crate) fn for_range(
    var: &str,
    from: Value,
    direction: &str,
    to: Value,
    step: Option<Value>,
    body: Vec<Value>,
) -> Value {
    let mut block = json!({"id": id(), "type": "control.for_range", "v": 1,
           "fields": {"VAR": decl(var), "DIRECTION": direction},
           "inputs": {"FROM": from, "TO": to}, "statements": {"BODY": body}});
    if let Some(step) = step {
        block["inputs"]["STEP"] = step;
    }
    block
}

pub(crate) fn forever(body: Vec<Value>) -> Value {
    json!({"id": id(), "type": "control.forever", "v": 1, "statements": {"BODY": body}})
}

pub(crate) fn brk() -> Value {
    json!({"id": id(), "type": "control.break", "v": 1})
}

pub(crate) fn cont() -> Value {
    json!({"id": id(), "type": "control.continue", "v": 1})
}

pub(crate) fn print(items: Vec<Value>) -> Value {
    let count = items.len();
    json!({"id": id(), "type": "io.print", "v": 1, "extra": {"itemCount": count}, "inputs": numbered("ITEM", items)})
}

pub(crate) fn ask(name: &str, prompt: Option<Value>) -> Value {
    let mut block = json!({"id": id(), "type": "io.ask", "v": 1, "fields": {"VAR": reference(name)}});
    if let Some(prompt) = prompt {
        block["inputs"] = json!({"PROMPT": prompt});
    }
    block
}

pub(crate) fn call_stmt(name: &str, args: Vec<Value>) -> Value {
    let count = args.len();
    json!({"id": id(), "type": "func.call_stmt", "v": 1, "fields": {"FUNC": reference(name)},
           "extra": {"argCount": count}, "inputs": numbered("ARG", args)})
}

pub(crate) fn ret(value: Option<Value>) -> Value {
    let mut block = json!({"id": id(), "type": "func.return", "v": 1});
    if let Some(value) = value {
        block["inputs"] = json!({"VALUE": value});
    }
    block
}

pub(crate) fn exit(code: Value) -> Value {
    json!({"id": id(), "type": "program.exit", "v": 1, "inputs": {"CODE": code}})
}

// --- Reporters (as inputs) ---------------------------------------------------------

pub(crate) fn get(name: &str) -> Value {
    input(json!({"id": id(), "type": "var.get", "v": 1, "fields": {"VAR": reference(name)}}))
}

pub(crate) fn num(text: &str) -> Value {
    input(json!({"id": id(), "type": "math.number", "v": 1, "fields": {"VALUE": text}}))
}

pub(crate) fn arith(op: &str, a: Value, b: Value) -> Value {
    input(
        json!({"id": id(), "type": "math.arithmetic", "v": 1, "fields": {"OP": op}, "inputs": {"A": a, "B": b}}),
    )
}

pub(crate) fn compare(op: &str, a: Value, b: Value) -> Value {
    input(
        json!({"id": id(), "type": "math.compare", "v": 1, "fields": {"OP": op}, "inputs": {"A": a, "B": b}}),
    )
}

pub(crate) fn random_int(low: Value, high: Value) -> Value {
    input(json!({"id": id(), "type": "math.random_int", "v": 1, "inputs": {"LOW": low, "HIGH": high}}))
}

pub(crate) fn convert(to: &str, value: Value) -> Value {
    input(
        json!({"id": id(), "type": "math.convert", "v": 1, "fields": {"TO": to}, "inputs": {"VALUE": value}}),
    )
}

pub(crate) fn boolean(value: bool) -> Value {
    input(json!({"id": id(), "type": "logic.boolean", "v": 1, "fields": {"VALUE": value.to_string()}}))
}

pub(crate) fn logic(op: &str, items: Vec<Value>) -> Value {
    let count = items.len();
    input(
        json!({"id": id(), "type": "logic.operation", "v": 1, "fields": {"OP": op},
                 "extra": {"itemCount": count}, "inputs": numbered("ITEM", items)}),
    )
}

pub(crate) fn not(a: Value) -> Value {
    input(json!({"id": id(), "type": "logic.not", "v": 1, "inputs": {"A": a}}))
}

pub(crate) fn ternary(cond: Value, then_value: Value, else_value: Value) -> Value {
    input(json!({"id": id(), "type": "logic.ternary", "v": 1,
                 "inputs": {"COND": cond, "THEN": then_value, "ELSE": else_value}}))
}

pub(crate) fn text(value: &str) -> Value {
    input(json!({"id": id(), "type": "text.literal", "v": 1, "fields": {"VALUE": value}}))
}

pub(crate) fn chr(value: &str) -> Value {
    input(json!({"id": id(), "type": "text.char", "v": 1, "fields": {"VALUE": value}}))
}

pub(crate) fn join(items: Vec<Value>) -> Value {
    let count = items.len();
    input(
        json!({"id": id(), "type": "text.join", "v": 1, "extra": {"itemCount": count},
                 "inputs": numbered("ITEM", items)}),
    )
}

pub(crate) fn call(name: &str, args: Vec<Value>) -> Value {
    let count = args.len();
    input(
        json!({"id": id(), "type": "func.call", "v": 1, "fields": {"FUNC": reference(name)},
                 "extra": {"argCount": count}, "inputs": numbered("ARG", args)}),
    )
}
