//! The catalog snapshot test (spec §9.2, *Snapshot*): every block of the core
//! catalog in its default configuration, and a few curated variants, goes
//! through the whole front end ([`run_frontend`]); its generated C++ is
//! snapshotted with `insta`, checked to include the header of every standard
//! name it uses, and compiled with `g++ -c` under C++17 and C++20.
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
//!   *ask* for text, and a text literal that becomes a `std::string`; and
//!   variables and parameters of library types read from a project file
//!   (format version 2), passed read-only (`const T&`) and editable.
//! * **Coverage.** Every block of the core catalog has exactly one default
//!   case, and every case's block is in the catalog and in the case's
//!   program, so a new block without a case fails
//!   `every_catalog_block_has_a_case`.
//! * **Case files.** The cases of the blocks of each catalog file
//!   (`catalog/core/<file>.toml`) live in `catalog_syntax_cases/<file>.rs`,
//!   each a module with a `cases` function, so that packages adding blocks
//!   to different catalog files edit different files.
//! * **Snapshots.** `tests/snapshots/catalog_syntax__<case>.snap` holds the
//!   code of the case as a C++20 project, with the app version in the banner
//!   masked so a release changes no snapshot. Where the code of the case as a
//!   C++17 project differs, it gets a second snapshot, `<case>@c++17`. Every
//!   case must also pass the front end without a warning. After an intended
//!   change, review the new snapshots with `cargo insta review`, or rewrite
//!   them with `INSTA_UPDATE=always` and review the diff.
//! * **Headers.** g++ cannot show a missing header: libstdc++'s headers
//!   include one another, so code that names `std::string` with only
//!   `<iostream>` still compiles. So each `std::` name in a case's code must
//!   have its header (one the C++ standard says declares it) among the
//!   file's `#include` lines, from the table [`HEADERS`]; a name missing from
//!   the table fails the test until it is added.
//! * **Compiling.** All cases go to g++ in one invocation per standard, the
//!   two standards in parallel: the compile flags of a debug build at the
//!   *strict* warning level, warnings as errors (`CommandPlan`, spec §7.4),
//!   and `-c` (not `-fsyntax-only`, which skips the warnings GCC's middle
//!   end gives, such as `-Wreturn-type` and `-Wuninitialized`), each object
//!   file written to the standard's folder and dropped. Without g++ this part
//!   is skipped, unless `B2C_REQUIRE_GXX` is set, as in CI, where the Linux
//!   job (GCC 13), the GCC 11 and GCC 15 containers and the Windows job
//!   (MSYS2) run it.

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
    /// The project file's format version: 2 for a case with a template
    /// type term (M3 contracts types.md §2).
    format_version: u32,
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
            format_version: 1,
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
            format_version: 1,
        }
    }

    /// The case saved with format version `version`.
    fn with_format_version(mut self, version: u32) -> Self {
        self.format_version = version;
        self
    }

    /// The case as project bytes for `standard`.
    fn project(&self, standard: CppStandard) -> Vec<u8> {
        let document = json!({
            "format": "blocks2cpp/project",
            "formatVersion": self.format_version,
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

    /// The toolbox's *create* block, declaring `x` as a text with `value`.
    fn create_text_x(&self, value: &Value) -> Value {
        let mut create = self.create_x(&json!({ "TYPE": "std::string" }));
        merge(&mut create, &json!({ "inputs": { "VALUE": value } }));
        create
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

/// Each catalog file's cases, in `catalog_syntax_cases/<file>.rs` (w2.md
/// §3.1): a block's default case and its curated variants live in the file
/// named after its catalog file, so packages that add blocks to different
/// files never edit the same case file.
#[path = "catalog_syntax_cases/collections.rs"]
mod collections;
#[path = "catalog_syntax_cases/control.rs"]
mod control;
#[path = "catalog_syntax_cases/errors.rs"]
mod errors;
#[path = "catalog_syntax_cases/files.rs"]
mod files;
#[path = "catalog_syntax_cases/functions.rs"]
mod functions;
#[path = "catalog_syntax_cases/io.rs"]
mod io;
#[path = "catalog_syntax_cases/logic.rs"]
mod logic;
#[path = "catalog_syntax_cases/math.rs"]
mod math;
#[path = "catalog_syntax_cases/program.rs"]
mod program;
#[path = "catalog_syntax_cases/text.rs"]
mod text;
#[path = "catalog_syntax_cases/types.rs"]
mod types;
#[path = "catalog_syntax_cases/variables.rs"]
mod variables;

/// Every case: each catalog file's, in the order of the files.
fn cases() -> Vec<Case> {
    let b = Blocks::default();
    let files: [fn(&Blocks) -> Vec<Case>; 12] = [
        collections::cases,
        control::cases,
        errors::cases,
        files::cases,
        functions::cases,
        io::cases,
        logic::cases,
        math::cases,
        program::cases,
        text::cases,
        types::cases,
        variables::cases,
    ];
    files.iter().flat_map(|file| file(&b)).collect()
}

/// Where most statement blocks stand.
const ONLY_STEP: &str = "the only step of main";

/// Where most reporters and predicates stand.
const PRINTED: &str = "the only item of a print, the only step of main";

/// The default case of the statement block `kind`, as the only step of
/// `main`.
fn only_step(b: &Blocks, kind: &'static str) -> Case {
    Case::new(kind, ONLY_STEP, vec![b.main(&[b.plain(kind)])])
}

/// The default case of the value block `kind`, as the only item of a print.
fn only_printed(b: &Blocks, kind: &'static str) -> Case {
    Case::new(kind, PRINTED, b.printed(kind))
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
                "{block} has no case: add one to the file of its catalog file in \
                 crates/b2c-build/tests/catalog_syntax_cases/, placing the block in the smallest \
                 program that compiles without warnings"
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

/// The header of each standard name the generated code uses: any one of
/// the listed headers declares it (C++ standard, \[headers\] and each
/// name's synopsis), so the code does not depend on one header including
/// another. A name used outside this table fails
/// `catalog_blocks_include_what_they_use`.
const HEADERS: &[(&str, &[&str])] = &[
    ("std::abs", &["<cstdlib>", "<cmath>"]),
    ("std::acos", &["<cmath>"]),
    ("std::asin", &["<cmath>"]),
    ("std::atan", &["<cmath>"]),
    ("std::atan2", &["<cmath>"]),
    ("std::cbrt", &["<cmath>"]),
    ("std::ceil", &["<cmath>"]),
    ("std::cerr", &["<iostream>"]),
    ("std::cin", &["<iostream>"]),
    ("std::clamp", &["<algorithm>"]),
    ("std::cos", &["<cmath>"]),
    ("std::cout", &["<iostream>"]),
    ("std::deque", &["<deque>"]),
    ("std::exception", &["<exception>"]),
    ("std::exit", &["<cstdlib>"]),
    ("std::exp", &["<cmath>"]),
    ("std::find", &["<algorithm>"]),
    ("std::fixed", &["<ios>", "<iostream>"]),
    ("std::floor", &["<cmath>"]),
    ("std::getline", &["<string>"]),
    ("std::greater", &["<functional>"]),
    ("std::hypot", &["<cmath>"]),
    ("std::invalid_argument", &["<stdexcept>"]),
    ("std::isspace", &["<cctype>"]),
    ("std::istream", &["<istream>", "<iostream>"]),
    ("std::istringstream", &["<sstream>"]),
    ("std::log", &["<cmath>"]),
    ("std::log10", &["<cmath>"]),
    ("std::map", &["<map>"]),
    ("std::max", &["<algorithm>"]),
    ("std::min", &["<algorithm>"]),
    ("std::mt19937", &["<random>"]),
    ("std::numeric_limits", &["<limits>"]),
    ("std::ostringstream", &["<sstream>"]),
    ("std::out_of_range", &["<stdexcept>"]),
    ("std::pow", &["<cmath>"]),
    ("std::priority_queue", &["<queue>"]),
    ("std::queue", &["<queue>"]),
    ("std::random_device", &["<random>"]),
    ("std::ranges", &["<algorithm>"]),
    ("std::round", &["<cmath>"]),
    ("std::runtime_error", &["<stdexcept>"]),
    ("std::set", &["<set>"]),
    ("std::setprecision", &["<iomanip>"]),
    ("std::sin", &["<cmath>"]),
    ("std::size_t", &["<cstddef>"]),
    ("std::sort", &["<algorithm>"]),
    ("std::sqrt", &["<cmath>"]),
    ("std::stack", &["<stack>"]),
    ("std::stod", &["<string>"]),
    ("std::stoi", &["<string>"]),
    ("std::streamsize", &["<ios>", "<iostream>"]),
    ("std::string", &["<string>"]),
    ("std::strtol", &["<cstdlib>"]),
    ("std::swap", &["<utility>"]),
    ("std::tan", &["<cmath>"]),
    ("std::to_string", &["<string>"]),
    ("std::tolower", &["<cctype>"]),
    ("std::toupper", &["<cctype>"]),
    ("std::uniform_int_distribution", &["<random>"]),
    ("std::uniform_real_distribution", &["<random>"]),
    ("std::vector", &["<vector>"]),
    ("std::ws", &["<istream>", "<iostream>"]),
];

/// The `std::` names in `code` (`std::numeric_limits`, not its members).
fn standard_names(code: &str) -> BTreeSet<&str> {
    let mut names = BTreeSet::new();
    let mut rest = code;
    while let Some(at) = rest.find("std::") {
        let tail = &rest[at..];
        let end = tail[5..]
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
            .map_or(tail.len(), |end| end + 5);
        // `xstd::` is no standard name.
        let starts_word = rest[..at]
            .chars()
            .next_back()
            .is_none_or(|c| !(c.is_ascii_alphanumeric() || c == '_'));
        if starts_word && end > 5 {
            names.insert(&tail[..end]);
        }
        rest = &tail[end.max(5)..];
    }
    names
}

#[test]
fn standard_names_are_found() {
    let names = standard_names("std::cout << std::numeric_limits<std::streamsize>::max(); mystd::x; std::");
    assert_eq!(
        names.into_iter().collect::<Vec<_>>(),
        ["std::cout", "std::numeric_limits", "std::streamsize"]
    );
}

/// Every `std::` name in a case's code has its header among the file's
/// `#include` lines (see [`HEADERS`]): what g++ cannot check, since
/// libstdc++'s headers include one another.
#[test]
fn catalog_blocks_include_what_they_use() {
    let mut failures = Vec::new();
    for case in cases() {
        for standard in STANDARDS {
            let project = match generate(&case, standard) {
                Ok(project) => project,
                Err(failure) => {
                    failures.push(failure);
                    continue;
                }
            };
            for file in &project.files {
                let included: BTreeSet<&str> = file
                    .contents
                    .lines()
                    .filter_map(|line| line.strip_prefix("#include "))
                    .collect();
                for name in standard_names(&file.contents) {
                    match HEADERS.iter().find(|(known, _)| *known == name) {
                        None => failures.push(format!(
                            "{} ({}): {name} is not in HEADERS; add it with the header that declares it",
                            case.name,
                            spelling(standard)
                        )),
                        Some((_, headers)) if !headers.iter().any(|header| included.contains(header)) => {
                            failures.push(format!(
                                "{} ({}, {}): uses {name} but includes none of {headers:?}",
                                case.name,
                                spelling(standard),
                                file.path
                            ));
                        }
                        Some(_) => {}
                    }
                }
            }
        }
    }
    finish(&failures);
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
/// each into a folder named after the case, and compiles them all with one
/// `g++ -c`.
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
    let command = compile_only(toolchain, standard, &folder, &sources)?;
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

/// `g++ -c` over `sources` (relative to `folder`, its working directory,
/// where the object files go: several are named `main.o`, and each
/// overwrites the last, which is all the same since only the messages count)
/// with the compile flags a debug build of a `standard` project gets at the
/// strict warning level with warnings as errors, and plain-text messages.
fn compile_only(
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
    args.push("-c".into());
    args.extend(sources.iter().map(|source| source.clone().into_os_string()));
    Ok(CompilerCommand {
        program: toolchain.path().to_path_buf(),
        args,
        format: DiagnosticsFormat::Plain,
        sarif_files: Vec::new(),
    })
}
