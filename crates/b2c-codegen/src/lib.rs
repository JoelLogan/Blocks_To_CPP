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
//! * Output files: `<stem>.cpp` per module and `<stem>.hpp` for a module that
//!   shares something (the program's layout, ADR-0019); support helpers
//!   inline in the single-module case (spec §3.9), in `b2c_support.hpp`
//!   otherwise. Every file ends with one newline and has no trailing
//!   whitespace.
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
//!   comparisons of `double`, because of NaN, nor of library and user types,
//!   which may hold one).
//!
//! # Types and includes (`docs/plans/m3-contracts/types.md` §5)
//!
//! * Types are printed from closed tables and validated identifiers:
//!   standard names in full (`std::map<std::string, std::vector<int>>`;
//!   06 §6.14.5 shortens them only inside a `using namespace` region, which
//!   no project has yet), user structs and enums by their qualified name.
//! * A read-only parameter is `const T&` for every type that is not a
//!   scalar: `std::string`, the library types and structs. Numbers,
//!   characters, `bool` and enums are copied.
//! * Each file includes exactly the standard headers its own code needs: those
//!   of the types it prints, of the values it uses through their type (a
//!   returned `std::string`), of the forms it chooses (`<cstdlib>` for
//!   `std::exit`, `<cstddef>` for an index cast, …), of the helpers it
//!   inlines, and the catalog's headers of the blocks lowered into each item
//!   ([`b2c_ir::headers::ItemHeaders`]). They are recorded per item part, the
//!   declaration apart from the definition, for ADR-0019's headers.
//! * The forms that the collection and definition blocks of milestone M3's
//!   second wave lower to are ready in the C++ AST and the emitter: `c.at(i)`
//!   with `static_cast<std::size_t>(i)` for a position that is not a literal,
//!   `static_cast<int>(c.size())`, range-`for` with a structured binding for
//!   a map, and structs and enums with the operators their ticks and uses
//!   promise (a defaulted `<=>` in C++20, six hidden friends on `std::tie` in
//!   C++17, a hidden-friend `operator<<`, an enum's `operator<<` and
//!   `to_string`).

mod cast;
mod emit;
mod helpers;
mod lower;
mod printer;

use std::collections::{BTreeMap, BTreeSet};

use b2c_ir::ids::{ModuleId, SymbolId};
use b2c_ir::layout::Layout;
use b2c_ir::sast::{ItemKind, Program, SymbolKind};
use b2c_ir::source_map::{FileKind, FileMap, GeneratedFile, GeneratedProject, SourceMap};
use b2c_ir::text::{FileStem, Ident};

/// The source-map format version this crate writes.
const SOURCE_MAP_VERSION: u32 = 1;

