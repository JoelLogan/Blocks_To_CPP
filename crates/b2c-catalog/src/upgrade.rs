//! Block upgrades on load (spec 05 §5.7, 03 §3.11.3, ADR-0015).
//!
//! When a project, a recovery snapshot or a clipboard payload is loaded,
//! every block saved with an older version of its definition, and every
//! block of a retired type, is upgraded in memory before the editor or the
//! compiler sees it:
//!
//! * a block whose type the catalog defines runs that type's chain of
//!   migrations ([`BlockMigration`]) up to the definition's version;
//! * a block of a retired type runs its own chain up to the version its
//!   replacement reads, then the replacement ([`BlockReplacement`]), which
//!   turns it into the successor type, and then the successor's chain.
//!
//! The walk is iterative, so the depth of a document never matters. Each
//! block is upgraded on its own: while its steps run, every nested block is
//! replaced with a small stand-in that carries its ID, and its loose stack is
//! taken out, so a step sees and copies only the block's own data. The walk
//! owns the block's type and version, and keeps its ID, position, comment and
//! flags whatever the steps do. It then checks what the steps made:
//!
//! * the nested blocks are exactly the ones it had, each once and unchanged
//!   (a step may move one to another input, as when an input is renamed),
//!   and no block was added below it;
//! * its expressions have no more tokens than before;
//! * it declares the same symbol IDs;
//! * it passes every limit of 05 §5.6 (`b2c_model::load_clipboard` of the
//!   block alone).
//!
//! A block whose upgrade fails, or breaks one of these rules, is left exactly
//! as it was and reported as `B2C-E0603`. When at least one block was
//! upgraded, the whole document is checked once more: its security hash and
//! security summary (08 §8.3.1) must be the ones it had, and it must load
//! (every limit of 05 §5.6 again, such as the nesting depth and the file
//! size). Otherwise every upgrade of the document is undone and each of its
//! blocks reported, so trust is always evaluated on content that equals the
//! file's.
//!
//! Upgrading is idempotent: an upgraded document has nothing left to
//! upgrade, and a block that could not be upgraded fails the same way again.

use std::collections::BTreeMap;

use b2c_ir::{BlockId, Diagnostic, Location, ModuleId};
use b2c_model::{Block, Clipboard, Document, FieldValue, Input, LoadError, SecuritySummary};
use serde_json::Value;

use crate::Catalog;
use crate::codes::{self, Diags};
use crate::definitions::{self, repeat_index};
use crate::migrate::{BLOCK_MIGRATIONS, BlockMigration};
use crate::resolve::{block_noun, describe_row_type, describe_type, quote, short_name};
use crate::schema::{BlockDef, ExtraKind, FieldKind};
use crate::signature;

/// A replacement that retires a block type (03 §3.11.3, ADR-0015): it turns
/// a block of type `from` at version `version` into a block of type `to` at
/// version `to_version`.
///
/// Like a [`BlockMigration`] step, it may rewrite the block's own fields,
/// value inputs (including renaming one, with the block in it), statement
/// inputs and `extra` settings, and the walk owns the rest: it sets the new
/// type and version, keeps the ID, position, comment, flags, loose stack
/// and nested blocks, and refuses a result that breaks the rules of
/// [`crate::upgrade_document`].
#[derive(Debug, Clone, Copy)]
pub struct BlockReplacement {
    /// The retired block type (listed in [`RETIRED_BLOCKS`]).
    pub from: &'static str,
    /// The version of the retired type that this replacement reads; older
    /// versions run the retired type's own migrations first.
    pub version: u32,
    /// The successor block type.
    pub to: &'static str,
    /// The version of the successor that the replacement produces; the
    /// successor's own migrations take it on from there.
    pub to_version: u32,
    /// The replacement. An error is a plain-English reason, shown after "…
    /// failed: ".
    pub apply: fn(&mut Block) -> Result<(), String>,
}

/// The replacements shipped with this catalog: one for each retired type.
/// Each one has a golden-file test in `crates/b2c-catalog/tests/migrations/`.
///
/// * `var.update` v1 → `var.change` v1 (catalog 1.2.0, M2 polish W4-5):
///   *change* took over the update operators as its menu `OP`
///   (`update_to_change`).
pub const BLOCK_REPLACEMENTS: &[BlockReplacement] = &[BlockReplacement {
    from: "var.update",
    version: 1,
    to: "var.change",
    to_version: 1,
    apply: update_to_change,
}];

/// The block types that no longer exist. A retired ID is never used for a
/// new block type again (a test checks that the catalog defines none of
/// them). Before 1.0 each one has an exact replacement in
/// [`BLOCK_REPLACEMENTS`].
pub const RETIRED_BLOCKS: &[&str] = &["var.update"];

/// Each operator of `var.update` v1 (its field `OP`: `+=`, `-=`, `*=`, `/=`
/// and `%=`) with the option of `var.change` v1's field `OP` that does the
/// same (*by*, *down by*, *times*, *divided by* and *mod*).
const UPDATE_OPS: [(&str, &str); 5] = [
    ("add", "add"),
    ("sub", "sub"),
    ("mul", "mul"),
    ("div", "div"),
    ("mod", "mod"),
];

/// Replaces an *update* block (`var.update` v1, `[score ▾] [*= ▾] (2)`) with
/// the *change* block that does the same (`var.change` v1,
/// `change [score ▾] [times ▾] (2)`): its value input `VALUE` becomes `BY`,
/// with the expression or block in it, and its operator becomes the same
/// option of *change*'s menu. The variable is kept, and so is anything else
/// a damaged file has, for the catalog to report. Both blocks lower to the
/// same compound assignment, so the generated C++ does not change.
///
/// An absent `OP` or `VALUE` stays absent: both blocks have the same
/// defaults (`add`, and the number 1). It fails, and the block is kept as it
/// was (`B2C-E0603`), when the operator is not one of `var.update`'s or the
/// block already has an input `BY`.
fn update_to_change(block: &mut Block) -> Result<(), String> {
    if block.inputs.contains_key("BY") {
        return Err(String::from("it already has an input \"BY\""));
    }
    match block.fields.get_mut("OP") {
        None => {}
        Some(FieldValue::Text(op)) => {
            let (_, option) = UPDATE_OPS
                .iter()
                .find(|(update, _)| update == op)
                .ok_or_else(|| format!("its operator {} is not one of +=, -=, *=, /= and %=", quote(op)))?;
            *op = String::from(*option);
        }
        Some(_) => return Err(String::from("its operator is not a choice from its menu")),
    }
    if let Some(value) = block.inputs.remove("VALUE") {
        block.inputs.insert(String::from("BY"), value);
    }
    Ok(())
}

/// The tables a block upgrade runs with: [`BlockUpgrades::SHIPPED`], or
/// others in tests.
#[derive(Debug, Clone, Copy)]
pub struct BlockUpgrades<'a> {
    /// The migrations, from one version of a type to the next.
    pub migrations: &'a [BlockMigration],
    /// The replacements of retired types.
    pub replacements: &'a [BlockReplacement],
    /// The retired types.
    pub retired: &'a [&'a str],
}

impl BlockUpgrades<'static> {
    /// The tables shipped with this catalog: [`BLOCK_MIGRATIONS`],
    /// [`BLOCK_REPLACEMENTS`] and [`RETIRED_BLOCKS`].
    pub const SHIPPED: Self = Self {
        migrations: BLOCK_MIGRATIONS,
        replacements: BLOCK_REPLACEMENTS,
        retired: RETIRED_BLOCKS,
    };
}

impl Default for BlockUpgrades<'static> {
    /// [`BlockUpgrades::SHIPPED`].
    fn default() -> Self {
        Self::SHIPPED
    }
}

impl BlockUpgrades<'_> {
    /// Whether `block_type` is retired: listed as retired, or replaced by a
    /// replacement.
    pub fn is_retired(&self, block_type: &str) -> bool {
        self.retired.contains(&block_type) || self.replacements.iter().any(|r| r.from == block_type)
    }

    /// The migration of `block_type` from version `from`.
    fn migration(&self, block_type: &str, from: u32) -> Option<&BlockMigration> {
        self.migrations
            .iter()
            .find(|step| step.block == block_type && step.from == from)
    }

    /// The replacement of `block_type` at version `version`.
    fn replacement(&self, block_type: &str, version: u32) -> Option<&BlockReplacement> {
        self.replacements
            .iter()
            .find(|replacement| replacement.from == block_type && replacement.version == version)
    }
}

/// What an upgrade did.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct UpgradeReport {
    /// How many blocks were upgraded. More than zero means the document
    /// changed: it has unsaved changes, and its upgraded form is written
    /// only when the user saves it (05 §5.7).
    pub upgraded: usize,
    /// One `B2C-E0603` for each block that needed an upgrade and was left as
    /// it was, because the upgrade failed or would break its rules. For a
    /// clipboard payload from a newer catalog, also one `B2C-E0602` for each
    /// block with a field, input, `extra` setting or option that this
    /// catalog does not know.
    pub diagnostics: Vec<Diagnostic>,
}

impl UpgradeReport {
    /// Whether any block was upgraded.
    pub fn changed(&self) -> bool {
        self.upgraded > 0
    }
}

/// The most steps the upgrade of one block may take; more means the tables
/// go round in a circle.
const MAX_STEPS: u32 = 1_000;

/// The type of the stand-in that takes the place of a nested block while its
/// parent is upgraded. It is a valid block type for the loader, so the
/// parent can be checked against 05 §5.6 with its stand-ins in place.
const STAND_IN: &str = "b2c.kept";

