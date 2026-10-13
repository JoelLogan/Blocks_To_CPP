//! The block catalog: definitions of every block type (spec §3.11) and the
//! "resolve" pipeline stage that checks a project against them (spec §6.3).
//!
//! CONTRACT (implemented in milestone M1):
//! * [`core_catalog`] parses the TOML files under `catalog/core/` (embedded
//!   with `include_str!`), validates them (unique IDs, field/input/extra
//!   definitions consistent, dropdown defaults among the options, repeat and
//!   `when` keys refer to declared extras, default tokens well-formed) and
//!   caches the result. A broken built-in catalog is a bug: tests must load it.
//! * [`resolve`] checks every block of a document against the catalog and
//!   returns a copy of the document with absent inputs filled from catalog
//!   defaults, plus diagnostics (`DiagSource::Catalog`, codes `B2C-E06xx`):
//!   unknown block type or newer version; wrong shape for the position
//!   (top-level vs statement vs value input); unknown, missing or ill-typed
//!   fields (dropdown value not an option, type not allowed, decl/ref shape);
//!   inputs and statement inputs not matching the definition after expanding
//!   `repeat` counts and `when` flags; `extra` keys unknown, missing (when no
//!   default) or out of range; `params` rows well-formed.
//!
//! Added in milestone M2 for the editor:
//! * Every `reporter` and `predicate` definition declares the type of its
//!   value ([`BlockDef::output`], [`OutputType`]), which the editor's
//!   connection checker uses.
//! * [`toolbox`] parses `catalog/toolbox.toml` (embedded like the catalog)
//!   and checks it against the core catalog: the categories in toolbox
//!   order, their entries and presets, and the dynamic Variables and
//!   Functions categories ([`Toolbox`]).
//! * [`resolve`] also checks loose statement stacks: the blocks stacked below
//!   a block directly on the canvas (spec §5.4, ADR-0011).
//!
//! Added in the M2 polish wave (ADR-0015):
//! * [`upgrade_document`] and [`upgrade_clipboard`] upgrade, in memory, every
//!   block saved with an older version of its definition ([`BlockMigration`])
//!   and every block of a retired type ([`BlockReplacement`]) when a project,
//!   recovery snapshot or clipboard payload is loaded (spec §5.7), so the
//!   editor never shows an upgradable block as a placeholder. [`resolve`]
//!   runs the same upgrade first.
//! * A document or payload from a newer catalog (`generator.catalog`) has its
//!   unknown fields, inputs, `extra` settings and options reported as made by
//!   a newer catalog (`B2C-E0602`), not as unknown (spec §3.11.3).
//!
//! The editor never reads the TOML files: a test exports the validated
//! catalog and toolbox as `packages/catalog-gen/catalog.json`, from which
//! `packages/catalog-gen` generates the editor's block definitions and the
//! block reference (`tests/export.rs`).

mod codes;
mod definitions;
mod migrate;
mod resolve;
pub mod schema;
mod stack;
mod toolbox;
mod upgrade;

use std::sync::OnceLock;

use b2c_ir::Diagnostic;
use b2c_model::{Clipboard, Document};

pub use migrate::{BLOCK_MIGRATIONS, BlockMigration};
pub use schema::*;
pub use toolbox::{
    DynamicCategory, MAX_CATEGORY_ICON_CHARS, MAX_CATEGORY_NAME_CHARS, MAX_ENTRY_LABEL_CHARS,
    MAX_TOOLBOX_BYTES, MAX_TOOLBOX_ENTRIES, Preset, PresetExtra, Toolbox, ToolboxCategory, ToolboxEntry,
    ToolboxError, ToolboxProblem,
};
pub use upgrade::{BLOCK_REPLACEMENTS, BlockReplacement, BlockUpgrades, RETIRED_BLOCKS, UpgradeReport};

