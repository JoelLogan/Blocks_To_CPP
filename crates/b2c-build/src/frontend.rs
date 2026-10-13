//! The pure front half of the pipeline: project bytes to generated C++
//! (`docs/spec/06-compiler-pipeline.md` §6.1, stages ①–⑦).
//!
//! Nothing here touches the file system or spawns a process, so the result
//! depends only on the input bytes and the options.
//!
//! The editor's live preview (`b2c-core-wasm`) composes the same stages with
//! the same [`CodegenOptions`], so the code view, the source map and what is
//! compiled are the same text (01 P3, 06 §6.1 invariant 4). Every option
//! here that changes the generated code is part of the build folder's
//! options hash (07 §7.5.1).

use std::convert::Infallible;

use b2c_codegen::{CodegenOptions, HelperPlacement};
use b2c_ir::sast::Program;
use b2c_ir::source_map::GeneratedProject;
use b2c_ir::{DiagSource, Diagnostic, Location};
use b2c_model::Document;
use b2c_process::CancelToken;

/// The generator needed error placeholders for a program the analyser
/// accepted (docs/reference/diagnostics/generator.md).
pub const GENERATOR_INCOMPLETE: &str = "B2C-E0701";

/// The indent widths the code style allows, in spaces (04 §4.3; tabs and
/// brace styles come later).
pub const INDENT_WIDTHS: [u8; 2] = [2, 4];

/// The default indent width, and the one the command-line tool always uses
/// (07 §7.9).
pub const DEFAULT_INDENT_WIDTH: u8 = 4;

/// How far the pipeline got.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Stage {
    /// The bytes were not a valid project; nothing else ran.
    Load,
    /// The project uses blocks the catalog rejects. The analyser still ran
    /// and its problems are listed too, but no C++ was generated.
    Resolve,
    /// Analysis reported errors; no C++ was generated.
    Analyze,
    /// Code generation ran (`generated` is absent if it reported an error).
    Generate,
}

/// Options for the front half of the pipeline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrontendOptions {
    /// Add the "Edit the blocks, not this file" banner (off for export).
    pub do_not_edit_banner: bool,
    /// Where support helpers go.
    pub helper_placement: HelperPlacement,
    /// Spaces per indentation level: 2 or 4 ([`INDENT_WIDTHS`]; the app's
    /// code style setting). Any other value is treated as
    /// [`DEFAULT_INDENT_WIDTH`]; see [`FrontendOptions::indent`].
    pub indent_width: u8,
}

impl FrontendOptions {
    /// The indent width actually used: [`Self::indent_width`] when it is one
    /// of [`INDENT_WIDTHS`], otherwise [`DEFAULT_INDENT_WIDTH`].
    pub fn indent(&self) -> u8 {
        if INDENT_WIDTHS.contains(&self.indent_width) {
            self.indent_width
        } else {
            DEFAULT_INDENT_WIDTH
        }
    }
}

impl Default for FrontendOptions {
    fn default() -> Self {
        Self {
            do_not_edit_banner: true,
            helper_placement: HelperPlacement::Inline,
            indent_width: DEFAULT_INDENT_WIDTH,
        }
    }
}

/// Everything the front half produced.
#[derive(Debug, Clone, PartialEq)]
pub struct Frontend {
    /// The last stage that ran to completion.
    pub stage: Stage,
    /// The loaded document with catalog defaults filled in (absent when
    /// loading failed).
    pub document: Option<Document>,
    /// The content hash of the loaded document (`b2c_model::content_hash`,
    /// 05 §5.11), before catalog defaults were filled in: the `projectHash`
    /// of a build. Absent when loading failed.
    pub project_hash: Option<[u8; 32]>,
    /// The analysed program (absent only when loading failed).
    pub program: Option<Program>,
    /// The generated C++ (present only when no stage reported an error).
    pub generated: Option<GeneratedProject>,
    /// Every diagnostic from every stage that ran, in pipeline order.
    pub diagnostics: Vec<Diagnostic>,
}