/// A block that was upgraded, as it was before.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Upgraded {
    module: Option<ModuleId>,
    block: BlockId,
    block_type: String,
    v: u32,
}

/// A block that could not be upgraded, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Failure {
    /// Its module (none in a clipboard payload).
    pub(crate) module: Option<ModuleId>,
    /// The block.
    pub(crate) block: BlockId,
    /// Why, as the whole `B2C-E0603` message.
    pub(crate) message: String,
}

/// What a walk did.
#[derive(Debug, Default)]
pub(crate) struct Outcome {
    upgraded: Vec<Upgraded>,
    /// The blocks that could not be upgraded, in walk order.
    pub(crate) failed: Vec<Failure>,
}

impl Outcome {
    /// Undoes every upgrade (the caller has restored the blocks) and reports
    /// each upgraded block as failed: the upgrade as a whole would `what`.
    fn undo(&mut self, catalog: &Catalog, what: &str) {
        for upgraded in std::mem::take(&mut self.upgraded) {
            let message = format!(
                "{}, and upgrading the old blocks would {what}, so it was left as it was. This is a bug in Blocks2Cpp.",
                subject(catalog, &upgraded.block_type, upgraded.v)
            );
            self.failed.push(Failure {
                module: upgraded.module,
                block: upgraded.block,
                message,
            });
        }
    }

    /// The report: the number of upgraded blocks and a `B2C-E0603` for each
    /// failure, at most `codes::MAX_DIAGNOSTICS` of them.
    fn report(self, mut diags: Diags) -> UpgradeReport {
        for failure in self.failed {
            diags.error(
                codes::OLD_BLOCK,
                Location::block(failure.module, failure.block),
                failure.message,
            );
        }
        UpgradeReport {
            upgraded: self.upgraded.len(),
            diagnostics: diags.finish(),
        }
    }
}

/// Upgrades the blocks of a document; see [`crate::upgrade_document`].
pub(crate) fn document(document: &mut Document, catalog: &Catalog, tables: &BlockUpgrades<'_>) -> Outcome {
    let mut walk = Walk {
        catalog,
        tables,
        outcome: Outcome::default(),
    };
    let anything = document
        .modules
        .iter()
        .any(|module| any_block(&module.workspace.blocks, |block| walk.needs_upgrade(block)));
    if !anything {
        return walk.outcome;
    }
    let original = document.clone();
    let trust = (
        b2c_model::security_hash(document),
        b2c_model::security_summary(document),
    );
    for module in &mut document.modules {
        walk.blocks(&mut module.workspace.blocks, Some(&module.id));
    }
    if !walk.outcome.upgraded.is_empty()
        && let Err(what) = recheck_document(document, &trust)
    {
        *document = original;
        walk.outcome.undo(catalog, &what);
    }
    walk.outcome
}

/// Upgrades the blocks of a document and reports what happened; see
/// [`crate::upgrade_document`].
pub(crate) fn document_report(
    document: &mut Document,
    catalog: &Catalog,
    tables: &BlockUpgrades<'_>,
) -> UpgradeReport {
    self::document(document, catalog, tables).report(Diags::default())
}

/// Upgrades the blocks of a clipboard payload; see
/// [`crate::upgrade_clipboard`].
pub(crate) fn clipboard(
    clipboard: &mut Clipboard,
    catalog: &Catalog,
    tables: &BlockUpgrades<'_>,
) -> UpgradeReport {
    let mut walk = Walk {
        catalog,
        tables,
        outcome: Outcome::default(),
    };
    if any_block(&clipboard.blocks, |block| walk.needs_upgrade(block)) {
        let original = clipboard.blocks.clone();
        walk.blocks(&mut clipboard.blocks, None);
        if !walk.outcome.upgraded.is_empty() {
            let text = b2c_model::to_canonical_clipboard_json(clipboard);
            if let Err(error) = b2c_model::load_clipboard(text.as_bytes()) {
                clipboard.blocks = original;
                walk.outcome.undo(catalog, &limit_problem(&error));
            }
        }
    }
    let mut diags = Diags::default();
    if b2c_model::is_newer_version(&clipboard.catalog, &catalog.version) {
        each_block(&clipboard.blocks, |block| {
            if let Some(def) = catalog.blocks.get(&block.block_type)
                && block.v == def.version
                && let Some(message) = newer_catalog_problem(def, block)
            {
                diags.error(
                    codes::NEWER_BLOCK,
                    Location::block(None, block.id.clone()),
                    message,
                );
            }
        });
    }
    walk.outcome.report(diags)
}

/// The security hash and summary of a document (08 §8.3.1).
type TrustView = ([u8; 32], SecuritySummary);

/// Checks an upgraded document as a whole: what trust looks at is what it
/// was, and the document loads (05 §5.6). The error says what the upgrade
/// would do.
fn recheck_document(document: &Document, trust: &TrustView) -> Result<(), String> {
    if b2c_model::security_hash(document) != trust.0 || b2c_model::security_summary(document) != trust.1 {
        return Err(String::from(
            "change what workspace trust looks at (Raw C++, libraries or blocks that use files)",
        ));
    }
    let text = b2c_model::to_canonical_json(document);
    b2c_model::load(text.as_bytes())
        .map(drop)
        .map_err(|error| limit_problem(&error))
}

/// "break a limit of project files (…)", with the loader's first problem.
fn limit_problem(error: &LoadError) -> String {
    let problem = error
        .diagnostics
        .first()
        .map_or("", |diagnostic| diagnostic.message.trim_end_matches('.'));
    format!("break a limit of project files ({problem})")
}

/// The start of every `B2C-E0603` message: *This "print" block is version 2*
/// for a type the catalog defines, *This block is version 1 of a retired
/// kind of block* for one it does not. The catalog ID is never shown
/// (03 §3.11.1).
fn subject(catalog: &Catalog, block_type: &str, v: u32) -> String {
    match catalog.blocks.get(block_type) {
        Some(def) => format!("This {} is version {v}", block_noun(def)),
        None => format!("This block is version {v} of a retired kind of block"),
    }
}

/// How a `B2C-E0603` message names the type that replaces a retired one:
/// *the "change" block*, or *another kind of block* for a type without a
/// short name.
fn successor(catalog: &Catalog, block_type: &str) -> String {
    catalog.blocks.get(block_type).and_then(short_name).map_or_else(
        || String::from("another kind of block"),
        |name| format!("the {} block", quote(name)),
    )
}

/// The `B2C-E0603` message for a block that needs an upgrade when the walk
/// left no message for it (resolve uses it for a document whose block IDs
/// are not unique, which only a document built in memory can have).
pub(crate) fn cannot_upgrade(catalog: &Catalog, block: &Block) -> String {
    format!(
        "{}, which this version of Blocks2Cpp cannot upgrade.",
        subject(catalog, &block.block_type, block.v)
    )
}

/// Whether any block of `blocks`, or nested in them, satisfies `test`. The
/// blocks are tested in document order: parents first, then the blocks in
/// their value inputs, their statement lists and their loose stack.
fn any_block(blocks: &[Block], mut test: impl FnMut(&Block) -> bool) -> bool {
    let mut pending: Vec<&Block> = blocks.iter().rev().collect();
    while let Some(block) = pending.pop() {
        if test(block) {
            return true;
        }
        pending.extend(block.stack.iter().rev());
        let nested: Vec<&Block> = children(block).collect();
        pending.extend(nested.into_iter().rev());
    }
    false
}

/// Calls `visit` for every block of `blocks` and every block nested in them.
fn each_block(blocks: &[Block], mut visit: impl FnMut(&Block)) {
    any_block(blocks, |block| {
        visit(block);
        false
    });
}

/// The blocks directly inside a block: in its value inputs and statement
/// lists (not its loose stack).
fn children(block: &Block) -> impl Iterator<Item = &Block> {
    let nested = block.inputs.values().filter_map(|input| match input {
        Input::Block(nested) => Some(&*nested.block),
        Input::Expr(_) => None,
    });
    nested.chain(block.statements.values().flatten())
}

/// [`children`], mutably.
fn children_mut(block: &mut Block) -> impl Iterator<Item = &mut Block> {
    nested_mut(&mut block.inputs, &mut block.statements)
}

/// The blocks in a block's value inputs and statement lists, mutably.
fn nested_mut<'a>(
    inputs: &'a mut BTreeMap<String, Input>,
    statements: &'a mut BTreeMap<String, Vec<Block>>,
) -> impl Iterator<Item = &'a mut Block> {
    let nested = inputs.values_mut().filter_map(|input| match input {
        Input::Block(nested) => Some(&mut *nested.block),
        Input::Expr(_) => None,
    });
    nested.chain(statements.values_mut().flatten())
}

/// One walk over blocks.
struct Walk<'c, 't> {
    catalog: &'c Catalog,
    tables: &'t BlockUpgrades<'t>,
    outcome: Outcome,
}

impl Walk<'_, '_> {
    /// Whether a block needs an upgrade: an older version of a type the
    /// catalog defines, or a retired type the catalog does not define.
    fn needs_upgrade(&self, block: &Block) -> bool {
        match self.catalog.blocks.get(&block.block_type) {
            Some(def) => block.v < def.version,
            None => self.tables.is_retired(&block.block_type),
        }
    }

