//! Validated inputs for command construction: project defines, library
//! profiles, `pkg-config` output and machine-local extra flags (spec
//! §7.4.1, §7.4.4, §7.4.5; `docs/spec/08-security.md` §8.5).

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use b2c_ir::Diagnostic;
use b2c_ir::text::{Ident, StrLit};
use b2c_model::{Define, DefineValue};
use serde::{Deserialize, Serialize};

use crate::codes;

// ---------------------------------------------------------------------------
// Defines
// ---------------------------------------------------------------------------

/// A project define checked for the command line: the name is a valid user
/// identifier ([`Ident::new`]: no keywords, no standard macros such as
/// `NDEBUG`, no `__` or `_X` reserved names) and the value is rendered by the
/// same literal encoder as generated code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidDefine {
    name: Ident,
    value: String,
}

impl ValidDefine {
    /// Checks one define.
    ///
    /// ```
    /// use b2c_model::{Define, DefineValue};
    /// use b2c_toolchain::flags::ValidDefine;
    ///
    /// let define = Define { name: "GREETING".into(), value: DefineValue::String("hi \"you\"".into()) };
    /// assert_eq!(ValidDefine::new(&define)?.argument(), r#"-DGREETING="hi \"you\"""#);
    ///
    /// let bad = Define { name: "NDEBUG".into(), value: DefineValue::Bool(true) };
    /// assert!(ValidDefine::new(&bad).is_err());
    /// # Ok::<(), b2c_ir::Diagnostic>(())
    /// ```
    ///
    /// # Errors
    /// A `B2C-T1019` diagnostic explaining what is wrong.
    pub fn new(define: &Define) -> Result<Self, Diagnostic> {
        let name = Ident::new(&define.name).map_err(|error| {
            codes::error(
                codes::BAD_DEFINE,
                format!(
                    "The define `{}` cannot be used: {error}.",
                    printable(&define.name)
                ),
            )
        })?;
        let value = match &define.value {
            DefineValue::Int(value) => value.to_string(),
            DefineValue::Bool(value) => String::from(if *value { "1" } else { "0" }),
            DefineValue::String(text) => StrLit::new(text)
                .map_err(|error| {
                    codes::error(
                        codes::BAD_DEFINE,
                        format!("The value of the define `{name}` cannot be used: {error}."),
                    )
                })?
                .to_cpp(),
        };
        Ok(Self { name, value })
    }

    /// The `-DNAME=value` argument (one argv entry; no shell quoting needed).
    pub fn argument(&self) -> String {
        format!("-D{}={}", self.name, self.value)
    }
}

/// Checks every define, collecting all problems.
///
/// # Errors
/// All `B2C-T1019` diagnostics, if any define is invalid.
pub fn validate_defines(defines: &[Define]) -> Result<Vec<ValidDefine>, Vec<Diagnostic>> {
    let mut valid = Vec::new();
    let mut problems = Vec::new();
    for define in defines {
        match ValidDefine::new(define) {
            Ok(define) => valid.push(define),
            Err(problem) => problems.push(problem),
        }
    }
    if problems.is_empty() {
        Ok(valid)
    } else {
        Err(problems)
    }
}

/// Text for a message: control characters escaped, at most 80 characters.
fn printable(text: &str) -> String {
    let escaped: String = text.chars().flat_map(char::escape_default).take(80).collect();
    escaped
}

// ---------------------------------------------------------------------------
// Library profiles
// ---------------------------------------------------------------------------

/// A library name for `-l`, matching `[A-Za-z0-9_+.-]{1,64}` and not
/// starting with `-`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct LinkName(String);

impl LinkName {
    /// Checks a library name.
    ///
    /// # Errors
    /// A `B2C-T1018` diagnostic if the name is not allowed.
    pub fn new(name: &str) -> Result<Self, Diagnostic> {
        let valid = !name.is_empty()
            && name.len() <= 64
            && !name.starts_with('-')
            && name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'+' | b'.' | b'-'));
        if valid {
            Ok(Self(name.to_owned()))
        } else {
            Err(codes::error(
                codes::BAD_LIBRARY,
                format!(
                    "`{}` is not a valid library name (use letters, digits and _ + . - only, at most 64).",
                    printable(name)
                ),
            ))
        }
    }

    /// The name.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for LinkName {
    type Error = String;

    fn try_from(name: String) -> Result<Self, String> {
        Self::new(&name).map_err(|diagnostic| diagnostic.message)
    }
}

