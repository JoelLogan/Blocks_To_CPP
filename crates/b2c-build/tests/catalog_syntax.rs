//! The catalog snapshot test (spec §9.2, *Snapshot*): every block of the core
//! catalog in its default configuration, and a few curated variants, goes
//! through the whole front end ([`run_frontend`]); its generated C++ is
//! snapshotted with `insta` and compiled with `g++ -fsyntax-only` under C++17
//! and C++20.
//!
//! * **Default configuration.** A block comes as the toolbox gives it: its
//!   type and version with the toolbox entry's preset and nothing else, so
//!   the catalog fills in the default fields, `extra` settings and input
//!   tokens (the editor's shadows) when the project loads (spec §5.4). The
//!   blocks the editor adds to the Variables and Functions categories for the
//!   open project get what the editor gives them: the chosen variable or
//!   function and, for *set*, the start value of the variable's type. A new
//!   *define* block has an empty parameter list, as the editor saves it.
//! * **Smallest valid surroundings.** A project needs its *when program
//!   starts* block (`B2C-E0405`), and the generated code must compile with
//!   the strict warnings as errors, so only `program.main` stands alone.
//!   Each case's description (in its snapshot) says how its block is placed:
//!   - `program.exit`, `control.if`, `control.while`, `control.repeat`,
//!     `control.for_range` (counting with `i`), `control.forever` and
//!     `io.print` are the only step of `main`;
//!   - `control.break` and `control.continue` need a loop: each is the body
//!     of a *forever* loop, the only step of `main`;
//!   - `func.define` (the function `f`, giving back nothing) sits next to an
//!     empty `main`, and so does such a function whose body is `func.return`;
//!     `func.call_stmt` calls an empty such function from `main`, and
//!     `func.call` is printed, calling a function `f` that returns the `int`
//!     0;
//!   - `var.declare` is the toolbox's *create* block, declaring `x` (an `int`
//!     starting at 0), followed by a print of `x`, since g++ warns about a
//!     variable that is never read; `io.ask`, `var.set` and `var.change` come
//!     between that *create* block and that print, and `var.get` is printed
//!     after the *create* block;
//!   - every other reporter and predicate is the only item of a print, the
//!     only step of `main`.
//! * **Curated variants** add the forms whose headers the code generator
//!   chooses rather than the catalog (catalog 1.3.0, M3 contracts types.md
//!   §5.3): *stop program* outside `main` (`std::exit`), a simple *ask*, an
//!   *ask* for text, and a text literal that becomes a `std::string`.
//! * **Coverage.** Every block of the core catalog has exactly one default
//!   case, and every case's block is in the catalog and in the case's
//!   program, so a new block without a case fails
//!   `every_catalog_block_has_a_case`.
//! * **Snapshots.** `tests/snapshots/catalog_syntax__<case>.snap` holds the
//!   code of the case as a C++20 project, with the app version in the banner
//!   masked so a release changes no snapshot. Where the code of the case as a
//!   C++17 project differs, it gets a second snapshot, `<case>@c++17`. Every
//!   case must also pass the front end without a warning. After an intended
//!   change, review the new snapshots with `cargo insta review`, or rewrite
//!   them with `INSTA_UPDATE=always` and review the diff.
//! * **Compiling.** All cases go to g++ in one invocation per standard, the
//!   two standards in parallel: the compile flags of a debug build at the
//!   *strict* warning level, warnings as errors (`CommandPlan`, spec §7.4),
//!   and `-fsyntax-only`. Without g++ this part is skipped, unless
//!   `B2C_REQUIRE_GXX` is set, as in CI, where the Linux job (GCC 13), the
//!   GCC 11 and GCC 15 containers and the Windows job (MSYS2) run it.

// Test helpers fail the test by panicking.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::cell::Cell;
use std::collections::BTreeSet;
use std::ffi::OsString;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::time::Duration;

use b2c_build::{FrontendOptions, run_frontend};
use b2c_catalog::{Preset, core_catalog, toolbox};
use b2c_ir::Severity;
use b2c_ir::sast::CppStandard;
use b2c_ir::source_map::{FileKind, GeneratedProject};
use b2c_model::{Configurations, Language, WarningLevel};
use b2c_toolchain::command::{BuildInputs, CommandPlan, CompilerCommand};
use b2c_toolchain::env::{HostEnv, compiler_env};
use b2c_toolchain::flags::ExtraFlags;
use b2c_toolchain::probe::{DiagnosticsFormat, Toolchain};
use serde_json::{Map, Value, json};

