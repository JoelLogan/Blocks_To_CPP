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
//!   value ([`BlockDef::output`]), which the editor's connection checker
//!   uses.
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
//! * An absent name or reference field (nothing chosen yet in the editor) is
//!   left to the analyser (ADR-0012), and [`resolve_report`] lists the blocks
//!   kept without a check, so the analyser does not repeat the catalog's
//!   problems (`b2c_lang::analyze_checked_with`).
//!
//! Added in milestone M3 (ADR-0017 §5, ADR-0018,
//! `docs/plans/m3-contracts/types.md` §4):
//! * A definition may declare a type signature: type variables with their
//!   classes (`where`), patterns on its output, fields and value inputs
//!   (`type`), `assign` inputs and `members` fields. The catalog checks it
//!   when it loads, and [`Catalog::sigs`] gives each block's
//!   [`b2c_ir::BlockSig`], which `b2c_lang` reads without depending on this
//!   crate.
//! * A type field accepts its `types` and every type its pattern allows,
//!   and a parameter row its `types` and its `type_class`; a type a newer
//!   catalog may have added is `B2C-E0602` in a document from that catalog.
//! * Headers come from the closed header table (`b2c_ir::Header`): a block's
//!   own, a dropdown option's (`headers` on the field) and the `header`
//!   field kind.
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
mod signature;
mod stack;
mod toolbox;
mod upgrade;

use std::collections::BTreeSet;
use std::sync::OnceLock;

use b2c_ir::sig::SigTable;
use b2c_ir::{BlockId, Diagnostic};
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
///
/// 1.2.0: *change* (`var.change`) has the menu `OP` (*by*, *down by*,
/// *times*, *divided by*, *mod*; default *by*), and *update* (`var.update`)
/// is retired: it is replaced by *change* on load (M2 polish W4-5,
/// ADR-0015). A file that leaves `OP` out loads as before.
///
/// 1.3.0 (every catalog change up to the M3 release ships as 1.3.0,
/// ADR-0017 §7): type signatures (`where`, `type` patterns, `assign`,
/// `members`, `type_class`, per-option `headers`) and the `header` field
/// kind; *create* takes every type a variable can hold, its value and
/// *set*'s value follow the variable's type, call arguments their
/// parameters' and *return*'s value the function's result; parameters take
/// every type a variable can hold; `std::string` is written `std::string`
/// in outputs; four blocks name only the headers all their forms need.
/// Files from 1.2.0 load unchanged.
pub const CATALOG_VERSION: &str = "1.3.0";

/// A loaded, validated catalog.
#[derive(Debug, Clone, PartialEq)]
pub struct Catalog {
    /// Catalog version.
    pub version: String,
    /// Block definitions by ID.
    pub blocks: std::collections::BTreeMap<String, BlockDef>,
    /// The signatures of the definitions, built by [`Catalog::new`].
    sigs: SigTable,
}

impl Catalog {
    /// A catalog of these definitions, with their signatures
    /// ([`Catalog::sigs`]). It does not check the definitions: [`resolve`]
    /// reports blocks whose definition is broken (`B2C-E0620`), and such a
    /// definition gets no signature.
    pub fn new(version: impl Into<String>, blocks: std::collections::BTreeMap<String, BlockDef>) -> Catalog {
        let sigs = signature::table(&blocks);
        Catalog {
            version: version.into(),
            blocks,
            sigs,
        }
    }

    /// The blocks' type signatures (ADR-0017 §5,
    /// `docs/plans/m3-contracts/types.md` §4.4), which `b2c_lang`'s
    /// `analyze_with` and `analyze_checked_with` read: one for every valid
    /// definition, by block type, with its parts in catalog order. They are
    /// built when the catalog is made, so a catalog whose `blocks` are
    /// changed afterwards keeps the signatures it was made with; make a new
    /// one with [`Catalog::new`] instead.
    ///
    /// ```
    /// use b2c_catalog::core_catalog;
    /// let sigs = core_catalog().sigs();
    /// assert_eq!(sigs.len(), core_catalog().blocks.len());
    /// // *set* binds `T` from its variable; its value is assigned to it.
    /// let set = sigs.get("var.set").expect("a signature");
    /// assert!(set.inputs["VALUE"].assign);
    /// ```
    pub fn sigs(&self) -> &SigTable {
        &self.sigs
    }
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
/// A name or reference field that is absent (a `symbol_decl` or
/// `symbol_ref` field without a value) is not reported: the editor saves a
/// block in which nothing is chosen yet that way, and the analyser reports
/// it (`B2C-E0430`) once the block is part of the program, so a parked or
/// disabled block without one never stops a build (ADR-0012).
///
/// [`resolve_report`] also tells which blocks were kept without a check.
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
    let report = resolve::run(document, catalog);
    (report.document, report.diagnostics)
}

/// What [`resolve_report`] found.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolveReport {
    /// The completed copy of the document, as [`resolve`] returns it.
    pub document: Document,
    /// Every problem found, as [`resolve`] returns them.
    pub diagnostics: Vec<Diagnostic>,
    /// The blocks kept exactly as they were, without a check against a
    /// definition: of an unknown type (`B2C-E0601`), made by a newer catalog
    /// (`B2C-E0602`), too old to upgrade (`B2C-E0603`), or of a type whose
    /// definition is broken (`B2C-E0620`; only the first such block carries
    /// the diagnostic). The blocks nested in them were checked. The analyser
    /// does not report the fields and inputs of these blocks again
    /// (`b2c_lang::analyze_checked_with`).
    pub unchecked: BTreeSet<BlockId>,
}

/// [`resolve`], also telling which blocks were kept without a check
/// ([`ResolveReport::unchecked`]), as the build and the editor's preview need
/// it to analyse the document without repeating the catalog's problems.
pub fn resolve_report(document: &Document, catalog: &Catalog) -> ResolveReport {
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
/// // Catalog 1.1.0 still had the *update* block, which 1.2.0 retired.
/// let bytes = br#"{
///   "format": "blocks2cpp/project", "formatVersion": 1,
///   "generator": {"app": "0.1.0", "catalog": "1.1.0"},
///   "project": {"id": "prj_demo", "name": "Demo", "language": {"standard": "c++20"}},
///   "modules": [{"id": "mod_main", "name": "main", "workspace": {"blocks": [
///     {"id": "b_main", "type": "program.main", "v": 1, "statements": {"BODY": [
///       {"id": "b_twice", "type": "var.update", "v": 1, "fields": {"OP": "mul"},
///        "inputs": {"VALUE": {"expr": [{"num": "2"}]}}}
///     ]}}
///   ]}}]
/// }"#;
/// let mut document = b2c_model::load(bytes).expect("a valid project");
/// let report = b2c_catalog::upgrade_document(&mut document, b2c_catalog::core_catalog());
/// assert_eq!(report.upgraded, 1);
/// assert!(report.diagnostics.is_empty());
/// // It is a *change* block now, with the same ID; its value is the amount.
/// let block = &document.modules[0].workspace.blocks[0].statements["BODY"][0];
/// assert_eq!((block.id.as_str(), block.block_type.as_str()), ("b_twice", "var.change"));
/// assert!(block.inputs.contains_key("BY"));
/// // Upgrading again changes nothing.
/// let again = b2c_catalog::upgrade_document(&mut document, b2c_catalog::core_catalog());
/// assert!(!again.changed());
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