impl From<LinkName> for String {
    fn from(name: LinkName) -> Self {
        name.0
    }
}

/// Which Windows subsystem a program uses (spec §7.4.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Subsystem {
    /// A console program.
    #[default]
    Console,
    /// A GUI program without a console (`-mwindows` on Windows).
    Windows,
}

/// A machine-local library profile, resolved for one build (spec §7.4.4):
/// the directories were chosen in a native dialog and canonicalised by the
/// settings layer; this type checks that they are absolute and that link
/// names are valid.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LibraryProfile {
    include_dirs: Vec<PathBuf>,
    lib_dirs: Vec<PathBuf>,
    link: Vec<LinkName>,
    runtime_dirs: Vec<PathBuf>,
    subsystem: Subsystem,
    pkg_config: PkgConfigFlags,
}

impl LibraryProfile {
    /// Checks a profile.
    ///
    /// ```
    /// use b2c_toolchain::flags::{LibraryProfile, LinkName, Subsystem};
    ///
    /// let profile = LibraryProfile::new(
    ///     vec!["/opt/sfml/include".into()],
    ///     vec!["/opt/sfml/lib".into()],
    ///     vec![LinkName::new("sfml-graphics")?],
    ///     vec![],
    ///     Subsystem::Console,
    /// )?;
    /// assert_eq!(profile.link()[0].as_str(), "sfml-graphics");
    /// # Ok::<(), b2c_ir::Diagnostic>(())
    /// ```
    ///
    /// # Errors
    /// A `B2C-T1018` diagnostic if a directory is not absolute.
    pub fn new(
        include_dirs: Vec<PathBuf>,
        lib_dirs: Vec<PathBuf>,
        link: Vec<LinkName>,
        runtime_dirs: Vec<PathBuf>,
        subsystem: Subsystem,
    ) -> Result<Self, Diagnostic> {
        for dir in include_dirs.iter().chain(&lib_dirs).chain(&runtime_dirs) {
            if !dir.is_absolute() {
                return Err(codes::error(
                    codes::BAD_LIBRARY,
                    format!(
                        "The library folder {} is not an absolute path; choose it again in the library settings.",
                        dir.display()
                    ),
                ));
            }
        }
        Ok(Self {
            include_dirs,
            lib_dirs,
            link,
            runtime_dirs,
            subsystem,
            pkg_config: PkgConfigFlags::default(),
        })
    }

    /// Adds the allowlisted flags from `pkg-config --cflags --libs`
    /// (see [`filter_pkg_config`]).
    #[must_use]
    pub fn with_pkg_config(mut self, flags: PkgConfigFlags) -> Self {
        self.pkg_config = flags;
        self
    }

    /// Header folders (`-isystem`).
    pub fn include_dirs(&self) -> &[PathBuf] {
        &self.include_dirs
    }

    /// Library folders (`-L`).
    pub fn lib_dirs(&self) -> &[PathBuf] {
        &self.lib_dirs
    }

    /// Libraries to link (`-l`).
    pub fn link(&self) -> &[LinkName] {
        &self.link
    }

    /// Folders added to `PATH` / `LD_LIBRARY_PATH` when the program runs.
    pub fn runtime_dirs(&self) -> &[PathBuf] {
        &self.runtime_dirs
    }

    /// The Windows subsystem.
    pub fn subsystem(&self) -> Subsystem {
        self.subsystem
    }

    /// The `pkg-config` flags.
    pub fn pkg_config(&self) -> &PkgConfigFlags {
        &self.pkg_config
    }
}

// ---------------------------------------------------------------------------
// pkg-config
// ---------------------------------------------------------------------------