    /// Upgrades `blocks` and every block nested in them, in the order of
    /// [`any_block`].
    fn blocks(&mut self, blocks: &mut [Block], module: Option<&ModuleId>) {
        let mut pending: Vec<&mut Block> = blocks.iter_mut().rev().collect();
        while let Some(block) = pending.pop() {
            if self.needs_upgrade(block) {
                self.upgrade(block, module);
            }
            let Block {
                inputs,
                statements,
                stack,
                ..
            } = block;
            pending.extend(stack.iter_mut().rev());
            let nested: Vec<&mut Block> = nested_mut(inputs, statements).collect();
            pending.extend(nested.into_iter().rev());
        }
    }

    /// Upgrades one block's own data and records the outcome.
    fn upgrade(&mut self, block: &mut Block, module: Option<&ModuleId>) {
        let before = Upgraded {
            module: module.cloned(),
            block: block.id.clone(),
            block_type: block.block_type.clone(),
            v: block.v,
        };
        match upgrade_block(block, self.catalog, self.tables) {
            Ok(()) => self.outcome.upgraded.push(before),
            Err(message) => self.outcome.failed.push(Failure {
                module: before.module,
                block: before.block,
                message,
            }),
        }
    }
}

/// A block's nested blocks and loose stack, taken out while its own data is
/// upgraded.
struct Detached {
    /// The nested blocks, in the order of [`children_mut`]; the stand-in of
    /// the one at index `i` has version `i`.
    children: Vec<Block>,
    /// Their IDs, by the same index.
    ids: Vec<BlockId>,
    /// The loose stack.
    stack: Vec<Block>,
}

/// The stand-in for the nested block `id` at index `index`.
fn stand_in(id: &BlockId, index: u32) -> Block {
    Block {
        id: id.clone(),
        block_type: String::from(STAND_IN),
        v: index,
        x: None,
        y: None,
        collapsed: false,
        disabled: false,
        comment: None,
        extra: BTreeMap::new(),
        fields: BTreeMap::new(),
        inputs: BTreeMap::new(),
        statements: BTreeMap::new(),
        stack: Vec::new(),
    }
}

impl Detached {
    /// Takes the nested blocks out of `block`, leaving stand-ins, and takes
    /// its loose stack. `None` (and nothing taken) when it has more nested
    /// blocks than a version number can count.
    fn take(block: &mut Block) -> Option<Self> {
        if u32::try_from(children(block).count()).is_err() {
            return None;
        }
        let mut detached = Self {
            children: Vec::new(),
            ids: Vec::new(),
            stack: std::mem::take(&mut block.stack),
        };
        for (index, slot) in (0..=u32::MAX).zip(children_mut(block)) {
            let child = std::mem::replace(slot, stand_in(&slot.id, index));
            detached.ids.push(child.id.clone());
            detached.children.push(child);
        }
        Some(detached)
    }

    /// Puts the nested blocks back in place of their stand-ins, wherever the
    /// stand-ins are now, and the loose stack back.
    fn put_back(self, block: &mut Block) {
        let mut children: Vec<Option<Block>> = self.children.into_iter().map(Some).collect();
        for slot in children_mut(block) {
            let child = usize::try_from(slot.v)
                .ok()
                .and_then(|index| children.get_mut(index))
                .and_then(Option::take);
            if let Some(child) = child {
                *slot = child;
            }
        }
        block.stack = self.stack;
    }
}

/// Upgrades one block's own data: its nested blocks and stack are taken out
/// meanwhile and put back afterwards. On failure the block is as it was.
fn upgrade_block(block: &mut Block, catalog: &Catalog, tables: &BlockUpgrades<'_>) -> Result<(), String> {
    let Some(detached) = Detached::take(block) else {
        return Err(format!(
            "{}, and it holds too many blocks to be upgraded.",
            subject(catalog, &block.block_type, block.v)
        ));
    };
    let result = upgrade_own_data(block, &detached.ids, catalog, tables);
    detached.put_back(block);
    result
}

/// Runs the steps on a copy of a block whose nested blocks are stand-ins,
/// checks the result and keeps it only when every check passes.
fn upgrade_own_data(
    block: &mut Block,
    kept: &[BlockId],
    catalog: &Catalog,
    tables: &BlockUpgrades<'_>,
) -> Result<(), String> {
    let mut upgraded = block.clone();
    run_steps(&mut upgraded, catalog, tables)?;
    // The walk owns what an upgrade keeps.
    upgraded.id.clone_from(&block.id);
    upgraded.x = block.x;
    upgraded.y = block.y;
    upgraded.collapsed = block.collapsed;
    upgraded.disabled = block.disabled;
    upgraded.comment.clone_from(&block.comment);
    check_kept(block, &upgraded, kept).map_err(|what| {
        format!(
            "{}, and upgrading it would {what}, so it was left as it was. This is a bug in Blocks2Cpp.",
            subject(catalog, &block.block_type, block.v)
        )
    })?;
    *block = upgraded;
    Ok(())
}

/// Runs migrations and replacements until the block is current. The error is
/// the whole `B2C-E0603` message.
fn run_steps(block: &mut Block, catalog: &Catalog, tables: &BlockUpgrades<'_>) -> Result<(), String> {
    let subject = subject(catalog, &block.block_type, block.v);
    for _ in 0..MAX_STEPS {
        let current = block.block_type.clone();
        let from = block.v;
        if let Some(def) = catalog.blocks.get(&current) {
            if from >= def.version {
                return Ok(());
            }
            let step = tables.migration(&current, from).ok_or_else(|| {
                format!(
                    "{subject}, which this version of Blocks2Cpp cannot upgrade to version {}.",
                    def.version
                )
            })?;
            migrate(block, step, &subject)?;
        } else if let Some(replacement) = tables.replacement(&current, from) {
            (replacement.apply)(block).map_err(|reason| {
                format!(
                    "{subject}, which is now {}, and replacing it failed: {reason}.",
                    successor(catalog, replacement.to)
                )
            })?;
            block.block_type = String::from(replacement.to);
            block.v = replacement.to_version;
        } else if tables.is_retired(&current) {
            // A retired type older than the version its replacement reads
            // runs its own migrations first.
            let step = tables
                .migration(&current, from)
                .ok_or_else(|| format!("{subject}, and this version of Blocks2Cpp cannot upgrade it."))?;
            migrate(block, step, &subject)?;
        } else {
            return Err(format!(
                "{subject}, and upgrading it leads to the block type {}, which this version of Blocks2Cpp does not know.",
                quote(&current)
            ));
        }
    }
    Err(format!(
        "{subject}, and upgrading it does not come to an end. This is a bug in Blocks2Cpp."
    ))
}

/// Runs one migration step; the walk sets the type and the next version.
fn migrate(block: &mut Block, step: &BlockMigration, subject: &str) -> Result<(), String> {
    let from = step.from;
    let next = from
        .checked_add(1)
        .ok_or_else(|| format!("{subject}, which cannot be upgraded past version {from}."))?;
    (step.apply)(block)
        .map_err(|reason| format!("{subject}, and upgrading it from version {from} failed: {reason}."))?;
    block.block_type = String::from(step.block);
    block.v = next;
    Ok(())
}

/// Checks what an upgrade made of a block's own data (see the module
/// comment). The error says what the upgrade would do.
fn check_kept(original: &Block, upgraded: &Block, kept: &[BlockId]) -> Result<(), String> {
    let mut found = vec![false; kept.len()];
    for nested in children(upgraded) {
        let index = usize::try_from(nested.v).ok();
        let slot = index.and_then(|index| Some((found.get_mut(index)?, kept.get(index)?)));
        let Some((seen, id)) = slot.filter(|(seen, id)| !**seen && nested.id == **id) else {
            // Not a stand-in, or one that is there twice.
            return Err(String::from("add a block"));
        };
        if *nested != stand_in(id, nested.v) {
            return Err(String::from("change a block inside it"));
        }
        *seen = true;
    }
    if found.contains(&false) {
        return Err(String::from("remove a block from inside it"));
    }
    if !upgraded.stack.is_empty() {
        return Err(String::from("add blocks below it"));
    }
    if tokens(upgraded) > tokens(original) {
        return Err(String::from("make the values in its inputs longer"));
    }
    if declared(upgraded) != declared(original) {
        return Err(String::from("change the ID of a name it declares"));
    }
    check_limits(upgraded)
}

/// The number of expression tokens in a block's own value inputs.
fn tokens(block: &Block) -> usize {
    block
        .inputs
        .values()
        .map(|input| match input {
            Input::Expr(expr) => expr.expr.len(),
            Input::Block(_) => 0,
        })
        .sum()
}

/// The symbol IDs a block's own data declares, sorted: its declaration
/// fields and the `sym` of every row of a list in `extra` (function
/// parameters).
fn declared(block: &Block) -> Vec<&str> {
    let mut symbols: Vec<&str> = block
        .fields
        .values()
        .filter_map(|value| match value {
            FieldValue::Decl(decl) => Some(decl.sym.as_str()),
            _ => None,
        })
        .collect();
    for value in block.extra.values() {
        if let Value::Array(rows) = value {
            symbols.extend(rows.iter().filter_map(|row| row.get("sym")?.as_str()));
        }
    }
    symbols.sort_unstable();
    symbols
}

/// Checks a block's own data (with stand-ins for its nested blocks) against
/// the limits and rules of 05 §5.6, as the loader checks a copied block.
fn check_limits(block: &Block) -> Result<(), String> {
    let payload = Clipboard {
        catalog: String::from(crate::CATALOG_VERSION),
        blocks: vec![block.clone()],
        refs: BTreeMap::new(),
    };
    let text = b2c_model::to_canonical_clipboard_json(&payload);
    b2c_model::load_clipboard(text.as_bytes())
        .map(drop)
        .map_err(|error| limit_problem(&error))
}

