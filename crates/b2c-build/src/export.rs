//! Export as a C++ project (`docs/spec/06-compiler-pipeline.md` §6.11;
//! `docs/plans/m3-contracts/w3-export-run.md` §4).
//!
//! ```text
//! <folder>/
//! ├── src/              the generated .cpp/.hpp, without the banner
//! ├── CMakeLists.txt    CMake 3.20 or newer
//! ├── Makefile          g++ and GNU make (<program>-<config>[-nosanitize][-noharden])
//! ├── README.md         how to build it
//! └── .b2c-export.json  the project's ID and the files this export wrote
//! ```
//!
//! * [`prepare`] runs the front end with the export's options and makes
//!   every file's text: the generated C++ is what the app compiles, minus
//!   the IDE init unit (which is never part of a project). The flags come
//!   from the lists the app's command lines are built from
//!   (`b2c_toolchain::command`): the standard, the UTF-8 character sets,
//!   each configuration's warnings, optimisation, debug information and
//!   assertions, and the project's defines (each checked as for a build,
//!   `B2C-T1019`). What the app probes a compiler for (sanitizers,
//!   hardening) CMake checks with `check_cxx_compiler_flag` and
//!   `check_linker_flag`; the Makefile turns it on and lets `SANITIZE=0` and
//!   `HARDEN=0` turn it off. Machine-only flags (extra flags, library
//!   profiles, the diagnostics format, `-iquote`, `-pipe`) are never
//!   written.
//! * [`write()`] writes the files into a folder: a new one (missing or empty),
//!   or, with [`ExportMode::Replace`], one that holds an earlier export of
//!   the same project, where only the files its `.b2c-export.json` lists are
//!   replaced or removed. It never follows a link (it checks the folders
//!   before it removes or writes anything), writes each file through a
//!   temporary file and a rename, and writes the manifest last.

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

use b2c_ir::sast::CppStandard;
use b2c_ir::text::is_device_name;
use b2c_ir::{DiagSource, Diagnostic, Location};
use b2c_model::{BuildConfiguration, Define, DefineValue, Document, Sanitizer};
use b2c_toolchain::codes;
use b2c_toolchain::command::{ENCODING_FLAGS, is_debug_like, optimisation_flags, warning_flags};
use b2c_toolchain::flags::ValidDefine;
use serde::{Deserialize, Serialize};

use crate::frontend::{FrontendOptions, run_frontend};

/// The manifest an export writes last.
pub const EXPORT_MANIFEST: &str = ".b2c-export.json";

/// The manifest's `format` tag.
const MANIFEST_FORMAT: &str = "blocks2cpp/export";

/// The largest manifest read back (it lists a few file names).
const MAX_MANIFEST_BYTES: u64 = 64 * 1024;

/// The manifest's `formatVersion`: 2 names the project; 1 (read only) did
/// not.
const MANIFEST_VERSION: u32 = 2;

/// The folder of the generated sources.
const SOURCE_DIR: &str = "src";

/// The CMake target. CMake reserves some names a program may have (`install`,
/// `test`, `all`), so the target has its own and `OUTPUT_NAME` names the
/// program.
const CMAKE_TARGET: &str = "b2c_program";

/// The longest folder name made from the project's name.
const MAX_FOLDER_NAME: usize = 64;

/// One file of an export: its path in the export folder (`/`-separated) and
/// its text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportFile {
    /// For example `src/main.cpp` or `CMakeLists.txt`.
    pub path: String,
    /// The file's text (UTF-8, `\n` line endings).
    pub contents: String,
}

/// An export, ready to be written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Export {
    /// The program's name (from the first module's name, as the app's builds
    /// name the program).
    pub name: String,
    /// The suggested folder name: the project's name made a plain file name
    /// ([`folder_name`]), or the program's name.
    pub folder_name: String,
    /// The project's ID, which the manifest records so that a replacement
    /// is only ever of the same project's export.
    pub project_id: String,
    /// The files, the manifest not included.
    pub files: Vec<ExportFile>,
}

/// Why a project cannot be exported.
#[derive(Debug, thiserror::Error)]
pub enum ExportError {
    /// The project has errors (or a define that cannot be used); nothing is
    /// written.
    #[error("the project has errors")]
    ProjectErrors(Vec<Diagnostic>),
    /// A text define holds `#`, `;` or `$`, which a build takes but the two
    /// build files cannot pass (one `B2C-T1019` error each); nothing is
    /// written.
    #[error("a define cannot be exported")]
    DefineNotExportable(Vec<Diagnostic>),
    /// The folder is not empty and holds no earlier export (or one that
    /// cannot be read), so nothing is written.
    #[error("the folder is not empty and holds no earlier export")]
    NotEmpty,
    /// [`ExportMode::Replace`] was asked for a folder without an earlier
    /// export.
    #[error("the folder holds no earlier export")]
    NoEarlierExport,
    /// [`ExportMode::Replace`] would write over a file (the path given) that
    /// the earlier export did not write; nothing is written.
    #[error("the folder holds {0}, which the earlier export did not write")]
    NotWrittenByExport(String),
    /// [`ExportMode::Replace`] was asked for a folder that holds the export
    /// of another project; nothing is written.
    #[error("the folder holds the export of another project")]
    OtherProject,
    /// The folder, or a file or folder in it, is a link or not what it
    /// should be; nothing more is written.
    #[error("refusing to write through a link or over something that is not a file")]
    NotAPlainFile,
    /// A file could not be written.
    #[error("cannot {action}: {source}")]
    Io {
        /// What failed.
        action: &'static str,
        /// The underlying error.
        source: io::Error,
    },
}