/// `pkg-config` output reduced to the allowlist of spec §7.4.4.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PkgConfigFlags {
    /// Compile flags: `-I…`, `-isystem …`, `-D…`, `-pthread`.
    pub compile: Vec<String>,
    /// Link flags: `-L…`, `-l…`, `-pthread`.
    pub link: Vec<String>,
    /// `B2C-T1017` warnings for every token that was dropped.
    pub dropped: Vec<Diagnostic>,
}

/// Tokenises `pkg-config --cflags --libs` output (whitespace-separated, with
/// backslash escapes and quotes as `pkg-config` writes them) and keeps only
/// `-I<dir>`, `-isystem <dir>`, `-L<dir>`, `-l<name>`, `-D<ident>[=<value>]`
/// and `-pthread`. Directories must be absolute. Everything else is dropped
/// with a warning.
///
/// ```
/// use b2c_toolchain::flags::filter_pkg_config;
///
/// let flags = filter_pkg_config("-I/usr/include/SDL2 -D_REENTRANT -lSDL2 -fplugin=/tmp/evil.so");
/// assert_eq!(flags.compile, ["-I/usr/include/SDL2", "-D_REENTRANT"]);
/// assert_eq!(flags.link, ["-lSDL2"]);
/// assert_eq!(flags.dropped.len(), 1);
/// ```
pub fn filter_pkg_config(output: &str) -> PkgConfigFlags {
    let mut flags = PkgConfigFlags::default();
    let tokens = tokenize(output);
    let mut index = 0;
    while let Some(token) = tokens.get(index) {
        index += 1;
        let token = token.as_str();
        let mut drop = || {
            flags.dropped.push(codes::warning(
                codes::PKG_CONFIG_DROPPED,
                format!(
                    "The flag `{}` from pkg-config was ignored, because only -I, -isystem, -L, -l, -D and -pthread are allowed.",
                    printable(token)
                ),
            ));
        };
        if token == "-pthread" {
            flags.compile.push(token.to_owned());
            flags.link.push(token.to_owned());
        } else if token == "-isystem" {
            match tokens.get(index) {
                Some(dir) if Path::new(dir).is_absolute() => {
                    flags.compile.push(format!("-isystem{dir}"));
                    index += 1;
                }
                Some(dir) => {
                    flags.dropped.push(codes::warning(
                        codes::PKG_CONFIG_DROPPED,
                        format!(
                            "The flag `-isystem {}` from pkg-config was ignored, because its folder is not an absolute path.",
                            printable(dir)
                        ),
                    ));
                    index += 1;
                }
                None => drop(),
            }
        } else if let Some(dir) = token.strip_prefix("-isystem") {
            if Path::new(dir).is_absolute() {
                flags.compile.push(token.to_owned());
            } else {
                drop();
            }
        } else if let Some(dir) = token.strip_prefix("-I") {
            if Path::new(dir).is_absolute() {
                flags.compile.push(token.to_owned());
            } else {
                drop();
            }
        } else if let Some(dir) = token.strip_prefix("-L") {
            if Path::new(dir).is_absolute() {
                flags.link.push(token.to_owned());
            } else {
                drop();
            }
        } else if let Some(name) = token.strip_prefix("-l") {
            if LinkName::new(name).is_ok() {
                flags.link.push(token.to_owned());
            } else {
                drop();
            }
        } else if let Some(define) = token.strip_prefix("-D") {
            let name = define.split_once('=').map_or(define, |(name, _)| name);
            if is_c_identifier(name) {
                flags.compile.push(token.to_owned());
            } else {
                drop();
            }
        } else {
            drop();
        }
    }
    flags
}

fn is_c_identifier(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && !name.as_bytes().first().is_some_and(u8::is_ascii_digit)
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

/// Splits on unquoted whitespace, handling `\x` escapes and `'…'`/`"…"`.
fn tokenize(text: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut in_token = false;
    let mut quote: Option<char> = None;
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        match (quote, c) {
            (_, '\\') if quote != Some('\'') => {
                if let Some(next) = chars.next() {
                    current.push(next);
                }
                in_token = true;
            }
            (None, '\'' | '"') => {
                quote = Some(c);
                in_token = true;
            }
            (Some(open), c) if c == open => quote = None,
            (None, c) if c.is_whitespace() => {
                if in_token {
                    tokens.push(std::mem::take(&mut current));
                    in_token = false;
                }
            }
            (_, c) => {
                current.push(c);
                in_token = true;
            }
        }
    }
    if in_token {
        tokens.push(current);
    }
    tokens
}

