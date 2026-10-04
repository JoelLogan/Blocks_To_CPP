//! Command construction (spec §7.4) for hand-built Linux and Windows
//! toolchains: exact argv, probe gating and the notes for dropped options.

// Helper functions outside `#[test]`s fail the test by panicking.
#![allow(clippy::unwrap_used)]

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use b2c_ir::sast::CppStandard;
use b2c_model::{
    BuildConfiguration, Configurations, Define, DefineValue, Language, Optimization, Sanitizer, WarningLevel,
};
use b2c_toolchain::codes;
use b2c_toolchain::command::{BuildInputs, CommandPlan, LinkMode};
use b2c_toolchain::fingerprint::Fingerprint;
use b2c_toolchain::flags::{ExtraFlags, LibraryProfile, LinkName, PkgConfigFlags, Subsystem, ValidDefine};
use b2c_toolchain::probe::{
    Capabilities, CompilerKind, DiagnosticsFormat, Hardening, LibraryFeatures, PROBE_FORMAT, Sanitizers,
    Standards, Toolchain,
};
use b2c_toolchain::target::{GccVersion, Target};
use proptest::prelude::*;

const GXX: &str = "/usr/bin/x86_64-linux-gnu-g++-13";
const WIN_GXX: &str = r"C:\msys64\ucrt64\bin\g++.exe";

fn toolchain(path: &str, triple: &str, major: u32, format: DiagnosticsFormat) -> Toolchain {
    Toolchain {
        format: PROBE_FORMAT,
        fingerprint: Fingerprint {
            path: PathBuf::from(path),
            size: 1,
            modified_ns: 0,
            sha256: "0".repeat(64),
        },
        kind: CompilerKind::Gcc,
        version: Some(GccVersion {
            major,
            minor: 1,
            patch: 0,
        }),
        version_text: format!("g++ {major}.1.0"),
        target: Target::parse(triple),
        capabilities: Capabilities {
            cc1plus: Some(PathBuf::from("/usr/libexec/gcc/x86_64-linux-gnu/13/cc1plus")),
            hello_world: true,
            standards: Standards {
                cpp17: Some("c++17".into()),
                cpp20: Some("c++20".into()),
                cpp23: Some("c++23".into()),
                cpp26: (major >= 14).then(|| "c++26".into()),
            },
            library: LibraryFeatures::default(),
            diagnostics: Some(format),
            sanitizers: Sanitizers {
                address_undefined: true,
                undefined: true,
                undefined_trap: true,
                leak_detection: true,
            },
            hardening: Hardening {
                fhardened: major >= 14,
                fortify_source: true,
                stack_protector_strong: true,
                stack_clash_protection: true,
                cf_protection: true,
                pie: true,
                relro_now: true,
                noexecstack: true,
                windows_aslr_dep: true,
            },
            static_link: true,
        },
        problems: Vec::new(),
    }
}

fn linux13() -> Toolchain {
    toolchain(GXX, "x86_64-linux-gnu", 13, DiagnosticsFormat::SarifFile)
}

fn windows14() -> Toolchain {
    toolchain(WIN_GXX, "x86_64-w64-mingw32", 14, DiagnosticsFormat::SarifFile)
}

fn cpp20() -> Language {
    Language {
        standard: CppStandard::Cpp20,
        gnu_extensions: false,
    }
}

fn strings(args: &[OsString]) -> Vec<String> {
    args.iter()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect()
}

fn debug() -> BuildConfiguration {
    Configurations::default().debug
}

fn release() -> BuildConfiguration {
    Configurations::default().release
}

fn plan(toolchain: &Toolchain, config: &BuildConfiguration) -> CommandPlan {
    let extra = ExtraFlags::none();
    CommandPlan::new(&BuildInputs::new(
        toolchain,
        config,
        cpp20(),
        PathBuf::from("/b/gen"),
        &extra,
    ))
    .unwrap()
}

