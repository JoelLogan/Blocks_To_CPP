//! The state the exports keep between calls: the analysis behind the last
//! preview.
//!
//! The editor previews after every edit (debounced) and then asks about the
//! same document many times: the symbols for a dropdown, the code of the
//! blocks being copied. A [`Session`] keeps the last preview's analysis, so
//! those answers never re-run the pipeline. The WebAssembly exports use one
//! session per instance (a thread-local in `lib.rs`); native callers and
//! tests create their own.
//!
//! What is kept is the analysis of the last **successful** load: a preview
//! whose document did not load forgets it, so the scope query answers `[]`
//! until the next good preview. The clipboard functions are given the
//! document they work on; they use the kept analysis only when that
//! document has the same content hash (05 §5.11) as the previewed one, and
//! otherwise analyse it themselves, so they are always right about the
//! document they were given.

use std::borrow::Cow;

use b2c_codegen::CodegenOptions;
use b2c_ir::{BlockId, Diagnostic, SymbolInfo};
use b2c_lang::Analysis;
use b2c_model::{Block, Document};

use crate::args::PasteTarget;
use crate::clipboard::{self, Unresolved};
use crate::error::{FacadeError, Failure};
use crate::facade::{self, Preview};
use crate::options::PreviewOptions;
use crate::scope;

/// What a session keeps from the last preview.
#[derive(Debug, Clone)]
struct LastPreview {
    /// The content hash of the previewed document.
    content_hash: [u8; 32],
    /// The analysis of the resolved document.
    analysis: Analysis,
    /// The options its code was generated with.
    codegen_options: CodegenOptions,
}

/// One editor's compiler core: the preview and the questions that are
/// answered from the last preview's analysis.
#[derive(Debug, Clone, Default)]
pub struct Session {
    last: Option<LastPreview>,
    /// The indent width of the last preview, for code generated when the
    /// kept analysis does not match a document (the default before any
    /// preview).
    options: PreviewOptions,
}

/// The result of [`Session::clipboard_make`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClipboardMade {
    /// The canonical clipboard payload (05 §5.12), for
    /// `application/x-blocks2cpp+json`.
    pub payload: String,
    /// The C++ of the copied blocks, for `text/plain`; `None` when none of
    /// them produce code (disabled or loose blocks).
    pub text: Option<String>,
}

/// The result of [`Session::paste_prepare`].
#[derive(Debug, Clone, PartialEq)]
pub struct Pasted {
    /// The blocks to insert at the target, in order: fresh block IDs, fresh
    /// IDs for the symbols they declare, references re-bound at the target.
    /// They have no canvas position. For a canvas target a copied loose
    /// stack stays one block with a `stack` (ADR-0011); for a target in or
    /// after a block, every block's `stack` is moved out to follow it, so
    /// none has one (a nested block with a `stack` is `B2C-E0139`) and the
    /// blocks can be inserted as they are.
    pub blocks: Vec<Block>,
    /// The references that found nothing to bind to; they still refer to
    /// their original symbols.
    pub unresolved: Vec<Unresolved>,
    /// One `B2C-E0201` per unresolved reference, naming the original.
    pub diagnostics: Vec<Diagnostic>,
}

impl Session {
    /// A session with no preview yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// Runs the preview ([`facade::preview_document`]) and keeps its analysis
    /// for the other questions. A document that does not load clears what
    /// was kept.
    pub fn preview(&mut self, document_json: &str, options: &PreviewOptions) -> Preview {
        self.options = *options;
        match facade::run_pipeline(document_json.as_bytes(), *options) {
            Ok(run) => {
                self.last = Some(LastPreview {
                    content_hash: run.content_hash,
                    analysis: run.analysis,
                    codegen_options: run.codegen_options,
                });
                run.preview
            }
            Err(error) => {
                self.last = None;
                Preview::load_failed(error.diagnostics)
            }
        }
    }

    /// Whether a preview's analysis is kept.
    pub fn has_preview(&self) -> bool {
        self.last.is_some()
    }

    /// The symbols visible at a block of the last previewed document
    /// ([`Analysis::symbols_in_scope`]): with `input` `None` or a value
    /// input, what is visible at the block; with a statement input, what is
    /// visible at the start of that list. A disabled statement in a list the
    /// analyser reaches answers for its position. Empty when there is no
    /// preview, for a block the document does not have, and for a block the
    /// analyser does not reach. Sorted by name, then ID.
    pub fn symbols_in_scope(&self, block: &str, input: Option<&str>) -> Vec<SymbolInfo> {
        let (Some(last), Ok(block)) = (&self.last, BlockId::new(block)) else {
            return Vec::new();
        };
        last.analysis.symbols_in_scope(&block, input)
    }

    /// The analysis of a loaded document, and the options to generate its
    /// code with: the kept ones when the content hashes match, otherwise
    /// new ones.
    fn analysis_for(&self, document: &Document) -> (Cow<'_, Analysis>, Cow<'_, CodegenOptions>) {
        let hash = b2c_model::content_hash(document);
        if let Some(last) = self.last.as_ref().filter(|last| last.content_hash == hash) {
            return (
                Cow::Borrowed(&last.analysis),
                Cow::Borrowed(&last.codegen_options),
            );
        }
        let (resolved, _, analysis) = facade::analyse(document);
        let options = facade::codegen_options(&resolved.project.name, self.options);
        (Cow::Owned(analysis), Cow::Owned(options))
    }