/// The standards every case is generated for and compiled with.
const STANDARDS: [CppStandard; 2] = [CppStandard::Cpp17, CppStandard::Cpp20];

/// The standard whose code a case's snapshot holds (the default of a new
/// project); the code for another standard has a snapshot of its own only
/// where it differs.
const SNAPSHOT_STANDARD: CppStandard = CppStandard::Cpp20;

/// How long the g++ invocation over every case may take: a build gives one
/// compiler invocation 2 minutes, but this one checks every case, and
/// Windows runners are slow.
const COMPILE_TIMEOUT: Duration = Duration::from_mins(10);

/// The symbol of the variable the cases declare (`x`).
const X: &str = "s_x";
/// The symbol of the function the cases define (`f`).
const F: &str = "s_f";
/// The symbol of the counting loop's variable (`i`).
const I: &str = "s_i";

/// One program to generate, snapshot and compile.
struct Case {
    /// The snapshot and folder name: the block's ID, or `<ID>-<variant>` for
    /// a curated variant.
    name: String,
    /// The catalog block the case is about.
    block: &'static str,
    /// Whether this is a curated variant rather than the block's default
    /// case.
    curated: bool,
    /// How the block is placed (its snapshot's description).
    placement: &'static str,
    /// The module's top-level blocks.
    blocks: Vec<Value>,
}

impl Case {
    /// The default case of `block`.
    fn new(block: &'static str, placement: &'static str, blocks: Vec<Value>) -> Self {
        Self {
            name: block.to_owned(),
            block,
            curated: false,
            placement,
            blocks,
        }
    }

    /// A curated variant of `block`.
    fn variant(block: &'static str, variant: &str, placement: &'static str, blocks: Vec<Value>) -> Self {
        Self {
            name: format!("{block}-{variant}"),
            block,
            curated: true,
            placement,
            blocks,
        }
    }

    /// The case as project bytes for `standard`.
    fn project(&self, standard: CppStandard) -> Vec<u8> {
        let document = json!({
            "format": "blocks2cpp/project",
            "formatVersion": 1,
            "generator": {
                "app": env!("CARGO_PKG_VERSION"),
                "catalog": b2c_catalog::CATALOG_VERSION,
            },
            "project": {
                "id": "prj_catalog_syntax",
                "name": format!("Catalog block {}", self.name),
                "language": { "standard": standard },
            },
            "modules": [{
                "id": "mod_main",
                "name": "main",
                "workspace": { "blocks": self.blocks },
            }],
        });
        serde_json::to_vec_pretty(&document).unwrap()
    }

    /// Whether the case's program holds a block of its type.
    fn uses_its_block(&self) -> bool {
        fn holds(value: &Value, kind: &str) -> bool {
            match value {
                Value::Object(map) => {
                    map.get("type").and_then(Value::as_str) == Some(kind)
                        || map.values().any(|value| holds(value, kind))
                }
                Value::Array(items) => items.iter().any(|value| holds(value, kind)),
                _ => false,
            }
        }
        self.blocks.iter().any(|block| holds(block, self.block))
    }
}

/// Makes block nodes as the editor saves them, each with a new ID.
#[derive(Default)]
struct Blocks {
    made: Cell<u32>,
}

impl Blocks {
    /// A `kind` block as the toolbox gives it (with its entry's preset), and
    /// `parts` merged in: an object whose `fields`, `extra`, `inputs` and
    /// `statements` add to the preset's or replace them. The catalog fills
    /// in what is still absent when the project loads.
    fn block(&self, kind: &str, parts: &Value) -> Value {
        let made = self.made.get() + 1;
        self.made.set(made);
        let definition = core_catalog()
            .blocks
            .get(kind)
            .unwrap_or_else(|| panic!("{kind} is not a block of the core catalog"));
        let mut block = json!({ "id": format!("b{made:03}"), "type": kind, "v": definition.version });
        if let Some(preset) = preset(kind) {
            merge(&mut block, &preset_parts(preset));
        }
        merge(&mut block, parts);
        block
    }