/// Whether [`write()`] makes a new export or replaces an earlier one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportMode {
    /// The folder must be missing or empty.
    New,
    /// The folder must hold an earlier export.
    Replace,
}

/// What a folder holds, for an export.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FolderState {
    /// Nothing is there, or an empty folder.
    Empty,
    /// An earlier export (a readable manifest).
    EarlierExport,
    /// Something else.
    Other,
}

/// The manifest's shape.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Manifest {
    format: String,
    format_version: u32,
    /// The project's ID (format 2; format 1 did not record it).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    project_id: Option<String>,
    files: Vec<String>,
}

/// Runs the front end on the project file `bytes` with the export's options
/// (no "do not edit" banner) and makes the export's files.
///
/// # Errors
/// [`ExportError::ProjectErrors`] with every problem when the project has
/// errors or a define that cannot be used, and
/// [`ExportError::DefineNotExportable`] for a define only the export cannot
/// use.
pub fn prepare(bytes: &[u8], indent_width: u8, app_version: &str) -> Result<Export, ExportError> {
    let options = FrontendOptions {
        do_not_edit_banner: false,
        indent_width,
        ..FrontendOptions::default()
    };
    let frontend = run_frontend(bytes, &options);
    let (Some(document), Some(generated), false) = (
        frontend.document.as_ref(),
        frontend.generated.as_ref(),
        frontend.has_errors(),
    ) else {
        return Err(ExportError::ProjectErrors(frontend.diagnostics));
    };
    let mut problems = Vec::new();
    let mut not_exportable = Vec::new();
    let defines: Vec<ValidDefine> = document
        .project
        .build
        .defines
        .iter()
        .filter_map(|define| {
            ValidDefine::new(define)
                .map_err(|problem| problems.push(problem))
                .ok()
                .filter(|_| {
                    exportable(define)
                        .map_err(|problem| not_exportable.push(problem))
                        .is_ok()
                })
        })
        .collect();
    if !problems.is_empty() {
        return Err(ExportError::ProjectErrors(problems));
    }
    if !not_exportable.is_empty() {
        return Err(ExportError::DefineNotExportable(not_exportable));
    }
    let name = crate::compile::program_name(document);
    let sources: Vec<String> = generated
        .files
        .iter()
        .filter(|file| file.kind == b2c_ir::source_map::FileKind::Source)
        .map(|file| format!("{SOURCE_DIR}/{}", file.path))
        .collect();
    let mut files: Vec<ExportFile> = generated
        .files
        .iter()
        .map(|file| ExportFile {
            path: format!("{SOURCE_DIR}/{}", file.path),
            contents: file.contents.clone(),
        })
        .collect();
    let defines: Vec<String> = defines
        .iter()
        .map(|define| define.argument().trim_start_matches("-D").to_owned())
        .collect();
    let flags = Flags::of(document, &defines);
    files.push(ExportFile {
        path: String::from("CMakeLists.txt"),
        contents: cmake_lists(&name, &sources, &flags, app_version),
    });
    files.push(ExportFile {
        path: String::from("Makefile"),
        contents: makefile(&name, &sources, &flags, app_version),
    });
    files.push(ExportFile {
        path: String::from("README.md"),
        contents: readme(&document.project.name, &name, app_version),
    });
    Ok(Export {
        folder_name: folder_name(&document.project.name).unwrap_or_else(|| name.clone()),
        name,
        project_id: document.project.id.to_string(),
        files,
    })
}

/// `project_name` as a folder name: lower-case ASCII letters and digits, with
/// one `-` for each run of anything else, at most 64 characters; `None` when
/// nothing is left or it is a Windows device name (`con`, `com1`, …).
///
/// ```
/// use b2c_build::export::folder_name;
///
/// assert_eq!(folder_name("My Game 2!").as_deref(), Some("my-game-2"));
/// assert_eq!(folder_name("¿¿¿"), None);
/// assert_eq!(folder_name("Con"), None);
/// ```
pub fn folder_name(project_name: &str) -> Option<String> {
    let mut name = String::new();
    for c in project_name.chars() {
        if c.is_ascii_alphanumeric() {
            name.push(c.to_ascii_lowercase());
        } else if !name.is_empty() && !name.ends_with('-') {
            name.push('-');
        }
    }
    name.truncate(MAX_FOLDER_NAME);
    let name = name.trim_end_matches('-');
    (!name.is_empty() && !is_device_name(name)).then(|| name.to_owned())
}