#[test]
fn linux_debug_single_step_is_exact() {
    let tc = linux13();
    let config = debug();
    let plan = plan(&tc, &config);
    let step = plan.compile_and_link(Path::new("/b/gen/main.cpp"), Path::new("/b/out/main"));
    assert_eq!(step.program, PathBuf::from(GXX));
    assert_eq!(
        strings(&step.args),
        [
            "-std=c++20",
            "-finput-charset=UTF-8",
            "-fexec-charset=UTF-8",
            "-fdiagnostics-color=never",
            "-fdiagnostics-urls=never",
            "-fmessage-length=0",
            "-fdiagnostics-format=sarif-file",
            "-Wbidi-chars=any",
            "-Wall",
            "-Wextra",
            "-Wpedantic",
            "-O0",
            "-g",
            "-fno-omit-frame-pointer",
            "-D_GLIBCXX_ASSERTIONS",
            "-fsanitize=address,undefined",
            "-fno-sanitize-recover=undefined",
            "-fstack-protector-strong",
            "-fstack-clash-protection",
            "-fcf-protection",
            "-fPIE",
            "-pipe",
            "-iquote",
            "/b/gen",
            "-pie",
            "-Wl,-z,relro,-z,now",
            "-Wl,-z,noexecstack",
            "/b/gen/main.cpp",
            "-o",
            "/b/out/main",
        ]
    );
    assert_eq!(step.format, DiagnosticsFormat::SarifFile);
    assert_eq!(step.sarif_file.as_deref(), Some("main.cpp.sarif"));
    assert!(plan.notes().is_empty(), "{:#?}", plan.notes());
}

#[test]
fn linux_release_fortifies_and_defines_ndebug() {
    let tc = linux13();
    let args = strings(
        &plan(&tc, &release())
            .compile_and_link(Path::new("/g/main.cpp"), Path::new("/o/main"))
            .args,
    );
    for flag in [
        "-O2",
        "-DNDEBUG",
        "-U_FORTIFY_SOURCE",
        "-D_FORTIFY_SOURCE=3",
        "-D_GLIBCXX_ASSERTIONS",
        "-pie",
    ] {
        assert!(args.contains(&flag.to_owned()), "{flag} missing from {args:?}");
    }
    for flag in ["-g", "-O0", "-fsanitize=address,undefined", "-fhardened"] {
        assert!(!args.contains(&flag.to_owned()), "{flag} in {args:?}");
    }
    let undef = args.iter().position(|a| a == "-U_FORTIFY_SOURCE").unwrap();
    assert_eq!(args[undef + 1], "-D_FORTIFY_SOURCE=3");
}

#[test]
fn gcc11_uses_fortify_2_json_and_no_bidi_warning() {
    let tc = toolchain("/usr/bin/g++-11", "x86_64-linux-gnu", 11, DiagnosticsFormat::Json);
    let step = plan(&tc, &release()).compile_and_link(Path::new("/g/main.cpp"), Path::new("/o/main"));
    let args = strings(&step.args);
    assert!(args.contains(&"-D_FORTIFY_SOURCE=2".to_owned()));
    assert!(args.contains(&"-fdiagnostics-format=json".to_owned()));
    assert!(!args.iter().any(|a| a.starts_with("-Wbidi-chars")));
    assert_eq!(step.sarif_file, None);
    assert_eq!(step.format, DiagnosticsFormat::Json);
}

#[test]
fn gcc14_uses_fhardened() {
    let tc = toolchain(
        "/usr/bin/g++-14",
        "x86_64-linux-gnu",
        14,
        DiagnosticsFormat::SarifFile,
    );
    let args = strings(
        &plan(&tc, &release())
            .compile_and_link(Path::new("/g/main.cpp"), Path::new("/o/main"))
            .args,
    );
    assert!(args.contains(&"-fhardened".to_owned()));
    for flag in ["-fstack-protector-strong", "-D_FORTIFY_SOURCE=3", "-pie"] {
        assert!(!args.contains(&flag.to_owned()), "{flag} with -fhardened");
    }
}