    /// A block without parts of its own.
    fn plain(&self, kind: &str) -> Value {
        self.block(kind, &json!({}))
    }

    /// *when program starts* with `body`, on the canvas.
    fn main(&self, body: &[Value]) -> Value {
        self.block(
            "program.main",
            &json!({ "x": 40, "y": 40, "statements": { "BODY": body } }),
        )
    }

    /// The toolbox's *define* block on the canvas, naming the function `f`,
    /// with `parts` merged in. Its parameter list (`params`, which has no
    /// catalog default) is empty, as the editor saves a new one.
    fn define_f(&self, parts: &Value) -> Value {
        let mut all = json!({
            "x": 400,
            "y": 40,
            "fields": { "NAME": declaration(F, "f") },
            "extra": { "params": [] },
        });
        merge(&mut all, parts);
        self.block("func.define", &all)
    }

    /// The function `f`, giving back `returns`, with `body`, on the canvas.
    fn function(&self, returns: &str, body: &[Value]) -> Value {
        self.define_f(&json!({ "fields": { "RETURNS": returns }, "statements": { "BODY": body } }))
    }

    /// The toolbox's *create* block, declaring `x`, with `fields` merged in.
    fn create_x(&self, fields: &Value) -> Value {
        let mut parts = json!({ "fields": { "NAME": declaration(X, "x") } });
        merge(&mut parts, &json!({ "fields": fields }));
        self.block("var.declare", &parts)
    }

    /// A print of one value (an input: tokens or a nested block).
    fn print(&self, item: &Value) -> Value {
        self.block("io.print", &json!({ "inputs": { "ITEM0": item } }))
    }

    /// A print of `x`.
    fn print_x(&self) -> Value {
        self.print(&tokens(&json!([reference(X)])))
    }

    /// `main` with only a print of the `kind` block, as the toolbox gives it.
    fn printed(&self, kind: &str) -> Vec<Value> {
        vec![self.main(&[self.print(&nested(&self.plain(kind)))])]
    }

    /// `main` with *create* `x`, `step` and a print of `x`.
    fn with_x(&self, step: Value) -> Vec<Value> {
        vec![self.main(&[self.create_x(&json!({})), step, self.print_x()])]
    }
}

/// The toolbox entry's preset of `kind`, if it has one.
fn preset(kind: &str) -> Option<&'static Preset> {
    toolbox()
        .categories
        .iter()
        .flat_map(|category| &category.entries)
        .find(|entry| entry.block == kind)
        .and_then(|entry| entry.preset.as_ref())
}

/// A preset as block parts: its fields, `extra` values and input tokens.
fn preset_parts(preset: &Preset) -> Value {
    let mut parts = Map::new();
    if !preset.fields.is_empty() {
        parts.insert("fields".into(), serde_json::to_value(&preset.fields).unwrap());
    }
    if !preset.extra.is_empty() {
        parts.insert("extra".into(), serde_json::to_value(&preset.extra).unwrap());
    }
    if !preset.inputs.is_empty() {
        let inputs: Map<String, Value> = preset
            .inputs
            .iter()
            .map(|(name, list)| (name.clone(), tokens(&serde_json::to_value(list).unwrap())))
            .collect();
        parts.insert("inputs".into(), Value::Object(inputs));
    }
    Value::Object(parts)
}

/// Merges `parts` into `block`: the entries of an object that both have are
/// merged, everything else is replaced.
fn merge(block: &mut Value, parts: &Value) {
    let (Value::Object(block), Value::Object(parts)) = (block, parts) else {
        panic!("blocks and their parts are objects");
    };
    for (key, value) in parts {
        match (block.get_mut(key), value) {
            (Some(Value::Object(old)), Value::Object(new)) => {
                old.extend(new.iter().map(|(key, value)| (key.clone(), value.clone())));
            }
            _ => {
                block.insert(key.clone(), value.clone());
            }
        }
    }
}

/// A declaration field.
fn declaration(symbol: &str, name: &str) -> Value {
    json!({ "sym": symbol, "name": name })
}

/// A reference, as a field or a token.
fn reference(symbol: &str) -> Value {
    json!({ "ref": symbol })
}

/// An expression input of these tokens.
fn tokens(list: &Value) -> Value {
    json!({ "expr": list })
}