/// What both build files need.
struct Flags<'a> {
    standard: CppStandard,
    gnu: bool,
    defines: &'a [String],
    debug: &'a BuildConfiguration,
    release: &'a BuildConfiguration,
}

impl<'a> Flags<'a> {
    fn of(document: &'a Document, defines: &'a [String]) -> Self {
        let configurations = &document.project.build.configurations;
        Self {
            standard: document.project.language.standard,
            gnu: document.project.language.gnu_extensions,
            defines,
            debug: &configurations.debug,
            release: &configurations.release,
        }
    }

    fn configurations(&self) -> [(&'static str, &'a BuildConfiguration); 2] {
        [("Debug", self.debug), ("Release", self.release)]
    }
}

/// The standard's number (`20`).
fn standard_number(standard: CppStandard) -> &'static str {
    match standard {
        CppStandard::Cpp17 => "17",
        CppStandard::Cpp20 => "20",
        CppStandard::Cpp23 => "23",
        CppStandard::Cpp26 => "26",
    }
}

/// The `-std=` value every supported g++ (11 to 15) accepts.
fn std_flag(standard: CppStandard, gnu: bool) -> String {
    let version = match standard {
        // C++26 needs GCC 14 or newer, which all take c++2c (15 also c++26).
        CppStandard::Cpp26 => "2c",
        other => standard_number(other),
    };
    format!("-std={}++{version}", if gnu { "gnu" } else { "c" })
}

/// The sanitizer flags of `config` on Linux, and on Windows (MinGW: no
/// AddressSanitizer, the undefined-behaviour checker traps), as the app's
/// builds use them (07 §7.4.3).
fn sanitizer_flags(config: &BuildConfiguration) -> (Vec<&'static str>, Vec<&'static str>) {
    let address = config.sanitizers.contains(&Sanitizer::Address);
    let undefined = config.sanitizers.contains(&Sanitizer::Undefined);
    let linux = match (address, undefined) {
        (true, true) => vec!["-fsanitize=address,undefined", "-fno-sanitize-recover=undefined"],
        (true, false) => vec!["-fsanitize=address"],
        (false, true) => vec!["-fsanitize=undefined", "-fno-sanitize-recover=undefined"],
        (false, false) => Vec::new(),
    };
    let windows = if undefined {
        vec!["-fsanitize=undefined", "-fsanitize-undefined-trap-on-error"]
    } else {
        Vec::new()
    };
    (linux, windows)
}

/// The hardening flags of `config` on Linux: for compiling only, for both
/// compiling and linking, and for linking only (07 §7.4.3, without
/// `-fhardened`, which only GCC 14 has; these are what it stands for).
fn linux_hardening(config: &BuildConfiguration) -> (Vec<&'static str>, Vec<&'static str>, Vec<&'static str>) {
    if !config.hardening {
        return (Vec::new(), Vec::new(), Vec::new());
    }
    let fortify = !is_debug_like(config) && !config.sanitizers.contains(&Sanitizer::Address);
    let mut compile = Vec::new();
    if fortify {
        compile.extend([
            "-U_FORTIFY_SOURCE",
            "-D_FORTIFY_SOURCE=2",
            "-D_GLIBCXX_ASSERTIONS",
        ]);
    }
    compile.push("-fPIE");
    (
        compile,
        vec![
            "-fstack-protector-strong",
            "-fstack-clash-protection",
            "-fcf-protection",
        ],
        vec!["-pie", "-Wl,-z,relro,-z,now", "-Wl,-z,noexecstack"],
    )
}

/// The hardening flags of `config` on Windows: for compiling and linking,
/// and for linking only.
fn windows_hardening(config: &BuildConfiguration) -> (Vec<&'static str>, Vec<&'static str>) {
    if config.hardening {
        (
            vec!["-fstack-protector-strong"],
            vec!["-Wl,--dynamicbase,--nxcompat,--high-entropy-va"],
        )
    } else {
        (Vec::new(), Vec::new())
    }
}

/// The characters a text define may not hold in an export: CMake reads `;`
/// as a list separator and `$<` as a generator expression in a define, its
/// Makefile generators cannot pass `#`, and make reads `$` and `#` itself.
const NOT_EXPORTABLE: [char; 3] = ['#', ';', '$'];

/// `Ok` when `define` can be written into both build files unchanged, else a
/// `B2C-T1019` error (a build passes such a define as one argument; only the
/// two build tools cannot).
fn exportable(define: &Define) -> Result<(), Diagnostic> {
    match &define.value {
        DefineValue::String(text) if text.contains(NOT_EXPORTABLE) => Err(Diagnostic::error(
            codes::BAD_DEFINE,
            DiagSource::Toolchain,
            Location::project(),
            format!(
                "The value of the define `{}` cannot be exported: CMake and make cannot pass `#`, `;` or `$` in a define. Put the text in a constant instead.",
                define.name
            ),
        )),
        _ => Ok(()),
    }
}

/// `text` as a CMake bracket argument, which takes everything literally.
fn cmake_literal(text: &str) -> String {
    let mut equals = String::new();
    while text.contains(&format!("]{equals}]")) {
        equals.push('=');
    }
    format!("[{equals}[{text}]{equals}]")
}