/// Where support helpers (`b2c::random_int`, `b2c::ask`, …) are emitted.
///
/// Ignored since milestone M3's third wave: the module count decides
/// (inline with one module, `b2c_support.hpp` from two on, for the build
/// and the preview alike; w3-modules.md §1.2). The field stays until the
/// callers that set it drop it (w3-modules.md §3.3).
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
    /// Where support helpers go: ignored, see [`HelperPlacement`].
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
/// Files come in project module order: each module's `<stem>.cpp`, then its
/// `<stem>.hpp` when it shares something, named by the layout's
/// [`FileStem`]s (`b2c_ir::layout::ModuleFiles::stem`); last
/// `b2c_support.hpp` when the project has several modules and any module
/// uses a helper. The source map has one entry per file, in the same order;
/// each file and map names its module (none for `b2c_support.hpp`).
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
    let flat;
    let layout = if layout_fits(&program.layout, program) {
        &program.layout
    } else {
        // A program built without the analyser: M2's layout.
        flat = Layout::flat(&program.modules, &program.symbols);
        &flat
    };
    let multi_module = program.modules.len() > 1;
    let stems: BTreeMap<&ModuleId, &FileStem> = layout
        .modules
        .iter()
        .map(|files| (&files.module, &files.stem))
        .collect();
    let stems_of = |modules: &[ModuleId]| -> Vec<&FileStem> {
        let mut listed: Vec<&FileStem> = modules.iter().filter_map(|id| stems.get(id).copied()).collect();
        listed.sort();
        listed.dedup();
        listed
    };
    let mut placeholders = 0_usize;
    let mut files = Vec::new();
    let mut maps = Vec::new();
    let mut helpers_used = BTreeSet::new();
    let mut push = |path: String, kind: FileKind, module: Option<&ModuleId>, file: emit::SourceFile| {
        placeholders = placeholders.saturating_add(file.placeholders);
        maps.push(FileMap {
            path: path.clone(),
            module: module.cloned(),
            ranges: file.ranges,
        });
        files.push(GeneratedFile {
            path,
            kind,
            module: module.cloned(),
            contents: file.contents,
        });
    };
    for (module, module_files) in program.modules.iter().zip(&layout.modules) {
        let lowered = lower::lower_module(program, module, Some(module_files), multi_module);
        helpers_used.extend(lowered.usage.helpers.iter().copied());
        let plan = emit::FilePlan {
            module_name: &module.name,
            multi_module,
            source_includes: stems_of(&module_files.source_includes),
            header_includes: stems_of(&module_files.header_includes),
            header_order: if module_files.header {
                &module_files.header_order
            } else {
                &[]
            },
            forward: forward_declarations(program, &module_files.forward),
        };
        let stem = module_files.stem.as_str();
        let source = emit::source_file(&lowered, &plan, options);
        push(format!("{stem}.cpp"), FileKind::Source, Some(&module.id), source);
        if module_files.header {
            let header = emit::header_file(&lowered, &plan, options);
            push(format!("{stem}.hpp"), FileKind::Header, Some(&module.id), header);
        }
    }
    if multi_module && !helpers_used.is_empty() {
        let contents = emit::support_header(&helpers_used, options);
        push(
            String::from(emit::SUPPORT_HEADER),
            FileKind::Header,
            None,
            emit::SourceFile {
                contents,
                ranges: Vec::new(),
                placeholders: 0,
            },
        );
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

/// Whether a layout has one entry per module of the program, in order.
fn layout_fits(layout: &Layout, program: &Program) -> bool {
    layout.modules.len() == program.modules.len()
        && layout
            .modules
            .iter()
            .zip(&program.modules)
            .all(|(files, module)| files.module == module.id)
}

/// The forward declarations of a header (`ModuleFiles::forward`): for each
/// struct or enum, its user namespaces, its keyword and its name. A symbol
/// that is not a struct or enum is left out.
fn forward_declarations(
    program: &Program,
    forward: &[SymbolId],
) -> Vec<(lower::Region, &'static str, Ident)> {
    forward
        .iter()
        .filter_map(|id| {
            let symbol = program.symbols.get(id)?;
            let keyword = match symbol.kind {
                SymbolKind::Struct { .. } => "struct",
                SymbolKind::Enum { .. } => "enum class",
                _ => return None,
            };
            let item = program
                .modules
                .iter()
                .flat_map(|module| &module.items)
                .find(|item| {
                    matches!(&item.kind, ItemKind::Struct(def) if def.symbol == *id)
                        || matches!(&item.kind, ItemKind::Enum(def) if def.symbol == *id)
                });
            let region = item.map_or_else(lower::Region::default, |item| lower::Region {
                anonymous: false,
                ..lower::Region::of(item, &program.symbols, false)
            });
            Some((region, keyword, symbol.name.clone()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use b2c_ir::ids::ModuleId;
    use b2c_ir::sast::{CppStandard, Module, SymbolTable};

    use super::*;

    fn module(id: &str, name: &str) -> Module {
        Module {
            id: ModuleId::new(id).unwrap(),
            name: name.to_owned(),
            items: Vec::new(),
        }
    }

    fn program(modules: Vec<Module>) -> Program {
        Program {
            standard: CppStandard::default(),
            modules,
            symbols: SymbolTable::default(),
            layout: Layout::default(),
            spellings: b2c_ir::spelling::Spellings::default(),
        }
    }

    /// File names come from the layout's stems, never from the module name
    /// as written: a name the stem refuses, or one an earlier module has,
    /// gets `module<N>`. Each file and its map name their module.
    #[test]
    fn files_are_named_by_the_layout() {
        let mut program = program(vec![
            module("mod_main", "main"),
            module("mod_evil", "../../etc/passwd"),
            module("mod_dup", "main"),
            module("mod_b2c", "b2c_support"),
            module("mod_tools", "tools"),
        ]);
        let paths = |project: &GeneratedProject| -> Vec<String> {
            project.files.iter().map(|file| file.path.clone()).collect()
        };
        // Without a layout that fits, M2's flat one.
        let project = generate(&program, &CodegenOptions::default());
        let expected = [
            "main.cpp",
            "module2.cpp",
            "module3.cpp",
            "module4.cpp",
            "tools.cpp",
        ];
        assert_eq!(paths(&project), expected);
        for (file, map) in project.files.iter().zip(&project.source_map.files) {
            assert_eq!(file.path, map.path);
            assert!(file.module.is_some());
            assert_eq!(file.module, map.module);
            assert_eq!(file.kind, FileKind::Source);
        }
        // The analyser's layout when it fits: its stems are used as given.
        program.layout = Layout::flat(&program.modules, &program.symbols);
        program.layout.modules[4].stem = FileStem::new("utils").unwrap();
        assert_eq!(
            paths(&generate(&program, &CodegenOptions::default()))[4],
            "utils.cpp"
        );
        // A layout for other modules is not used.
        program.modules.swap(0, 4);
        assert_eq!(
            paths(&generate(&program, &CodegenOptions::default()))[0],
            "tools.cpp"
        );
    }

    #[test]
    fn empty_program() {
        let project = generate(&program(Vec::new()), &CodegenOptions::default());
        assert!(project.files.is_empty());
        assert_eq!(project.source_map.version, 1);
    }
}