#[test]
fn gcc14_debug_builds_use_single_hardening_flags() {
    // -fhardened would turn on _FORTIFY_SOURCE and _GLIBCXX_ASSERTIONS, which
    // an unoptimised build with AddressSanitizer cannot have (-Whardened).
    let tc = toolchain(
        "/usr/bin/g++-14",
        "x86_64-linux-gnu",
        14,
        DiagnosticsFormat::SarifFile,
    );
    let args = strings(
        &plan(&tc, &debug())
            .compile_and_link(Path::new("/g/main.cpp"), Path::new("/o/main"))
            .args,
    );
    for flag in ["-O0", "-D_GLIBCXX_ASSERTIONS", "-fstack-protector-strong", "-pie"] {
        assert!(args.contains(&flag.to_owned()), "{flag} missing from {args:?}");
    }
    for flag in ["-fhardened", "-D_FORTIFY_SOURCE=3"] {
        assert!(!args.contains(&flag.to_owned()), "{flag} in {args:?}");
    }
}

#[test]
fn structured_diagnostics_can_be_swapped_for_plain_text() {
    for (major, format, removed) in [
        (
            13,
            DiagnosticsFormat::SarifFile,
            "-fdiagnostics-format=sarif-file",
        ),
        (11, DiagnosticsFormat::Json, "-fdiagnostics-format=json"),
        (15, DiagnosticsFormat::AddOutputSarif, "-fdiagnostics-add-output="),
    ] {
        let tc = toolchain("/usr/bin/g++", "x86_64-linux-gnu", major, format);
        let step = plan(&tc, &debug()).compile_and_link(Path::new("/g/main.cpp"), Path::new("/o/main"));
        let plain = step.with_plain_diagnostics().unwrap();
        let args = strings(&plain.args);
        assert!(!args.iter().any(|a| a.starts_with(removed)), "{args:?}");
        assert_eq!(
            args.iter().filter(|a| *a == "-fdiagnostics-plain-output").count(),
            1,
            "{args:?}"
        );
        assert_eq!(args.len(), step.args.len(), "only the format flag changes");
        assert_eq!(plain.format, DiagnosticsFormat::Plain);
        assert_eq!(plain.sarif_file, None);
        assert!(plain.with_plain_diagnostics().is_none());
    }
}

#[test]
fn gcc15_writes_sarif_beside_text() {
    let tc = toolchain(
        "/usr/bin/g++-15",
        "x86_64-linux-gnu",
        15,
        DiagnosticsFormat::AddOutputSarif,
    );
    let plan = plan(&tc, &debug());
    let step = plan.compile(Path::new("/b/gen/player.cpp"), Path::new("/b/obj/player-0123.o"));
    let args = strings(&step.args);
    assert!(args.contains(&"-fdiagnostics-add-output=sarif:version=2.1,file=player.cpp.sarif".to_owned()));
    assert_eq!(step.sarif_file.as_deref(), Some("player.cpp.sarif"));
    // The per-source flag is not part of the cache key.
    assert!(
        !strings(plan.compile_flags_for_key())
            .iter()
            .any(|a| a.contains("player"))
    );
}

#[test]
fn separate_compile_and_link_steps() {
    let tc = linux13();
    let config = debug();
    let plan = plan(&tc, &config);
    let compile = strings(
        &plan
            .compile(Path::new("/b/gen/main.cpp"), Path::new("/b/obj/main-1.o"))
            .args,
    );
    assert_eq!(
        &compile[compile.len() - 4..],
        ["-c", "/b/gen/main.cpp", "-o", "/b/obj/main-1.o"]
    );
    assert!(!compile.contains(&"-pie".to_owned()));
    assert_eq!(
        strings(plan.compile_flags_for_key()),
        compile[..compile.len() - 4]
    );

    let link = plan.link(
        &[
            PathBuf::from("/b/obj/main-1.o"),
            PathBuf::from("/b/obj/player-2.o"),
        ],
        Path::new("/b/out/game"),
    );
    assert_eq!(link.format, DiagnosticsFormat::Plain);
    assert_eq!(link.sarif_file, None);
    assert_eq!(
        strings(&link.args),
        [
            "-fdiagnostics-color=never",
            "-fdiagnostics-urls=never",
            "-fmessage-length=0",
            "-fdiagnostics-plain-output",
            "-fsanitize=address,undefined",
            "-fno-sanitize-recover=undefined",
            "-fstack-protector-strong",
            "-fstack-clash-protection",
            "-fcf-protection",
            "-pie",
            "-Wl,-z,relro,-z,now",
            "-Wl,-z,noexecstack",
            "-o",
            "/b/out/game",
            "/b/obj/main-1.o",
            "/b/obj/player-2.o",
        ]
    );
}