/// The project, the standard and the target with its sources, name,
/// standard flag and defines.
fn cmake_target(text: &mut String, name: &str, sources: &[String], flags: &Flags<'_>) {
    text.push_str("cmake_minimum_required(VERSION 3.20)\n");
    let _ = writeln!(text, "project({name} LANGUAGES CXX)\n");
    // CMake before 3.25 knows no C++26, and later ones know no flag for it
    // with GCC 13 and older: the flag is given below instead.
    if flags.standard != CppStandard::Cpp26 {
        let _ = writeln!(
            text,
            "set(CMAKE_CXX_STANDARD {})",
            standard_number(flags.standard)
        );
        text.push_str("set(CMAKE_CXX_STANDARD_REQUIRED ON)\n");
        let _ = writeln!(
            text,
            "set(CMAKE_CXX_EXTENSIONS {})\n",
            if flags.gnu { "ON" } else { "OFF" }
        );
    }
    text.push_str(
        "if(NOT CMAKE_BUILD_TYPE AND NOT CMAKE_CONFIGURATION_TYPES)\n  set(CMAKE_BUILD_TYPE Debug CACHE STRING \"Debug or Release\" FORCE)\nendif()\n\n",
    );
    let _ = writeln!(text, "add_executable({CMAKE_TARGET}");
    for source in sources {
        let _ = writeln!(text, "  {source}");
    }
    text.push_str(")\n");
    let _ = writeln!(
        text,
        "set_target_properties({CMAKE_TARGET} PROPERTIES OUTPUT_NAME {name})"
    );
    if flags.standard == CppStandard::Cpp26 {
        let _ = writeln!(
            text,
            "if(MSVC)\n  target_compile_options({CMAKE_TARGET} PRIVATE /std:c++latest)\nelse()\n  target_compile_options({CMAKE_TARGET} PRIVATE {})\nendif()",
            std_flag(flags.standard, flags.gnu)
        );
    }
    if !flags.defines.is_empty() {
        let _ = writeln!(text, "target_compile_definitions({CMAKE_TARGET} PRIVATE");
        for define in flags.defines {
            let _ = writeln!(text, "  {}", cmake_literal(define));
        }
        text.push_str(")\n");
    }
}

/// `CMakeLists.txt`.
fn cmake_lists(name: &str, sources: &[String], flags: &Flags<'_>, app_version: &str) -> String {
    let mut text = String::new();
    let _ = writeln!(
        text,
        "# Generated by Blocks2Cpp {app_version}. Build with:\n#   cmake -S . -B build\n#   cmake --build build\n# Release: cmake -S . -B build-release -DCMAKE_BUILD_TYPE=Release\n"
    );
    cmake_target(&mut text, name, sources, flags);
    text.push_str("\nif(MSVC)\n  # MSVC's default mode skips the two-phase lookup this code relies on.\n");
    let _ = writeln!(
        text,
        "  target_compile_options({CMAKE_TARGET} PRIVATE /utf-8 /permissive- /Zc:__cplusplus /W4)"
    );
    text.push_str("  return()\nendif()\n\n");
    text.push_str(
        "# The same flags as the Blocks2Cpp app: CMake's own per-configuration flags\n# are replaced by the project's.\nset(CMAKE_CXX_FLAGS_DEBUG \"\")\nset(CMAKE_CXX_FLAGS_RELEASE \"\")\n",
    );
    let _ = writeln!(
        text,
        "target_compile_options({CMAKE_TARGET} PRIVATE {})",
        ENCODING_FLAGS.join(" ")
    );
    for (config, settings) in flags.configurations() {
        let mut list = warning_flags(settings);
        list.extend(optimisation_flags(settings));
        let _ = writeln!(
            text,
            "target_compile_options({CMAKE_TARGET} PRIVATE \"$<$<CONFIG:{config}>:{}>\")",
            list.join(";")
        );
    }
    let _ = write!(
        text,
        r#"
# What the app checks each compiler for, CMake checks here.
include(CheckCXXCompilerFlag)
include(CheckLinkerFlag)
function(b2c_flags config where)
  foreach(flag IN LISTS ARGN)
    string(MAKE_C_IDENTIFIER "B2C_HAS_${{flag}}" ok)
    # The test program is linked too, so a sanitizer must reach the link.
    set(CMAKE_REQUIRED_LINK_OPTIONS "${{flag}}")
    check_cxx_compiler_flag("${{flag}}" ${{ok}}_COMPILE)
    unset(CMAKE_REQUIRED_LINK_OPTIONS)
    check_linker_flag(CXX "${{flag}}" ${{ok}}_LINK)
    if(${{ok}}_COMPILE AND ${{ok}}_LINK)
      if(NOT where STREQUAL "link")
        target_compile_options({CMAKE_TARGET} PRIVATE "$<$<CONFIG:${{config}}>:${{flag}}>")
      endif()
      if(NOT where STREQUAL "compile")
        target_link_options({CMAKE_TARGET} PRIVATE "$<$<CONFIG:${{config}}>:${{flag}}>")
      endif()
    endif()
  endforeach()
endfunction()
"#
    );
    text.push_str("\nif(WIN32)\n");
    let _ = writeln!(text, "  target_link_options({CMAKE_TARGET} PRIVATE -static)");
    for (config, settings) in flags.configurations() {
        let (_, sanitizers) = sanitizer_flags(settings);
        let (both, link) = windows_hardening(settings);
        cmake_calls(&mut text, config, "both", &sanitizers);
        cmake_calls(&mut text, config, "both", &both);
        cmake_calls(&mut text, config, "link", &link);
    }
    text.push_str("else()\n");
    for (config, settings) in flags.configurations() {
        let (sanitizers, _) = sanitizer_flags(settings);
        let (compile, both, link) = linux_hardening(settings);
        cmake_calls(&mut text, config, "both", &sanitizers);
        cmake_calls(&mut text, config, "compile", &compile);
        cmake_calls(&mut text, config, "both", &both);
        cmake_calls(&mut text, config, "link", &link);
    }
    text.push_str("endif()\n");
    text
}