/// The catalog version (semver of the `catalog/` directory). Compatible changes
/// (a field, `extra` key or input with a default; a dropdown option; label, help
/// or colour text) raise the minor version, breaking ones a block's `version`
/// (spec §3.11.3).
///
/// 1.1.0: the labels name every field and the help texts mention ⊕/⊖ and the
/// loop menu (M2 polish W1-8, W1-17). Files from 1.0.0 load unchanged.
pub const CATALOG_VERSION: &str = "1.1.0";

/// A loaded, validated catalog.
#[derive(Debug, Clone, PartialEq)]
pub struct Catalog {
    /// Catalog version.
    pub version: String,
    /// Block definitions by ID.
    pub blocks: std::collections::BTreeMap<String, BlockDef>,
}

/// The built-in core catalog, parsed and checked on first use.
///
/// The catalog files are compiled in, so they cannot fail at run time unless
/// they were shipped broken; tests make sure they are not. Should that happen
/// anyway, the broken parts are left out and [`resolve`] reports the blocks
/// that use them, instead of panicking.
pub fn core_catalog() -> &'static Catalog {
    static CORE: OnceLock<Catalog> = OnceLock::new();
    CORE.get_or_init(|| definitions::build(&definitions::CORE_FILES).0)
}

/// The built-in toolbox (`catalog/toolbox.toml`), parsed and checked against
/// the [core catalog](core_catalog) on first use.
///
/// Like the catalog, the file is compiled in and tests keep it free of
/// problems. Should it be broken anyway, the categories and entries with
/// problems are left out instead of panicking.
///
/// ```
/// let toolbox = b2c_catalog::toolbox();
/// assert_eq!(toolbox.categories[0].id, b2c_catalog::Category::Program);
/// // Every block of the catalog can be reached.
/// assert_eq!(toolbox.reachable_blocks().len(), b2c_catalog::core_catalog().blocks.len());
/// ```
pub fn toolbox() -> &'static Toolbox {
    static TOOLBOX: OnceLock<Toolbox> = OnceLock::new();
    TOOLBOX.get_or_init(|| toolbox::build(toolbox::CORE_TOOLBOX, core_catalog()).0)
}

/// Checks a document against the catalog and fills absent inputs from defaults.
///
/// Returns the completed copy of the document and every problem found
/// (`DiagSource::Catalog`, codes `B2C-E06xx`), in document order. The copy's
/// blocks are upgraded first, as [`upgrade_document`] upgrades them; a block
/// that cannot be upgraded is `B2C-E0603` at its place in that order. When
/// the document's `generator.catalog` is newer than the catalog's version, a
/// block with a field, input, `extra` setting or dropdown option the
/// definition does not have is `B2C-E0602` (made by a newer catalog) and is
/// kept as it is, instead of being reported as unknown (spec §3.11.3,
/// ADR-0015). Besides absent value inputs (filled
/// with the catalog's default tokens), absent fields and absent `count`/`flag`
/// extras that have catalog defaults are filled in too, so later stages see
/// complete, current blocks. Blocks whose type is unknown, or that cannot be
/// upgraded, are kept unchanged (their children are still checked). The
/// catalog's own definitions are checked as well, so a hand-made [`Catalog`]
/// with a broken definition produces `B2C-E0620` instead of a panic.
/// Resolving the completed document again reports the same problems and
/// changes nothing.
///
/// ```
/// let bytes = br#"{
///   "format": "blocks2cpp/project", "formatVersion": 1,
///   "generator": {"app": "0.1.0", "catalog": "1.0.0"},
///   "project": {"id": "prj_demo", "name": "Demo", "language": {"standard": "c++20"}},
///   "modules": [{"id": "mod_main", "name": "main", "workspace": {"blocks": [
///     {"id": "b_main", "type": "program.main", "v": 1,
///      "statements": {"BODY": [{"id": "b_print", "type": "io.print", "v": 1}]}}
///   ]}}]
/// }"#;
/// let document = b2c_model::load(bytes).expect("a valid project");
/// let (completed, diagnostics) = b2c_catalog::resolve(&document, b2c_catalog::core_catalog());
/// assert!(diagnostics.is_empty());
/// // The print block got its default item, "Hello, world!".
/// let print = &completed.modules[0].workspace.blocks[0].statements["BODY"][0];
/// assert!(print.inputs.contains_key("ITEM0"));
/// ```
pub fn resolve(document: &Document, catalog: &Catalog) -> (Document, Vec<Diagnostic>) {
    resolve::run(document, catalog)
}