#[test]
fn windows_debug_uses_trap_ubsan_static_and_notes_asan() {
    let tc = windows14();
    let config = debug();
    let plan = plan(&tc, &config);
    let args = strings(
        &plan
            .compile_and_link(Path::new(r"C:\b\gen\main.cpp"), Path::new(r"C:\b\out\main.exe"))
            .args,
    );
    for flag in [
        "-fsanitize=undefined",
        "-fsanitize-undefined-trap-on-error",
        "-fstack-protector-strong",
        "-Wl,--dynamicbase,--nxcompat,--high-entropy-va",
        "-static",
    ] {
        assert!(args.contains(&flag.to_owned()), "{flag} missing from {args:?}");
    }
    for flag in [
        "-fsanitize=address,undefined",
        "-fhardened",
        "-pie",
        "-fPIE",
        "-D_FORTIFY_SOURCE=3",
    ] {
        assert!(!args.contains(&flag.to_owned()), "{flag} in {args:?}");
    }
    let codes: Vec<_> = plan.notes().iter().map(|d| d.code.0.as_str()).collect();
    assert_eq!(codes, [codes::SANITIZER_DROPPED]);
    assert!(plan.notes()[0].message.contains("AddressSanitizer"));
}

#[test]
fn windows_dynamic_and_missing_static() {
    let mut tc = windows14();
    let config = release();
    let extra = ExtraFlags::none();
    let mut inputs = BuildInputs::new(&tc, &config, cpp20(), PathBuf::from(r"C:\b\gen"), &extra);
    inputs.link_mode = LinkMode::Dynamic;
    let args = strings(
        &CommandPlan::new(&inputs)
            .unwrap()
            .link(&[], Path::new(r"C:\o\a.exe"))
            .args,
    );
    assert!(!args.contains(&"-static".to_owned()));

    tc.capabilities.static_link = false;
    let inputs = BuildInputs::new(&tc, &config, cpp20(), PathBuf::from(r"C:\b\gen"), &extra);
    let plan = CommandPlan::new(&inputs).unwrap();
    assert!(!strings(&plan.link(&[], Path::new(r"C:\o\a.exe")).args).contains(&"-static".to_owned()));
    assert_eq!(plan.notes()[0].code.0, codes::STATIC_DROPPED);
}

#[test]
fn unsupported_hardening_is_dropped_with_one_note() {
    let mut tc = linux13();
    tc.capabilities.hardening.cf_protection = false;
    tc.capabilities.hardening.relro_now = false;
    let config = release();
    let plan = plan(&tc, &config);
    let args = strings(
        &plan
            .compile_and_link(Path::new("/g/m.cpp"), Path::new("/o/m"))
            .args,
    );
    assert!(!args.contains(&"-fcf-protection".to_owned()));
    assert!(!args.contains(&"-Wl,-z,relro,-z,now".to_owned()));
    assert!(args.contains(&"-fstack-clash-protection".to_owned()));
    assert_eq!(plan.notes().len(), 1);
    assert_eq!(plan.notes()[0].code.0, codes::HARDENING_DROPPED);
    assert!(
        plan.notes()[0]
            .message
            .contains("-fcf-protection, -Wl,-z,relro,-z,now")
    );
}

#[test]
fn unsupported_sanitizers_fall_back_or_are_noted() {
    let mut tc = linux13();
    tc.capabilities.sanitizers.address_undefined = false;
    tc.capabilities.sanitizers.undefined = false;
    let config = debug();
    let plan = plan(&tc, &config);
    let args = strings(
        &plan
            .compile_and_link(Path::new("/g/m.cpp"), Path::new("/o/m"))
            .args,
    );
    assert!(args.contains(&"-fsanitize-undefined-trap-on-error".to_owned()));
    assert!(!args.iter().any(|a| a.contains("address")));
    assert_eq!(plan.notes().len(), 2);
    assert!(plan.notes().iter().all(|d| d.code.0 == codes::SANITIZER_DROPPED));

    tc.capabilities.sanitizers.undefined_trap = false;
    let plan = self::plan(&tc, &config);
    let args = strings(
        &plan
            .compile_and_link(Path::new("/g/m.cpp"), Path::new("/o/m"))
            .args,
    );
    assert!(!args.iter().any(|a| a.starts_with("-fsanitize")));
}