/// One `b2c_flags` call, when there are flags.
fn cmake_calls(text: &mut String, config: &str, where_: &str, flags: &[&str]) {
    if !flags.is_empty() {
        let _ = writeln!(text, "  b2c_flags({config} {where_} {})", flags.join(" "));
    }
}

/// `text` for a Makefile variable that becomes one shell word: in single
/// quotes (a `'` inside is closed, escaped and reopened), with `$` doubled
/// for make and `#` escaped, so make never reads a comment.
fn make_shell_word(text: &str) -> String {
    let quoted = text.replace('\'', r"'\''").replace('$', "$$").replace('#', r"\#");
    format!("'{quoted}'")
}

/// `Makefile`.
fn makefile(name: &str, sources: &[String], flags: &Flags<'_>, app_version: &str) -> String {
    let mut text = String::new();
    let _ = writeln!(
        text,
        "# Generated by Blocks2Cpp {app_version}. Build with `make` (debug) or\n# `make CONFIG=release`; `make SANITIZE=0` builds without the sanitizers and\n# `make HARDEN=0` without the hardening flags, for a g++ that lacks them.\n# Each choice builds a program file of its own (<program>-debug,\n# <program>-release-nosanitize, …), so changing one builds again. The build\n# runs in cmd.exe too; defines and `make clean` need a POSIX shell (MSYS2).\n"
    );
    text.push_str("CXX ?= g++\nCONFIG ?= debug\nSANITIZE ?= 1\nHARDEN ?= 1\n\n");
    text.push_str("ifeq ($(OS),Windows_NT)\n  EXE := .exe\n  PLATFORM := windows\nelse\n  EXE :=\n  PLATFORM := linux\n  CF_PROTECTION := $(if $(filter x86_64 i%86,$(shell uname -m)),-fcf-protection)\nendif\n\n");
    let _ = writeln!(text, "PROGRAM := {name}");
    let _ = writeln!(text, "SOURCES := {}", sources.join(" "));
    text.push_str("HEADERS := $(wildcard src/*.hpp)\n\n");
    let _ = write!(
        text,
        "COMMON := {} {}",
        std_flag(flags.standard, flags.gnu),
        ENCODING_FLAGS.join(" ")
    );
    for define in flags.defines {
        let _ = write!(text, " {}", make_shell_word(&format!("-D{define}")));
    }
    text.push_str("\n\n");
    for (config, settings) in flags.configurations() {
        let upper = config.to_ascii_uppercase();
        let mut list = warning_flags(settings);
        list.extend(optimisation_flags(settings));
        let _ = writeln!(text, "{upper}_FLAGS := {}", list.join(" "));
        let (linux_sanitizers, windows_sanitizers) = sanitizer_flags(settings);
        let (compile, both, link) = linux_hardening(settings);
        let (windows_both, windows_link) = windows_hardening(settings);
        let _ = writeln!(text, "{upper}_SANITIZE_linux := {}", linux_sanitizers.join(" "));
        let _ = writeln!(
            text,
            "{upper}_SANITIZE_windows := {}",
            windows_sanitizers.join(" ")
        );
        // -fcf-protection exists only for x86 (CF_PROTECTION below).
        let portable: Vec<&str> = both
            .iter()
            .copied()
            .filter(|flag| *flag != "-fcf-protection")
            .collect();
        let cf = if both.contains(&"-fcf-protection") {
            " $(CF_PROTECTION)"
        } else {
            ""
        };
        let _ = writeln!(
            text,
            "{upper}_HARDEN_linux := {}{cf}",
            compile
                .iter()
                .chain(&portable)
                .copied()
                .collect::<Vec<_>>()
                .join(" ")
        );
        let _ = writeln!(
            text,
            "{upper}_HARDEN_LINK_linux := {}{cf}",
            portable
                .iter()
                .chain(&link)
                .copied()
                .collect::<Vec<_>>()
                .join(" ")
        );
        let _ = writeln!(text, "{upper}_HARDEN_windows := {}", windows_both.join(" "));
        let _ = writeln!(
            text,
            "{upper}_HARDEN_LINK_windows := {}\n",
            windows_both
                .iter()
                .chain(&windows_link)
                .copied()
                .collect::<Vec<_>>()
                .join(" ")
        );
    }
    text.push_str(
        r"ifeq ($(CONFIG),debug)
  CONFIG_NAME := DEBUG
else ifeq ($(CONFIG),release)
  CONFIG_NAME := RELEASE
else
  $(error CONFIG must be debug or release)
endif

ALL_CXXFLAGS := $(COMMON) $($(CONFIG_NAME)_FLAGS)
ALL_LDFLAGS :=
ifeq ($(SANITIZE),1)
  ALL_CXXFLAGS += $($(CONFIG_NAME)_SANITIZE_$(PLATFORM))
  ALL_LDFLAGS += $($(CONFIG_NAME)_SANITIZE_$(PLATFORM))
endif
ifeq ($(HARDEN),1)
  ALL_CXXFLAGS += $($(CONFIG_NAME)_HARDEN_$(PLATFORM))
  ALL_LDFLAGS += $($(CONFIG_NAME)_HARDEN_LINK_$(PLATFORM))
endif
ifeq ($(PLATFORM),windows)
  ALL_LDFLAGS += -static
endif

# The suffixes follow the same tests as the flags: any SANITIZE or HARDEN
# other than 1 turns them off, and names a file of its own.
OUT := $(PROGRAM)-$(CONFIG)$(if $(filter 1,$(SANITIZE)),,-nosanitize)$(if $(filter 1,$(HARDEN)),,-noharden)$(EXE)

$(OUT): $(SOURCES) $(HEADERS)
	$(CXX) $(ALL_CXXFLAGS) $(CXXFLAGS) $(SOURCES) -o $@ $(ALL_LDFLAGS) $(LDFLAGS)

",
    );
    // Every program file a choice can build, and nothing else.
    text.push_str("clean:\n\trm -f");
    for config in ["debug", "release"] {
        for sanitize in ["", "-nosanitize"] {
            for harden in ["", "-noharden"] {
                let _ = write!(text, " $(PROGRAM)-{config}{sanitize}{harden}$(EXE)");
            }
        }
    }
    text.push_str("\n\n.PHONY: clean\n");
    // An empty list leaves `NAME := ` with a trailing space.
    let mut tidy = String::with_capacity(text.len());
    for line in text.lines() {
        tidy.push_str(line.trim_end());
        tidy.push('\n');
    }
    tidy
}