/// Upgrades, in memory, every block of a loaded document that was saved with
/// an older version of its definition, and every block of a retired type,
/// with the tables shipped with this catalog ([`BlockUpgrades::SHIPPED`];
/// spec §5.7, ADR-0015).
///
/// Run it after the document has passed `b2c_model::load` (every limit of
/// spec §5.6) and before trust is evaluated. An upgrade keeps each block's
/// ID, position, comment, flags, loose stack and nested blocks; it never adds
/// blocks, nesting or expression tokens, never changes the IDs of declared
/// symbols, never changes the security hash or security summary (spec
/// §8.3.1), and the upgraded document passes the limits of §5.6 again. A
/// block whose upgrade fails or would break one of these rules is left
/// exactly as it was and reported as `B2C-E0603`; if the document as a whole
/// would break them, no block is upgraded and each one is reported.
/// Upgrading is idempotent.
///
/// Blocks of an unknown type or of a newer version are left alone:
/// [`resolve`] reports them (`B2C-E0601`, `B2C-E0602`).
///
/// ```
/// let bytes = br#"{
///   "format": "blocks2cpp/project", "formatVersion": 1,
///   "generator": {"app": "0.1.0", "catalog": "1.1.0"},
///   "project": {"id": "prj_demo", "name": "Demo", "language": {"standard": "c++20"}},
///   "modules": [{"id": "mod_main", "name": "main", "workspace": {"blocks": [
///     {"id": "b_main", "type": "program.main", "v": 1}
///   ]}}]
/// }"#;
/// let mut document = b2c_model::load(bytes).expect("a valid project");
/// let report = b2c_catalog::upgrade_document(&mut document, b2c_catalog::core_catalog());
/// // Every block of this project is current.
/// assert!(!report.changed());
/// assert!(report.diagnostics.is_empty());
/// ```
pub fn upgrade_document(document: &mut Document, catalog: &Catalog) -> UpgradeReport {
    upgrade::document_report(document, catalog, &BlockUpgrades::SHIPPED)
}

/// [`upgrade_document`] with other tables: the shipped ones joined with
/// test-only migrations and replacements, in tests.
pub fn upgrade_document_with(
    document: &mut Document,
    catalog: &Catalog,
    upgrades: &BlockUpgrades<'_>,
) -> UpgradeReport {
    upgrade::document_report(document, catalog, upgrades)
}

/// Upgrades the blocks of a loaded clipboard payload like
/// [`upgrade_document`] (the whole payload passes `b2c_model::load_clipboard`
/// again; there is no trust to keep, because pasting is an edit in the app).
///
/// When the payload's `catalog` is newer than this catalog, each block with a
/// field, input, `extra` setting or dropdown option its definition does not
/// have is also reported, as `B2C-E0602` (made by a newer catalog), and kept
/// as it is (spec §3.11.3).
pub fn upgrade_clipboard(clipboard: &mut Clipboard, catalog: &Catalog) -> UpgradeReport {
    upgrade::clipboard(clipboard, catalog, &BlockUpgrades::SHIPPED)
}

/// [`upgrade_clipboard`] with other tables, in tests.
pub fn upgrade_clipboard_with(
    clipboard: &mut Clipboard,
    catalog: &Catalog,
    upgrades: &BlockUpgrades<'_>,
) -> UpgradeReport {
    upgrade::clipboard(clipboard, catalog, upgrades)
}
