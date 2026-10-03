//! Code generation: the Semantic AST to readable, deterministic C++ with
//! source maps (`docs/spec/06-compiler-pipeline.md` §6.7–6.11).
//!
//! CONTRACT (implemented in milestone M1):
//! * `generate` never panics on any `Program`. It is meant for programs whose
//!   analysis reported no errors; for others it still produces best-effort
//!   output (e.g. `0 /* error */` for expressions of `Type::Error`).
//! * All user text reaches the output only through `b2c_ir::text` encoders;
//!   output is re-printed from the AST, never spliced from input strings.
//! * Deterministic: identical input and options give byte-identical output.
//! * Output files: `<module>.cpp` per module; support helpers inline in the
//!   single-module case (spec §3.9). Every file ends with one newline and has
//!   no trailing whitespace.
//!
//! # Pipeline
//!
//! 1. **Desugar** (`lower`): each module's SAST becomes a private C++ AST.
//!    Language-level constructs (`repeat`, `print`, `ask`, `join`, …) turn
//!    into concrete C++, parentheses are decided by C++ precedence, and the
//!    headers and support helpers in use are recorded.
//! 2. **Emit** (`emit`): the C++ AST is printed through the token writer
//!    (`printer`), which is the only code that appends text. Its methods take
//!    typed text leaves (`Ident`, `StrLit`, `CharLit`, `NumLit`, `Comment`) or
//!    `&'static str` text chosen by the generator, never a runtime string. The
//!    printer tracks line and column to build the source map.
//!
//! # Semantics worth knowing
//!
//! * `repeat (n) times` uses a counter named `i`, `j`, `k`, then `i2`, `i3`, …,
//!   choosing the first name that nothing visible, nothing in the count and
//!   nothing in the body uses.
//! * Loop bounds (`repeat` counts, `for` ends and steps) that might change
//!   while the loop runs (they call a function, or read a variable the body
//!   changes) are evaluated once before the loop, in an extra declarator:
//!   `for (int i = 0, n = b2c::random_int(1, 6); i < n; ++i)`.
//! * `repeat until <c>` becomes `while (!(c))`, written as the inverse
//!   comparison where that is exact (`a != b` for `a == b`; not for ordering
//!   comparisons of `double`, because of NaN).

mod cast;
mod emit;
mod helpers;
mod lower;
mod printer;

use std::collections::BTreeSet;

use b2c_ir::sast::{Module, Program};
use b2c_ir::source_map::{FileKind, FileMap, GeneratedFile, GeneratedProject, SourceMap};

/// The source-map format version this crate writes.
const SOURCE_MAP_VERSION: u32 = 1;

/// Where support helpers (`b2c::random_int`, `b2c::ask`, …) are emitted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HelperPlacement {
    /// In a delimited section at the top of the file that uses them.
    #[default]
    Inline,
    /// In a shared `b2c_support.hpp` header.
    Header,
}

/// Options for code generation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodegenOptions {
    /// Project name, shown in the file header comment.
    pub project_name: String,
    /// App version, shown in the file header comment.
    pub app_version: String,
    /// Add the "Edit the blocks, not this file" line (omitted for export).
    pub do_not_edit_banner: bool,
    /// Spaces per indentation level.
    pub indent_width: u8,
    /// Where support helpers go.
    pub helper_placement: HelperPlacement,
}

impl Default for CodegenOptions {
    fn default() -> Self {
        Self {
            project_name: String::from("Untitled"),
            app_version: String::from(env!("CARGO_PKG_VERSION")),
            do_not_edit_banner: true,
            indent_width: 4,
            helper_placement: HelperPlacement::Inline,
        }
    }
}

/// Generates C++ files and the source map for a program.
///
/// Files come in project module order (`<module>.cpp`), followed by
/// `b2c_support.hpp` when helpers are placed in a header and any module uses
/// one. The source map has one entry per file, in the same order.
pub fn generate(program: &Program, options: &CodegenOptions) -> GeneratedProject {
    generate_with_report(program, options).project
}

/// What [`generate_with_report`] produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Generation {
    /// The generated files and source map.
    pub project: GeneratedProject,
    /// How many error placeholders (`0 /* error */`, `/* error */;`, or an
    /// `int /* error */` type) the output contains. It is zero for every
    /// program the analyser accepts; anything else means part of the program
    /// could not be generated (a bug in Blocks2Cpp, or input past the
    /// generator's nesting limit), so the output must not be compiled.
    pub placeholders: usize,
}