/// Whether a block that a newer catalog may have extended compatibly has
/// something this definition does not know (03 §3.11.3, ADR-0015): a field,
/// a dropdown option, a type in a type field or a parameter row that this
/// catalog does not know or does not allow there (a new type, or a widened
/// field), a header of a newer header table, an `extra` setting, a value
/// input or a statement input. The message for `B2C-E0602` names the first
/// such thing. A value that no catalog could have written (a type name that
/// breaks the name rule, a template with the wrong arguments) is not one: it
/// stays `B2C-E0607` or `E0614`.
pub(crate) fn newer_catalog_problem(def: &BlockDef, block: &Block) -> Option<String> {
    let field = block.fields.iter().find_map(|(name, value)| {
        let Some(field) = definitions::field_def(def, name) else {
            return Some(format!("the field {}", quote(name)));
        };
        match value {
            FieldValue::Text(text)
                if field.kind == FieldKind::Dropdown
                    && !field.options.iter().any(|[_, option]| option == text) =>
            {
                Some(format!("the choice {} in its field {}", quote(text), quote(name)))
            }
            // A new type, or a type field a newer catalog widened.
            _ if field.kind == FieldKind::Type
                && signature::field_type_fit(def, field, value).may_be_newer() =>
            {
                Some(format!(
                    "the type {} in its field {}",
                    describe_type(value),
                    quote(name)
                ))
            }
            // A header of a newer standard, in a newer header table.
            FieldValue::Text(text)
                if field.kind == FieldKind::Header
                    && !signature::header_fits(text)
                    && signature::looks_like_header(text) =>
            {
                Some(format!("the header {} in its field {}", quote(text), quote(name)))
            }
            _ => None,
        }
    });
    let what = field
        .or_else(|| {
            let extra = block
                .extra
                .keys()
                .find(|key| !def.extra.iter().any(|e| &e.name == *key))?;
            Some(format!("the setting {}", quote(extra)))
        })
        .or_else(|| newer_row_type(def, block))
        .or_else(|| {
            let input = block.inputs.keys().find(|name| {
                !def.input.iter().any(|input| match input.repeat {
                    None => input.name == **name,
                    Some(_) => repeat_index(name, &input.name).is_some(),
                })
            })?;
            Some(format!("the input {}", quote(input)))
        })
        .or_else(|| {
            let part = block.statements.keys().find(|name| {
                !def.statement.iter().any(|statement| match statement.repeat {
                    None => statement.name == **name,
                    Some(_) => repeat_index(name, &statement.name).is_some(),
                })
            })?;
            Some(format!("the part {}", quote(part)))
        })?;
    Some(format!(
        "This {} was made with a newer version of Blocks2Cpp: it has {what}, which this version does not know. Update Blocks2Cpp to use it; until then the block is kept as it is.",
        block_noun(def)
    ))
}

/// The first parameter or field row whose `type` a newer catalog may have
/// added or allowed, as [`newer_catalog_problem`] names it.
fn newer_row_type(def: &BlockDef, block: &Block) -> Option<String> {
    def.extra
        .iter()
        .filter(|extra| extra.kind.has_row_types())
        .find_map(|extra| {
            let noun = if extra.kind == ExtraKind::Params {
                "parameter"
            } else {
                "field"
            };
            let rows = block.extra.get(&extra.name)?.as_array()?;
            rows.iter().enumerate().find_map(|(index, row)| {
                let ty = row.get("type")?;
                signature::row_type_fit(extra, ty)
                    .may_be_newer()
                    .then(|| format!("the type {} in {noun} {}", describe_row_type(Some(ty)), index + 1))
            })
        })
}

#[cfg(test)]
mod tests {
    use b2c_model::{BlockComment, BlockInput, ExprInput, SymbolDecl, Token};
    use serde_json::json;

    use super::*;

    /// A block from its JSON form (tests only; files go through the loader).
    fn block(value: Value) -> Block {
        serde_json::from_value(value).unwrap()
    }

    fn print(id: &str, v: u32) -> Value {
        json!({"id": id, "type": "io.print", "v": v, "fields": {"SEP": "space"}})
    }

    /// The core catalog with `io.print` at version 3.
    fn catalog_v3() -> Catalog {
        let mut catalog = crate::core_catalog().clone();
        catalog.blocks.get_mut("io.print").unwrap().version = 3;
        catalog
    }

    /// v1 → v2: the separator `space` is now written `spaces`.
    fn rename_separator(block: &mut Block) -> Result<(), String> {
        match block.fields.get_mut("SEP") {
            Some(FieldValue::Text(sep)) if sep == "space" => *sep = String::from("spaces"),
            Some(FieldValue::Text(sep)) if sep == "bad" => {
                return Err(String::from("the separator is unknown"));
            }
            _ => {}
        }
        Ok(())
    }

    /// v2 → v3: back to `space`, so the result passes the real catalog.
    #[allow(clippy::unnecessary_wraps, reason = "the signature every step has")]
    fn rename_back(block: &mut Block) -> Result<(), String> {
        if let Some(FieldValue::Text(sep)) = block.fields.get_mut("SEP")
            && sep == "spaces"
        {
            *sep = String::from("space");
        }
        Ok(())
    }

    /// A step that tries to change everything the walk owns.
    #[allow(clippy::unnecessary_wraps, reason = "the signature every step has")]
    fn meddle(block: &mut Block) -> Result<(), String> {
        block.id = BlockId::new("other").unwrap();
        block.block_type = String::from("x.other");
        block.v = 99;
        block.x = Some(1);
        block.y = Some(2);
        block.collapsed = !block.collapsed;
        block.disabled = !block.disabled;
        block.comment = None;
        Ok(())
    }

    /// `test.say` v1 → `io.print` v1: `WHAT` becomes `ITEM0`.
    fn say_to_print(block: &mut Block) -> Result<(), String> {
        let what = block.inputs.remove("WHAT").ok_or("the block has no WHAT input")?;
        block.inputs.insert(String::from("ITEM0"), what);
        block.extra.clear();
        block.fields.clear();
        Ok(())
    }

    /// `test.say` v0 → v1: drops the old `LOUD` flag.
    #[allow(clippy::unnecessary_wraps, reason = "the signature every step has")]
    fn drop_loud(block: &mut Block) -> Result<(), String> {
        block.fields.remove("LOUD");
        Ok(())
    }

    const CHAIN: &[BlockMigration] = &[
        BlockMigration {
            block: "io.print",
            from: 2,
            apply: rename_back,
        },
        BlockMigration {
            block: "io.print",
            from: 1,
            apply: rename_separator,
        },
        BlockMigration {
            block: "test.say",
            from: 0,
            apply: drop_loud,
        },
    ];

    const REPLACEMENTS: &[BlockReplacement] = &[BlockReplacement {
        from: "test.say",
        version: 1,
        to: "io.print",
        to_version: 1,
        apply: say_to_print,
    }];