#[test]
fn hardening_off_and_warning_levels() {
    let tc = linux13();
    let config = BuildConfiguration {
        optimization: Optimization::Size,
        debug_info: false,
        sanitizers: vec![Sanitizer::Undefined],
        warnings: WarningLevel::Strict,
        warnings_as_errors: true,
        hardening: false,
    };
    let args = strings(
        &plan(&tc, &config)
            .compile_and_link(Path::new("/g/m.cpp"), Path::new("/o/m"))
            .args,
    );
    for flag in [
        "-Os",
        "-Wshadow",
        "-Wconversion",
        "-Wformat=2",
        "-Wimplicit-fallthrough",
        "-Werror",
        "-fno-omit-frame-pointer",
    ] {
        assert!(args.contains(&flag.to_owned()), "{flag} missing");
    }
    assert!(args.contains(&"-fsanitize=undefined".to_owned()));
    assert!(!args.iter().any(|a| a.contains("stack-protector") || a == "-pie"));

    let minimal = BuildConfiguration {
        warnings: WarningLevel::Minimal,
        optimization: Optimization::Debug,
        ..release()
    };
    let args = strings(
        &plan(&tc, &minimal)
            .compile_and_link(Path::new("/g/m.cpp"), Path::new("/o/m"))
            .args,
    );
    assert!(args.contains(&"-Wall".to_owned()) && args.contains(&"-Og".to_owned()));
    assert!(!args.contains(&"-Wextra".to_owned()));
    assert!(!args.contains(&"-DNDEBUG".to_owned()));
}

#[test]
fn standards_and_gnu_extensions() {
    let mut tc = linux13();
    tc.capabilities.standards.cpp23 = Some("c++2b".into());
    let config = release();
    let extra = ExtraFlags::none();
    let language = Language {
        standard: CppStandard::Cpp23,
        gnu_extensions: true,
    };
    let inputs = BuildInputs::new(&tc, &config, language, PathBuf::from("/g"), &extra);
    let args = strings(
        &CommandPlan::new(&inputs)
            .unwrap()
            .compile(Path::new("/g/m.cpp"), Path::new("/o/m.o"))
            .args,
    );
    assert_eq!(args[0], "-std=gnu++2b");

    let language = Language {
        standard: CppStandard::Cpp26,
        gnu_extensions: false,
    };
    let inputs = BuildInputs::new(&tc, &config, language, PathBuf::from("/g"), &extra);
    let error = CommandPlan::new(&inputs).unwrap_err();
    assert_eq!(error.code.0, codes::STANDARD_UNSUPPORTED);
    assert!(error.message.contains("C++26"));
}

/// An absolute path on the machine running the tests: `C:\a\b` on Windows,
/// `/a/b` elsewhere (library folders must be absolute on the host).
fn host(unix: &str) -> String {
    if cfg!(windows) {
        format!("C:{}", unix.replace('/', "\\"))
    } else {
        unix.to_owned()
    }
}