// ---------------------------------------------------------------------------
// Machine-local extra flags
// ---------------------------------------------------------------------------

/// Machine-local extra compiler and linker flags that passed the denylist
/// of spec §7.4.5 (see [`check_extra_flags`]).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ExtraFlags {
    compile: Vec<OsString>,
    link: Vec<OsString>,
}

impl ExtraFlags {
    /// No extra flags.
    pub fn none() -> Self {
        Self::default()
    }

    /// Checks compile and link flags (one argv entry each).
    ///
    /// ```
    /// use b2c_toolchain::flags::ExtraFlags;
    ///
    /// assert!(ExtraFlags::new(&["-fno-exceptions".into()], &["-Wl,--as-needed".into()]).is_ok());
    /// assert!(ExtraFlags::new(&["-fplugin=/tmp/x.so".into()], &[]).is_err());
    /// ```
    ///
    /// # Errors
    /// A `B2C-T1015` diagnostic for the first refused flag.
    pub fn new(compile: &[String], link: &[String]) -> Result<Self, Diagnostic> {
        check_extra_flags(compile).map_err(|refusal| refusal.diagnostic())?;
        check_extra_flags(link).map_err(|refusal| refusal.diagnostic())?;
        Ok(Self {
            compile: compile.iter().map(OsString::from).collect(),
            link: link.iter().map(OsString::from).collect(),
        })
    }

    /// The compile flags.
    pub fn compile(&self) -> &[OsString] {
        &self.compile
    }

    /// The link flags.
    pub fn link(&self) -> &[OsString] {
        &self.link
    }
}

/// A refused extra flag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlagRefusal {
    /// Index of the flag in the list.
    pub index: usize,
    /// The flag.
    pub flag: String,
    /// Why it was refused.
    pub reason: &'static str,
}

impl FlagRefusal {
    /// The `B2C-T1015` diagnostic for this refusal.
    pub fn diagnostic(&self) -> Diagnostic {
        codes::error(
            codes::EXTRA_FLAG_REFUSED,
            format!(
                "The extra compiler flag `{}` is not allowed: {}. Remove it from the machine settings.",
                printable(&self.flag),
                self.reason
            ),
        )
    }
}

const RUNS_CODE: &str = "it could make the compiler run other programs or load plugins";
const WRITES_FILES: &str = "it could make the compiler write files outside the build folder";
const BREAKS_PIPELINE: &str = "it would break how Blocks2Cpp runs the compiler and reads its messages";
const READS_INPUT: &str = "an extra flag must start with `-`; a bare file name would become compiler input";

/// Checks machine-local extra flags against the denylist of spec §7.4.5,
/// which refuses flags that execute code, write files or break the
/// pipeline: `-fplugin*`, `-B*`, `-wrapper`, `-specs*`/`--specs*`, `@file`,
/// `-o*`, `-x*`, `-save-temps*`, `-fdump-*`, `-dump*`, `-M*`,
/// `-fprofile-*=…`, `-fdiagnostics-*`, linker plugins and outputs through
/// `-Wl,…` or `-Xlinker …`, `-Wp,…`/`-Xpreprocessor`, `-Wa,…`/`-Xassembler`,
/// `-fuse-ld=<path>`/`--ld-path=…`, `--sysroot*`/`-isysroot`,
/// `-iplugindir*`, `-print-*`, `-v`/`--verbose`/`-###`, `-c`/`-S`/`-E`, and
/// anything not starting with `-` (except the argument after `-Xlinker`).
///
/// # Errors
/// The first refused flag.
pub fn check_extra_flags(flags: &[String]) -> Result<(), FlagRefusal> {
    let mut index = 0;
    while let Some(flag) = flags.get(index) {
        let refuse = |reason| FlagRefusal {
            index,
            flag: flag.clone(),
            reason,
        };
        if flag == "-Xlinker" {
            let Some(argument) = flags.get(index + 1) else {
                return Err(refuse(BREAKS_PIPELINE));
            };
            if let Some(reason) = linker_option_refusal(argument) {
                return Err(FlagRefusal {
                    index: index + 1,
                    flag: argument.clone(),
                    reason,
                });
            }
            index += 2;
            continue;
        }
        if let Some(reason) = flag_refusal(flag) {
            return Err(refuse(reason));
        }
        index += 1;
    }
    Ok(())
}

