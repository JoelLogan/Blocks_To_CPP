//! Export as a C++ project (`docs/spec/06-compiler-pipeline.md` §6.11;
//! `docs/plans/m3-contracts/w3-export-run.md` §4).
//!
//! ```text
//! <folder>/
//! ├── src/              the generated .cpp/.hpp, without the banner
//! ├── CMakeLists.txt    CMake 3.20 or newer
//! ├── Makefile          g++ and GNU make (<program>-<config>[-nosanitize][-noharden])
//! ├── README.md         how to build it
//! └── .b2c-export.json  the project's ID, and the files this export wrote with
//!                       the SHA-256 of each
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
//!   replaced or removed, and of those only the ones whose content still has
//!   the SHA-256 the manifest records: a file the student changed (or whose
//!   content an older manifest did not record) is kept and named in the
//!   result ([`Written::kept`]), and the rest of the export goes on
//!   (`docs/plans/m3-contracts/w3-modules.md` §2.8). It never follows a link
//!   (it checks the folders before it removes or writes anything), writes
//!   each file through a temporary file and a rename, and writes the
//!   manifest last. Before it removes or writes anything it writes a
//!   manifest that also accepts what each file held before, so that an
//!   export interrupted part of the way through leaves a folder a later
//!   Replace takes as an export (none of its files kept as changed, none of
//!   its new files in the way).

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::{self, Write as _};
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

use b2c_ir::sast::CppStandard;
use b2c_ir::source_map::GeneratedProject;
use b2c_ir::text::is_device_name;
use b2c_ir::{DiagSource, Diagnostic, Location};
use b2c_model::{BuildConfiguration, Define, DefineValue, Document, Sanitizer};
use b2c_toolchain::codes;
use b2c_toolchain::command::{ENCODING_FLAGS, is_debug_like, optimisation_flags, warning_flags};
use b2c_toolchain::flags::ValidDefine;
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use crate::frontend::{FrontendOptions, run_frontend};

/// The manifest an export writes last.
pub const EXPORT_MANIFEST: &str = ".b2c-export.json";

/// The manifest's `format` tag.
const MANIFEST_FORMAT: &str = "blocks2cpp/export";

/// The largest manifest read back: 1,024 entries with their hashes fit.
const MAX_MANIFEST_BYTES: u64 = 256 * 1024;

/// The most files a manifest read back may list: an export writes at most
/// 516 (256 sources and headers, `b2c_support.hpp` and three build files),
/// and keeps listing the files of earlier exports that a student changed.
const MAX_MANIFEST_ENTRIES: usize = 1024;

/// The manifest's `formatVersion`: 3 records each file's SHA-256; 2 (read
/// only) named the project but recorded no hashes; 1 (read only) named
/// neither.
const MANIFEST_VERSION: u32 = 3;

/// The longest build command the README says nothing about: cmd.exe takes
/// 8,191 characters, and the `Makefile` runs its one command there unless an
/// `sh.exe` is on `PATH` (w3-modules.md §2.8).
const CMD_EXE_COMMAND_LIMIT: usize = 8_000;

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

/// What [`write()`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Written {
    /// The paths written, the manifest last.
    pub files: Vec<String>,
    /// The files of the earlier export that were left as they are, because
    /// they may hold the student's own changes, in the earlier manifest's
    /// order.
    pub kept: Vec<KeptFile>,
}

/// A file of an earlier export that [`write()`] did not replace or remove.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeptFile {
    /// Its path in the export folder (`src/utils.cpp`).
    pub path: String,
    /// Why it was kept.
    pub reason: KeptReason,
    /// Whether this export no longer writes the file (a renamed or deleted
    /// module's), rather than having a new version of it.
    pub no_longer_written: bool,
}

/// Why a file of an earlier export was kept.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeptReason {
    /// Its content is not what the earlier export wrote.
    Changed,
    /// The earlier export's manifest (format 1 or 2) did not record what it
    /// wrote, so the file may have been changed.
    NotRecorded,
}

impl fmt::Display for KeptFile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let reason = match self.reason {
            KeptReason::Changed => "it was changed after the earlier export",
            KeptReason::NotRecorded => {
                "the earlier export did not record its content, so it may have been changed"
            }
        };
        let what = if self.no_longer_written {
            "this export no longer has the file"
        } else {
            "this export's version of it was not written"
        };
        write!(f, "{} was kept because {reason} ({what})", self.path)
    }
}

