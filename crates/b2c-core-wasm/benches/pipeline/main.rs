//! The native pipeline benchmark (docs/spec/09-quality-and-delivery.md §9.2
//! "Benchmarks", 01 §1.4 N4): the stages the editor's live preview runs,
//! natively, on the generated 1,000-block single-module document
//! ([`document`]).
//!
//! The group `pipeline` measures
//!
//! * `load`: [`b2c_model::load`] on the canonical file bytes;
//! * `resolve`: [`b2c_catalog::resolve`] against the core catalog;
//! * `analyze`: [`b2c_lang::analyze`];
//! * `generate`: [`b2c_codegen::generate_with_report`] with the options a
//!   build uses;
//! * `preview`: the four together, as the editor's WebAssembly core runs
//!   them ([`b2c_core_wasm::preview_document`]); this is the one the nightly
//!   job gates (`tools/bench-compare.py`).
//!
//! Run it with `cargo bench -p b2c-core-wasm --bench pipeline` (add
//! `-- --quick` for a short run). Criterion writes each benchmark's samples
//! to `<target>/criterion/pipeline/<name>/new/sample.json`, which
//! `tools/bench-compare.py criterion` reads. Before measuring, the document
//! is checked: it must load, have exactly 1,000 blocks and preview without
//! errors, so a broken generator cannot pass for a fast pipeline.

#[allow(
    dead_code,
    reason = "the parity and limit helpers are for the document's tests (bench_document)"
)]
mod document;

use std::hint::black_box;
use std::io::Write as _;
use std::process::ExitCode;

use b2c_codegen::{CodegenOptions, HelperPlacement};
use b2c_core_wasm::{APP_VERSION, PreviewOptions, preview_document};
use criterion::{Criterion, SamplingMode};

use crate::document::{Shape, count_blocks, generate};

/// The document the editor's preview must handle within N4 (01 §1.4).
const SHAPE: Shape = Shape {
    blocks: 1_000,
    drag_handle: false,
};

/// Samples per benchmark (`tools/bench-compare.py` needs at least 10).
const SAMPLES: usize = 50;

/// The benchmark document as a saved file has it, checked; or why it cannot
/// be benchmarked.
fn prepared_document() -> Result<String, String> {
    let generated = generate(SHAPE).map_err(|error| error.to_string())?;
    let blocks = count_blocks(&generated);
    if blocks != SHAPE.blocks {
        return Err(format!(
            "the generated document has {blocks} blocks, not {}",
            SHAPE.blocks
        ));
    }
    let loaded = b2c_model::load(generated.to_string().as_bytes()).map_err(|error| {
        let codes: Vec<&str> = error.diagnostics.iter().map(|d| d.code.0.as_str()).collect();
        format!("the generated document does not load: {}", codes.join(", "))
    })?;
    let text = b2c_model::to_canonical_json(&loaded);
    let preview = preview_document(&text, &PreviewOptions::default());
    if !preview.buildable || preview.placeholders > 0 {
        let codes: Vec<&str> = preview.diagnostics.iter().map(|d| d.code.0.as_str()).collect();
        return Err(format!(
            "the generated document does not preview cleanly: {}",
            codes.join(", ")
        ));
    }
    Ok(text)
}

/// The options a build generates code with (the facade uses the same).
fn codegen_options(project_name: &str) -> CodegenOptions {
    CodegenOptions {
        project_name: project_name.to_owned(),
        app_version: String::from(APP_VERSION),
        do_not_edit_banner: true,
        indent_width: 4,
        helper_placement: HelperPlacement::Inline,
    }
}

/// The `pipeline` group on `text`, which [`prepared_document`] checked.
fn bench_pipeline(criterion: &mut Criterion, text: &str) -> Result<(), String> {
    let bytes = text.as_bytes();
    let loaded = b2c_model::load(bytes).map_err(|_| "the document no longer loads")?;
    let catalog = b2c_catalog::core_catalog();
    let (resolved, _) = b2c_catalog::resolve(&loaded, catalog);
    let analysis = b2c_lang::analyze(&resolved);
    let options = codegen_options(&resolved.project.name);
    let preview_options = PreviewOptions::default();

    let mut group = criterion.benchmark_group("pipeline");
    // Each iteration takes milliseconds: the same number of iterations in every sample (flat
    // sampling) keeps a run to seconds per benchmark, and 50 samples are well above the 10 the
    // comparison needs (`--quick` and the other command-line options still apply).
    group.sampling_mode(SamplingMode::Flat).sample_size(SAMPLES);
    group.bench_function("load", |b| b.iter(|| b2c_model::load(black_box(bytes))));
    group.bench_function("resolve", |b| {
        b.iter(|| b2c_catalog::resolve(black_box(&loaded), catalog));
    });
    group.bench_function("analyze", |b| b.iter(|| b2c_lang::analyze(black_box(&resolved))));
    group.bench_function("generate", |b| {
        b.iter(|| b2c_codegen::generate_with_report(black_box(&analysis.program), &options));
    });
    group.bench_function("preview", |b| {
        b.iter(|| preview_document(black_box(text), &preview_options));
    });
    group.finish();
    Ok(())
}

fn main() -> ExitCode {
    let outcome = prepared_document().and_then(|text| {
        let mut criterion = Criterion::default().configure_from_args();
        bench_pipeline(&mut criterion, &text)?;
        criterion.final_summary();
        Ok(())
    });
    match outcome {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            // Not a print macro (the workspace denies them): the reason goes to
            // standard error, and the exit code fails the job.
            let _ = writeln!(std::io::stderr(), "pipeline benchmark: {message}");
            ExitCode::FAILURE
        }
    }
}