/// A value input holding `block`.
fn nested(block: &Value) -> Value {
    json!({ "block": block })
}

/// Every case: one per catalog block, then the curated variants.
fn cases() -> Vec<Case> {
    let b = Blocks::default();
    let mut cases = statement_cases(&b);
    cases.extend(value_cases(&b));
    cases.extend(variants(&b));
    cases
}

/// The hat, the definition and the statements.
fn statement_cases(b: &Blocks) -> Vec<Case> {
    let step = |kind: &'static str| Case::new(kind, "the only step of main", vec![b.main(&[b.plain(kind)])]);
    let in_forever = |kind: &'static str| {
        let forever = b.block(
            "control.forever",
            &json!({ "statements": { "BODY": [b.plain(kind)] } }),
        );
        Case::new(
            kind,
            "the body of a forever loop, the only step of main",
            vec![b.main(&[forever])],
        )
    };
    vec![
        Case::new("program.main", "alone", vec![b.main(&[])]),
        step("program.exit"),
        step("control.if"),
        step("control.while"),
        step("control.repeat"),
        Case::new(
            "control.for_range",
            "the only step of main, counting with i",
            vec![b.main(&[b.block(
                "control.for_range",
                &json!({ "fields": { "VAR": declaration(I, "i") } }),
            )])],
        ),
        step("control.forever"),
        in_forever("control.break"),
        in_forever("control.continue"),
        Case::new(
            "func.define",
            "the void function f, next to an empty main",
            vec![b.main(&[]), b.define_f(&json!({}))],
        ),
        Case::new(
            "func.call_stmt",
            "the only step of main, calling the empty void function f",
            vec![
                b.main(&[b.block("func.call_stmt", &json!({ "fields": { "FUNC": reference(F) } }))]),
                b.function("void", &[]),
            ],
        ),
        Case::new(
            "func.return",
            "the body of the void function f, next to an empty main",
            vec![b.main(&[]), b.function("void", &[b.plain("func.return")])],
        ),
        step("io.print"),
        Case::new(
            "io.ask",
            "asks for the int x, after create x and before a print of x, in main",
            b.with_x(b.block("io.ask", &json!({ "fields": { "VAR": reference(X) } }))),
        ),
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
    ]
}

/// The reporters and predicates.
fn value_cases(b: &Blocks) -> Vec<Case> {
    const PRINTED: &str = "the only item of a print, the only step of main";
    let printed = |kind: &'static str| Case::new(kind, PRINTED, b.printed(kind));
    let return_zero = b.block(
        "func.return",
        &json!({ "inputs": { "VALUE": tokens(&json!([{ "num": "0" }])) } }),
    );
    vec![
        Case::new(
            "func.call",
            "calls the function f, which returns the int 0, as the only item of a print in main",
            vec![
                b.main(&[b.print(&nested(
                    &b.block("func.call", &json!({ "fields": { "FUNC": reference(F) } })),
                ))]),
                b.function("int", &[return_zero]),
            ],
        ),
        printed("logic.boolean"),
        printed("logic.operation"),
        printed("logic.not"),
        printed("logic.ternary"),
        printed("math.number"),
        printed("math.arithmetic"),
        printed("math.compare"),
        printed("math.random_int"),
        printed("math.convert"),
        printed("text.literal"),
        printed("text.char"),
        printed("text.join"),
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
    ]
}

/// The forms whose headers the code generator chooses (catalog 1.3.0).
fn variants(b: &Blocks) -> Vec<Case> {
    let text_x = |value: &Value| {
        let mut create = b.create_x(&json!({ "TYPE": "std::string" }));
        merge(&mut create, &json!({ "inputs": { "VALUE": value } }));
        create
    };
    vec![
        Case::variant(
            "program.exit",
            "in-function",
            "the body of the void function f (std::exit), next to an empty main",
            vec![b.main(&[]), b.function("void", &[b.plain("program.exit")])],
        ),
        Case::variant(
            "io.ask",
            "simple",
            "a simple ask for the int x, after create x and before a print of x, in main",
            b.with_x(b.block(
                "io.ask",
                &json!({ "fields": { "VAR": reference(X), "MODE": "simple" } }),
            )),
        ),
        Case::variant(
            "io.ask",
            "text",
            "asks for the text x, after create x (an empty text) and before a print of x, in main",
            vec![b.main(&[
                text_x(&tokens(&json!([{ "str": "" }]))),
                b.block("io.ask", &json!({ "fields": { "VAR": reference(X) } })),
                b.print_x(),
            ])],
        ),
        Case::variant(
            "text.literal",
            "as-string",
            "the value of create x, a text, before a print of x, in main",
            vec![b.main(&[text_x(&nested(&b.plain("text.literal"))), b.print_x()])],
        ),
    ]
}