impl Frontend {
    /// Whether any stage reported an error.
    pub fn has_errors(&self) -> bool {
        b2c_ir::has_errors(&self.diagnostics)
    }
}

/// Runs load → resolve → analyse → generate on untrusted project bytes.
///
/// Once the project has loaded, the catalog check and the analyser always
/// both run, even when the catalog rejects some blocks, so `b2c check`, the
/// build and the editor's preview list the same problems (spec §6.1).
/// Generated C++ is produced only for a project without errors (building
/// requires stages ①–⑤ to report zero errors). Warnings, such as a parked
/// block's `B2C-W0505`, never stop it.
pub fn run_frontend(bytes: &[u8], options: &FrontendOptions) -> Frontend {
    let Ok(frontend) = run_stages(bytes, options, &mut || Ok::<(), Infallible>(()));
    frontend
}

/// [`run_frontend`] for a build session: `None` once `cancel` has fired. The
/// token is checked after loading, after resolving and after analysing, so
/// a cancelled build (one replaced by a newer build of its project, for
/// example) stops within one stage instead of generating C++ nobody wants
/// (`docs/spec/07-toolchain-build-run.md` §7.5.4).
pub(crate) fn run_frontend_cancellable(
    bytes: &[u8],
    options: &FrontendOptions,
    cancel: &CancelToken,
) -> Option<Frontend> {
    let mut checkpoint = || if cancel.is_cancelled() { Err(()) } else { Ok(()) };
    run_stages(bytes, options, &mut checkpoint)
        .ok()
        .filter(|_| !cancel.is_cancelled())
}