/// Why one compiler flag is refused, if it is.
fn flag_refusal(flag: &str) -> Option<&'static str> {
    if !flag.starts_with('-') {
        return Some(if flag.starts_with('@') {
            RUNS_CODE
        } else {
            READS_INPUT
        });
    }
    if flag.contains(['\0', '\n', '\r']) {
        return Some(BREAKS_PIPELINE);
    }
    let starts = |prefix: &str| flag.starts_with(prefix);
    if starts("-fplugin") || starts("-B") || starts("-wrapper") || starts("-specs") || starts("--specs") {
        return Some(RUNS_CODE);
    }
    if starts("-iplugindir") || starts("--ld-path") || starts("-fuse-ld=") && flag.contains(['/', '\\']) {
        return Some(RUNS_CODE);
    }
    if starts("--sysroot") || starts("-isysroot") {
        return Some(RUNS_CODE);
    }
    if starts("-o") || starts("--output") || starts("-save-temps") || starts("-fdump-") || starts("-dump") {
        return Some(WRITES_FILES);
    }
    if starts("-M") || starts("-fprofile-") && flag.contains('=') || starts("-fauto-profile=") {
        return Some(WRITES_FILES);
    }
    if starts("-Wp,") || starts("-Xpreprocessor") || starts("-Wa,") || starts("-Xassembler") {
        return Some(WRITES_FILES);
    }
    if let Some(options) = flag.strip_prefix("-Wl,") {
        return options.split(',').find_map(linker_option_refusal);
    }
    if starts("-x")
        || starts("-fdiagnostics-")
        || starts("-print-")
        || starts("--print-")
        || matches!(
            flag,
            "-v" | "--verbose" | "-###" | "-c" | "-S" | "-E" | "--help" | "--version" | "-pass-exit-codes"
        )
        || starts("--help=")
    {
        return Some(BREAKS_PIPELINE);
    }
    None
}