/// `text` with Markdown's special characters escaped.
fn markdown_text(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for c in text.chars() {
        if "\\`*_{}[]<>()#+-.!|~&".contains(c) {
            escaped.push('\\');
        }
        escaped.push(c);
    }
    escaped
}

/// `README.md`.
fn readme(project_name: &str, name: &str, app_version: &str) -> String {
    format!(
        r"# {title}

This is the C++ of a Blocks2Cpp project, exported by Blocks2Cpp {app_version}.
It is ordinary C++ that builds without Blocks2Cpp, with the same flags the
app uses. `src/` holds the code; edit it as you like, but exporting the
project again replaces these files (other files in this folder are kept).

## Build with CMake (3.20 or newer)

```sh
cmake -S . -B build
cmake --build build
./build/{name}
```

For an optimised build: `cmake -S . -B build-release -DCMAKE_BUILD_TYPE=Release`.
CMake checks which sanitizers and hardening flags your compiler supports.

## Build with make and g++ (11 or newer)

```sh
make
./{name}-debug
```

`make CONFIG=release` builds the optimised program, `{name}-release` (on
Windows, `.exe` follows each name). Debug builds use the sanitizers (AddressSanitizer and UndefinedBehaviorSanitizer on Linux); if
your g++ lacks them, build with `make SANITIZE=0`, and with `make HARDEN=0`
if it lacks the hardening flags; those builds are `{name}-debug-nosanitize`
and the like, so a change of choice always builds again.

On Windows, run `make` in an MSYS2 shell, or with another `sh.exe` on
`PATH`. `mingw32-make` from cmd.exe or PowerShell builds a project without
defines, but a project with defines, and `make clean`, need a POSIX shell.
",
        title = markdown_text(project_name),
    )
}

/// What `folder` holds (see [`FolderState`]). A link, or anything that is
/// not a folder, is [`FolderState::Other`].
pub fn folder_state(folder: &Path) -> FolderState {
    match fs::symlink_metadata(folder) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => FolderState::Empty,
        Ok(metadata) if metadata.is_dir() && !is_link(&metadata) => {
            if fs::read_dir(folder).is_ok_and(|mut entries| entries.next().is_none()) {
                FolderState::Empty
            } else if read_manifest(folder).is_some() {
                FolderState::EarlierExport
            } else {
                FolderState::Other
            }
        }
        _ => FolderState::Other,
    }
}