/// The manifest's shape.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Manifest {
    format: String,
    format_version: u32,
    /// The project's ID (format 2 and later; format 1 did not record it).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    project_id: Option<String>,
    files: Vec<ManifestEntry>,
}

/// One file a manifest lists: its path alone (formats 1 and 2), or its path
/// and the SHA-256 of what the export wrote (format 3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
enum ManifestEntry {
    /// Formats 1 and 2.
    Path(String),
    /// Format 3.
    Hashed(HashedEntry),
}

/// A format 3 entry. `sha256` is missing only for a file an earlier export
/// (format 1 or 2) wrote without recording it, which a later export kept
/// ([`KeptReason::NotRecorded`]): such a file is never replaced or removed.
/// `previous` is only in the manifest an export writes before it writes
/// anything else: the SHA-256 of what an earlier export wrote into the file
/// it is about to replace, which the file still holds if that export is
/// interrupted before it gets to it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct HashedEntry {
    path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    previous: Option<String>,
}

/// A file an earlier export listed, with the SHA-256 (lower-case hex) of
/// what it wrote when the manifest records it, and of what an earlier one
/// wrote there when an interrupted export left both possible.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Listed {
    path: String,
    sha256: Option<String>,
    previous: Option<String>,
}

impl Listed {
    /// Whether `hash` is the SHA-256 of something an export wrote there.
    fn written_by_export(&self, hash: &str) -> bool {
        self.sha256.as_deref() == Some(hash) || self.previous.as_deref() == Some(hash)
    }
}

/// An earlier export's manifest, checked.
#[derive(Debug, Clone, PartialEq, Eq)]
struct EarlierExport {
    project_id: Option<String>,
    files: Vec<Listed>,
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
    assemble(document, generated, app_version)
}