/// Why one linker option (from `-Wl,` or after `-Xlinker`) is refused.
fn linker_option_refusal(option: &str) -> Option<&'static str> {
    let starts = |prefix: &str| option.starts_with(prefix);
    if starts("-plugin") || starts("--plugin") || starts("-load-pass-plugin") {
        return Some(RUNS_CODE);
    }
    if option == "-o"
        || starts("--output")
        || starts("-Map")
        || starts("--Map")
        || starts("--dependency-file")
        || starts("--out-implib")
    {
        return Some(WRITES_FILES);
    }
    None
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    fn refused(flag: &str) -> bool {
        check_extra_flags(&[flag.to_owned()]).is_err()
    }

    #[test]
    fn every_banned_flag_is_refused() {
        let banned = [
            // -fplugin*
            "-fplugin=/tmp/evil.so",
            "-fplugin-arg-evil-x=1",
            // -B*
            "-B/tmp/evil",
            "-Bprefix",
            // -wrapper
            "-wrapper",
            "-wrapper=gdb",
            // -specs* / --specs*
            "-specs=/tmp/evil.specs",
            "--specs=/tmp/evil.specs",
            // @file
            "@/tmp/args.rsp",
            "@args",
            // -o
            "-o",
            "-o/tmp/out",
            "--output=/tmp/out",
            // -x
            "-x",
            "-xc++",
            "-xnone",
            // -save-temps*
            "-save-temps",
            "-save-temps=obj",
            // -fdump-*
            "-fdump-tree-all",
            "-fdump-rtl-expand",
            // -dump*
            "-dumpbase",
            "-dumpdir",
            "-dumpversion",
            // -M*
            "-M",
            "-MM",
            "-MD",
            "-MMD",
            "-MF",
            "-MFdeps.d",
            "-MT",
            "-MQ",
            // -fprofile-*=<path>
            "-fprofile-generate=/tmp/p",
            "-fprofile-use=/tmp/p",
            "-fprofile-dir=/tmp",
            "-fauto-profile=/tmp/p",
            // -fdiagnostics-*
            "-fdiagnostics-format=json",
            "-fdiagnostics-color=always",
            "-fdiagnostics-add-output=sarif",
            // linker plugins and outputs
            "-Wl,-plugin,/tmp/evil.so",
            "-Wl,--plugin=/tmp/evil.so",
            "-Wl,--as-needed,-plugin=/tmp/x",
            "-Wl,-Map=/tmp/map",
            "-Wl,-o,/tmp/out",
            "-Wl,--dependency-file=/tmp/d",
            // preprocessor and assembler pass-through
            "-Wp,-MD,/tmp/deps",
            "-Wa,-adhln=/tmp/listing",
            "-Xpreprocessor",
            "-Xassembler",
            // -fuse-ld=<path>
            "-fuse-ld=/tmp/evil-ld",
            r"-fuse-ld=C:\evil\ld.exe",
            "--ld-path=/tmp/evil-ld",
            // --sysroot
            "--sysroot=/tmp/root",
            "--sysroot",
            "-isysroot",
            // -iplugindir*
            "-iplugindir=/tmp",
            // -print-*
            "-print-search-dirs",
            "-print-prog-name=cc1plus",
            "--print-file-name=libc.so",
            // -v / -###
            "-v",
            "--verbose",
            "-###",
            // pipeline breakers
            "-c",
            "-S",
            "-E",
            "--help",
            "--version",
            // not a flag
            "main.cpp",
            "/etc/passwd",
            "",
            // control characters
            "-DX=1\nY",
        ];
        for flag in banned {
            assert!(refused(flag), "{flag:?} was accepted");
        }
    }

    #[test]
    fn xlinker_arguments_are_checked() {
        let flags = |list: &[&str]| list.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        assert!(check_extra_flags(&flags(&["-Xlinker", "--as-needed"])).is_ok());
        let refusal = check_extra_flags(&flags(&["-O2", "-Xlinker", "-plugin"])).unwrap_err();
        assert_eq!(refusal.index, 2);
        assert!(check_extra_flags(&flags(&["-Xlinker", "--plugin=/x.so"])).is_err());
        assert!(check_extra_flags(&flags(&["-Xlinker", "-o"])).is_err());
        assert!(check_extra_flags(&flags(&["-Xlinker"])).is_err());
    }

    #[test]
    fn ordinary_flags_are_allowed() {
        for flag in [
            "-fno-exceptions",
            "-march=native",
            "-O3",
            "-DFOO=1",
            "-Wl,--as-needed",
            "-Wl,-z,relro",
            "-fuse-ld=mold",
            "-fuse-ld=gold",
            "-fprofile-arcs",
            "-mwindows",
            "-I/opt/include",
            "-lm",
            "-Wno-unused",
            "-fsanitize=thread",
        ] {
            assert!(!refused(flag), "{flag:?} was refused");
        }
    }

    #[test]
    fn refusal_diagnostic_is_friendly() {
        let refusal = check_extra_flags(&["-fplugin=x".to_owned()]).unwrap_err();
        let diagnostic = refusal.diagnostic();
        assert_eq!(diagnostic.code.0, codes::EXTRA_FLAG_REFUSED);
        assert!(diagnostic.message.contains("`-fplugin=x`"));
        assert!(diagnostic.message.contains("plugins"));
    }

    #[test]
    fn defines_render() {
        let define = |name: &str, value: DefineValue| Define {
            name: name.to_owned(),
            value,
        };
        assert_eq!(
            ValidDefine::new(&define("LEVEL", DefineValue::Int(-3)))
                .unwrap()
                .argument(),
            "-DLEVEL=-3"
        );
        assert_eq!(
            ValidDefine::new(&define("ON", DefineValue::Bool(true)))
                .unwrap()
                .argument(),
            "-DON=1"
        );
        assert_eq!(
            ValidDefine::new(&define("OFF", DefineValue::Bool(false)))
                .unwrap()
                .argument(),
            "-DOFF=0"
        );
        assert_eq!(
            ValidDefine::new(&define("TEXT", DefineValue::String("a\nb".into())))
                .unwrap()
                .argument(),
            r#"-DTEXT="a\nb""#
        );
        for bad in [
            "",
            "1X",
            "__X",
            "_Reserved",
            "int",
            "NDEBUG",
            "b2c_x",
            "has space",
            "A-B",
        ] {
            assert!(
                ValidDefine::new(&define(bad, DefineValue::Int(1))).is_err(),
                "{bad:?}"
            );
        }
        assert!(ValidDefine::new(&define("X", DefineValue::String("nul\0".into()))).is_err());
        let problems = validate_defines(&[
            define("OK", DefineValue::Int(1)),
            define("int", DefineValue::Int(1)),
        ])
        .unwrap_err();
        assert_eq!(problems.len(), 1);
        assert_eq!(problems[0].code.0, codes::BAD_DEFINE);
    }

    #[test]
    fn link_names() {
        for good in ["m", "sfml-graphics", "stdc++fs", "SDL2_image", "boost.system"] {
            assert!(LinkName::new(good).is_ok(), "{good}");
        }
        for bad in ["", "-lfoo", "a b", "a/b", "a;b", &"x".repeat(65), "x\0"] {
            assert!(LinkName::new(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn library_dirs_must_be_absolute() {
        assert!(
            LibraryProfile::new(
                vec!["relative".into()],
                vec![],
                vec![],
                vec![],
                Subsystem::Console
            )
            .is_err()
        );
    }

    #[test]
    fn pkg_config_is_allowlisted() {
        let flags = filter_pkg_config(
            "-pthread -I/usr/include/a\\ b -isystem /opt/x -isystem rel -DX -DY=2 -D1BAD \
             -L/usr/lib -lfoo -l-bad -Lrelative -Wl,-plugin,x -fplugin=y -O2 /usr/lib/libz.a",
        );
        assert_eq!(
            flags.compile,
            ["-pthread", "-I/usr/include/a b", "-isystem/opt/x", "-DX", "-DY=2"]
        );
        assert_eq!(flags.link, ["-pthread", "-L/usr/lib", "-lfoo"]);
        // -isystem rel, -D1BAD, -l-bad, -Lrelative, -Wl,…, -fplugin, -O2, the archive.
        assert_eq!(flags.dropped.len(), 8, "{:#?}", flags.dropped);
        assert!(
            flags
                .dropped
                .iter()
                .all(|d| d.code.0 == codes::PKG_CONFIG_DROPPED)
        );
    }

    #[test]
    fn tokenizer_handles_quotes_and_escapes() {
        assert_eq!(tokenize(r#"a 'b c' "d e" f\ g  "#), ["a", "b c", "d e", "f g"]);
        assert_eq!(tokenize(""), Vec::<String>::new());
        assert_eq!(tokenize("''"), [""]);
    }

    proptest! {
        #[test]
        fn denylist_never_panics(flags in proptest::collection::vec("\\PC{0,20}", 0..8)) {
            let _ = check_extra_flags(&flags);
        }

        #[test]
        fn pkg_config_output_is_always_allowlisted(text in "[ -~]{0,200}") {
            let flags = filter_pkg_config(&text);
            for flag in flags.compile.iter().chain(&flags.link) {
                prop_assert!(
                    ["-I", "-isystem", "-L", "-l", "-D", "-pthread"].iter().any(|p| flag.starts_with(p)),
                    "{}", flag
                );
                prop_assert!(!flag.starts_with("-fplugin"));
            }
        }
    }
}