/// The generated project of `case` for `standard`, or what went wrong: any
/// diagnostic other than a note fails a case.
fn generate(case: &Case, standard: CppStandard) -> Result<GeneratedProject, String> {
    let frontend = run_frontend(&case.project(standard), &FrontendOptions::default());
    let reported: Vec<String> = frontend
        .diagnostics
        .iter()
        .filter(|d| d.severity != Severity::Info)
        .map(|d| format!("{} {:?}: {}", d.code.0, d.primary, d.message))
        .collect();
    match frontend.generated {
        Some(generated) if reported.is_empty() => Ok(generated),
        _ => Err(format!(
            "{} (as a {} project): {reported:#?}",
            case.name,
            spelling(standard)
        )),
    }
}

/// A standard as a project spells it (`c++17`).
fn spelling(standard: CppStandard) -> String {
    match serde_json::to_value(standard) {
        Ok(Value::String(text)) => text,
        other => panic!("{standard:?} is spelled {other:?}"),
    }
}

/// Every file of a project as one text (one file: just its contents), with
/// the app version in the banner masked.
fn render(project: &GeneratedProject) -> String {
    let banner = format!("Blocks2Cpp {} from project", env!("CARGO_PKG_VERSION"));
    let mask = |text: &str| text.replacen(&banner, "Blocks2Cpp [version] from project", 1);
    if let [file] = project.files.as_slice() {
        return mask(&file.contents);
    }
    project
        .files
        .iter()
        .map(|file| format!("==> {} <==\n{}", file.path, mask(&file.contents)))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Panics with every collected failure, if any.
fn finish(failures: &[String]) {
    if !failures.is_empty() {
        let mut message = format!("{} catalog case failure(s):\n", failures.len());
        for failure in failures {
            let _ = writeln!(message, "--- {failure}");
        }
        panic!("{message}");
    }
}

#[test]
fn every_catalog_block_has_a_case() {
    let cases = cases();
    let mut failures = Vec::new();
    let mut names = BTreeSet::new();
    let mut covered = BTreeSet::new();
    for case in &cases {
        if !names.insert(case.name.as_str()) {
            failures.push(format!("{}: two cases have this name", case.name));
        }
        if !core_catalog().blocks.contains_key(case.block) {
            failures.push(format!(
                "{}: {} is not a block of the core catalog",
                case.name, case.block
            ));
        }
        if !case.uses_its_block() {
            failures.push(format!("{}: its program has no {} block", case.name, case.block));
        }
        if !case.curated && !covered.insert(case.block) {
            failures.push(format!("{}: a second default case for {}", case.name, case.block));
        }
    }
    for block in core_catalog().blocks.keys() {
        if !covered.contains(block.as_str()) {
            failures.push(format!(
                "{block} has no case: add one to statement_cases or value_cases in \
                 crates/b2c-build/tests/catalog_syntax.rs, placing the block in the smallest program \
                 that compiles without warnings"
            ));
        }
    }
    finish(&failures);
}

#[test]
fn catalog_blocks_generate_the_expected_cpp() {
    let mut failures = Vec::new();
    let mut texts = Vec::new();
    for case in cases() {
        let generated: Result<Vec<(CppStandard, String)>, String> = STANDARDS
            .iter()
            .map(|&standard| generate(&case, standard).map(|project| (standard, render(&project))))
            .collect();
        match generated {
            Ok(generated) => texts.push((case, generated)),
            Err(failure) => failures.push(failure),
        }
    }
    finish(&failures);
    for (case, generated) in texts {
        let main = generated
            .iter()
            .find(|(standard, _)| *standard == SNAPSHOT_STANDARD)
            .map(|(_, text)| text.clone())
            .unwrap();
        let mut settings = insta::Settings::clone_current();
        settings.set_description(format!("{}: {}", case.block, case.placement));
        settings.set_omit_expression(true);
        settings.bind(|| {
            insta::assert_snapshot!(case.name.clone(), main);
            for (standard, text) in &generated {
                if *text != main {
                    insta::assert_snapshot!(format!("{}@{}", case.name, spelling(*standard)), text);
                }
            }
        });
    }
}

#[test]
fn catalog_blocks_compile_with_the_strict_warnings() {
    let Some(toolchain) = common::real_toolchain() else {
        return;
    };
    let cases = cases();
    let root = tempfile::tempdir().unwrap();
    let failures: Vec<String> = std::thread::scope(|scope| {
        let checks: Vec<_> = STANDARDS
            .iter()
            .map(|&standard| {
                let (toolchain, cases, root) = (&toolchain, &cases, root.path());
                scope.spawn(move || syntax_check(toolchain, cases, standard, root))
            })
            .collect();
        checks
            .into_iter()
            .filter_map(|check| check.join().unwrap().err())
            .collect()
    });
    finish(&failures);
}

/// Generates every case as a `standard` project into a folder of `root`,
/// each into a folder named after the case, and checks them all with one
/// `g++ -fsyntax-only`.
fn syntax_check(
    toolchain: &Toolchain,
    cases: &[Case],
    standard: CppStandard,
    root: &Path,
) -> Result<(), String> {
    let folder = root.join(spelling(standard));
    std::fs::create_dir(&folder).unwrap();
    let mut sources = Vec::new();
    for case in cases {
        let generated = generate(case, standard)?;
        let dir = folder.join(&case.name);
        std::fs::create_dir(&dir).unwrap();
        for file in &generated.files {
            std::fs::write(dir.join(&file.path), &file.contents).unwrap();
            if file.kind == FileKind::Source {
                sources.push(Path::new(&case.name).join(&file.path));
            }
        }
    }
    let command = syntax_only(toolchain, standard, &folder, &sources)?;
    let env = compiler_env(
        toolchain.platform(),
        toolchain.bin_dir(),
        &folder,
        &HostEnv::from_process(&[]),
    );
    let process = command
        .process_command(&folder, &env, Some(COMPILE_TIMEOUT))
        .map_err(|error| error.to_string())?;
    let captured = b2c_process::run_captured(&process).map_err(|error| error.to_string())?;
    if captured.status == b2c_process::ExitStatus::Exited(0) {
        Ok(())
    } else {
        Err(format!(
            "g++ rejected the code of the cases as {} projects ({:?}); each file is in a folder named after \
             its case:\n{}",
            spelling(standard),
            captured.status,
            String::from_utf8_lossy(&captured.stderr)
        ))
    }
}

/// `g++ -fsyntax-only` over `sources` (relative to `folder`, its working
/// directory) with the compile flags a debug build of a `standard` project
/// gets at the strict warning level with warnings as errors, and plain-text
/// messages.
fn syntax_only(
    toolchain: &Toolchain,
    standard: CppStandard,
    folder: &Path,
    sources: &[PathBuf],
) -> Result<CompilerCommand, String> {
    let mut configuration = Configurations::default().debug;
    configuration.warnings = WarningLevel::Strict;
    configuration.warnings_as_errors = true;
    let language = Language {
        standard,
        gnu_extensions: false,
    };
    let extra = ExtraFlags::none();
    let inputs = BuildInputs::new(toolchain, &configuration, language, folder.to_path_buf(), &extra);
    let plan = CommandPlan::new(&inputs).map_err(|problem| problem.message)?;
    let mut args: Vec<OsString> = plan
        .compile_flags_for_key()
        .iter()
        .filter(|flag| {
            let flag = flag.to_string_lossy();
            !flag.starts_with("-fdiagnostics-format=") && !flag.starts_with("-fdiagnostics-add-output=")
        })
        .cloned()
        .collect();
    let plain_output = OsString::from("-fdiagnostics-plain-output");
    if !args.contains(&plain_output) {
        args.push(plain_output);
    }
    args.push("-fsyntax-only".into());
    args.extend(sources.iter().map(|source| source.clone().into_os_string()));
    Ok(CompilerCommand {
        program: toolchain.path().to_path_buf(),
        args,
        format: DiagnosticsFormat::Plain,
        sarif_files: Vec::new(),
    })
}