    /// Copies blocks: the clipboard payload and the blocks' C++.
    ///
    /// `blocks` are block IDs of the document in copy order; a listed block
    /// inside another listed block is copied once, as part of it, and a
    /// top-level block keeps its loose stack. The payload holds copies
    /// without canvas positions, and `refs` for the symbols they use but do
    /// not declare (see [`crate::clipboard`] for the names). The payload is
    /// checked with [`b2c_model::load_clipboard`] before it is returned, so
    /// what is copied can always be pasted.
    ///
    /// The C++ is the last preview's code when that preview shows this
    /// document (generated again from its kept analysis and options, so the
    /// text is the same), and otherwise code generated now for this
    /// document at the last preview's indent width; it is cut by whole
    /// blocks (see `cut.rs`).
    ///
    /// # Errors
    /// [`Failure::Diagnostics`] when the document does not load, or when the
    /// payload would not load either (for example a copy of a whole document
    /// near the 32 MiB limit, whose payload is larger); [`Failure::Error`]
    /// with [`FacadeError::InvalidArguments`] when an ID is not a block of the
    /// document.
    pub fn clipboard_make(&self, document_json: &str, blocks: &[BlockId]) -> Result<ClipboardMade, Failure> {
        let document = b2c_model::load(document_json.as_bytes())?;
        let copies = clipboard::select(&document, blocks)?;
        let top: Vec<BlockId> = copies.iter().map(|block| block.id.clone()).collect();
        let payload = b2c_model::to_canonical_clipboard_json(&clipboard::payload(&document, copies));
        b2c_model::load_clipboard(payload.as_bytes())?;

        let (analysis, options) = self.analysis_for(&document);
        let generation = b2c_codegen::generate_with_report(&analysis.program, &options);
        let text = crate::cut::cut_blocks(&generation.project.files, &generation.project.source_map, &top);
        Ok(ClipboardMade { payload, text })
    }

    /// Prepares a clipboard payload for pasting into a document.
    ///
    /// The payload is validated with [`b2c_model::load_clipboard`] (the
    /// same parser, limits and codes as a project file). Its blocks get
    /// fresh block IDs and fresh IDs for the symbols they declare
    /// ([`b2c_model::remap_ids`] with [`b2c_model::SeededIds`] from `seed`),
    /// none of them used in the document. References to other symbols are
    /// bound again by qualified name and kind among the symbols visible at
    /// `target` (see [`crate::clipboard`] for the target and binding
    /// rules); those that find nothing stay references to their original
    /// symbols and are reported as unresolved, with a `B2C-E0201` naming the
    /// original. When no visible symbol has the recorded name, a reference
    /// whose original symbol is visible at the target with the same kind
    /// keeps it, even if it was renamed since the copy.
    ///
    /// For a target in or after a block (`target.block` set), each block
    /// with a loose `stack` is followed by its stacked blocks and loses the
    /// `stack`, so the result can be inserted into a statement list as it is;
    /// for the canvas the stack is kept (see [`Pasted::blocks`]).
    ///
    /// # Errors
    /// [`Failure::Diagnostics`] when the payload or the document does not
    /// load; [`Failure::Error`] with [`FacadeError::InvalidArguments`] when
    /// the target's module or block is not in the document, or
    /// [`FacadeError::Internal`] when no fresh IDs could be made.
    pub fn paste_prepare(
        &self,
        clipboard_text: &str,
        document_json: &str,
        target: &PasteTarget,
        seed: [u8; 32],
    ) -> Result<Pasted, Failure> {
        let payload = b2c_model::load_clipboard(clipboard_text.as_bytes())?;
        let document = b2c_model::load(document_json.as_bytes())?;
        let module = document
            .modules
            .iter()
            .find(|module| module.id == target.module)
            .ok_or_else(|| {
                FacadeError::InvalidArguments(format!(
                    "the paste target's module {} is not in the document",
                    target.module
                ))
            })?;
        let target_block = match &target.block {
            Some(id) => Some(scope::find_block(&module.workspace.blocks, id).ok_or_else(|| {
                FacadeError::InvalidArguments(format!(
                    "the paste target's block {id} is not in module {}",
                    target.module
                ))
            })?),
            None => None,
        };

        let mut blocks = payload.blocks;
        // Fresh IDs must not be used in the document, and must not be one
        // that the payload refers to without declaring it: an unresolved
        // reference would otherwise find a pasted declaration by accident.
        let mut taken = b2c_model::used_ids(&document);
        taken.extend(
            b2c_model::outside_refs(&blocks)
                .iter()
                .map(|sym| sym.as_str().to_owned()),
        );
        taken.extend(payload.refs.keys().map(|sym| sym.as_str().to_owned()));
        b2c_model::remap_ids(&mut blocks, &taken, &mut b2c_model::SeededIds::new(seed))
            .map_err(|error| FacadeError::Internal(format!("no fresh IDs for the pasted blocks: {error}")))?;

        let (analysis, _) = self.analysis_for(&document);
        let visible = scope::visible_at(&analysis, &target.module, target_block, target.input.as_deref());
        let outside = b2c_model::outside_refs(&blocks);
        let binding = clipboard::rebind(&outside, &payload.refs, &visible);
        b2c_model::rewrite_refs(&mut blocks, &binding.rebound);
        let diagnostics =
            clipboard::unresolved_diagnostics(&blocks, &binding.unresolved, &payload.refs, &target.module);
        if target.block.is_some() {
            // Inside or after a block: no block can keep a loose stack there.
            blocks = clipboard::unstack(blocks);
        }
        Ok(Pasted {
            blocks,
            unresolved: binding.unresolved,
            diagnostics,
        })
    }
}