/// The export of `document` whose C++ is `generated` (generated without the
/// banner, as [`prepare`] does): the generated files under `src/`, and the
/// build files and README made for them. [`prepare`] runs the front end and
/// calls this; tests call it with a project they made by hand.
///
/// # Errors
/// As [`prepare`], for the project's defines.
pub fn assemble(
    document: &Document,
    generated: &GeneratedProject,
    app_version: &str,
) -> Result<Export, ExportError> {
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
    let long_command = longest_command(&name, &sources, &flags) > CMD_EXE_COMMAND_LIMIT;
    files.push(ExportFile {
        path: String::from("README.md"),
        contents: readme(&document.project.name, &name, app_version, long_command),
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

/// The length of the longest command the `Makefile`'s one recipe runs, for
/// any configuration and choice of `SANITIZE` and `HARDEN`, on Linux or
/// Windows, with `CXX` as `g++` and no `CXXFLAGS` or `LDFLAGS`: about 75
/// characters per module, so more than cmd.exe takes for a project of about
/// a hundred modules with long names.
fn longest_command(name: &str, sources: &[String], flags: &Flags<'_>) -> usize {
    let words = |list: &[&str]| list.iter().map(|word| word.len() + 1).sum::<usize>();
    let common = std_flag(flags.standard, flags.gnu).len()
        + 1
        + words(&ENCODING_FLAGS)
        + flags
            .defines
            .iter()
            .map(|define| make_shell_word(&format!("-D{define}")).len() + 1)
            .sum::<usize>();
    let config = flags
        .configurations()
        .iter()
        .map(|(_, settings)| {
            let mut list = warning_flags(settings);
            list.extend(optimisation_flags(settings));
            let (linux_sanitizers, windows_sanitizers) = sanitizer_flags(settings);
            let (compile, both, link) = linux_hardening(settings);
            let (windows_both, windows_link) = windows_hardening(settings);
            // Sanitizers and the hardening flags for both are given when
            // compiling and when linking; `-static` only on Windows.
            let linux = 2 * words(&linux_sanitizers) + words(&compile) + 2 * words(&both) + words(&link);
            let windows = 2 * words(&windows_sanitizers)
                + 2 * words(&windows_both)
                + words(&windows_link)
                + words(&["-static"]);
            words(&list) + linux.max(windows)
        })
        .max()
        .unwrap_or(0);
    let program = format!("{name}-release-nosanitize-noharden.exe");
    words(&["g++"])
        + common
        + config
        + words(&sources.iter().map(String::as_str).collect::<Vec<_>>())
        + words(&["-o", &program])
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

/// `README.md`. `long_command` adds that the build command is too long
/// for cmd.exe ([`longest_command`]).
fn readme(project_name: &str, name: &str, app_version: &str, long_command: bool) -> String {
    let shell = if long_command {
        "\nThis project has so many modules that its build command is longer than\ncmd.exe takes (8,191 characters): on Windows, run `make` in an MSYS2 shell,\nor build with CMake.\n"
    } else {
        ""
    };
    format!(
        r"# {title}

This is the C++ of a Blocks2Cpp project, exported by Blocks2Cpp {app_version}.
It is ordinary C++ that builds without Blocks2Cpp, with the same flags the
app uses. `src/` holds the code; edit it as you like. Exporting the project
again replaces only the files it wrote that nobody has changed since: a file
you changed is kept as it is, and Blocks2Cpp says which (other files in this
folder are kept too).

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
{shell}",
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
/// the paths written, the manifest last, and the files of the earlier
/// export that were kept because they may hold changes of the student's.
///
/// # Errors
/// [`ExportError::NotEmpty`] for [`ExportMode::New`] and a folder that is
/// not empty, [`ExportError::NoEarlierExport`] for [`ExportMode::Replace`]
/// without one, [`ExportError::OtherProject`] when it is another project's,
/// [`ExportError::NotWrittenByExport`] for a file in the way that the earlier
/// export did not write, [`ExportError::NotAPlainFile`] for a link (the folder or
/// anything to be written or removed), and [`ExportError::Io`].
pub fn write(folder: &Path, export: &Export, mode: ExportMode) -> Result<Written, ExportError> {
    write_with(folder, export, mode, replace_file)
}

/// [`write()`], with `put` writing each file (`replace_file`, or in a test
/// one that fails part of the way through).
fn write_with(
    folder: &Path,
    export: &Export,
    mode: ExportMode,
    mut put: impl FnMut(&Path, &[u8], u32) -> Result<(), ExportError>,
) -> Result<Written, ExportError> {
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
    let listed = |path: &str| earlier.iter().any(|old| old.path == path);
    if !folder.exists() {
        fs::create_dir(folder).map_err(io("create the export folder"))?;
    }
    if !is_plain_dir(folder) {
        return Err(ExportError::NotAPlainFile);
    }
    let new: BTreeSet<&str> = export.files.iter().map(|file| file.path.as_str()).collect();
    // Every folder a file is written into or removed from is a real one (or
    // missing) before anything is touched: a linked `src` is never followed.
    for path in new
        .iter()
        .copied()
        .chain(earlier.iter().map(|old| old.path.as_str()))
    {
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
    for path in new.iter().filter(|path| !listed(path)) {
        let target = target(folder, path).ok_or(ExportError::NotAPlainFile)?;
        if fs::symlink_metadata(&target).is_ok() {
            return Err(ExportError::NotWrittenByExport((*path).to_owned()));
        }
    }
    let plan = plan_replacement(folder, export, &earlier)?;
    let mode = file_mode();
    let to_write: Vec<&ExportFile> = export
        .files
        .iter()
        .filter(|file| !plan.keep.contains(&file.path))
        .collect();
    // Before anything is removed or written, a manifest that also accepts
    // what each file holds now (see `interim_entries`).
    let interim = interim_entries(&to_write, &plan);
    put_manifest(folder, &export.project_id, interim, false, mode, &mut put)?;
    for (_, path, _) in &plan.remove {
        fs::remove_file(path).map_err(io("remove a file of the earlier export"))?;
    }
    let mut written = Vec::new();
    let mut entries = Vec::new();
    for file in to_write {
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
        put(&path, file.contents.as_bytes(), mode)?;
        written.push(file.path.clone());
        entries.push(ManifestEntry::Hashed(HashedEntry {
            path: file.path.clone(),
            sha256: Some(sha256_hex(file.contents.as_bytes())),
            previous: None,
        }));
    }
    // A kept file stays listed as the earlier export recorded it, so a later
    // export keeps it too while it differs, and replaces it once it is back
    // to what was written. Past the manifest's limit a kept file is no longer
    // listed, which makes it one of the student's own files.
    let room = MAX_MANIFEST_ENTRIES.saturating_sub(entries.len());
    entries.extend(plan.carried.iter().take(room).map(carried_entry));
    put_manifest(folder, &export.project_id, entries, true, mode, &mut put)?;
    written.push(String::from(EXPORT_MANIFEST));
    Ok(Written {
        files: written,
        kept: plan.kept,
    })
}

/// The entries of the manifest an export writes before it removes or writes
/// anything: every file it touches, with each content the file may have if
/// the export is interrupted (what an earlier export wrote and what this
/// one writes), then the kept files. A later Replace then neither keeps a
/// file this one wrote as the student's change nor refuses a new one as in
/// the way.
fn interim_entries(to_write: &[&ExportFile], plan: &Replacement) -> Vec<ManifestEntry> {
    let mut interim: Vec<ManifestEntry> = to_write
        .iter()
        .map(|file| {
            let sha256 = sha256_hex(file.contents.as_bytes());
            let previous = plan
                .current
                .get(&file.path)
                .filter(|now| **now != sha256)
                .cloned();
            ManifestEntry::Hashed(HashedEntry {
                path: file.path.clone(),
                sha256: Some(sha256),
                previous,
            })
        })
        .collect();
    interim.extend(plan.remove.iter().map(|(path, _, sha256)| {
        ManifestEntry::Hashed(HashedEntry {
            path: path.clone(),
            sha256: Some(sha256.clone()),
            previous: None,
        })
    }));
    interim.extend(plan.carried.iter().map(carried_entry));
    interim.truncate(MAX_MANIFEST_ENTRIES);
    interim
}

/// The entry of a kept file in a new manifest: as the earlier export
/// recorded it, without the content an interrupted export had not replaced
/// yet (a kept file is never replaced).
fn carried_entry(old: &Listed) -> ManifestEntry {
    ManifestEntry::Hashed(HashedEntry {
        path: old.path.clone(),
        sha256: old.sha256.clone(),
        previous: None,
    })
}

/// Writes the manifest of `entries` with `put`: indented when `last` (the
/// one an export leaves), on one line before the files are written.
fn put_manifest(
    folder: &Path,
    project_id: &str,
    entries: Vec<ManifestEntry>,
    last: bool,
    mode: u32,
    put: &mut impl FnMut(&Path, &[u8], u32) -> Result<(), ExportError>,
) -> Result<(), ExportError> {
    // A path listed twice, even in another case, would make the manifest
    // unreadable: the first entry stays.
    let mut seen = BTreeSet::new();
    let entries = entries
        .into_iter()
        .filter(|entry| match entry {
            ManifestEntry::Path(path) | ManifestEntry::Hashed(HashedEntry { path, .. }) => {
                seen.insert(path.to_ascii_lowercase())
            }
        })
        .collect();
    let manifest = Manifest {
        format: String::from(MANIFEST_FORMAT),
        format_version: MANIFEST_VERSION,
        project_id: Some(project_id.to_owned()),
        files: entries,
    };
    let text = if last {
        serde_json::to_string_pretty(&manifest)
    } else {
        serde_json::to_string(&manifest)
    };
    let mut text = text.map_err(|error| ExportError::Io {
        action: "write the export manifest",
        source: io::Error::other(error),
    })?;
    text.push('\n');
    put(&folder.join(EXPORT_MANIFEST), text.as_bytes(), mode)
}

/// What a replacement does with the files an earlier export listed.
#[derive(Debug, Default)]
struct Replacement {
    /// Unchanged files this export no longer writes, to delete: the path in
    /// the export, in the folder, and the file's SHA-256.
    remove: Vec<(String, PathBuf, String)>,
    /// The SHA-256 of each listed file this export writes again, as the
    /// file is now.
    current: BTreeMap<String, String>,
    /// Changed files this export would write: left alone.
    keep: BTreeSet<String>,
    /// The files kept, for the result.
    kept: Vec<KeptFile>,
    /// The entries of the kept files, for the new manifest.
    carried: Vec<Listed>,
}

/// Decides, before anything is written, what happens to each file the
/// earlier export listed: a missing one is forgotten; one whose content
/// still has a recorded SHA-256 is replaced, or deleted when this export
/// no longer writes it; one that already holds exactly what this export
/// writes is written again; any other is kept (w3-modules.md §2.8).
fn plan_replacement(folder: &Path, export: &Export, earlier: &[Listed]) -> Result<Replacement, ExportError> {
    let mut plan = Replacement::default();
    for old in earlier {
        let path = target(folder, &old.path).ok_or(ExportError::NotAPlainFile)?;
        match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.is_file() && !is_link(&metadata) => {}
            Ok(_) => return Err(ExportError::NotAPlainFile),
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(source) => {
                return Err(ExportError::Io {
                    action: "inspect a file of the earlier export",
                    source,
                });
            }
        }
        let current = file_sha256(&path).map_err(|source| ExportError::Io {
            action: "read a file of the earlier export",
            source,
        })?;
        let replacement = export.files.iter().find(|file| file.path == old.path);
        if old.written_by_export(&current) {
            if replacement.is_none() {
                plan.remove.push((old.path.clone(), path, current));
            } else {
                plan.current.insert(old.path.clone(), current);
            }
            continue;
        }
        if replacement.is_some_and(|file| sha256_hex(file.contents.as_bytes()) == current) {
            plan.current.insert(old.path.clone(), current);
            continue;
        }
        plan.kept.push(KeptFile {
            path: old.path.clone(),
            reason: if old.sha256.is_some() {
                KeptReason::Changed
            } else {
                KeptReason::NotRecorded
            },
            no_longer_written: replacement.is_none(),
        });
        if replacement.is_some() {
            plan.keep.insert(old.path.clone());
        }
        plan.carried.push(old.clone());
    }
    Ok(plan)
}

/// The SHA-256 of `bytes`, in lower-case hex.
fn sha256_hex(bytes: &[u8]) -> String {
    b2c_model::hex(&Sha256::digest(bytes))
}

/// The SHA-256 of the regular file at `path`, in lower-case hex, read in
/// pieces.
fn file_sha256(path: &Path) -> io::Result<String> {
    crate::manifest::digest_file(path).map(|digest| b2c_model::hex(&digest.sha256))
}

/// The project an earlier export in `folder` was made from: `Some(None)`
/// for an export that did not record it (manifest format 1), `None` when
/// `folder` holds no export.
pub fn earlier_export_project(folder: &Path) -> Option<Option<String>> {
    read_manifest(folder).map(|manifest| manifest.project_id)
}

/// An earlier export's manifest, when it is a regular file of at most
/// 256 KiB that reads as one: its format, version and project ID agree;
/// every entry has the version's shape and names a file an export writes,
/// with at most one hash (64 lower-case hex digits) and, only beside it, the
/// hash a file held before an interrupted export; no path is listed twice,
/// even ignoring case; at most [`MAX_MANIFEST_ENTRIES`]. A manifest is
/// untrusted input: the folder may have been copied, cloned or edited.
fn read_manifest(folder: &Path) -> Option<EarlierExport> {
    let path = folder.join(EXPORT_MANIFEST);
    let metadata = fs::symlink_metadata(&path).ok()?;
    if !metadata.is_file() || is_link(&metadata) || metadata.len() > MAX_MANIFEST_BYTES {
        return None;
    }
    let manifest: Manifest = serde_json::from_slice(&fs::read(&path).ok()?).ok()?;
    let valid = manifest.format == MANIFEST_FORMAT
        && (1..=MANIFEST_VERSION).contains(&manifest.format_version)
        && (manifest.format_version >= 2) == manifest.project_id.is_some()
        && manifest.files.len() <= MAX_MANIFEST_ENTRIES;
    if !valid {
        return None;
    }
    let hashed = manifest.format_version >= 3;
    let mut files = Vec::with_capacity(manifest.files.len());
    let mut seen = BTreeSet::new();
    for entry in manifest.files {
        let listed = match entry {
            ManifestEntry::Path(path) if !hashed => Listed {
                path,
                sha256: None,
                previous: None,
            },
            ManifestEntry::Hashed(HashedEntry {
                path,
                sha256,
                previous,
            }) if hashed => Listed {
                path,
                sha256,
                previous,
            },
            _ => return None,
        };
        let is_hash = |hash: &str| {
            hash.len() == 64
                && hash
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        };
        let well_formed = is_export_path(&listed.path)
            && listed.sha256.as_deref().is_none_or(is_hash)
            && listed
                .previous
                .as_deref()
                .is_none_or(|hash| listed.sha256.is_some() && is_hash(hash))
            && seen.insert(listed.path.to_ascii_lowercase());
        if !well_formed {
            return None;
        }
        files.push(listed);
    }
    Some(EarlierExport {
        project_id: manifest.project_id,
        files,
    })
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
    fn kept_files_say_why() {
        let file = KeptFile {
            path: String::from("src/main.cpp"),
            reason: KeptReason::NotRecorded,
            no_longer_written: false,
        };
        assert_eq!(
            file.to_string(),
            "src/main.cpp was kept because the earlier export did not record its content, so it may have been changed (this export's version of it was not written)"
        );
    }

    /// An export of these files, as `prepare` would make one.
    fn export_of(files: &[(&str, &str)]) -> Export {
        Export {
            name: String::from("main"),
            folder_name: String::from("main"),
            project_id: String::from("prj_test"),
            files: files
                .iter()
                .map(|(path, contents)| ExportFile {
                    path: (*path).to_owned(),
                    contents: (*contents).to_owned(),
                })
                .collect(),
        }
    }

    /// What `write` puts into `folder`, by path.
    fn contents(folder: &Path, export: &Export) -> Vec<(String, String)> {
        export
            .files
            .iter()
            .map(|file| {
                let text = fs::read_to_string(folder.join(&file.path)).unwrap_or_default();
                (file.path.clone(), text)
            })
            .collect()
    }

    /// A Replace interrupted at any file (a full disk, a file another
    /// program holds open, the app closed) leaves a folder that a later
    /// Replace of a changed project accepts: the files written before the
    /// interruption are neither kept as the student's changes nor in the
    /// way, and a file the interrupted export had not written yet is still
    /// the earlier export's.
    #[test]
    fn an_interrupted_replace_leaves_a_folder_a_later_replace_accepts() {
        let first = export_of(&[
            ("src/main.cpp", "main A\n"),
            ("src/old.cpp", "old A\n"),
            ("CMakeLists.txt", "cmake A\n"),
            ("Makefile", "make A\n"),
            ("README.md", "readme\n"),
        ]);
        let second = export_of(&[
            ("src/main.cpp", "main B\n"),
            ("src/extra.cpp", "extra B\n"),
            ("CMakeLists.txt", "cmake B\n"),
            ("Makefile", "make B\n"),
            ("README.md", "readme\n"),
        ]);
        let third = export_of(&[
            ("src/main.cpp", "main C\n"),
            ("src/extra.cpp", "extra C\n"),
            ("CMakeLists.txt", "cmake C\n"),
            ("Makefile", "make C\n"),
            ("README.md", "readme\n"),
        ]);
        // Stopped at its first write, then its second, and so on, until it
        // runs to the end.
        for stop in 1.. {
            let dir = tempfile::tempdir().unwrap();
            let folder = dir.path().join("export");
            write(&folder, &first, ExportMode::New).unwrap();
            let mut puts = 0;
            let mut stopped_at = None;
            let interrupted = write_with(&folder, &second, ExportMode::Replace, |path, contents, mode| {
                puts += 1;
                if puts == stop {
                    stopped_at = Some(path.to_path_buf());
                    return Err(ExportError::Io {
                        action: "write an exported file",
                        source: io::Error::other("interrupted"),
                    });
                }
                replace_file(path, contents, mode)
            });
            let Some(stopped_at) = stopped_at else {
                assert!(interrupted.is_ok());
                break;
            };
            let at = stopped_at.display();
            assert!(matches!(interrupted, Err(ExportError::Io { .. })), "{at}");
            assert_eq!(folder_state(&folder), FolderState::EarlierExport, "{at}");
            let written = write(&folder, &third, ExportMode::Replace)
                .unwrap_or_else(|error| panic!("after a stop at {at}: {error}"));
            assert_eq!(written.kept, [], "after a stop at {at}");
            assert_eq!(
                contents(&folder, &third),
                contents_of(&third),
                "after a stop at {at}"
            );
            assert!(!folder.join("src/old.cpp").exists(), "{at}");
            // The manifest records one hash a file, and a later Replace keeps
            // nothing either.
            let manifest = fs::read_to_string(folder.join(EXPORT_MANIFEST)).unwrap();
            assert!(!manifest.contains("previous"), "{manifest}");
            let again = write(&folder, &first, ExportMode::Replace).unwrap();
            assert_eq!(again.kept, [], "after a stop at {at}");
        }
    }

    /// An export's files, by path.
    fn contents_of(export: &Export) -> Vec<(String, String)> {
        export
            .files
            .iter()
            .map(|file| (file.path.clone(), file.contents.clone()))
            .collect()
    }

    #[test]
    fn markdown_specials_in_a_project_name_are_escaped() {
        assert_eq!(markdown_text("A *bold* [link](x)"), r"A \*bold\* \[link\]\(x\)");
    }
}