/// Generates C++ like [`generate`] and also reports how many error
/// placeholders were needed. Build tools use this to refuse output that
/// would compile but not do what the blocks say.
pub fn generate_with_report(program: &Program, options: &CodegenOptions) -> Generation {
    let mut placeholders = 0_usize;
    let mut files = Vec::new();
    let mut maps = Vec::new();
    let mut used_paths = BTreeSet::new();
    let mut helpers_used = BTreeSet::new();
    for module in &program.modules {
        let lowered = lower::lower_module(program, module);
        helpers_used.extend(lowered.usage.helpers.iter().copied());
        let path = source_path(module, &mut used_paths);
        let source = emit::source_file(&lowered, &module.name, options);
        placeholders = placeholders.saturating_add(source.placeholders);
        maps.push(FileMap {
            path: path.clone(),
            ranges: source.ranges,
        });
        files.push(GeneratedFile {
            path,
            kind: FileKind::Source,
            contents: source.contents,
        });
    }
    if options.helper_placement == HelperPlacement::Header && !helpers_used.is_empty() {
        let path = String::from(emit::SUPPORT_HEADER);
        let contents = emit::support_header(&helpers_used, options);
        maps.push(FileMap {
            path: path.clone(),
            ranges: Vec::new(),
        });
        files.push(GeneratedFile {
            path,
            kind: FileKind::Header,
            contents,
        });
    }
    Generation {
        project: GeneratedProject {
            files,
            source_map: SourceMap {
                version: SOURCE_MAP_VERSION,
                files: maps,
            },
        },
        placeholders,
    }
}

/// Whether a module name is a valid file stem (`[a-z][a-z0-9_-]{0,63}`).
fn is_file_stem(name: &str) -> bool {
    let mut bytes = name.bytes();
    name.len() <= 64
        && bytes.next().is_some_and(|b| b.is_ascii_lowercase())
        && bytes.all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
}

/// The source file path for a module: `<name>.cpp`. The loader validates
/// module names already; if a name is not a valid file stem anyway, or two
/// modules would share a file (names are unique case-insensitively), the
/// module ID is used instead, and a number is added as a last resort.
fn source_path(module: &Module, used: &mut BTreeSet<String>) -> String {
    let mut stem = if is_file_stem(&module.name) {
        module.name.clone()
    } else {
        module.id.as_str().to_owned()
    };
    if used.contains(&stem.to_ascii_lowercase()) {
        stem = format!("{}_{}", stem, module.id.as_str());
    }
    let mut candidate = stem.clone();
    let mut counter = 2_usize;
    while used.contains(&candidate.to_ascii_lowercase()) || candidate.eq_ignore_ascii_case("b2c_support") {
        candidate = format!("{stem}_{counter}");
        counter += 1;
    }
    used.insert(candidate.to_ascii_lowercase());
    format!("{candidate}.cpp")
}

#[cfg(test)]
mod tests {
    use b2c_ir::ids::ModuleId;
    use b2c_ir::sast::{CppStandard, SymbolTable};

    use super::*;

    fn module(id: &str, name: &str) -> Module {
        Module {
            id: ModuleId::new(id).unwrap(),
            name: name.to_owned(),
            items: Vec::new(),
        }
    }

    #[test]
    fn file_stems() {
        for good in ["main", "a", "player_2", "my-game"] {
            assert!(is_file_stem(good), "{good}");
        }
        for bad in [
            "",
            "Main",
            "2d",
            "../x",
            "a/b",
            "a.b",
            "a b",
            "con\u{0}",
            &"a".repeat(65),
        ] {
            assert!(!is_file_stem(bad), "{bad}");
        }
    }

    #[test]
    fn source_paths_are_safe_and_unique() {
        let mut used = BTreeSet::new();
        assert_eq!(source_path(&module("mod_main", "main"), &mut used), "main.cpp");
        assert_eq!(
            source_path(&module("mod_evil", "../../etc/passwd"), &mut used),
            "mod_evil.cpp"
        );
        assert_eq!(
            source_path(&module("mod_dup", "main"), &mut used),
            "main_mod_dup.cpp"
        );
        assert_eq!(source_path(&module("Main", "\n"), &mut used), "Main_Main.cpp");
        assert_eq!(
            source_path(&module("x", "b2c_support"), &mut used),
            "b2c_support_2.cpp"
        );
    }

    #[test]
    fn empty_program() {
        let program = Program {
            standard: CppStandard::default(),
            modules: Vec::new(),
            symbols: SymbolTable::default(),
        };
        let project = generate(&program, &CodegenOptions::default());
        assert!(project.files.is_empty());
        assert_eq!(project.source_map.version, 1);
    }
}