    const TABLES: BlockUpgrades<'static> = BlockUpgrades {
        migrations: CHAIN,
        replacements: REPLACEMENTS,
        retired: &["test.say"],
    };

    /// Upgrades one top-level block list with the test tables.
    fn walk(blocks: &mut [Block], catalog: &Catalog, tables: &BlockUpgrades<'_>) -> Outcome {
        let mut walk = Walk {
            catalog,
            tables,
            outcome: Outcome::default(),
        };
        walk.blocks(blocks, None);
        walk.outcome
    }

    fn messages(outcome: &Outcome) -> Vec<&str> {
        outcome.failed.iter().map(|f| f.message.as_str()).collect()
    }

    fn with(tables: &[BlockMigration]) -> BlockUpgrades<'_> {
        BlockUpgrades {
            migrations: tables,
            replacements: &[],
            retired: &[],
        }
    }

    #[test]
    fn a_chain_upgrades_step_by_step() {
        let catalog = catalog_v3();
        let mut blocks = vec![
            block(print("b1", 1)),
            block(print("b2", 2)),
            block(print("b3", 3)),
        ];
        let outcome = walk(&mut blocks, &catalog, &TABLES);
        assert_eq!(outcome.upgraded.len(), 2);
        assert!(outcome.failed.is_empty());
        for upgraded in &blocks {
            assert_eq!(upgraded.v, 3);
            assert_eq!(upgraded.fields["SEP"], FieldValue::Text("space".into()));
        }
        // Only the first step ran on b2's way: v2 → v3 keeps "space".
        let mut once = vec![block(print("b1", 1))];
        walk(&mut once, &catalog, &with(&CHAIN[1..2]));
        assert_eq!(once[0].v, 1, "a missing step leaves the block unchanged");
        // Idempotent: nothing is left to do.
        assert_eq!(walk(&mut blocks, &catalog, &TABLES).upgraded.len(), 0);
    }

    #[test]
    fn failures_leave_the_block_unchanged() {
        let catalog = catalog_v3();
        let mut blocks = vec![
            block(print("b1", 0)),
            block(json!({"id": "b2", "type": "io.print", "v": 1, "fields": {"SEP": "bad"}})),
            block(print("b3", 4)),
        ];
        let before = blocks.clone();
        let outcome = walk(&mut blocks, &catalog, &TABLES);
        assert_eq!(blocks, before);
        assert_eq!(outcome.upgraded.len(), 0);
        // A newer version is not an upgrade: resolve reports it (E0602).
        assert_eq!(
            messages(&outcome),
            [
                "This \"print\" block is version 0, which this version of Blocks2Cpp cannot upgrade to version 3.",
                "This \"print\" block is version 1, and upgrading it from version 1 failed: the separator is unknown.",
            ]
        );
        // A step of another type never applies.
        let mut other = vec![block(json!({"id": "b1", "type": "io.ask", "v": 1}))];
        let mut catalog = catalog_v3();
        catalog.blocks.get_mut("io.ask").unwrap().version = 2;
        let outcome = walk(&mut other, &catalog, &TABLES);
        assert_eq!(other[0].v, 1);
        assert_eq!(outcome.failed.len(), 1);
    }

    #[test]
    fn the_walk_owns_the_type_version_id_position_comment_and_flags() {
        let chain = [BlockMigration {
            block: "io.print",
            from: 1,
            apply: meddle,
        }];
        let mut catalog = crate::core_catalog().clone();
        catalog.blocks.get_mut("io.print").unwrap().version = 2;
        let mut blocks = vec![block(json!({
            "id": "b1", "type": "io.print", "v": 1, "x": 10, "y": 20,
            "collapsed": true, "comment": {"text": "keep me", "pinned": true}
        }))];
        let outcome = walk(&mut blocks, &catalog, &with(&chain));
        assert_eq!(outcome.upgraded.len(), 1);
        let upgraded = &blocks[0];
        assert_eq!(upgraded.id.as_str(), "b1");
        assert_eq!(upgraded.block_type, "io.print");
        assert_eq!(upgraded.v, 2);
        assert_eq!((upgraded.x, upgraded.y), (Some(10), Some(20)));
        assert!(upgraded.collapsed);
        assert!(!upgraded.disabled);
        assert_eq!(
            upgraded.comment,
            Some(BlockComment {
                text: "keep me".into(),
                pinned: true
            })
        );
    }

    /// `main` holding a disabled `test.say` v0 with a comment, whose `WHAT`
    /// holds a `math.add` with a nested number, and a loose `test.say` with a
    /// stack below it.
    fn retired_blocks() -> Vec<Block> {
        let number = json!({"id": "n1", "type": "math.number", "v": 1, "fields": {"VALUE": "2"}});
        let add = json!({"id": "a1", "type": "math.arithmetic", "v": 1, "fields": {"OP": "add"},
            "inputs": {"A": {"block": number}, "B": {"expr": [{"num": "3"}]}}});
        let say = json!({"id": "s1", "type": "test.say", "v": 0, "disabled": true,
            "comment": {"text": "hi"}, "fields": {"LOUD": true}, "inputs": {"WHAT": {"block": add}}});
        let main = json!({"id": "m", "type": "program.main", "v": 1, "x": 0, "y": 0,
            "statements": {"BODY": [say]}});
        let loose = json!({"id": "s2", "type": "test.say", "v": 1, "x": 5, "y": 300,
            "inputs": {"WHAT": {"expr": [{"str": "x"}]}},
            "stack": [{"id": "s3", "type": "test.say", "v": 1, "inputs": {"WHAT": {"expr": []}}}]});
        vec![block(main), block(loose)]
    }

    #[test]
    fn a_replacement_keeps_the_id_position_comment_flags_stack_and_nested_blocks() {
        let mut blocks = retired_blocks();
        let outcome = walk(&mut blocks, crate::core_catalog(), &TABLES);
        assert_eq!(outcome.upgraded.len(), 3);
        assert!(outcome.failed.is_empty(), "{:?}", outcome.failed);
        let say = &blocks[0].statements["BODY"][0];
        assert_eq!(
            (say.id.as_str(), say.block_type.as_str(), say.v),
            ("s1", "io.print", 1)
        );
        assert!(say.disabled);
        assert_eq!(say.comment.as_ref().map(|c| c.text.as_str()), Some("hi"));
        assert!(
            say.fields.is_empty(),
            "v0 → v1 dropped LOUD before the replacement"
        );
        let Input::Block(add) = &say.inputs["ITEM0"] else {
            panic!("the nested block moved with its input")
        };
        assert_eq!(add.block.id.as_str(), "a1");
        let Input::Block(number) = &add.block.inputs["A"] else {
            panic!("the nested block's own block is kept")
        };
        assert_eq!(number.block.fields["VALUE"], FieldValue::Text("2".into()));
        let loose = &blocks[1];
        assert_eq!((loose.x, loose.y), (Some(5), Some(300)));
        assert_eq!(loose.block_type, "io.print");
        assert_eq!(loose.stack.len(), 1);
        assert_eq!(
            loose.stack[0].block_type, "io.print",
            "stacked blocks are upgraded too"
        );
        // Idempotent.
        let again = blocks.clone();
        assert_eq!(
            walk(&mut blocks, crate::core_catalog(), &TABLES).upgraded.len(),
            0
        );
        assert_eq!(blocks, again);
    }

    #[test]
    fn retired_types_without_a_way_forward_fail() {
        let mut blocks = vec![
            block(json!({"id": "b1", "type": "test.say", "v": 2})),
            block(json!({"id": "b2", "type": "test.gone", "v": 1})),
            block(json!({"id": "b3", "type": "test.say", "v": 1})),
        ];
        let tables = BlockUpgrades {
            retired: &["test.say", "test.gone"],
            ..TABLES
        };
        let before = blocks.clone();
        let outcome = walk(&mut blocks, crate::core_catalog(), &tables);
        assert_eq!(blocks, before);
        assert_eq!(
            messages(&outcome),
            [
                "This block is version 2 of a retired kind of block, and this version of Blocks2Cpp cannot upgrade it.",
                "This block is version 1 of a retired kind of block, and this version of Blocks2Cpp cannot upgrade it.",
                "This block is version 1 of a retired kind of block, which is now the \"print\" block, and replacing it failed: the block has no WHAT input.",
            ]
        );
        // A replacement to a type the catalog does not define, and tables
        // that go round in a circle.
        let lost = [BlockReplacement {
            to: "test.nowhere",
            ..REPLACEMENTS[0]
        }];
        let circle = [
            BlockReplacement {
                from: "test.a",
                version: 1,
                to: "test.b",
                to_version: 1,
                apply: drop_loud,
            },
            BlockReplacement {
                from: "test.b",
                version: 1,
                to: "test.a",
                to_version: 1,
                apply: drop_loud,
            },
        ];
        let mut blocks = [
            block(json!({"id": "b1", "type": "test.say", "v": 1, "inputs": {"WHAT": {"expr": []}}})),
            block(json!({"id": "b2", "type": "test.a", "v": 1})),
        ];
        let lost = walk(
            &mut blocks[..1],
            crate::core_catalog(),
            &BlockUpgrades {
                replacements: &lost,
                ..TABLES
            },
        );
        assert_eq!(
            messages(&lost),
            [
                "This block is version 1 of a retired kind of block, and upgrading it leads to the block type \"test.nowhere\", which this version of Blocks2Cpp does not know."
            ]
        );
        let circle = walk(
            &mut blocks[1..],
            crate::core_catalog(),
            &BlockUpgrades {
                replacements: &circle,
                ..TABLES
            },
        );
        assert_eq!(
            messages(&circle),
            [
                "This block is version 1 of a retired kind of block, and upgrading it does not come to an end. This is a bug in Blocks2Cpp."
            ]
        );
    }

    #[test]
    fn a_retired_type_the_catalog_defines_again_is_not_replaced() {
        let mut blocks = vec![block(print("b1", 1))];
        let tables = BlockUpgrades {
            retired: &["io.print"],
            ..TABLES
        };
        let outcome = walk(&mut blocks, crate::core_catalog(), &tables);
        assert_eq!(outcome.upgraded.len(), 0);
        assert!(outcome.failed.is_empty());
    }

    /// A step that rewrites the block with `change` and expects the checks to
    /// refuse it with `what`.
    fn refused(start: Value, change: fn(&mut Block) -> Result<(), String>, what: &str) {
        let chain = [BlockMigration {
            block: "io.print",
            from: 1,
            apply: change,
        }];
        let mut catalog = crate::core_catalog().clone();
        catalog.blocks.get_mut("io.print").unwrap().version = 2;
        let mut blocks = vec![block(start)];
        let before = blocks.clone();
        let outcome = walk(&mut blocks, &catalog, &with(&chain));
        assert_eq!(blocks, before, "{what}");
        assert_eq!(
            messages(&outcome),
            [format!(
                "This \"print\" block is version 1, and upgrading it would {what}, so it was left as it was. This is a bug in Blocks2Cpp."
            )]
        );
    }

    fn with_nested() -> Value {
        json!({"id": "p", "type": "io.print", "v": 1,
            "inputs": {"ITEM0": {"block": {"id": "n", "type": "math.number", "v": 1}},
                       "ITEM1": {"expr": [{"num": "1"}, {"op": "+"}, {"num": "2"}]}},
            "statements": {"X": [{"id": "s", "type": "var.change", "v": 1}]},
            "stack": [{"id": "t", "type": "var.change", "v": 1}]})
    }

    #[test]
    fn upgrades_never_add_or_remove_blocks() {
        #[allow(clippy::unnecessary_wraps, reason = "the signature every step has")]
        fn add_block(block: &mut Block) -> Result<(), String> {
            let extra = block.clone();
            block.statements.insert("NEW".into(), vec![extra]);
            Ok(())
        }
        #[allow(clippy::unnecessary_wraps, reason = "the signature every step has")]
        fn duplicate(block: &mut Block) -> Result<(), String> {
            let copy = block.inputs["ITEM0"].clone();
            block.inputs.insert("ITEM2".into(), copy);
            Ok(())
        }
        #[allow(clippy::unnecessary_wraps, reason = "the signature every step has")]
        fn drop_nested(block: &mut Block) -> Result<(), String> {
            block.statements.clear();
            Ok(())
        }
        #[allow(clippy::unnecessary_wraps, reason = "the signature every step has")]
        fn add_stack(block: &mut Block) -> Result<(), String> {
            block.stack.push(stand_in(&BlockId::new("z").unwrap(), 9));
            Ok(())
        }
        #[allow(clippy::unnecessary_wraps, reason = "the signature every step has")]
        fn touch_nested(block: &mut Block) -> Result<(), String> {
            if let Some(list) = block.statements.get_mut("X") {
                list[0].disabled = true;
            }
            Ok(())
        }
        refused(with_nested(), add_block, "add a block");
        refused(with_nested(), duplicate, "add a block");
        refused(with_nested(), touch_nested, "change a block inside it");
        refused(with_nested(), drop_nested, "remove a block from inside it");
        refused(with_nested(), add_stack, "add blocks below it");
    }

    #[test]
    fn upgrades_may_move_a_nested_block_to_another_input() {
        #[allow(clippy::unnecessary_wraps, reason = "the signature every step has")]
        fn swap(block: &mut Block) -> Result<(), String> {
            let nested = block.inputs.remove("ITEM0");
            let expr = block.inputs.remove("ITEM1");
            block.inputs.extend(nested.map(|n| ("ITEM1".to_owned(), n)));
            block.inputs.extend(expr.map(|e| ("ITEM0".to_owned(), e)));
            let list = block.statements.remove("X").unwrap_or_default();
            block.statements.insert("Y".into(), list);
            Ok(())
        }
        let chain = [BlockMigration {
            block: "io.print",
            from: 1,
            apply: swap,
        }];
        let mut catalog = crate::core_catalog().clone();
        catalog.blocks.get_mut("io.print").unwrap().version = 2;
        let mut blocks = vec![block(with_nested())];
        let outcome = walk(&mut blocks, &catalog, &with(&chain));
        assert!(outcome.failed.is_empty(), "{:?}", outcome.failed);
        let Input::Block(nested) = &blocks[0].inputs["ITEM1"] else {
            panic!("the nested block moved")
        };
        assert_eq!(nested.block.block_type, "math.number");
        assert_eq!(blocks[0].statements["Y"][0].id.as_str(), "s");
        assert_eq!(blocks[0].stack[0].id.as_str(), "t");
    }

    #[test]
    fn upgrades_never_add_tokens_or_change_declared_ids() {
        #[allow(clippy::unnecessary_wraps, reason = "the signature every step has")]
        fn longer(block: &mut Block) -> Result<(), String> {
            if let Some(Input::Expr(expr)) = block.inputs.get_mut("ITEM1") {
                expr.expr.push(Token::Op("+".into()));
            }
            Ok(())
        }
        #[allow(clippy::unnecessary_wraps, reason = "the signature every step has")]
        fn shorter(block: &mut Block) -> Result<(), String> {
            block.inputs.insert(
                "ITEM1".into(),
                Input::Expr(ExprInput {
                    expr: vec![Token::Num("3".into())],
                    draft: false,
                }),
            );
            Ok(())
        }
        #[allow(clippy::unnecessary_wraps, reason = "the signature every step has")]
        fn new_decl(block: &mut Block) -> Result<(), String> {
            block.fields.insert(
                "NAME".into(),
                FieldValue::Decl(SymbolDecl {
                    sym: b2c_ir::SymbolId::new("s_new").unwrap(),
                    name: "x".into(),
                }),
            );
            Ok(())
        }
        #[allow(clippy::unnecessary_wraps, reason = "the signature every step has")]
        fn new_param(block: &mut Block) -> Result<(), String> {
            block
                .extra
                .insert("params".into(), json!([{"sym": "s_p", "name": "p"}]));
            Ok(())
        }
        #[allow(clippy::unnecessary_wraps, reason = "the signature every step has")]
        fn rename(block: &mut Block) -> Result<(), String> {
            if let Some(FieldValue::Decl(decl)) = block.fields.get_mut("NAME") {
                decl.name = String::from("renamed");
            }
            Ok(())
        }
        refused(with_nested(), longer, "make the values in its inputs longer");
        refused(with_nested(), new_decl, "change the ID of a name it declares");
        refused(with_nested(), new_param, "change the ID of a name it declares");
        // Fewer tokens and a renamed declaration are fine.
        for change in [shorter as fn(&mut Block) -> Result<(), String>, rename] {
            let chain = [BlockMigration {
                block: "io.print",
                from: 1,
                apply: change,
            }];
            let mut catalog = crate::core_catalog().clone();
            catalog.blocks.get_mut("io.print").unwrap().version = 2;
            let mut start = with_nested();
            start["fields"] = json!({"NAME": {"sym": "s_old", "name": "old"}});
            let mut blocks = vec![block(start)];
            let outcome = walk(&mut blocks, &catalog, &with(&chain));
            assert!(outcome.failed.is_empty(), "{:?}", outcome.failed);
        }
    }

    #[test]
    fn upgrades_never_break_the_limits_of_a_block() {
        #[allow(clippy::unnecessary_wraps, reason = "the signature every step has")]
        fn bidi(block: &mut Block) -> Result<(), String> {
            block
                .fields
                .insert("SEP".into(), FieldValue::Text("a\u{202e}b".into()));
            Ok(())
        }
        #[allow(clippy::unnecessary_wraps, reason = "the signature every step has")]
        fn huge(block: &mut Block) -> Result<(), String> {
            block
                .fields
                .insert("SEP".into(), FieldValue::Text("x".repeat(70_000)));
            Ok(())
        }
        let chain = [BlockMigration {
            block: "io.print",
            from: 1,
            apply: bidi,
        }];
        let mut catalog = crate::core_catalog().clone();
        catalog.blocks.get_mut("io.print").unwrap().version = 2;
        for change in [bidi as fn(&mut Block) -> Result<(), String>, huge] {
            let chain = [BlockMigration {
                apply: change,
                ..chain[0]
            }];
            let mut blocks = vec![block(with_nested())];
            let before = blocks.clone();
            let outcome = walk(&mut blocks, &catalog, &with(&chain));
            assert_eq!(blocks, before);
            let message = &outcome.failed[0].message;
            assert!(
                message.starts_with(
                    "This \"print\" block is version 1, and upgrading it would break a limit of project files ("
                ),
                "{message}"
            );
            assert!(message.ends_with("), so it was left as it was. This is a bug in Blocks2Cpp."));
        }
    }

    #[test]
    fn stand_ins_are_put_back_where_they_are() {
        let mut parent = block(with_nested());
        let original = parent.clone();
        let detached = Detached::take(&mut parent).unwrap();
        assert!(parent.stack.is_empty());
        assert_eq!(detached.ids.len(), 2);
        assert!(children(&parent).all(|nested| nested.block_type == STAND_IN));
        detached.put_back(&mut parent);
        assert_eq!(parent, original);
    }

    /// A project document holding `blocks` in its one module.
    fn document(blocks: &[Value]) -> Document {
        let value = json!({
            "format": "blocks2cpp/project", "formatVersion": 1,
            "generator": {"app": "0.1.0", "catalog": "1.1.0"},
            "project": {"id": "p", "name": "P", "language": {"standard": "c++20"},
                        "build": {"libraries": ["m"], "defines": [{"name": "N", "value": {"int": 3}}]}},
            "modules": [{"id": "m", "name": "main", "workspace": {"blocks": blocks}}]
        });
        b2c_model::load(&serde_json::to_vec(&value).unwrap()).unwrap()
    }

    #[test]
    fn a_document_is_upgraded_and_reported() {
        let blocks = retired_blocks();
        let values: Vec<Value> = blocks.iter().map(|b| serde_json::to_value(b).unwrap()).collect();
        let mut doc = document(&values);
        let hash = b2c_model::security_hash(&doc);
        let report = document_report(&mut doc, crate::core_catalog(), &TABLES);
        assert_eq!(
            report,
            UpgradeReport {
                upgraded: 3,
                diagnostics: vec![]
            }
        );
        assert!(report.changed());
        assert_eq!(b2c_model::security_hash(&doc), hash);
        // It still loads, and a second upgrade changes nothing.
        b2c_model::load(b2c_model::to_canonical_json(&doc).as_bytes()).unwrap();
        let again = document_report(&mut doc, crate::core_catalog(), &TABLES);
        assert!(!again.changed());
        // Failures are B2C-E0603 at their block.
        let mut doc = document(&[json!({"id": "b1", "type": "test.say", "v": 1})]);
        let report = document_report(&mut doc, crate::core_catalog(), &TABLES);
        assert_eq!(report.upgraded, 0);
        assert_eq!(report.diagnostics.len(), 1);
        assert_eq!(report.diagnostics[0].code.0, codes::OLD_BLOCK);
        assert_eq!(
            report.diagnostics[0].primary.block.as_ref().map(BlockId::as_str),
            Some("b1")
        );
        assert_eq!(
            report.diagnostics[0]
                .primary
                .module
                .as_ref()
                .map(ModuleId::as_str),
            Some("m")
        );
    }

    #[test]
    fn an_upgrade_that_changes_what_trust_sees_is_undone() {
        #[allow(clippy::unnecessary_wraps, reason = "the signature every step has")]
        fn no_change(_: &mut Block) -> Result<(), String> {
            Ok(())
        }
        // A test catalog with a Raw C++ block type.
        let mut catalog = crate::core_catalog().clone();
        let mut raw = catalog.blocks["io.print"].clone();
        raw.id = String::from("raw.code");
        catalog.blocks.insert(raw.id.clone(), raw);
        let to_raw = [BlockReplacement {
            from: "test.say",
            version: 1,
            to: "raw.code",
            to_version: 1,
            apply: no_change,
        }];
        let tables = BlockUpgrades {
            replacements: &to_raw,
            ..TABLES
        };
        let start = vec![
            json!({"id": "b1", "type": "test.say", "v": 1}),
            json!({"id": "b2", "type": "test.say", "v": 0, "fields": {"LOUD": true}}),
        ];
        let mut doc = document(&start);
        let before = doc.clone();
        let report = document_report(&mut doc, &catalog, &tables);
        assert_eq!(doc, before, "every upgrade of the document is undone");
        assert_eq!(report.upgraded, 0);
        let messages: Vec<&str> = report.diagnostics.iter().map(|d| d.message.as_str()).collect();
        assert_eq!(messages.len(), 2);
        assert!(messages.iter().all(|m| m.ends_with(
            ", and upgrading the old blocks would change what workspace trust looks at (Raw C++, libraries or blocks that use files), so it was left as it was. This is a bug in Blocks2Cpp."
        )));
        assert!(messages[0].starts_with("This block is version "));
    }

    #[test]
    fn an_upgrade_that_breaks_a_limit_of_the_document_is_undone() {
        // A block nested 40 statements deep whose upgrade adds deeply nested
        // `extra` data: fine for the block alone, too deep in the document.
        #[allow(clippy::unnecessary_wraps, reason = "the signature every step has")]
        fn deep_extra(block: &mut Block) -> Result<(), String> {
            let mut value = json!(1);
            for _ in 0..20 {
                value = json!([value]);
            }
            block.extra.insert("deep".into(), value);
            Ok(())
        }
        let replacement = [BlockReplacement {
            from: "test.say",
            version: 1,
            to: "io.print",
            to_version: 1,
            apply: deep_extra,
        }];
        let mut innermost = json!({"id": "deep", "type": "test.say", "v": 1});
        for level in 0..36 {
            innermost = json!({"id": format!("f{level}"), "type": "control.forever", "v": 1,
                "statements": {"BODY": [innermost]}});
        }
        innermost["x"] = json!(0);
        innermost["y"] = json!(0);
        let mut doc = document(&[innermost]);
        let before = doc.clone();
        let report = document_report(
            &mut doc,
            crate::core_catalog(),
            &BlockUpgrades {
                replacements: &replacement,
                ..TABLES
            },
        );
        assert_eq!(doc, before);
        assert_eq!(report.diagnostics.len(), 1);
        assert!(
            report.diagnostics[0]
                .message
                .contains("and upgrading the old blocks would break a limit of project files (Lists and objects in the project file are nested more than 128 levels deep"),
            "{}",
            report.diagnostics[0].message
        );
    }

    #[test]
    fn documents_without_old_blocks_are_left_alone() {
        let mut doc = document(&[
            json!({"id": "m", "type": "program.main", "v": 1}),
            json!({"id": "x", "type": "x.unknown", "v": 7}),
        ]);
        let before = doc.clone();
        assert_eq!(
            document_report(&mut doc, crate::core_catalog(), &BlockUpgrades::SHIPPED),
            UpgradeReport::default()
        );
        assert_eq!(doc, before);
    }

    fn clipboard_of(catalog: &str, blocks: &[Value]) -> Clipboard {
        let value = json!({"format": "blocks2cpp/clipboard", "formatVersion": 1, "catalog": catalog,
            "blocks": blocks, "refs": {}});
        b2c_model::load_clipboard(&serde_json::to_vec(&value).unwrap()).unwrap()
    }

    #[test]
    fn a_clipboard_payload_is_upgraded_and_a_newer_catalog_reported() {
        let mut payload = clipboard_of(
            "1.1.0",
            &[
                json!({"id": "s1", "type": "test.say", "v": 1, "inputs": {"WHAT": {"expr": [{"str": "x"}]}}}),
                json!({"id": "s2", "type": "test.say", "v": 1}),
            ],
        );
        let report = clipboard(&mut payload, crate::core_catalog(), &TABLES);
        assert_eq!(report.upgraded, 1);
        assert_eq!(payload.blocks[0].block_type, "io.print");
        assert_eq!(payload.blocks[1].block_type, "test.say");
        assert_eq!(report.diagnostics.len(), 1);
        assert_eq!(report.diagnostics[0].code.0, codes::OLD_BLOCK);
        assert_eq!(report.diagnostics[0].primary.module, None);

        // From a newer catalog, a field this one does not know is E0602.
        let newer = vec![
            json!({"id": "p1", "type": "io.print", "v": 1, "fields": {"LOUDNESS": "11"}}),
            json!({"id": "p2", "type": "io.print", "v": 1}),
        ];
        let mut payload = clipboard_of("1.4.0", &newer);
        let report = clipboard(&mut payload, crate::core_catalog(), &BlockUpgrades::SHIPPED);
        assert_eq!(report.upgraded, 0);
        let found: Vec<(&str, Option<&str>)> = report
            .diagnostics
            .iter()
            .map(|d| (d.code.0.as_str(), d.primary.block.as_ref().map(BlockId::as_str)))
            .collect();
        assert_eq!(found, [(codes::NEWER_BLOCK, Some("p1"))]);
        // From this catalog or an older one, it is left to resolve (E0605).
        for catalog in [crate::CATALOG_VERSION, "1.1.0", "1.0.0", "not a version"] {
            let mut payload = clipboard_of(catalog, &newer);
            assert_eq!(
                clipboard(&mut payload, crate::core_catalog(), &BlockUpgrades::SHIPPED),
                UpgradeReport::default()
            );
        }
    }

    #[test]
    fn a_clipboard_upgrade_that_breaks_a_limit_is_undone() {
        #[allow(clippy::unnecessary_wraps, reason = "the signature every step has")]
        fn grow(block: &mut Block) -> Result<(), String> {
            block
                .fields
                .insert("SEP".into(), FieldValue::Text("x".repeat(60_000)));
            Ok(())
        }
        // Each block alone is fine; together they pass 32 MiB.
        let replacement = [BlockReplacement {
            from: "test.say",
            version: 1,
            to: "io.print",
            to_version: 1,
            apply: grow,
        }];
        let blocks: Vec<Value> = (0..600)
            .map(|i| json!({"id": format!("b{i}"), "type": "test.say", "v": 1}))
            .collect();
        let mut payload = clipboard_of("1.1.0", &blocks);
        let before = payload.clone();
        let report = clipboard(
            &mut payload,
            crate::core_catalog(),
            &BlockUpgrades {
                replacements: &replacement,
                ..TABLES
            },
        );
        assert_eq!(payload, before);
        assert_eq!(report.upgraded, 0);
        assert_eq!(report.diagnostics.len(), 600);
        assert!(
            report.diagnostics[0]
                .message
                .contains("would break a limit of project files (The pasted data is larger than")
        );
    }

    #[test]
    fn newer_catalogs_add_fields_options_settings_inputs_and_parts() {
        let catalog = crate::core_catalog();
        let print = &catalog.blocks["io.print"];
        let problem = |value: Value| newer_catalog_problem(print, &block(value));
        let base = json!({"id": "p", "type": "io.print", "v": 1, "fields": {"SEP": "space"},
            "extra": {"itemCount": 2}, "inputs": {"ITEM0": {"expr": []}, "ITEM5": {"expr": []}}});
        assert_eq!(
            problem(base.clone()),
            None,
            "known names, even past the item count"
        );
        let cases = [
            (
                "fields",
                json!({"SEP": "space", "VOLUME": "3"}),
                "the field \"VOLUME\"",
            ),
            (
                "fields",
                json!({"SEP": "tabs"}),
                "the choice \"tabs\" in its field \"SEP\"",
            ),
            (
                "extra",
                json!({"itemCount": 1, "style": 2}),
                "the setting \"style\"",
            ),
            (
                "inputs",
                json!({"ITEM0": {"expr": []}, "TO": {"expr": []}}),
                "the input \"TO\"",
            ),
            ("statements", json!({"THEN": []}), "the part \"THEN\""),
            ("inputs", json!({"ITEMS": {"expr": []}}), "the input \"ITEMS\""),
        ];
        for (key, value, what) in cases {
            let mut changed = base.clone();
            changed[key] = value;
            assert_eq!(
                problem(changed),
                Some(format!(
                    "This \"print\" block was made with a newer version of Blocks2Cpp: it has {what}, which this version does not know. Update Blocks2Cpp to use it; until then the block is kept as it is."
                ))
            );
        }
        // A wrong value of a field that is not a dropdown is not newer.
        assert_eq!(
            problem(json!({"id": "p", "type": "io.print", "v": 1, "fields": {"NEWLINE": "yes"}})),
            None
        );
        // Repeated parts by their index.
        let if_block = &catalog.blocks["control.if"];
        let parts = block(json!({"id": "i", "type": "control.if", "v": 1,
            "statements": {"DO0": [], "DO3": [], "ELSE": []}}));
        assert_eq!(newer_catalog_problem(if_block, &parts), None);
        // Inputs that are not repeated by their name.
        let change = &catalog.blocks["var.change"];
        let inputs = |inputs: Value| {
            block(json!({"id": "c", "type": "var.change", "v": 1,
                "fields": {"VAR": {"ref": "s_n"}}, "inputs": inputs}))
        };
        assert_eq!(
            newer_catalog_problem(change, &inputs(json!({"BY": {"expr": []}}))),
            None
        );
        let unknown =
            newer_catalog_problem(change, &inputs(json!({"BY": {"expr": []}, "STEP": {"expr": []}})));
        assert!(unknown.unwrap().contains("it has the input \"STEP\","));
    }

    /// An *update* block of catalog 1.1.0 with operator `op` and value
    /// `value`; `null` leaves the key out.
    fn update(id: &str, op: Value, value: Value) -> Value {
        let mut update = json!({"id": id, "type": "var.update", "v": 1,
            "fields": {"VAR": {"ref": "s_n"}}, "inputs": {}});
        if !op.is_null() {
            update["fields"]["OP"] = op;
        }
        if !value.is_null() {
            update["inputs"]["VALUE"] = value;
        }
        update
    }

    #[test]
    fn update_blocks_become_change_blocks() {
        let ops = ["add", "sub", "mul", "div", "mod"];
        let mut blocks: Vec<Block> = ops
            .iter()
            .map(|op| block(update(op, json!(op), json!({"expr": [{"num": "2"}]}))))
            .collect();
        let sum = json!({"id": "sum", "type": "math.arithmetic", "v": 1, "fields": {"OP": "add"},
            "inputs": {"A": {"expr": [{"num": "1"}]}, "B": {"expr": [{"num": "2"}]}}});
        let mut nested = update("nested", json!("mul"), json!({"block": sum}));
        nested["disabled"] = json!(true);
        nested["comment"] = json!({"text": "twice"});
        nested["stack"] = json!([update("below", json!("sub"), Value::Null)]);
        blocks.push(block(nested));
        blocks.push(block(update("defaults", Value::Null, Value::Null)));
        let outcome = walk(&mut blocks, crate::core_catalog(), &BlockUpgrades::SHIPPED);
        assert!(outcome.failed.is_empty(), "{:?}", outcome.failed);
        assert_eq!(outcome.upgraded.len(), 8);

        for (change, op) in blocks.iter().zip(ops) {
            let expected = block(json!({"id": op, "type": "var.change", "v": 1,
                "fields": {"VAR": {"ref": "s_n"}, "OP": op},
                "inputs": {"BY": {"expr": [{"num": "2"}]}}}));
            assert_eq!(*change, expected);
        }
        let nested = &blocks[5];
        assert_eq!(
            (nested.id.as_str(), nested.block_type.as_str()),
            ("nested", "var.change")
        );
        assert!(nested.disabled);
        assert_eq!(nested.comment.as_ref().map(|c| c.text.as_str()), Some("twice"));
        let Input::Block(sum) = &nested.inputs["BY"] else {
            panic!("the value's block moves to BY")
        };
        assert_eq!(sum.block.id.as_str(), "sum");
        assert!(!nested.inputs.contains_key("VALUE"));
        assert_eq!(nested.stack[0].block_type, "var.change");
        assert_eq!(nested.stack[0].fields["OP"], FieldValue::Text("sub".into()));
        // A block that left the defaults out still leaves them out.
        let defaults = &blocks[6];
        assert_eq!(defaults.block_type, "var.change");
        assert!(!defaults.fields.contains_key("OP"));
        assert!(defaults.inputs.is_empty());
        // Every result is a valid `change` block of this catalog.
        let catalog = crate::core_catalog();
        let def = &catalog.blocks["var.change"];
        for change in &blocks {
            assert_eq!(newer_catalog_problem(def, change), None);
        }
    }

    #[test]
    fn update_blocks_that_cannot_be_changed_are_kept() {
        let mut by = update("by", json!("add"), json!({"expr": [{"num": "1"}]}));
        by["inputs"]["BY"] = json!({"expr": [{"num": "2"}]});
        let mut blocks = vec![
            block(update("pow", json!("pow"), json!({"expr": [{"num": "2"}]}))),
            block(update("ref", json!({"ref": "s_n"}), Value::Null)),
            block(by),
        ];
        let before = blocks.clone();
        let outcome = walk(&mut blocks, crate::core_catalog(), &BlockUpgrades::SHIPPED);
        assert!(outcome.upgraded.is_empty());
        assert_eq!(blocks, before);
        let start = "This block is version 1 of a retired kind of block, which is now the \"change\" block, and replacing it failed:";
        assert_eq!(
            messages(&outcome),
            [
                format!("{start} its operator \"pow\" is not one of +=, -=, *=, /= and %=."),
                format!("{start} its operator is not a choice from its menu."),
                format!("{start} it already has an input \"BY\"."),
            ]
        );
    }

    /// The problems of a set of upgrade tables (03 §3.11.3, ADR-0015).
    fn table_problems(catalog: &Catalog, tables: &BlockUpgrades<'_>) -> Vec<String> {
        let mut problems = Vec::new();
        for (index, retired) in tables.retired.iter().enumerate() {
            if catalog.blocks.contains_key(*retired) {
                problems.push(format!("{retired}: a retired ID is defined again"));
            }
            if !crate::definitions::is_block_id(retired) {
                problems.push(format!("{retired}: not a block ID"));
            }
            if tables.retired[..index].contains(retired) {
                problems.push(format!("{retired}: retired twice"));
            }
            if !tables.replacements.iter().any(|r| r.from == *retired) {
                problems.push(format!(
                    "{retired}: no replacement (before 1.0 every retired type has one)"
                ));
            }
        }
        for (index, replacement) in tables.replacements.iter().enumerate() {
            let name = format!("{}@{}", replacement.from, replacement.version);
            if !tables.retired.contains(&replacement.from) {
                problems.push(format!("{name}: replaces a type that is not listed as retired"));
            }
            if replacement.version == 0 || replacement.to_version == 0 {
                problems.push(format!("{name}: versions start at 1"));
            }
            if tables.replacements[..index]
                .iter()
                .any(|r| (r.from, r.version) == (replacement.from, replacement.version))
            {
                problems.push(format!("{name}: replaced twice"));
            }
            // Follow the chain of replacements to a defined type.
            let (mut to, mut version, mut hops) = (replacement.to, replacement.to_version, 0);
            while let Some(next) = tables.replacement(to, version) {
                (to, version) = (next.to, next.to_version);
                hops += 1;
                if hops > tables.replacements.len() {
                    break;
                }
            }
            match catalog.blocks.get(to) {
                Some(def) if version <= def.version => {}
                _ => problems.push(format!("{name}: does not lead to a block type of the catalog")),
            }
        }
        for (index, step) in tables.migrations.iter().enumerate() {
            let name = format!("{}@{}", step.block, step.from);
            match catalog.blocks.get(step.block) {
                Some(def) if step.from >= 1 && step.from < def.version => {}
                None if tables.is_retired(step.block) && step.from >= 1 => {}
                _ => problems.push(format!("{name}: not an older version of a type")),
            }
            if tables.migrations[..index]
                .iter()
                .any(|s| (s.block, s.from) == (step.block, step.from))
            {
                problems.push(format!("{name}: two steps"));
            }
        }
        problems
    }

    #[test]
    fn the_shipped_tables_are_consistent() {
        assert_eq!(
            table_problems(crate::core_catalog(), &BlockUpgrades::SHIPPED),
            Vec::<String>::new()
        );
        assert_eq!(BlockUpgrades::default().retired, RETIRED_BLOCKS);
    }

    #[test]
    fn the_table_check_finds_problems() {
        let good = BlockUpgrades {
            migrations: &CHAIN[..2],
            ..TABLES
        };
        assert_eq!(table_problems(&catalog_v3(), &good), Vec::<String>::new());
        let bad_replacements = [
            BlockReplacement {
                to: "test.nowhere",
                ..REPLACEMENTS[0]
            },
            REPLACEMENTS[0],
            BlockReplacement {
                from: "io.ask",
                version: 0,
                ..REPLACEMENTS[0]
            },
        ];
        let bad = BlockUpgrades {
            migrations: &[CHAIN[1], CHAIN[1], CHAIN[2]],
            replacements: &bad_replacements,
            retired: &["test.say", "io.print", "Not An ID", "test.say"],
        };
        assert_eq!(
            table_problems(crate::core_catalog(), &bad),
            [
                "io.print: a retired ID is defined again",
                "io.print: no replacement (before 1.0 every retired type has one)",
                "Not An ID: not a block ID",
                "Not An ID: no replacement (before 1.0 every retired type has one)",
                "test.say: retired twice",
                "test.say@1: does not lead to a block type of the catalog",
                "test.say@1: replaced twice",
                "io.ask@0: replaces a type that is not listed as retired",
                "io.ask@0: versions start at 1",
                "io.print@1: not an older version of a type",
                "io.print@1: not an older version of a type",
                "io.print@1: two steps",
                "test.say@0: not an older version of a type",
            ]
        );
    }

    #[test]
    fn declared_ids_include_parameter_rows() {
        let define = block(json!({"id": "d", "type": "func.define", "v": 1,
            "fields": {"NAME": {"sym": "s_f", "name": "f"}},
            "extra": {"params": [{"sym": "s_b", "name": "b"}, {"sym": "s_a", "name": "a"}, {"name": "no sym"}]}}));
        assert_eq!(declared(&define), ["s_a", "s_b", "s_f"]);
        assert_eq!(tokens(&block(with_nested())), 3);
        let _ = BlockInput {
            block: Box::new(stand_in(&BlockId::new("x").unwrap(), 0)),
        };
    }
}