/// The stages of [`run_frontend`]. `checkpoint` is called after loading and
/// after resolving (whatever the catalog reported), and after an analysis
/// that leaves the project without errors, before the next stage starts;
/// when it returns an error, nothing more runs and that error is returned.
fn run_stages<E>(
    bytes: &[u8],
    options: &FrontendOptions,
    checkpoint: &mut dyn FnMut() -> Result<(), E>,
) -> Result<Frontend, E> {
    let loaded = match b2c_model::load(bytes) {
        Ok(document) => document,
        Err(error) => {
            return Ok(Frontend {
                stage: Stage::Load,
                document: None,
                project_hash: None,
                program: None,
                generated: None,
                diagnostics: error.diagnostics,
            });
        }
    };
    let project_hash = Some(b2c_model::content_hash(&loaded));
    checkpoint()?;

    let (document, mut diagnostics) = b2c_catalog::resolve(&loaded, b2c_catalog::core_catalog());
    let catalog_errors = b2c_ir::has_errors(&diagnostics);
    checkpoint()?;

    // The analyser accepts a document the catalog rejected (it skips what it
    // cannot lower), as the editor's preview relies on.
    let analysis = b2c_lang::analyze(&document);
    diagnostics.extend(analysis.diagnostics);
    if catalog_errors || b2c_ir::has_errors(&diagnostics) {
        return Ok(Frontend {
            stage: if catalog_errors {
                Stage::Resolve
            } else {
                Stage::Analyze
            },
            document: Some(document),
            project_hash,
            program: Some(analysis.program),
            generated: None,
            diagnostics,
        });
    }
    checkpoint()?;

    let codegen_options = CodegenOptions {
        project_name: document.project.name.clone(),
        app_version: String::from(env!("CARGO_PKG_VERSION")),
        do_not_edit_banner: options.do_not_edit_banner,
        indent_width: options.indent(),
        helper_placement: options.helper_placement,
    };
    let generation = b2c_codegen::generate_with_report(&analysis.program, &codegen_options);
    // Code with placeholders compiles but does not do what the blocks say, so
    // it is never handed on (spec §7.5.3 "generator bug detection").
    let generated = if generation.placeholders == 0 {
        Some(generation.project)
    } else {
        diagnostics.push(Diagnostic::error(
            GENERATOR_INCOMPLETE,
            DiagSource::Generator,
            Location::project(),
            "Part of this program could not be turned into C++, so it was not built. This looks like a bug \
             in Blocks2Cpp; please report it with the project file.",
        ));
        None
    };
    Ok(Frontend {
        stage: Stage::Generate,
        document: Some(document),
        project_hash,
        program: Some(analysis.program),
        generated,
        diagnostics,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const HELLO: &[u8] = include_bytes!("../../../examples/hello_world.b2c");

    fn main_cpp(frontend: &Frontend) -> &str {
        let generated = frontend.generated.as_ref().expect("generated");
        &generated
            .files
            .iter()
            .find(|file| file.path == "main.cpp")
            .expect("main.cpp")
            .contents
    }

    #[test]
    fn the_indent_width_reaches_the_generator() {
        let four = run_frontend(HELLO, &FrontendOptions::default());
        let two = run_frontend(
            HELLO,
            &FrontendOptions {
                indent_width: 2,
                ..FrontendOptions::default()
            },
        );
        assert!(main_cpp(&four).contains("\n    "), "{}", main_cpp(&four));
        assert!(!main_cpp(&two).contains("\n    "), "{}", main_cpp(&two));
        assert!(main_cpp(&two).contains("\n  "), "{}", main_cpp(&two));
        // The source maps follow the text.
        assert_ne!(
            four.generated.as_ref().map(|g| &g.source_map),
            two.generated.as_ref().map(|g| &g.source_map)
        );
        // The content hash is of the project, not of the generated code.
        assert_eq!(four.project_hash, two.project_hash);
    }

    #[test]
    fn unsupported_indent_widths_fall_back_to_four() {
        for width in [0, 1, 3, 5, 8, u8::MAX] {
            let options = FrontendOptions {
                indent_width: width,
                ..FrontendOptions::default()
            };
            assert_eq!(options.indent(), DEFAULT_INDENT_WIDTH, "{width}");
        }
        for width in INDENT_WIDTHS {
            let options = FrontendOptions {
                indent_width: width,
                ..FrontendOptions::default()
            };
            assert_eq!(options.indent(), width);
        }
        let odd = run_frontend(
            HELLO,
            &FrontendOptions {
                indent_width: 3,
                ..FrontendOptions::default()
            },
        );
        assert_eq!(
            main_cpp(&odd),
            main_cpp(&run_frontend(HELLO, &FrontendOptions::default()))
        );
    }

    #[test]
    fn the_project_hash_is_the_content_hash_of_the_loaded_document() {
        let frontend = run_frontend(HELLO, &FrontendOptions::default());
        let loaded = b2c_model::load(HELLO).expect("hello world loads");
        assert_eq!(frontend.project_hash, Some(b2c_model::content_hash(&loaded)));
        let broken = run_frontend(b"{", &FrontendOptions::default());
        assert_eq!(broken.stage, Stage::Load);
        assert_eq!(broken.project_hash, None);
    }

    /// The checkpoint runs between the stages, and stops the pipeline at
    /// once when it fails (a cancelled build stops within one stage).
    #[test]
    fn checkpoints_come_after_load_resolve_and_analyse() {
        let options = FrontendOptions::default();
        let mut calls = 0;
        let complete = run_stages(HELLO, &options, &mut || {
            calls += 1;
            Ok::<(), ()>(())
        });
        assert_eq!(calls, 3);
        assert_eq!(complete.map(|frontend| frontend.stage), Ok(Stage::Generate));
        for stop_at in 1..=3 {
            let mut calls = 0;
            let stopped = run_stages(HELLO, &options, &mut || {
                calls += 1;
                if calls == stop_at { Err(stop_at) } else { Ok(()) }
            });
            assert_eq!(stopped.err(), Some(stop_at));
            assert_eq!(calls, stop_at, "nothing runs after a failed checkpoint");
        }
        // A project that does not load reaches no checkpoint.
        let mut calls = 0;
        let broken = run_stages(b"{", &options, &mut || {
            calls += 1;
            Err(())
        });
        assert_eq!(broken.map(|frontend| frontend.stage), Ok(Stage::Load));
        assert_eq!(calls, 0);
        // A project the catalog rejects is still analysed after the
        // checkpoint that follows resolving, and then stops.
        let mut calls = 0;
        let rejected = run_stages(&with_unknown_block(), &options, &mut || {
            calls += 1;
            Ok::<(), ()>(())
        });
        assert_eq!(rejected.map(|frontend| frontend.stage), Ok(Stage::Resolve));
        assert_eq!(calls, 2);
    }

    #[test]
    fn a_cancelled_token_stops_the_front_end() {
        let options = FrontendOptions::default();
        let cancel = CancelToken::new();
        assert_eq!(
            run_frontend_cancellable(HELLO, &options, &cancel),
            Some(run_frontend(HELLO, &options))
        );
        cancel.cancel();
        assert_eq!(run_frontend_cancellable(HELLO, &options, &cancel), None);
        // Even a project with errors gives nothing once cancelled.
        assert_eq!(run_frontend_cancellable(b"{", &options, &cancel), None);
    }

    // --- Parked blocks and catalog errors (ADR-0012, 06 §6.1) ---------------

    /// `hello_world` as JSON.
    fn hello() -> serde_json::Value {
        serde_json::from_slice(HELLO).expect("hello world is JSON")
    }

    /// The top-level blocks of a project's first module.
    fn canvas(project: &mut serde_json::Value) -> &mut Vec<serde_json::Value> {
        project["modules"][0]["workspace"]["blocks"]
            .as_array_mut()
            .expect("blocks")
    }

    fn bytes(project: &serde_json::Value) -> Vec<u8> {
        serde_json::to_vec(project).expect("JSON")
    }

    /// `hello_world` with a block of an unknown type and a `print` of a
    /// variable that does not exist in `main`: one catalog and one analyser
    /// error.
    fn with_unknown_block() -> Vec<u8> {
        let mut project = hello();
        let body = canvas(&mut project)[0]["statements"]["BODY"]
            .as_array_mut()
            .expect("body");
        body.push(serde_json::json!({"id": "u1", "type": "pack.unknown", "v": 1}));
        body.push(serde_json::json!({"id": "u2", "type": "io.print", "v": 1,
            "inputs": {"ITEM0": {"expr": [{"ref": "sym_ghost"}]}}}));
        bytes(&project)
    }

    fn codes(frontend: &Frontend) -> Vec<&str> {
        frontend.diagnostics.iter().map(|d| d.code.0.as_str()).collect()
    }

    /// Catalog errors no longer hide the analyser's problems: `b2c check`
    /// and the build list what the editor's preview lists (06 §6.1).
    #[test]
    fn the_analyser_runs_after_catalog_errors() {
        let frontend = run_frontend(&with_unknown_block(), &FrontendOptions::default());
        assert_eq!(codes(&frontend), ["B2C-E0601", "B2C-E0201"]);
        assert_eq!(frontend.stage, Stage::Resolve);
        assert!(frontend.program.is_some());
        assert!(frontend.generated.is_none());
    }

    /// A block left on the canvas is parked: one warning, and the same C++
    /// as without it (ADR-0012).
    #[test]
    fn a_parked_block_warns_and_still_generates_the_same_cpp() {
        let mut project = hello();
        canvas(&mut project).push(serde_json::json!({
            "id": "p1", "type": "io.print", "v": 1, "x": 400, "y": 40,
            "inputs": {"ITEM0": {"expr": [{"str": "parked"}]}}
        }));
        let frontend = run_frontend(&bytes(&project), &FrontendOptions::default());
        assert_eq!(codes(&frontend), ["B2C-W0505"]);
        assert!(!frontend.has_errors());
        assert_eq!(frontend.stage, Stage::Generate);
        let plain = run_frontend(HELLO, &FrontendOptions::default());
        assert_eq!(frontend.generated, plain.generated);
        assert!(!main_cpp(&frontend).contains("parked"));
    }

    mod parked_properties {
        use proptest::prelude::*;
        use serde_json::{Value, json};

        use super::super::*;
        use super::{canvas, codes};

        /// Every example project, by name.
        fn examples() -> Vec<(String, Vec<u8>)> {
            let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
            let mut found: Vec<(String, Vec<u8>)> = std::fs::read_dir(dir)
                .expect("examples")
                .map(|entry| entry.expect("entry").path())
                .filter(|path| path.extension().is_some_and(|e| e == "b2c"))
                .map(|path| {
                    let name = path
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned();
                    (name, std::fs::read(&path).expect("example"))
                })
                .collect();
            found.sort();
            found
        }

        /// A statement the catalog accepts, with fresh IDs from `next`. It
        /// may declare, change and read variables of its own or of `main`,
        /// which do not matter while it is parked.
        fn statement(kind: u8, next: &mut dyn FnMut() -> String) -> Value {
            let id = next();
            match kind % 6 {
                0 => json!({"id": id, "type": "io.print", "v": 1,
                            "inputs": {"ITEM0": {"expr": [{"str": "parked"}]}}}),
                1 => json!({"id": id, "type": "var.declare", "v": 1,
                            "fields": {"NAME": {"sym": format!("sym_{id}"), "name": format!("v{id}")}},
                            "inputs": {"VALUE": {"expr": [{"num": "7"}]}}}),
                2 => json!({"id": id, "type": "var.set", "v": 1, "fields": {"VAR": {"ref": "sym_elsewhere"}},
                            "inputs": {"VALUE": {"expr": [{"num": "1"}]}}}),
                3 => {
                    let inner = statement(0, next);
                    json!({"id": id, "type": "control.repeat", "v": 1, "statements": {"BODY": [inner]}})
                }
                4 => json!({"id": id, "type": "control.break", "v": 1}),
                _ => json!({"id": id, "type": "program.exit", "v": 1}),
            }
        }

        /// A value block the catalog accepts, with fresh IDs from `next`.
        fn value(kind: u8, next: &mut dyn FnMut() -> String) -> Value {
            let id = next();
            match kind % 3 {
                0 => json!({"id": id, "type": "math.number", "v": 1, "fields": {"VALUE": "42"}}),
                1 => json!({"id": id, "type": "logic.boolean", "v": 1}),
                _ => {
                    let inner = value(0, next);
                    json!({"id": id, "type": "math.arithmetic", "v": 1, "inputs": {"A": {"block": inner}}})
                }
            }
        }

        /// One parked stack: `(is a value, kind, disabled, stacked kinds and
        /// whether each is disabled)`.
        type Stack = (bool, u8, bool, Vec<(u8, bool)>);

        fn stacks() -> impl Strategy<Value = Vec<Stack>> {
            proptest::collection::vec(
                (
                    any::<bool>(),
                    any::<u8>(),
                    any::<bool>(),
                    proptest::collection::vec((any::<u8>(), any::<bool>()), 0..4),
                ),
                1..5,
            )
        }

        /// The parked stacks added to a project by [`park`].
        struct Parked {
            /// How many stacks have an enabled head.
            enabled_heads: usize,
            /// How many blocks were added (heads, stacked and nested blocks),
            /// with the IDs `zp1` to `zp<blocks>`.
            blocks: usize,
        }

        /// Adds the parked stacks to the project's first module.
        fn park(project: &mut Value, stacks: &[Stack]) -> Parked {
            let mut counter = 0;
            let mut next = || {
                counter += 1;
                format!("zp{counter}")
            };
            let mut enabled_heads = 0;
            for (index, (is_value, kind, disabled, stacked)) in stacks.iter().enumerate() {
                let mut head = if *is_value {
                    value(*kind, &mut next)
                } else {
                    statement(*kind, &mut next)
                };
                head["x"] = json!(600);
                head["y"] = json!(40 + 200 * index);
                if *disabled {
                    head["disabled"] = json!(true);
                } else {
                    enabled_heads += 1;
                }
                // Only a statement can have blocks below it.
                if !is_value && !stacked.is_empty() {
                    let below: Vec<Value> = stacked
                        .iter()
                        .map(|(kind, off)| {
                            let mut block = statement(*kind, &mut next);
                            if *off {
                                block["disabled"] = json!(true);
                            }
                            block
                        })
                        .collect();
                    head["stack"] = Value::Array(below);
                }
                canvas(project).push(head);
            }
            Parked {
                enabled_heads,
                blocks: counter,
            }
        }

        /// Gives the block with this ID another type; whether it was found.
        fn retype(value: &mut Value, id: &str, block_type: &str) -> bool {
            match value {
                Value::Object(map) => {
                    if map.get("id").and_then(Value::as_str) == Some(id) && map.contains_key("type") {
                        map.insert(String::from("type"), json!(block_type));
                        return true;
                    }
                    map.values_mut().any(|v| retype(v, id, block_type))
                }
                Value::Array(items) => items.iter_mut().any(|v| retype(v, id, block_type)),
                _ => false,
            }
        }

        proptest! {
            #![proptest_config(ProptestConfig::with_cases(48))]

            /// Parked stacks never change the generated C++ (or its source
            /// map): each enabled head adds one B2C-W0505, and nothing else
            /// changes.
            #[test]
            fn parked_stacks_never_change_the_generated_cpp(
                example in proptest::sample::select(examples()),
                stacks in stacks(),
            ) {
                let (name, original) = example;
                let options = FrontendOptions::default();
                let plain = run_frontend(&original, &options);
                let mut project: Value = serde_json::from_slice(&original).expect("JSON");
                let added = park(&mut project, &stacks);
                let parked = run_frontend(&serde_json::to_vec(&project).expect("JSON"), &options);
                prop_assert!(!parked.has_errors(), "{}: {:?}", name, codes(&parked));
                prop_assert_eq!(parked.stage, Stage::Generate);
                prop_assert_eq!(&parked.generated, &plain.generated, "{}", name);
                let warnings = codes(&parked).iter().filter(|code| **code == "B2C-W0505").count();
                prop_assert_eq!(warnings, added.enabled_heads, "{}", name);
                let others: Vec<&str> =
                    codes(&parked).into_iter().filter(|code| *code != "B2C-W0505").collect();
                prop_assert_eq!(others, codes(&plain), "{}", name);
            }

            /// A parked block of an unknown type (such as a Raw C++ block
            /// before M4), anywhere in parked code and disabled or not, is
            /// still B2C-E0601 and stops the build (ADR-0012).
            #[test]
            fn a_parked_unknown_block_still_stops_the_build(
                example in proptest::sample::select(examples()),
                stacks in stacks(),
                at in any::<prop::sample::Index>(),
                unknown_type in prop_oneof![Just("raw.cpp"), Just("sfml.window.open"), Just("pack.missing")],
            ) {
                let (name, original) = example;
                let mut project: Value = serde_json::from_slice(&original).expect("JSON");
                let added = park(&mut project, &stacks);
                let id = format!("zp{}", at.index(added.blocks) + 1);
                prop_assert!(retype(&mut project, &id, unknown_type), "{} has no {}", name, id);
                let frontend = run_frontend(&serde_json::to_vec(&project).expect("JSON"), &FrontendOptions::default());
                let unknown: Vec<Option<&str>> = frontend
                    .diagnostics
                    .iter()
                    .filter(|d| d.code.0 == "B2C-E0601")
                    .map(|d| d.primary.block.as_ref().map(b2c_ir::BlockId::as_str))
                    .collect();
                prop_assert_eq!(unknown, [Some(id.as_str())], "{}", name);
                prop_assert_eq!(frontend.stage, Stage::Resolve);
                prop_assert!(frontend.generated.is_none());
            }
        }
    }
}