#[test]
fn defines_libraries_threads_extra_flags_and_trace() {
    let tc = windows14();
    let config = release();
    let defines = vec![
        ValidDefine::new(&Define {
            name: "LEVEL".into(),
            value: DefineValue::Int(3),
        })
        .unwrap(),
        ValidDefine::new(&Define {
            name: "TITLE".into(),
            value: DefineValue::String("My \"game\"".into()),
        })
        .unwrap(),
    ];
    let library = LibraryProfile::new(
        vec![PathBuf::from(&host("/libs/SFML/include"))],
        vec![PathBuf::from(&host("/libs/SFML/lib"))],
        vec![
            LinkName::new("sfml-graphics").unwrap(),
            LinkName::new("sfml-system").unwrap(),
        ],
        vec![PathBuf::from(&host("/libs/SFML/bin"))],
        Subsystem::Windows,
    )
    .unwrap()
    .with_pkg_config(PkgConfigFlags {
        compile: vec!["-DSFML_STATIC".into()],
        link: vec!["-lopengl32".into()],
        dropped: Vec::new(),
    });
    let libraries = [library];
    let extra = ExtraFlags::new(&["-fno-exceptions".into()], &["-Wl,--as-needed".into()]).unwrap();
    let mut inputs = BuildInputs::new(&tc, &config, cpp20(), PathBuf::from(&host("/b/gen")), &extra);
    inputs.defines = &defines;
    inputs.libraries = &libraries;
    inputs.threads = true;
    inputs.trace_header = Some(PathBuf::from(&host("/b/ide/b2c_ide.hpp")));
    let plan = CommandPlan::new(&inputs).unwrap();
    let args = strings(
        &plan
            .compile_and_link(
                Path::new(&host("/b/gen/main.cpp")),
                Path::new(&host("/b/out/main.exe")),
            )
            .args,
    );
    let position = |flag: &str| {
        args.iter()
            .position(|a| a == flag)
            .unwrap_or_else(|| panic!("{flag} missing: {args:?}"))
    };
    assert!(args.contains(&"-DLEVEL=3".to_owned()));
    assert!(args.contains(&r#"-DTITLE="My \"game\"""#.to_owned()));
    assert_eq!(args[position("-isystem") + 1], host("/libs/SFML/include"));
    assert_eq!(args[position("-include") + 1], host("/b/ide/b2c_ide.hpp"));
    assert!(args.contains(&"-DSFML_STATIC".to_owned()));
    assert!(args.contains(&"-pthread".to_owned()));
    assert!(args.contains(&"-mwindows".to_owned()));
    assert!(args.contains(&"-fno-exceptions".to_owned()));
    // Libraries come after the source; linker extras before it.
    let source = position(&host("/b/gen/main.cpp"));
    assert!(position("-Wl,--as-needed") < source);
    assert!(position("-lsfml-graphics") > source);
    assert!(position("-lopengl32") > source);
    assert_eq!(args[position("-L") + 1], host("/libs/SFML/lib"));
    // The link step passes -pthread too.
    let link = strings(
        &plan
            .link(
                &[PathBuf::from(&host("/b/obj/main.o"))],
                Path::new(&host("/b/out/main.exe")),
            )
            .args,
    );
    assert!(link.contains(&"-pthread".to_owned()));
    assert!(!link.contains(&"-fno-exceptions".to_owned()));
}

#[test]
fn unprobed_toolchains_are_refused() {
    let mut tc = linux13();
    tc.capabilities.diagnostics = None;
    let config = debug();
    let extra = ExtraFlags::none();
    let error = CommandPlan::new(&BuildInputs::new(
        &tc,
        &config,
        cpp20(),
        PathBuf::from("/g"),
        &extra,
    ))
    .unwrap_err();
    assert_eq!(error.code.0, codes::BROKEN_INSTALL);
}

#[cfg(unix)]
#[test]
fn process_command_applies_env_limits_and_removes_stale_sarif() {
    let dir = tempfile::tempdir().unwrap();
    let diag = dir.path().canonicalize().unwrap();
    std::fs::write(diag.join("main.cpp.sarif"), "stale").unwrap();
    let tc = linux13();
    let config = debug();
    let step = plan(&tc, &config).compile_and_link(Path::new("/g/main.cpp"), Path::new("/o/main"));
    let env = b2c_toolchain::env::CompilerEnv {
        vars: vec![("PATH".into(), "/usr/bin".into())],
        refused: Vec::new(),
    };
    let command = step.process_command(&diag, &env, None).unwrap();
    assert!(!diag.join("main.cpp.sarif").exists());
    assert_eq!(command.program(), Path::new(GXX));
    assert_eq!(command.working_dir(), diag);
    assert_eq!(command.get_envs().count(), 1);
    assert_eq!(
        command.get_limits(),
        &b2c_toolchain::command::compiler_limits(std::time::Duration::from_mins(2))
    );
    assert_eq!(command.get_args(), step.args.as_slice());
}

#[test]
fn read_diagnostics_reads_the_sarif_file() {
    let dir = tempfile::tempdir().unwrap();
    let tc = linux13();
    let config = debug();
    let step = plan(&tc, &config).compile_and_link(Path::new("/g/main.cpp"), Path::new("/o/main"));
    std::fs::write(
        dir.path().join("main.cpp.sarif"),
        r#"{"version": "2.1.0", "runs": [{"results": [{"ruleId": "error", "level": "error",
            "message": {"text": "boom"}, "locations": [{"physicalLocation":
            {"artifactLocation": {"uri": "/g/main.cpp"}, "region": {"startLine": 2, "startColumn": 3}}}]}]}]}"#,
    )
    .unwrap();
    let parsed = step.read_diagnostics(dir.path(), b"collect2: error: ld returned 1 exit status\n");
    assert_eq!(parsed.messages.len(), 2);
    assert_eq!(parsed.messages[0].message, "boom");
    assert_eq!(parsed.messages[1].message, "ld returned 1 exit status");

    // Without the file, standard error is read as text.
    std::fs::remove_file(dir.path().join("main.cpp.sarif")).unwrap();
    let parsed = step.read_diagnostics(dir.path(), b"/g/main.cpp:2:3: error: boom\n");
    assert_eq!(parsed.messages.len(), 1);
}