/// Writes `export` into `folder` (see the module documentation). Returns
/// the paths written, the manifest last.
///
/// # Errors
/// [`ExportError::NotEmpty`] for [`ExportMode::New`] and a folder that is
/// not empty, [`ExportError::NoEarlierExport`] for [`ExportMode::Replace`]
/// without one, [`ExportError::OtherProject`] when it is another project's,
/// [`ExportError::NotWrittenByExport`] for a file in the way that the earlier
/// export did not write, [`ExportError::NotAPlainFile`] for a link (the folder or
/// anything to be written or removed), and [`ExportError::Io`].
pub fn write(folder: &Path, export: &Export, mode: ExportMode) -> Result<Vec<String>, ExportError> {
    let io = |action: &'static str| move |source: io::Error| ExportError::Io { action, source };
    let earlier = match (mode, folder_state(folder)) {
        (ExportMode::New, FolderState::Empty) => Vec::new(),
        (ExportMode::New, _) => return Err(ExportError::NotEmpty),
        (ExportMode::Replace, FolderState::EarlierExport) => {
            let manifest = read_manifest(folder).ok_or(ExportError::NoEarlierExport)?;
            if manifest.project_id.is_some_and(|id| id != export.project_id) {
                return Err(ExportError::OtherProject);
            }
            manifest.files
        }
        (ExportMode::Replace, _) => return Err(ExportError::NoEarlierExport),
    };
    if !folder.exists() {
        fs::create_dir(folder).map_err(io("create the export folder"))?;
    }
    if !is_plain_dir(folder) {
        return Err(ExportError::NotAPlainFile);
    }
    let new: BTreeSet<&str> = export.files.iter().map(|file| file.path.as_str()).collect();
    // Every folder a file is written into or removed from is a real one (or
    // missing) before anything is touched: a linked `src` is never followed.
    for path in new.iter().copied().chain(earlier.iter().map(String::as_str)) {
        let path = target(folder, path).ok_or(ExportError::NotAPlainFile)?;
        if let Some(parent) = path.parent()
            && parent != folder
            && fs::symlink_metadata(parent).is_ok()
            && !is_plain_dir(parent)
        {
            return Err(ExportError::NotAPlainFile);
        }
    }
    // A file someone added after the earlier export is never written over.
    for path in new.iter().filter(|path| !earlier.iter().any(|old| old == *path)) {
        let target = target(folder, path).ok_or(ExportError::NotAPlainFile)?;
        if fs::symlink_metadata(&target).is_ok() {
            return Err(ExportError::NotWrittenByExport((*path).to_owned()));
        }
    }
    // Files of the earlier export that this one does not write.
    for old in earlier.iter().filter(|old| !new.contains(old.as_str())) {
        let path = target(folder, old).ok_or(ExportError::NotAPlainFile)?;
        match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.is_file() && !is_link(&metadata) => {
                fs::remove_file(&path).map_err(io("remove a file of the earlier export"))?;
            }
            Ok(_) => return Err(ExportError::NotAPlainFile),
            Err(_) => {}
        }
    }
    let mode = file_mode();
    let mut written = Vec::new();
    for file in &export.files {
        let path = target(folder, &file.path).ok_or(ExportError::NotAPlainFile)?;
        if let Some(parent) = path.parent()
            && parent != folder
        {
            match fs::create_dir(parent) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(io("create the source folder")(error)),
            }
            if !is_plain_dir(parent) {
                return Err(ExportError::NotAPlainFile);
            }
        }
        replace_file(&path, file.contents.as_bytes(), mode)?;
        written.push(file.path.clone());
    }
    let manifest = Manifest {
        format: String::from(MANIFEST_FORMAT),
        format_version: MANIFEST_VERSION,
        project_id: Some(export.project_id.clone()),
        files: written.clone(),
    };
    let mut text = serde_json::to_string_pretty(&manifest)
        .map_err(|error| io("write the export manifest")(io::Error::other(error)))?;
    text.push('\n');
    replace_file(&folder.join(EXPORT_MANIFEST), text.as_bytes(), mode)?;
    written.push(String::from(EXPORT_MANIFEST));
    Ok(written)
}

/// The project an earlier export in `folder` was made from: `Some(None)`
/// for an export that did not record it (manifest format 1), `None` when
/// `folder` holds no export.
pub fn earlier_export_project(folder: &Path) -> Option<Option<String>> {
    read_manifest(folder).map(|manifest| manifest.project_id)
}

/// An earlier export's manifest, when it is a regular file that reads as
/// one and every name in it is one an export writes.
fn read_manifest(folder: &Path) -> Option<Manifest> {
    let path = folder.join(EXPORT_MANIFEST);
    let metadata = fs::symlink_metadata(&path).ok()?;
    if !metadata.is_file() || is_link(&metadata) || metadata.len() > MAX_MANIFEST_BYTES {
        return None;
    }
    let manifest: Manifest = serde_json::from_slice(&fs::read(&path).ok()?).ok()?;
    let valid = manifest.format == MANIFEST_FORMAT
        && (1..=MANIFEST_VERSION).contains(&manifest.format_version)
        && (manifest.format_version >= 2) == manifest.project_id.is_some()
        && manifest.files.iter().all(|file| is_export_path(file));
    valid.then_some(manifest)
}

/// Whether `path` is a name an export writes: one of the build files, or
/// `src/<name>` with a plain file name.
fn is_export_path(path: &str) -> bool {
    matches!(path, "CMakeLists.txt" | "Makefile" | "README.md")
        || path.strip_prefix("src/").is_some_and(|name| {
            !name.is_empty()
                && name.len() <= 80
                && name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-' || b == b'.')
                && !name.starts_with('.')
        })
}

/// `folder` joined with the export path `path`, when it is one.
fn target(folder: &Path, path: &str) -> Option<PathBuf> {
    if !is_export_path(path) {
        return None;
    }
    let relative = Path::new(path);
    relative
        .components()
        .all(|component| matches!(component, Component::Normal(_)))
        .then(|| folder.join(relative))
}

/// Writes `contents` to `path` through a temporary file next to it and a
/// rename, with the permissions `mode` (Unix); `path` must not be a link or
/// anything but a regular file.
fn replace_file(path: &Path, contents: &[u8], mode: u32) -> Result<(), ExportError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if !metadata.is_file() || is_link(&metadata) => {
            return Err(ExportError::NotAPlainFile);
        }
        _ => {}
    }
    let parent = path.parent().ok_or(ExportError::NotAPlainFile)?;
    let mut temporary = tempfile::Builder::new()
        .prefix(".b2c-export-")
        .tempfile_in(parent)
        .map_err(|source| ExportError::Io {
            action: "write an exported file",
            source,
        })?;
    io::Write::write_all(&mut temporary, contents).map_err(|source| ExportError::Io {
        action: "write an exported file",
        source,
    })?;
    #[cfg(not(unix))]
    let _ = mode;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        fs::set_permissions(temporary.path(), fs::Permissions::from_mode(mode)).map_err(|source| {
            ExportError::Io {
                action: "write an exported file",
                source,
            }
        })?;
    }
    temporary.persist(path).map_err(|error| ExportError::Io {
        action: "write an exported file",
        source: error.error,
    })?;
    Ok(())
}

/// The permissions every new file of the user gets (`0666` less the
/// process's file-mode creation mask), not a temporary file's owner-only
/// ones: a file made with the widest mode shows them, so the mask is never
/// changed.
#[cfg(unix)]
fn file_mode() -> u32 {
    use std::os::unix::fs::PermissionsExt as _;
    let Ok(probe) = tempfile::Builder::new().prefix(".b2c-umask-").tempdir() else {
        return 0o644;
    };
    let file = probe.path().join("probe");
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o666);
    }
    match options.open(&file).and_then(|created| created.metadata()) {
        Ok(metadata) => metadata.permissions().mode() & 0o666,
        Err(_) => 0o644,
    }
}

/// Windows has no modes.
#[cfg(not(unix))]
fn file_mode() -> u32 {
    0
}

/// Whether `path` is a real folder (not a link).
fn is_plain_dir(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|metadata| metadata.is_dir() && !is_link(&metadata))
}

/// Whether `metadata` (not followed) is a link: a symbolic link or, on
/// Windows, any reparse point.
fn is_link(metadata: &fs::Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt as _;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
        if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defines_reach_make_and_cmake_as_one_literal_word() {
        assert_eq!(make_shell_word("-DNAME=1"), "'-DNAME=1'");
        assert_eq!(
            make_shell_word(r#"-DGREETING="it's $5 #1""#),
            r#"'-DGREETING="it'\''s $$5 \#1"'"#
        );
        assert_eq!(cmake_literal("A=\"x\""), "[[A=\"x\"]]");
        assert_eq!(cmake_literal("A=\"]]\""), "[=[A=\"]]\"]=]");
        assert_eq!(cmake_literal("]]]=]"), "[==[]]]=]]==]");
    }

    #[test]
    fn standards_are_spelled_for_every_supported_gcc() {
        assert_eq!(std_flag(CppStandard::Cpp17, false), "-std=c++17");
        assert_eq!(std_flag(CppStandard::Cpp20, true), "-std=gnu++20");
        assert_eq!(std_flag(CppStandard::Cpp26, false), "-std=c++2c");
        assert_eq!(standard_number(CppStandard::Cpp26), "26");
    }

    #[test]
    fn only_names_an_export_writes_are_accepted_from_a_manifest() {
        for good in [
            "CMakeLists.txt",
            "Makefile",
            "README.md",
            "src/main.cpp",
            "src/b2c_support.hpp",
        ] {
            assert!(is_export_path(good), "{good}");
        }
        for bad in [
            "src/../x",
            "src/.hidden",
            "../Makefile",
            "/etc/passwd",
            "src/a/b.cpp",
            "src/",
            "notes.txt",
            "src\\x",
        ] {
            assert!(!is_export_path(bad), "{bad}");
        }
    }

    #[test]
    fn markdown_specials_in_a_project_name_are_escaped() {
        assert_eq!(markdown_text("A *bold* [link](x)"), r"A \*bold\* \[link\]\(x\)");
    }
}