fn optimization() -> impl Strategy<Value = Optimization> {
    prop_oneof![
        Just(Optimization::None),
        Just(Optimization::Debug),
        Just(Optimization::Speed),
        Just(Optimization::Size)
    ]
}

fn warnings() -> impl Strategy<Value = WarningLevel> {
    prop_oneof![
        Just(WarningLevel::Minimal),
        Just(WarningLevel::Helpful),
        Just(WarningLevel::Strict)
    ]
}

proptest! {
    /// Whatever the closed options, the argv is deterministic, has one
    /// `-std=`, one `-o` and the source once, and never contains a flag the
    /// extra-flags denylist would refuse except Blocks2Cpp's own output and
    /// diagnostics flags.
    #[test]
    fn argv_is_well_formed(
        optimization in optimization(),
        warnings in warnings(),
        debug_info: bool,
        address: bool,
        undefined: bool,
        as_errors: bool,
        hardening: bool,
        windows: bool,
        major in 11_u32..16,
        caps in proptest::collection::vec(any::<bool>(), 12),
    ) {
        let mut tc = if windows { windows14() } else { linux13() };
        tc.version = Some(GccVersion { major, minor: 0, patch: 0 });
        tc.capabilities.sanitizers = Sanitizers {
            address_undefined: caps[0],
            undefined: caps[1],
            undefined_trap: caps[2],
            leak_detection: caps[3],
        };
        tc.capabilities.hardening = Hardening {
            fhardened: caps[4],
            fortify_source: caps[5],
            stack_protector_strong: caps[6],
            stack_clash_protection: caps[7],
            cf_protection: caps[8],
            pie: caps[9],
            relro_now: caps[10],
            noexecstack: caps[11],
            windows_aslr_dep: caps[4],
        };
        let mut sanitizers = Vec::new();
        if address { sanitizers.push(Sanitizer::Address); }
        if undefined { sanitizers.push(Sanitizer::Undefined); }
        let config = BuildConfiguration { optimization, debug_info, sanitizers, warnings, warnings_as_errors: as_errors, hardening };
        let first = plan(&tc, &config).compile_and_link(Path::new("/g/main.cpp"), Path::new("/o/main"));
        let again = plan(&tc, &config).compile_and_link(Path::new("/g/main.cpp"), Path::new("/o/main"));
        prop_assert_eq!(&first, &again);
        let args = strings(&first.args);
        prop_assert_eq!(args.iter().filter(|a| a.starts_with("-std=")).count(), 1);
        prop_assert_eq!(args.iter().filter(|a| *a == "-o").count(), 1);
        prop_assert_eq!(args.iter().filter(|a| *a == "/g/main.cpp").count(), 1);
        let own = |a: &str| a == "-o" || a.starts_with("-fdiagnostics-") || a.starts_with('/');
        let checked: Vec<String> = args.iter().filter(|a| !own(a)).cloned().collect();
        prop_assert!(b2c_toolchain::flags::check_extra_flags(&checked).is_ok(), "{:?}", checked);
        let mut unique = args.clone();
        unique.sort();
        unique.dedup();
        prop_assert_eq!(unique.len(), args.len(), "duplicate flag in {:?}", args);
    }
}
