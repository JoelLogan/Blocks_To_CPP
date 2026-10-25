//! The resolve stage (spec §6.3): checks every block of a document against
//! its catalog definition and completes the document with catalog defaults.
//!
//! The document's blocks are upgraded first ([`crate::upgrade`]): blocks saved
//! with an older version of their definition and blocks of a retired type.
//! A block that could not be upgraded is `B2C-E0603` at its place. In a
//! document from a newer catalog (`generator.catalog`), a block with a field,
//! input, `extra` setting or option its definition does not have, or with a
//! type or header this catalog does not know or does not allow there, is
//! `B2C-E0602` and is kept as it is (spec §3.11.3, ADR-0015).
//!
//! A type field takes its `types` and every type its pattern allows, and a
//! parameter row its `types` and its `type_class` (ADR-0017 §5,
//! `docs/plans/m3-contracts/types.md` §4.1); a user type always passes here,
//! since only the analyser knows what it names.
//!
//! Defaults filled in: absent fields with a default, absent `count`/`flag`
//! extras with a default, and absent value inputs with default tokens (the
//! "shadow" blocks of the editor). Statement inputs are never filled: an
//! absent one simply means an empty list. An absent name or reference (a
//! `symbol_decl` or `symbol_ref` field) is no catalog problem: it is what the
//! editor saves while nothing is chosen yet, and the analyser reports it
//! (`B2C-E0430`) once the block is part of the program, so a parked or
//! disabled block without one never stops a build (ADR-0012).
//!
//! Every block that is kept exactly as it was, without being checked against
//! a definition (`B2C-E0601`, `B2C-E0602`, `B2C-E0603`, `B2C-E0620`), is
//! listed in [`crate::ResolveReport::unchecked`], so the analyser does not
//! report its fields and inputs again.
//!
//! A block directly on the canvas may carry a loose statement stack (spec
//! §5.4, ADR-0011): the blocks attached below it. Stacked blocks are checked
//! like blocks in a statement list. A statement, reporter or predicate
//! directly on the canvas is *parked* with its stack (ADR-0012): it is checked
//! against the catalog like every other block, but its place is not a catalog
//! problem (the analyser reports it as the warning `B2C-W0505`).
//! `B2C-E0604` (*block in the wrong place*) is left for the placements the
//! editor cannot produce: a hat or definition inside a block, a statement in
//! a value input, a reporter or predicate as a step, and blocks stacked below
//! a block that is not a statement (reported once, on the head).
//!
//! Messages name a block by its short name ([`block_noun`]), never by its
//! catalog ID, except for an unknown block type (`B2C-E0601`) and a broken
//! catalog definition (`B2C-E0620`, a bug report).

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use b2c_ir::types::NoFacts;
use b2c_ir::{BlockId, Location, ModuleId, Part, SymbolId, Type, TypeTerm};
use b2c_model::{Block, Document, ExprInput, FieldValue, Input};
use serde_json::Value;

use crate::codes::{self, Diags};
use crate::definitions::{self, PARAM_MODES, count_default, default_tokens, field_def, repeat_index};
use crate::schema::{BlockDef, ExtraDef, ExtraKind, FieldDef, FieldDefault, FieldKind, Shape};
use crate::signature::{self, TypeFit};
use crate::stack;
use crate::upgrade::{self, BlockUpgrades};
use crate::{Catalog, ResolveReport};

/// Where a block sits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Position {
    /// Directly on a module's canvas, with this many blocks stacked below it.
    TopLevel {
        /// The length of the block's loose stack.
        stacked: usize,
    },
    /// In a statement list or a loose stack.
    Statement,
    /// In a value input.
    Value,
}

/// An `extra` value after checking.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Resolved {
    Count(u32),
    Flag(bool),
    /// A row extra (`params`, `fields`, `values`) with this many rows.
    Rows(u32),
    /// Missing or invalid (already reported).
    Invalid,
}

/// Runs the resolve stage with the shipped block upgrades.
pub(crate) fn run(document: &Document, catalog: &Catalog) -> ResolveReport {
    run_with(document, catalog, &BlockUpgrades::SHIPPED)
}

/// Runs the resolve stage with the given block upgrades.
fn run_with(document: &Document, catalog: &Catalog, upgrades: &BlockUpgrades<'_>) -> ResolveReport {
    let mut completed = document.clone();
    let mut resolver = Resolver::new(catalog, upgrades, &document.generator.catalog);
    let has_blocks = completed.modules.iter().any(|m| !m.workspace.blocks.is_empty());
    if catalog.blocks.is_empty() {
        if has_blocks {
            resolver.diags.error(
                codes::EMPTY_CATALOG,
                Location::project(),
                "No block definitions are available (the block catalog is empty or could not be loaded), so no block can be checked. This is a bug in Blocks2Cpp.",
            );
        }
        return resolver.finish(completed);
    }
    for failure in upgrade::document(&mut completed, catalog, upgrades).failed {
        resolver
            .failed_upgrades
            .entry(failure.block)
            .or_insert(failure.message);
    }
    for module in &mut completed.modules {
        resolver.module = Some(module.id.clone());
        for block in &mut module.workspace.blocks {
            let mut stacked = stack::take(block);
            resolver.top_level(block, &mut stacked);
            stack::restore(block, stacked);
        }
    }
    resolver.finish(completed)
}

/// The first problem of every broken definition.
fn broken_definitions(catalog: &Catalog) -> BTreeMap<String, String> {
    let mut broken = BTreeMap::new();
    for problem in definitions::check_catalog(catalog) {
        if let Some(block) = problem.block {
            broken.entry(block).or_insert(problem.message);
        }
    }
    broken
}

struct Resolver<'c> {
    catalog: &'c Catalog,
    upgrades: &'c BlockUpgrades<'c>,
    /// Why each block the upgrade could not upgrade stayed as it was.
    failed_upgrades: BTreeMap<BlockId, String>,
    /// Whether the document names a newer catalog than this one.
    newer_catalog: bool,
    broken: BTreeMap<String, String>,
    reported_broken: BTreeSet<String>,
    module: Option<ModuleId>,
    diags: Diags,
    /// The blocks kept as they were without being checked against a
    /// definition ([`ResolveReport::unchecked`]).
    unchecked: BTreeSet<BlockId>,
}

impl<'c> Resolver<'c> {
    /// A resolver for a document that names `document_catalog` as its
    /// catalog version.
    fn new(catalog: &'c Catalog, upgrades: &'c BlockUpgrades<'c>, document_catalog: &str) -> Self {
        Self {
            catalog,
            upgrades,
            failed_upgrades: BTreeMap::new(),
            newer_catalog: b2c_model::is_newer_version(document_catalog, &catalog.version),
            broken: broken_definitions(catalog),
            reported_broken: BTreeSet::new(),
            module: None,
            diags: Diags::default(),
            unchecked: BTreeSet::new(),
        }
    }

    /// What the stage found in `completed`, the document it completed.
    fn finish(self, completed: Document) -> ResolveReport {
        ResolveReport {
            document: completed,
            diagnostics: self.diags.finish(),
            unchecked: self.unchecked,
        }
    }
}

impl Resolver<'_> {
    /// A block directly on a module's canvas, and the loose stack below it.
    /// Stacked blocks are checked like the blocks of a statement list, so a
    /// stacked statement gets no placement error of its own: the stack is
    /// reported once, on its head (see [`Self::check_place`]).
    fn top_level(&mut self, head: &mut Block, stacked: &mut [Block]) {
        self.block(
            head,
            Position::TopLevel {
                stacked: stacked.len(),
            },
        );
        for block in stacked {
            self.block(block, Position::Statement);
        }
    }

    fn block(&mut self, block: &mut Block, position: Position) {
        let location = Location::block(self.module.clone(), block.id.clone());
        let catalog = self.catalog;
        let Some(def) = catalog.blocks.get(&block.block_type) else {
            self.unchecked.insert(block.id.clone());
            if self.upgrades.is_retired(&block.block_type) {
                // A retired type whose replacement failed.
                let message = self.upgrade_failure(block);
                self.diags.error(codes::OLD_BLOCK, location, message);
            } else {
                self.diags.error(
                    codes::UNKNOWN_BLOCK,
                    location,
                    unknown_block_message(&block.block_type),
                );
            }
            self.children(block);
            return;
        };
        if let Some(problem) = self.broken.get(&def.id) {
            self.unchecked.insert(block.id.clone());
            if self.reported_broken.insert(def.id.clone()) {
                let message = format!(
                    "The block catalog's definition of {} is broken ({problem}), so blocks of this type cannot be checked. This is a bug in the catalog.",
                    quote(&def.id)
                );
                self.diags.error(codes::BROKEN_DEFINITION, location, message);
            }
            self.children(block);
            return;
        }
        if block.v > def.version {
            self.unchecked.insert(block.id.clone());
            let message = newer_block_message(def, block.v);
            self.diags.error(codes::NEWER_BLOCK, location, message);
            self.children(block);
            return;
        }
        if block.v < def.version {
            // The document was upgraded first, so this block could not be.
            self.unchecked.insert(block.id.clone());
            let message = self.upgrade_failure(block);
            self.diags.error(codes::OLD_BLOCK, location, message);
            self.children(block);
            return;
        }
        if self.newer_catalog
            && let Some(message) = upgrade::newer_catalog_problem(def, block)
        {
            self.unchecked.insert(block.id.clone());
            self.diags.error(codes::NEWER_BLOCK, location, message);
            self.children(block);
            return;
        }
        self.check_place(def, position, &location);
        let extras = self.extras(def, block, &location);
        self.fields(def, block, &extras, &location);
        self.inputs(def, block, &extras, &location);
        self.statements(def, block, &extras, &location);
    }

    /// Why the upgrade left `block` as it was.
    fn upgrade_failure(&self, block: &Block) -> String {
        self.failed_upgrades
            .get(&block.id)
            .cloned()
            .unwrap_or_else(|| upgrade::cannot_upgrade(self.catalog, block))
    }

    /// Checks the children of a block whose own definition is unusable.
    fn children(&mut self, block: &mut Block) {
        for input in block.inputs.values_mut() {
            if let Input::Block(nested) = input {
                self.block(&mut nested.block, Position::Value);
            }
        }
        for list in block.statements.values_mut() {
            for child in list {
                self.block(child, Position::Statement);
            }
        }
    }

    // -----------------------------------------------------------------------
    // Shapes
    // -----------------------------------------------------------------------

    /// Reports a block whose shape does not fit its place (`B2C-E0604`). Only
    /// the placements the editor cannot produce are reported (ADR-0012): a
    /// statement, reporter or predicate directly on the canvas is parked,
    /// which is the analyser's to report (`B2C-W0505`), not an error.
    fn check_place(&mut self, def: &BlockDef, position: Position, location: &Location) {
        let noun = block_noun(def);
        if let Position::TopLevel { stacked } = position
            && stacked > 0
            && def.shape != Shape::Statement
        {
            // Only a statement has a connection below it (spec §3.3).
            let message = if stacked == 1 {
                format!(
                    "1 block is stacked below this {noun}, but nothing can be attached below it, so the stacked block would never run. Move it inside a block, or delete it."
                )
            } else {
                format!(
                    "{stacked} blocks are stacked below this {noun}, but nothing can be attached below it, so the stacked blocks would never run. Move them inside a block, or delete them."
                )
            };
            self.diags.error(codes::WRONG_PLACE, location.clone(), message);
        }
        let message = match (position, def.shape) {
            // Hats and definitions belong on the canvas, and every other
            // shape is parked there.
            (Position::TopLevel { .. }, _)
            | (Position::Statement, Shape::Statement)
            | (Position::Value, Shape::Reporter | Shape::Predicate) => return,
            (Position::Statement | Position::Value, Shape::Hat | Shape::Definition) => {
                format!("A {noun} must sit directly on the canvas, not inside another block.")
            }
            (Position::Statement, Shape::Reporter | Shape::Predicate) => format!(
                "This {noun} gives a value and cannot be a step on its own. Put it into an input of another block."
            ),
            (Position::Value, Shape::Statement) => {
                format!("This {noun} is a step, not a value, so it cannot be plugged into an input.")
            }
        };
        self.diags.error(codes::WRONG_PLACE, location.clone(), message);
    }

    // -----------------------------------------------------------------------
    // Extras
    // -----------------------------------------------------------------------

    fn extras<'d>(
        &mut self,
        def: &'d BlockDef,
        block: &mut Block,
        location: &Location,
    ) -> BTreeMap<&'d str, Resolved> {
        for key in block.extra.keys() {
            if !def.extra.iter().any(|e| &e.name == key) {
                let message = format!(
                    "This block has no setting {} in \"extra\". Remove it or check its spelling.",
                    quote(key)
                );
                self.diags.error(codes::UNKNOWN_EXTRA, location.clone(), message);
            }
        }
        let mut resolved = BTreeMap::new();
        for extra in &def.extra {
            let state = if let Some(value) = block.extra.get(&extra.name) {
                self.extra(extra, value, location)
            } else {
                self.extra_default(extra, block, location)
            };
            resolved.insert(extra.name.as_str(), state);
        }
        resolved
    }

    /// Fills an absent extra from its default, or reports it missing.
    fn extra_default(&mut self, extra: &ExtraDef, block: &mut Block, location: &Location) -> Resolved {
        let default = match (extra.kind, &extra.default) {
            (ExtraKind::Count, Some(default)) => {
                count_default(extra, default).map(|n| (Value::from(n), Resolved::Count(n)))
            }
            (ExtraKind::Flag, Some(FieldDefault::Bool(flag))) => {
                Some((Value::Bool(*flag), Resolved::Flag(*flag)))
            }
            _ => None,
        };
        if let Some((value, state)) = default {
            block.extra.insert(extra.name.clone(), value);
            state
        } else {
            let message = format!(
                "This block is missing its setting {} in \"extra\".",
                quote(&extra.name)
            );
            self.diags.error(codes::MISSING_EXTRA, location.clone(), message);
            Resolved::Invalid
        }
    }

    fn extra(&mut self, extra: &ExtraDef, value: &Value, location: &Location) -> Resolved {
        let name = &extra.name;
        let (state, problem) = match extra.kind {
            ExtraKind::Count => match value.as_u64().and_then(|n| u32::try_from(n).ok()) {
                Some(n) if (extra.min..=extra.max).contains(&n) => (Resolved::Count(n), None),
                _ => (
                    Resolved::Invalid,
                    Some(format!(
                        "\"extra.{name}\" of this block is {}, but it must be a whole number from {} to {}.",
                        describe(value),
                        extra.min,
                        extra.max
                    )),
                ),
            },
            ExtraKind::Flag => match value {
                Value::Bool(flag) => (Resolved::Flag(*flag), None),
                _ => (
                    Resolved::Invalid,
                    Some(format!(
                        "\"extra.{name}\" of this block should be true or false, but it is {}.",
                        describe(value)
                    )),
                ),
            },
            ExtraKind::Params | ExtraKind::Fields | ExtraKind::Values => match value {
                Value::Array(rows) => {
                    let limit = usize::try_from(extra.max).unwrap_or(usize::MAX);
                    if rows.len() > limit {
                        let message = format!(
                            "This block has {} {}, but at most {} are allowed.",
                            rows.len(),
                            row_noun(extra.kind).1,
                            extra.max
                        );
                        self.diags.error(codes::BAD_EXTRA, location.clone(), message);
                    }
                    for (index, row) in rows.iter().enumerate() {
                        self.row(index + 1, row, extra, location);
                    }
                    let count = u32::try_from(rows.len()).unwrap_or(u32::MAX);
                    (Resolved::Rows(count), None)
                }
                _ => (
                    Resolved::Invalid,
                    Some(format!(
                        "\"extra.{name}\" of this block should be a list of {}, but it is {}.",
                        row_noun(extra.kind).1,
                        describe(value)
                    )),
                ),
            },
        };
        if let Some(message) = problem {
            self.diags.error(codes::BAD_EXTRA, location.clone(), message);
        }
        state
    }

    /// One row of a row extra: a function's parameter, a struct's field or
    /// an enum's value (w2.md §2.1).
    fn row(&mut self, number: usize, row: &Value, extra: &ExtraDef, location: &Location) {
        let keys = extra.kind.row_keys();
        let (noun, plural) = row_noun(extra.kind);
        let mut problems = Vec::new();
        if let Value::Object(map) = row {
            for key in map.keys() {
                if !keys.contains(&key.as_str()) {
                    problems.push(format!("has an unknown key {}", quote(key)));
                }
            }
            match map.get("sym") {
                Some(Value::String(sym)) if SymbolId::new(sym).is_ok() => {}
                _ => problems.push(String::from("needs a valid symbol ID in \"sym\"")),
            }
            if !matches!(map.get("name"), Some(Value::String(_))) {
                problems.push(String::from("needs a name in \"name\""));
            }
            let row_type = map.get("type");
            if extra.kind.has_row_types()
                && row_type.is_none_or(|ty| signature::row_type_fit(extra, ty) != TypeFit::Fits)
            {
                let wider = extra
                    .type_class
                    .map(|class| format!(", or {}", signature::class_phrase(class)))
                    .unwrap_or_default();
                problems.push(format!(
                    "has the type {}, but {plural} can only be {}{wider}",
                    describe_row_type(row_type),
                    list(extra.types.iter().map(String::as_str))
                ));
            }
            if extra.kind == ExtraKind::Params {
                match map.get("mode") {
                    Some(Value::String(m)) if PARAM_MODES.contains(&m.as_str()) => {}
                    other => problems.push(format!(
                        "has the mode {}, but the mode must be {}",
                        describe_choice(other),
                        list(PARAM_MODES)
                    )),
                }
            }
        } else {
            let keys: Vec<String> = keys.iter().map(|key| format!("\"{key}\"")).collect();
            let (last, first) = keys
                .split_last()
                .map_or((String::new(), &[][..]), |(last, first)| (last.clone(), first));
            problems.push(format!(
                "should be an object with {} and {last}, but it is {}",
                first.join(", "),
                describe(row)
            ));
        }
        for problem in problems {
            self.diags.error(
                codes::BAD_PARAM,
                location.clone(),
                format!("{noun} {number} of this block {problem}."),
            );
        }
    }

    // -----------------------------------------------------------------------
    // Fields
    // -----------------------------------------------------------------------

    fn fields(
        &mut self,
        def: &BlockDef,
        block: &mut Block,
        extras: &BTreeMap<&str, Resolved>,
        location: &Location,
    ) {
        for name in block.fields.keys() {
            let message = match field_def(def, name) {
                None => format!(
                    "This block has no field {}. Remove it or check its spelling.",
                    quote(name)
                ),
                Some(field) => match (&field.repeat, repeat_index(name, &field.name)) {
                    (Some(repeat), Some(index)) => {
                        match Self::repeat_count(extras, &repeat.count, repeat.plus) {
                            Some(count) if index >= count => format!(
                                "This block has no field {}: it has {count} {} field(s), set by \"extra.{}\".",
                                quote(name),
                                field.name,
                                repeat.count
                            ),
                            _ => continue,
                        }
                    }
                    _ => continue,
                },
            };
            self.diags
                .error(codes::UNKNOWN_FIELD, field_location(location, name), message);
        }
        for field in &def.field {
            let names: Vec<String> = match &field.repeat {
                None => vec![field.name.clone()],
                Some(repeat) => match Self::repeat_count(extras, &repeat.count, repeat.plus) {
                    Some(count) => (0..count).map(|i| format!("{}{i}", field.name)).collect(),
                    None => Vec::new(),
                },
            };
            for name in names {
                if let Some(value) = block.fields.get(&name) {
                    self.field(def, field, &name, value, location);
                    continue;
                }
                match &field.default {
                    Some(FieldDefault::Bool(flag)) => {
                        block.fields.insert(name, FieldValue::Bool(*flag));
                    }
                    Some(FieldDefault::Text(text)) => {
                        block.fields.insert(name, FieldValue::Text(text.clone()));
                    }
                    // Nothing chosen yet: the editor saves an unchosen name or
                    // reference, or a user type not chosen in a type field
                    // that lists only the module's types, as an absent field.
                    // That is the analyser's to report, once the block is part
                    // of the program (ADR-0012).
                    None if matches!(field.kind, FieldKind::SymbolDecl | FieldKind::SymbolRef)
                        || (field.kind == FieldKind::Type && field.types.is_empty()) => {}
                    None => {
                        let message = format!("This block is missing its field {name}.");
                        self.diags
                            .error(codes::MISSING_FIELD, field_location(location, &name), message);
                    }
                }
            }
        }
    }

    /// Checks the value of the field part `name` of `field`'s definition.
    fn field(
        &mut self,
        def: &BlockDef,
        field: &FieldDef,
        name: &str,
        value: &FieldValue,
        location: &Location,
    ) {
        let problem = match (field.kind, value) {
            (FieldKind::Dropdown, FieldValue::Text(text))
                if field.options.iter().any(|[_, option]| option == text) =>
            {
                return;
            }
            (FieldKind::Dropdown, _) => format!(
                "The field {name} is {}, but it must be one of {}.",
                describe_field(value),
                list(field.options.iter().map(|[_, option]| option.as_str()))
            ),
            (FieldKind::Checkbox, FieldValue::Bool(_))
            | (FieldKind::Text, FieldValue::Text(_))
            | (FieldKind::SymbolDecl, FieldValue::Decl(_))
            | (FieldKind::SymbolRef, FieldValue::Ref(_)) => return,
            (FieldKind::Number, FieldValue::Text(text)) if !text.trim().is_empty() => return,
            (FieldKind::Type, _) if signature::field_type_fit(def, field, value) == TypeFit::Fits => return,
            (FieldKind::Header, FieldValue::Text(text)) if signature::header_fits(text) => return,
            (FieldKind::Checkbox, _) => format!(
                "The field {name} should be true or false, but it is {}.",
                describe_field(value)
            ),
            (FieldKind::Text, _) => format!(
                "The field {name} should be text, but it is {}.",
                describe_field(value)
            ),
            (FieldKind::Number, _) => format!(
                "The field {name} should be a number written as text, such as \"42\", but it is {}.",
                describe_field(value)
            ),
            (FieldKind::Type, _) => {
                let wider = signature::field_wider(def, field)
                    .map(|wider| format!(", or {wider}"))
                    .unwrap_or_default();
                format!(
                    "The field {name} is {}, but it must be one of the types {}{wider}.",
                    describe_type(value),
                    list(field.types.iter().map(String::as_str))
                )
            }
            (FieldKind::Header, _) => format!(
                "The field {name} is {}, but it must be a standard header such as \"<vector>\".",
                describe_field(value)
            ),
            (FieldKind::SymbolDecl, _) => format!(
                "The field {name} should declare a name, like {{\"sym\": \"sym_x\", \"name\": \"score\"}}, but it is {}.",
                describe_field(value)
            ),
            (FieldKind::SymbolRef, _) => format!(
                "The field {name} should refer to a variable or function, like {{\"ref\": \"sym_x\"}}, but it is {}.",
                describe_field(value)
            ),
        };
        self.diags
            .error(codes::BAD_FIELD, field_location(location, name), problem);
    }

    // -----------------------------------------------------------------------
    // Value inputs and statement inputs
    // -----------------------------------------------------------------------

    /// The number of parts a repeated input or statement has, if known.
    fn repeat_count(extras: &BTreeMap<&str, Resolved>, count: &str, plus: u32) -> Option<u32> {
        match extras.get(count) {
            Some(Resolved::Count(n) | Resolved::Rows(n)) => Some(n.saturating_add(plus)),
            _ => None,
        }
    }

    fn inputs(
        &mut self,
        def: &BlockDef,
        block: &mut Block,
        extras: &BTreeMap<&str, Resolved>,
        location: &Location,
    ) {
        for name in block.inputs.keys() {
            let problem = def.input.iter().find_map(|input| match &input.repeat {
                None => (input.name == *name).then_some(None),
                Some(repeat) => repeat_index(name, &input.name).map(|index| {
                    let count = Self::repeat_count(extras, &repeat.count, repeat.plus)?;
                    (index >= count).then(|| {
                        format!(
                            "This block has no input {}: it has {count} {} input(s), set by \"extra.{}\".",
                            quote(name),
                            input.name,
                            repeat.count
                        )
                    })
                }),
            });
            let message = match problem {
                Some(None) => continue,
                Some(Some(message)) => message,
                None => format!(
                    "This block has no input {}. Remove it or check its spelling.",
                    quote(name)
                ),
            };
            self.diags
                .error(codes::UNKNOWN_INPUT, input_location(location, name), message);
        }
        for input in &def.input {
            let names: Vec<String> = match &input.repeat {
                None => vec![input.name.clone()],
                Some(repeat) => match Self::repeat_count(extras, &repeat.count, repeat.plus) {
                    Some(count) => (0..count).map(|i| format!("{}{i}", input.name)).collect(),
                    None => Vec::new(),
                },
            };
            let defaults = default_tokens(input).unwrap_or_default();
            for name in names {
                if block.inputs.contains_key(&name) {
                    continue;
                }
                if !defaults.is_empty() {
                    let filled = Input::Expr(ExprInput {
                        expr: defaults.clone(),
                        draft: false,
                    });
                    block.inputs.insert(name, filled);
                } else if !input.optional {
                    let message = format!("This block needs a value in its input {name}.");
                    self.diags
                        .error(codes::MISSING_INPUT, input_location(location, &name), message);
                }
            }
        }
        for input in block.inputs.values_mut() {
            if let Input::Block(nested) = input {
                self.block(&mut nested.block, Position::Value);
            }
        }
    }

    fn statements(
        &mut self,
        def: &BlockDef,
        block: &mut Block,
        extras: &BTreeMap<&str, Resolved>,
        location: &Location,
    ) {
        for name in block.statements.keys() {
            let problem = def.statement.iter().find_map(|statement| {
                if let Some(repeat) = &statement.repeat {
                    return repeat_index(name, &statement.name).map(|index| {
                        let count = Self::repeat_count(extras, &repeat.count, repeat.plus)?;
                        (index >= count).then(|| {
                            format!(
                                "This block has no part {}: it has {count} {} part(s), set by \"extra.{}\".",
                                quote(name),
                                statement.name,
                                repeat.count
                            )
                        })
                    });
                }
                if statement.name != *name {
                    return None;
                }
                let Some(flag) = statement.when.as_deref() else {
                    return Some(None);
                };
                match extras.get(flag) {
                    Some(Resolved::Flag(false)) => Some(Some(format!(
                        "This block has the part {}, but \"extra.{flag}\" is false, so it has no such part. Set \"extra.{flag}\" to true or remove the part.",
                        quote(name)
                    ))),
                    _ => Some(None),
                }
            });
            let message = match problem {
                Some(None) => continue,
                Some(Some(message)) => message,
                None => format!(
                    "This block has no part {} for other blocks. Remove it or check its spelling.",
                    quote(name)
                ),
            };
            self.diags
                .error(codes::UNKNOWN_STATEMENT, input_location(location, name), message);
        }
        for list in block.statements.values_mut() {
            for child in list {
                self.block(child, Position::Statement);
            }
        }
    }
}

/// How messages name one row and the rows of a row extra.
fn row_noun(kind: ExtraKind) -> (&'static str, &'static str) {
    match kind {
        ExtraKind::Fields => ("Field", "fields"),
        ExtraKind::Values => ("Value", "values"),
        ExtraKind::Params | ExtraKind::Count | ExtraKind::Flag => ("Parameter", "parameters"),
    }
}

/// The message for a block type the catalog does not define. Block IDs are
/// namespaced by pack (`sfml.window.open` comes from the pack `sfml`), so a
/// well-formed ID names the pack that is probably missing (spec §5.4, §6.3).
fn unknown_block_message(block_type: &str) -> String {
    let pack = block_type
        .split('.')
        .next()
        .filter(|_| definitions::is_block_id(block_type));
    match pack {
        Some(pack) => format!(
            "This block has the type {}, which this version of Blocks2Cpp does not know. Missing pack: {}. Install that library pack, or update Blocks2Cpp if the block comes from a newer version.",
            quote(block_type),
            quote(pack)
        ),
        None => format!(
            "This block has the type {}, which is not a valid block type. Block types look like \"io.print\".",
            quote(block_type)
        ),
    }
}

/// A block's short name (03 §3.11.1): its friendly label up to the first
/// field or input, without the spaces and punctuation that lead into it
/// (`print`, `repeat`, `when program starts`, `character` for `character
/// '%VALUE'`). `None` when the label starts with a field or input
/// (`%A %OP %B`, `"%VALUE"`).
pub(crate) fn short_name(def: &BlockDef) -> Option<&str> {
    let label = def.label.friendly.as_str();
    let end = label.find(['%', '…']).unwrap_or(label.len());
    let name = label[..end]
        .trim_end_matches(|c: char| c.is_whitespace() || "'\"([{=,:".contains(c))
        .trim_start();
    (!name.is_empty()).then_some(name)
}

/// How a message names a block: `"print" block`, or `block` for a block
/// without a short name, as in *This "print" block …* and *This block …*.
/// The catalog ID is never shown (03 §3.11.1).
pub(crate) fn block_noun(def: &BlockDef) -> String {
    match short_name(def) {
        Some(name) => format!("{} block", quote(name)),
        None => String::from("block"),
    }
}

/// The message of `B2C-E0602`: a block saved by a newer version of its
/// definition than the catalog has.
fn newer_block_message(def: &BlockDef, saved: u32) -> String {
    format!(
        "This {} was made with a newer version of Blocks2Cpp: it is version {saved}, and this version knows only up to version {}. Update Blocks2Cpp to use it.",
        block_noun(def),
        def.version
    )
}

fn field_location(location: &Location, name: &str) -> Location {
    location.clone().with_part(Part::Field {
        name: name.to_owned(),
    })
}

fn input_location(location: &Location, name: &str) -> Location {
    location.clone().with_part(Part::Input {
        name: name.to_owned(),
    })
}

/// `"a", "b" or "c"`.
fn list<'a>(items: impl IntoIterator<Item = &'a str>) -> String {
    let quoted: Vec<String> = items.into_iter().map(quote).collect();
    match quoted.split_last() {
        None => String::from("nothing"),
        Some((last, [])) => last.clone(),
        Some((last, rest)) => format!("{} or {last}", rest.join(", ")),
    }
}

/// Describes a JSON value for a message.
fn describe(value: &Value) -> String {
    match value {
        Value::Null => String::from("null"),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(text) => format!("the text {}", quote(text)),
        Value::Array(_) => String::from("a list"),
        Value::Object(_) => String::from("an object"),
    }
}

/// Describes the value of a setting that must be one of a list of names:
/// `"float"` for text, otherwise like [`describe`].
fn describe_choice(value: Option<&Value>) -> String {
    match value {
        None => String::from("nothing"),
        Some(Value::String(text)) => quote(text),
        Some(other) => describe(other),
    }
}

/// Describes the value of a type field for a message: a name as it is
/// written, a template by its C++ spelling, a reference as a user type.
pub(crate) fn describe_type(value: &FieldValue) -> String {
    match value {
        FieldValue::Type(term) => describe_term(term),
        FieldValue::Ref(_) => String::from("a struct or enum"),
        FieldValue::Bool(_) | FieldValue::Text(_) | FieldValue::Decl(_) => describe_field(value),
    }
}

/// Describes the `type` of a parameter row for a message, as
/// [`describe_type`] does.
pub(crate) fn describe_row_type(value: Option<&Value>) -> String {
    match value {
        Some(value @ (Value::Object(_) | Value::Array(_))) => {
            match serde_json::from_value::<TypeTerm>(value.clone()) {
                Ok(TypeTerm::Ref(_)) => String::from("a struct or enum"),
                Ok(term) => describe_term(&term),
                Err(_) => describe(value),
            }
        }
        other => describe_choice(other),
    }
}

/// A type term by its C++ spelling (`"std::vector<int>"`), or by its JSON
/// when it is no type.
fn describe_term(term: &TypeTerm) -> String {
    match Type::from_term(term) {
        Ok(ty) => quote(&ty.cpp(&NoFacts).to_string()),
        Err(_) => quote(&term.key()),
    }
}

/// Describes a field value for a message.
fn describe_field(value: &FieldValue) -> String {
    match value {
        FieldValue::Bool(b) => b.to_string(),
        FieldValue::Text(text) => quote(text),
        FieldValue::Decl(_) => String::from("a name declaration"),
        FieldValue::Ref(_) => String::from("a reference to a variable or function"),
        FieldValue::Type(_) => String::from("a type"),
    }
}

/// Characters that must not be shown raw in a message: controls and every
/// invisible character the C++ encoders escape
/// ([`b2c_ir::text::is_invisible`], which covers all format characters,
/// including the invisible "tag" characters that can smuggle hidden text),
/// plus a few more that render as nothing.
fn is_unsafe_to_show(c: char) -> bool {
    c.is_control()
        || b2c_ir::text::is_invisible(c)
        || matches!(c,
            '\u{00AD}' | '\u{061C}' | '\u{180E}'
            | '\u{200B}'..='\u{200F}'
            | '\u{2028}'..='\u{202E}'
            | '\u{2060}'..='\u{206F}'
            | '\u{FEFF}' | '\u{FFF9}'..='\u{FFFB}')
}

/// Quotes untrusted text for a message: invisible and control characters as
/// `\u{XXXX}`, cut after 40 characters.
pub(crate) fn quote(text: &str) -> String {
    let mut out = String::from("\"");
    for (index, c) in text.chars().enumerate() {
        if index == 40 {
            out.push('…');
            break;
        }
        if is_unsafe_to_show(c) {
            let _ = write!(out, "\\u{{{:04X}}}", u32::from(c));
        } else if c == '"' || c == '\\' {
            out.push('\\');
            out.push(c);
        } else {
            out.push(c);
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use b2c_ir::Diagnostic;

    use super::*;

    /// The completed document and the diagnostics of a report.
    fn parts(report: ResolveReport) -> (Document, Vec<Diagnostic>) {
        (report.document, report.diagnostics)
    }

    /// The core catalog with two definitions of the shapes W2 adds
    /// (w2.md §2.1): `test.record`, with `fields` and `values` rows and an
    /// input repeated by the rows of `fields`, and `test.attempt`, with a
    /// repeated type field and a repeated name typed by it.
    fn catalog_w2() -> Catalog {
        let text = r#"
[[block]]
id = "test.record"
version = 1
category = "types"
shape = "definition"
label = { friendly = "record %NAME", cpp = "struct %NAME" }
lowering = "builtin"
help = "A record."

[[block.field]]
name = "NAME"
kind = "symbol_decl"

[[block.extra]]
name = "fields"
kind = "fields"
max = 4
types = ["int", "std::string"]
type_class = "value"

[[block.extra]]
name = "values"
kind = "values"
max = 4

[[block.input]]
name = "DEFAULT"
check = "any"
optional = true
repeat = { count = "fields" }

[[block]]
id = "test.attempt"
version = 1
category = "errors"
shape = "statement"
label = { friendly = "try %TRY catch %ERROR as %NAME %CATCH catch …", cpp = "try" }
lowering = "builtin"
help = "Tries."

[[block.extra]]
name = "catchCount"
kind = "count"
min = 1
max = 4
default = "1"

[[block.field]]
name = "ERROR"
kind = "type"
types = ["std::exception", "std::runtime_error"]
default = "std::exception"
repeat = { count = "catchCount" }

[[block.field]]
name = "NAME"
kind = "symbol_decl"
type = "field:ERROR"
repeat = { count = "catchCount" }

[[block.statement]]
name = "TRY"

[[block.statement]]
name = "CATCH"
repeat = { count = "catchCount" }
"#;
        let file: crate::schema::CatalogFile = toml::from_str(text).unwrap();
        let mut catalog = crate::core_catalog().clone();
        for def in file.block {
            assert_eq!(definitions::check_block(&def), Vec::<String>::new(), "{}", def.id);
            catalog.blocks.insert(def.id.clone(), def);
        }
        catalog
    }

    /// A document with `blocks` on the canvas and `statements` in `main`.
    fn w2_document(blocks: Vec<Value>, statements: &[Value]) -> Document {
        let mut top = vec![serde_json::json!({
            "id": "b_main", "type": "program.main", "v": 1, "statements": {"BODY": statements}
        })];
        top.extend(blocks);
        let value = serde_json::json!({
            "format": "blocks2cpp/project",
            "formatVersion": 1,
            "generator": {"app": "0.1.0", "catalog": "1.3.0"},
            "project": {"id": "p", "name": "P", "language": {"standard": "c++20"}},
            "modules": [{"id": "m", "name": "main", "workspace": {"blocks": top}}]
        });
        b2c_model::load(&serde_json::to_vec(&value).unwrap()).unwrap()
    }

    fn messages(diagnostics: &[Diagnostic]) -> Vec<(&str, &str)> {
        diagnostics
            .iter()
            .map(|d| (d.code.0.as_str(), d.message.as_str()))
            .collect()
    }

    #[test]
    fn field_and_value_rows_are_checked_like_parameters() {
        let catalog = catalog_w2();
        let record = |extra: Value, inputs: Value| {
            serde_json::json!({"id": "b_rec", "type": "test.record", "v": 1, "x": 0, "y": 0,
                "fields": {"NAME": {"sym": "s_rec", "name": "Student"}}, "extra": extra, "inputs": inputs})
        };
        let good = record(
            serde_json::json!({
                "fields": [{"sym": "s_name", "name": "name", "type": "std::string"},
                           {"sym": "s_list", "name": "marks", "type": {"std::vector": "int"}}],
                "values": [{"sym": "s_a", "name": "A"}]
            }),
            serde_json::json!({"DEFAULT1": {"expr": [{"num": "0"}]}}),
        );
        let (_, diagnostics) = parts(run(&w2_document(vec![good], &[]), &catalog));
        assert_eq!(messages(&diagnostics), []);

        let bad = record(
            serde_json::json!({
                "fields": [{"sym": "s_name", "name": "name", "type": "std::string", "mode": "copy"},
                           {"sym": "s_x", "name": "x", "type": "void"}],
                "values": [{"sym": "s_a", "name": "A", "type": "int"}, "B"]
            }),
            serde_json::json!({"DEFAULT2": {"expr": [{"num": "0"}]}}),
        );
        let (_, diagnostics) = parts(run(&w2_document(vec![bad], &[]), &catalog));
        assert_eq!(
            messages(&diagnostics),
            [
                (
                    codes::BAD_PARAM,
                    "Field 1 of this block has an unknown key \"mode\"."
                ),
                (
                    codes::BAD_PARAM,
                    "Field 2 of this block has the type \"void\", but fields can only be \"int\" or \"std::string\", or another type a variable can hold."
                ),
                (
                    codes::BAD_PARAM,
                    "Value 1 of this block has an unknown key \"type\"."
                ),
                (
                    codes::BAD_PARAM,
                    "Value 2 of this block should be an object with \"sym\" and \"name\", but it is the text \"B\"."
                ),
                (
                    codes::UNKNOWN_INPUT,
                    "This block has no input \"DEFAULT2\": it has 2 DEFAULT input(s), set by \"extra.fields\"."
                ),
            ]
        );
    }

    #[test]
    fn repeated_fields_have_a_part_per_count() {
        let catalog = catalog_w2();
        let attempt = |fields: Value| {
            serde_json::json!({"id": "b_try", "type": "test.attempt", "v": 1,
                "extra": {"catchCount": 2}, "fields": fields})
        };
        let filled = attempt(serde_json::json!({
            "ERROR0": "std::runtime_error",
            "NAME0": {"sym": "s_e", "name": "e"}
        }));
        let (completed, diagnostics) = parts(run(&w2_document(vec![], &[filled]), &catalog));
        assert_eq!(messages(&diagnostics), []);
        let block = &completed.modules[0].workspace.blocks[0].statements["BODY"][0];
        // The missing part takes the field's default; an unchosen name stays
        // absent, for the analyser to report.
        assert_eq!(
            block.fields["ERROR0"],
            FieldValue::Text("std::runtime_error".into())
        );
        assert_eq!(block.fields["ERROR1"], FieldValue::Text("std::exception".into()));
        assert!(!block.fields.contains_key("NAME1"));
        assert!(!block.fields.contains_key("ERROR"));

        let extra = attempt(
            serde_json::json!({"ERROR2": "std::exception", "ERROR": "std::exception", "ERROR0": "int"}),
        );
        let (_, diagnostics) = parts(run(&w2_document(vec![], &[extra]), &catalog));
        assert_eq!(
            messages(&diagnostics),
            [
                (
                    codes::UNKNOWN_FIELD,
                    "This block has no field \"ERROR\". Remove it or check its spelling."
                ),
                (
                    codes::UNKNOWN_FIELD,
                    "This block has no field \"ERROR2\": it has 2 ERROR field(s), set by \"extra.catchCount\"."
                ),
                (
                    codes::BAD_FIELD,
                    "The field ERROR0 is \"int\", but it must be one of the types \"std::exception\" or \"std::runtime_error\"."
                ),
            ]
        );
    }

    /// A synthetic `io.print` v1 → v2 step: the separator `space` was
    /// renamed to `spaces`.
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

    /// The core catalog with `io.print` at version 2, whose `SEP` options
    /// say `spaces` instead of `space`.
    fn catalog_v2() -> Catalog {
        let mut catalog = crate::core_catalog().clone();
        let print = catalog.blocks.get_mut("io.print").unwrap();
        print.version = 2;
        for option in &mut print.field.iter_mut().find(|f| f.name == "SEP").unwrap().options {
            if option[1] == "space" {
                option[1] = String::from("spaces");
            }
        }
        catalog
    }

    fn document(sep: &str) -> Document {
        let value = serde_json::json!({
            "format": "blocks2cpp/project",
            "formatVersion": 1,
            "generator": {"app": "0.1.0", "catalog": "1.0.0"},
            "project": {"id": "p", "name": "P", "language": {"standard": "c++20"}},
            "modules": [{"id": "m", "name": "main", "workspace": {"blocks": [{
                "id": "b_main", "type": "program.main", "v": 1,
                "statements": {"BODY": [{"id": "b1", "type": "io.print", "v": 1, "fields": {"SEP": sep}}]}
            }]}}]
        });
        b2c_model::load(&serde_json::to_vec(&value).unwrap()).unwrap()
    }

    const CHAIN: BlockUpgrades<'static> = BlockUpgrades {
        migrations: &[crate::BlockMigration {
            block: "io.print",
            from: 1,
            apply: rename_separator,
        }],
        replacements: &[],
        retired: &[],
    };

    #[test]
    fn older_blocks_are_upgraded_before_they_are_checked() {
        let catalog = catalog_v2();
        let (completed, diagnostics) = parts(run_with(&document("space"), &catalog, &CHAIN));
        assert_eq!(diagnostics, []);
        let print = &completed.modules[0].workspace.blocks[0].statements["BODY"][0];
        assert_eq!(print.v, 2);
        assert_eq!(print.fields["SEP"], FieldValue::Text("spaces".into()));
        // The upgraded block is complete, and resolving it again is a no-op.
        assert_eq!(print.fields["STREAM"], FieldValue::Text("out".into()));
        assert_eq!(
            parts(run_with(&completed, &catalog, &CHAIN)),
            (completed.clone(), vec![])
        );
    }

    #[test]
    fn blocks_that_cannot_be_upgraded_are_reported() {
        let catalog = catalog_v2();
        let (completed, diagnostics) = parts(run_with(&document("bad"), &catalog, &CHAIN));
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code.0, codes::OLD_BLOCK);
        assert_eq!(
            diagnostics[0].message,
            "This \"print\" block is version 1, and upgrading it from version 1 failed: the separator is unknown."
        );
        let print = &completed.modules[0].workspace.blocks[0].statements["BODY"][0];
        assert_eq!(print.v, 1, "a failed upgrade leaves the block as it was");

        let none = BlockUpgrades {
            migrations: &[],
            ..CHAIN
        };
        let (_, diagnostics) = parts(run_with(&document("space"), &catalog, &none));
        assert_eq!(
            diagnostics[0].message,
            "This \"print\" block is version 1, which this version of Blocks2Cpp cannot upgrade to version 2."
        );
    }

    /// `test.say` v1 → `io.print` v1, which needs a `WHAT` input.
    fn say_to_print(block: &mut Block) -> Result<(), String> {
        let what = block.inputs.remove("WHAT").ok_or("the block has no WHAT input")?;
        block.inputs.insert(String::from("ITEM0"), what);
        Ok(())
    }

    const RETIRING: BlockUpgrades<'static> = BlockUpgrades {
        migrations: &[],
        replacements: &[crate::BlockReplacement {
            from: "test.say",
            version: 1,
            to: "io.print",
            to_version: 1,
            apply: say_to_print,
        }],
        retired: &["test.say"],
    };

    #[test]
    fn retired_blocks_are_replaced_or_reported_as_old() {
        let value = serde_json::json!({
            "format": "blocks2cpp/project", "formatVersion": 1,
            "generator": {"app": "0.1.0", "catalog": "1.0.0"},
            "project": {"id": "p", "name": "P", "language": {"standard": "c++20"}},
            "modules": [{"id": "m", "name": "main", "workspace": {"blocks": [{
                "id": "b_main", "type": "program.main", "v": 1,
                "statements": {"BODY": [
                    {"id": "b1", "type": "test.say", "v": 1, "inputs": {"WHAT": {"expr": [{"str": "hi"}]}}},
                    {"id": "b2", "type": "test.say", "v": 1, "statements": {"X": [print("b3")]}}
                ]}
            }]}}]
        });
        let document = b2c_model::load(&serde_json::to_vec(&value).unwrap()).unwrap();
        let (completed, diagnostics) = parts(run_with(&document, crate::core_catalog(), &RETIRING));
        let body = &completed.modules[0].workspace.blocks[0].statements["BODY"];
        assert_eq!(body[0].block_type, "io.print");
        assert!(body[0].inputs.contains_key("ITEM0"));
        // The one that cannot be replaced is old (E0603), not unknown
        // (E0601), and the block inside it is still checked.
        assert_eq!(body[1].block_type, "test.say");
        assert!(body[1].statements["X"][0].inputs.contains_key("ITEM0"));
        assert_eq!(
            codes_and_blocks(&diagnostics),
            [(codes::OLD_BLOCK.to_owned(), "b2".to_owned())]
        );
        assert_eq!(
            diagnostics[0].message,
            "This block is version 1 of a retired kind of block, which is now the \"print\" block, and replacing it failed: the block has no WHAT input."
        );
        // Without the replacement, the type is simply unknown.
        let (_, diagnostics) = parts(run(&document, crate::core_catalog()));
        assert_eq!(diagnostics.len(), 2);
        assert!(diagnostics.iter().all(|d| d.code.0 == codes::UNKNOWN_BLOCK));
    }

    #[test]
    fn a_block_the_upgrade_did_not_see_is_reported_as_old() {
        // The resolver alone, without the upgrade before it.
        let old = serde_json::json!({"id": "b1", "type": "io.print", "v": 0});
        let (_, _, diagnostics) = resolve_stack(old, vec![]);
        assert_eq!(diagnostics[0].code.0, codes::OLD_BLOCK);
        assert_eq!(
            diagnostics[0].message,
            "This \"print\" block is version 0, which this version of Blocks2Cpp cannot upgrade."
        );
    }

    #[test]
    fn a_newer_catalog_makes_unknown_names_newer_blocks() {
        let print = serde_json::json!({"id": "b1", "type": "io.print", "v": 1,
            "fields": {"SEP": "tabs", "VOLUME": "3"}, "extra": {"style": 1}});
        let value = |catalog: &str| {
            serde_json::json!({
                "format": "blocks2cpp/project", "formatVersion": 1,
                "generator": {"app": "0.1.0", "catalog": catalog},
                "project": {"id": "p", "name": "P", "language": {"standard": "c++20"}},
                "modules": [{"id": "m", "name": "main", "workspace": {"blocks": [{
                    "id": "b_main", "type": "program.main", "v": 1,
                    "statements": {"BODY": [print.clone()]}
                }]}}]
            })
        };
        let resolve = |catalog: &str| {
            let document = b2c_model::load(&serde_json::to_vec(&value(catalog)).unwrap()).unwrap();
            parts(run(&document, crate::core_catalog()))
        };
        let (completed, diagnostics) = resolve("1.4.0");
        assert_eq!(
            codes_and_blocks(&diagnostics),
            [(codes::NEWER_BLOCK.to_owned(), "b1".to_owned())]
        );
        // The first thing it does not know, in field order.
        assert!(
            diagnostics[0]
                .message
                .contains("it has the choice \"tabs\" in its field \"SEP\"")
        );
        // Kept as it is: no defaults filled in.
        let kept = &completed.modules[0].workspace.blocks[0].statements["BODY"][0];
        assert!(!kept.inputs.contains_key("ITEM0"));
        assert!(!kept.fields.contains_key("STREAM"));
        // The same block from this catalog or an older one is wrong instead.
        for catalog in [crate::CATALOG_VERSION, "1.1.0", "1.0.0", "1.3"] {
            let (_, diagnostics) = resolve(catalog);
            let found: Vec<&str> = diagnostics.iter().map(|d| d.code.0.as_str()).collect();
            assert_eq!(
                found,
                [codes::UNKNOWN_EXTRA, codes::UNKNOWN_FIELD, codes::BAD_FIELD],
                "{catalog}"
            );
        }
    }

    /// A block from its JSON form (tests only; files go through the loader).
    fn block(value: Value) -> Block {
        serde_json::from_value(value).unwrap()
    }

    /// Resolves one top-level block with a loose stack against the core
    /// catalog, as the stage does for every block on a canvas.
    fn resolve_stack(head: Value, stacked: Vec<Value>) -> (Block, Vec<Block>, Vec<Diagnostic>) {
        let catalog = crate::core_catalog();
        let mut resolver = Resolver::new(catalog, &BlockUpgrades::SHIPPED, &catalog.version);
        resolver.module = ModuleId::new("m").ok();
        let mut head = block(head);
        let mut stacked: Vec<Block> = stacked.into_iter().map(block).collect();
        resolver.top_level(&mut head, &mut stacked);
        (head, stacked, resolver.diags.finish())
    }

    fn print(id: &str) -> Value {
        serde_json::json!({"id": id, "type": "io.print", "v": 1})
    }

    fn codes_and_blocks(diagnostics: &[Diagnostic]) -> Vec<(String, String)> {
        diagnostics
            .iter()
            .map(|d| {
                let block = d.primary.block.as_ref().map_or("", |b| b.as_str());
                (d.code.0.clone(), block.to_owned())
            })
            .collect()
    }

    #[test]
    fn a_parked_stack_is_no_catalog_problem() {
        // A statement on the canvas, with a stack below it, is parked
        // (ADR-0012): the analyser reports it, not the catalog.
        let (head, stacked, diagnostics) = resolve_stack(print("b1"), vec![print("b2"), print("b3")]);
        assert_eq!(diagnostics, []);
        // Parked blocks are completed like any other block.
        assert!(head.inputs.contains_key("ITEM0"));
        assert!(stacked.iter().all(|b| b.inputs.contains_key("ITEM0")));
        // A head alone, a disabled head, and parked values: no problem either.
        let disabled = serde_json::json!({"id": "b1", "type": "io.print", "v": 1, "disabled": true});
        let reporter =
            serde_json::json!({"id": "b1", "type": "math.number", "v": 1, "fields": {"VALUE": "1"}});
        let predicate = serde_json::json!({"id": "b1", "type": "logic.boolean", "v": 1});
        for head in [print("b1"), disabled, reporter, predicate] {
            let (_, _, diagnostics) = resolve_stack(head, vec![]);
            assert_eq!(diagnostics, []);
        }
    }

    #[test]
    fn parked_blocks_keep_every_other_catalog_check() {
        let bad_field =
            serde_json::json!({"id": "b1", "type": "io.print", "v": 1, "fields": {"SEP": "tabs"}});
        let (_, _, diagnostics) = resolve_stack(bad_field, vec![]);
        assert_eq!(
            codes_and_blocks(&diagnostics),
            [(codes::BAD_FIELD.to_owned(), "b1".to_owned())]
        );
        // An unknown type (such as Raw C++ before M4) stays an error when it
        // is parked, as the head and in the stack.
        let raw = serde_json::json!({"id": "b1", "type": "raw.cpp", "v": 1});
        let unknown = serde_json::json!({"id": "b2", "type": "x.nope", "v": 1});
        let (_, _, diagnostics) = resolve_stack(raw, vec![unknown]);
        assert_eq!(
            codes_and_blocks(&diagnostics),
            [
                (codes::UNKNOWN_BLOCK.to_owned(), "b1".to_owned()),
                (codes::UNKNOWN_BLOCK.to_owned(), "b2".to_owned()),
            ]
        );
        let newer = serde_json::json!({"id": "b1", "type": "control.break", "v": 2});
        let (_, _, diagnostics) = resolve_stack(newer, vec![]);
        assert_eq!(
            codes_and_blocks(&diagnostics),
            [(codes::NEWER_BLOCK.to_owned(), "b1".to_owned())]
        );
        assert_eq!(
            diagnostics[0].message,
            "This \"leave loop\" block was made with a newer version of Blocks2Cpp: it is version 2, and this version knows only up to version 1. Update Blocks2Cpp to use it."
        );
    }

    #[test]
    fn stacked_blocks_are_checked_like_statements() {
        let bad_field =
            serde_json::json!({"id": "b3", "type": "io.print", "v": 1, "fields": {"SEP": "tabs"}});
        let reporter = serde_json::json!({"id": "b4", "type": "math.number", "v": 1});
        let hat = serde_json::json!({"id": "b5", "type": "program.main", "v": 1});
        let unknown = serde_json::json!({"id": "b6", "type": "x.nope", "v": 1});
        let join = serde_json::json!({"id": "b7", "type": "text.join", "v": 1});
        let (_, _, diagnostics) = resolve_stack(
            print("b1"),
            vec![print("b2"), bad_field, reporter, hat, unknown, join],
        );
        assert_eq!(
            codes_and_blocks(&diagnostics),
            [
                (codes::BAD_FIELD.to_owned(), "b3".to_owned()),
                // A value or a hat is not a statement, so the editor cannot
                // stack it.
                (codes::WRONG_PLACE.to_owned(), "b4".to_owned()),
                (codes::WRONG_PLACE.to_owned(), "b5".to_owned()),
                (codes::UNKNOWN_BLOCK.to_owned(), "b6".to_owned()),
                (codes::WRONG_PLACE.to_owned(), "b7".to_owned()),
            ]
        );
        let messages: Vec<&str> = diagnostics.iter().map(|d| d.message.as_str()).collect();
        assert_eq!(
            messages[1..3],
            [
                "This block gives a value and cannot be a step on its own. Put it into an input of another block.",
                "A \"when program starts\" block must sit directly on the canvas, not inside another block.",
            ]
        );
        assert_eq!(
            messages[4],
            "This \"join\" block gives a value and cannot be a step on its own. Put it into an input of another block."
        );
    }

    #[test]
    fn only_a_statement_can_head_a_stack() {
        let main = serde_json::json!({"id": "b1", "type": "program.main", "v": 1});
        let (_, _, diagnostics) = resolve_stack(main.clone(), vec![print("b2")]);
        assert_eq!(
            codes_and_blocks(&diagnostics),
            [(codes::WRONG_PLACE.to_owned(), "b1".to_owned())]
        );
        assert_eq!(
            diagnostics[0].message,
            "1 block is stacked below this \"when program starts\" block, but nothing can be attached below it, so the stacked block would never run. Move it inside a block, or delete it."
        );
        let (_, _, diagnostics) = resolve_stack(main, vec![print("b2"), print("b3")]);
        assert!(
            diagnostics[0]
                .message
                .starts_with("2 blocks are stacked below this \"when program starts\" block")
        );
        // A parked value block with a stack: the stack is the one problem,
        // reported on the head.
        let number = serde_json::json!({"id": "b1", "type": "math.number", "v": 1});
        let (_, _, diagnostics) = resolve_stack(number, vec![print("b2")]);
        assert_eq!(
            codes_and_blocks(&diagnostics),
            [(codes::WRONG_PLACE.to_owned(), "b1".to_owned())]
        );
        assert!(
            diagnostics[0]
                .message
                .starts_with("1 block is stacked below this block, but nothing can be attached below it")
        );
        // A head the catalog does not know: its stack is still checked.
        let unknown = serde_json::json!({"id": "b1", "type": "x.nope", "v": 1});
        let bad_field =
            serde_json::json!({"id": "b2", "type": "io.print", "v": 1, "fields": {"SEP": "tabs"}});
        let (_, _, diagnostics) = resolve_stack(unknown, vec![bad_field]);
        assert_eq!(
            codes_and_blocks(&diagnostics),
            [
                (codes::UNKNOWN_BLOCK.to_owned(), "b1".to_owned()),
                (codes::BAD_FIELD.to_owned(), "b2".to_owned()),
            ]
        );
    }

    #[test]
    fn a_stack_is_taken_out_and_put_back() {
        let mut head = block(print("b1"));
        head.stack = vec![block(print("b2")), block(print("b3"))];
        let stacked = stack::take(&mut head);
        assert_eq!(stacked.len(), 2);
        assert!(head.stack.is_empty());
        stack::restore(&mut head, stacked);
        assert_eq!(head.stack.len(), 2);
    }

    /// Every block kept as it was without a check is listed as unchecked,
    /// and only those (not the blocks nested in them): the analyser does
    /// not report their fields and inputs again.
    #[test]
    fn blocks_kept_without_a_check_are_listed() {
        let mut catalog = crate::core_catalog().clone();
        // A broken definition: a dropdown default that is not an option.
        let ask = catalog.blocks.get_mut("io.ask").unwrap();
        ask.field.iter_mut().find(|f| f.name == "MODE").unwrap().default =
            Some(FieldDefault::Text(String::from("bogus")));
        let document = |generator: &str, body: Vec<Value>| {
            let value = serde_json::json!({
                "format": "blocks2cpp/project", "formatVersion": 1,
                "generator": {"app": "0.1.0", "catalog": generator},
                "project": {"id": "p", "name": "P", "language": {"standard": "c++20"}},
                "modules": [{"id": "m", "name": "main", "workspace": {"blocks": [{
                    "id": "b_main", "type": "program.main", "v": 1, "statements": {"BODY": body}
                }]}}]
            });
            b2c_model::load(&serde_json::to_vec(&value).unwrap()).unwrap()
        };
        let body = vec![
            print("b_ok"),
            serde_json::json!({"id": "b_unknown", "type": "x.nope", "v": 1, "statements": {"X": [print("b_inner")]}}),
            serde_json::json!({"id": "b_newer", "type": "control.break", "v": 2}),
            serde_json::json!({"id": "b_old", "type": "io.print", "v": 0}),
            serde_json::json!({"id": "b_ask1", "type": "io.ask", "v": 1}),
            serde_json::json!({"id": "b_ask2", "type": "io.ask", "v": 1}),
        ];
        let report = run(&document("1.0.0", body), &catalog);
        let unchecked: Vec<&str> = report.unchecked.iter().map(BlockId::as_str).collect();
        assert_eq!(unchecked, ["b_ask1", "b_ask2", "b_newer", "b_old", "b_unknown"]);
        // Only the first block of a broken definition carries the diagnostic.
        let broken = report
            .diagnostics
            .iter()
            .filter(|d| d.code.0 == codes::BROKEN_DEFINITION);
        assert_eq!(broken.count(), 1);

        let mut newer = print("b_newer_field");
        newer["fields"] = serde_json::json!({"VOLUME": "3"});
        let report = run(
            &document("99.0.0", vec![print("b_ok"), newer]),
            crate::core_catalog(),
        );
        let unchecked: Vec<&str> = report.unchecked.iter().map(BlockId::as_str).collect();
        assert_eq!(unchecked, ["b_newer_field"]);
        assert_eq!(
            codes_and_blocks(&report.diagnostics),
            [(codes::NEWER_BLOCK.to_owned(), "b_newer_field".to_owned())]
        );
    }

    #[test]
    fn a_document_with_a_parked_stack_resolves_cleanly() {
        let document: Document = serde_json::from_value(serde_json::json!({
            "format": "blocks2cpp/project", "formatVersion": 1,
            "generator": {"app": "0.1.0", "catalog": "1.0.0"},
            "project": {"id": "p", "name": "T", "language": {"standard": "c++20"}},
            "modules": [{"id": "m", "name": "main", "workspace": {"blocks": [
                {"id": "main", "type": "program.main", "v": 1, "x": 0, "y": 0},
                {"id": "b1", "type": "io.print", "v": 1, "x": 0, "y": 200,
                 "stack": [print("b2"), print("b3")]}
            ]}}]
        }))
        .expect("document");
        let (completed, diagnostics) = parts(run(&document, crate::core_catalog()));
        assert_eq!(diagnostics, []);
        // The stack survives resolving, completed like any other block.
        let head = &completed.modules[0].workspace.blocks[1];
        assert_eq!(head.stack.len(), 2);
        assert_eq!(head.stack[1].id.as_str(), "b3");
        assert!(head.stack[1].inputs.contains_key("ITEM0"));
    }

    /// The short names of the core blocks (03 §3.11.1): the friendly label up
    /// to the first field or input.
    #[test]
    fn short_names() {
        let catalog = crate::core_catalog();
        let name = |id: &str| short_name(&catalog.blocks[id]);
        assert_eq!(name("io.print"), Some("print"));
        assert_eq!(name("io.ask"), Some("ask"));
        assert_eq!(name("program.main"), Some("when program starts"));
        assert_eq!(name("program.exit"), Some("stop program with exit code"));
        assert_eq!(name("control.while"), Some("repeat"));
        assert_eq!(name("control.if"), Some("if"));
        assert_eq!(name("control.break"), Some("leave loop"));
        assert_eq!(name("var.declare"), Some("create"));
        assert_eq!(name("func.define"), Some("define"));
        // The quote that leads into the field goes too.
        assert_eq!(name("text.char"), Some("character"));
        // Labels that start with a field or input have no short name.
        for id in [
            "math.number",
            "math.arithmetic",
            "text.literal",
            "var.get",
            "func.call",
        ] {
            assert_eq!(name(id), None, "{id}");
        }
        assert_eq!(block_noun(&catalog.blocks["io.print"]), "\"print\" block");
        assert_eq!(block_noun(&catalog.blocks["math.number"]), "block");
        for def in catalog.blocks.values() {
            if let Some(name) = short_name(def) {
                assert!(!name.contains(['%', '…']), "{}: {name}", def.id);
                assert_eq!(name, name.trim(), "{}", def.id);
                assert_ne!(name, def.id);
            }
        }
    }

    /// Every message about a block of a known type names it without its
    /// catalog ID (03 §3.11.1): misplaced, with a stack below a value, newer
    /// than the catalog, from a newer catalog, or too old to upgrade, both
    /// as the upgrade walk reports it and as resolve does.
    #[test]
    fn messages_never_show_a_catalog_id() {
        let catalog = crate::core_catalog();
        for def in catalog.blocks.values() {
            let id = def.id.as_str();
            let versioned = |v: u32| serde_json::json!({"id": "b1", "type": id, "v": v});
            let mut diagnostics = Vec::new();
            for position in [Position::Statement, Position::Value] {
                let mut resolver = Resolver::new(catalog, &BlockUpgrades::SHIPPED, &catalog.version);
                resolver.newer_catalog = true;
                let mut from_newer_catalog = versioned(def.version);
                from_newer_catalog["fields"] = serde_json::json!({"B2C_NEWER": "x"});
                for value in [
                    versioned(def.version),
                    versioned(def.version + 1),
                    versioned(0),
                    from_newer_catalog,
                ] {
                    resolver.block(&mut block(value), position);
                }
                diagnostics.extend(resolver.diags.finish());
            }
            let (_, _, stacked) = resolve_stack(versioned(def.version), vec![print("b2")]);
            diagnostics.extend(stacked);
            // The upgrade walk's own message for a block it cannot upgrade.
            let old = serde_json::json!({
                "format": "blocks2cpp/project", "formatVersion": 1,
                "generator": {"app": "0.1.0", "catalog": "1.0.0"},
                "project": {"id": "p", "name": "P", "language": {"standard": "c++20"}},
                "modules": [{"id": "m", "name": "main", "workspace": {"blocks": [versioned(0)]}}]
            });
            let old = b2c_model::load(&serde_json::to_vec(&old).unwrap()).unwrap();
            let (_, walked) = parts(run(&old, catalog));
            assert!(
                walked.iter().any(|d| d.code.0 == codes::OLD_BLOCK),
                "{id}: no {} in {walked:?}",
                codes::OLD_BLOCK
            );
            diagnostics.extend(walked);
            for code in [codes::NEWER_BLOCK, codes::OLD_BLOCK, codes::WRONG_PLACE] {
                assert!(
                    diagnostics.iter().any(|d| d.code.0 == code),
                    "{id}: no {code} in {diagnostics:?}"
                );
            }
            assert!(
                diagnostics
                    .iter()
                    .any(|d| d.code.0 == codes::NEWER_BLOCK && d.message.contains("it has the field")),
                "{id}: no newer-catalog problem in {diagnostics:?}"
            );
            for diagnostic in &diagnostics {
                assert!(!diagnostic.message.contains(id), "{id}: {}", diagnostic.message);
            }
        }
    }

    #[test]
    fn quoting_and_lists() {
        assert_eq!(quote("a\u{202e}\"b"), "\"a\\u{202E}\\\"b\"");
        assert_eq!(quote(&"x".repeat(50)), format!("\"{}…\"", "x".repeat(40)));
        // Every format character is escaped, such as the invisible tag
        // characters (which can carry hidden text), U+0600 and U+1D173.
        assert_eq!(
            quote("a\u{e0001}\u{e0041}\u{600}\u{1d173}b"),
            "\"a\\u{E0001}\\u{E0041}\\u{0600}\\u{1D173}b\""
        );
        assert_eq!(list([]), "nothing");
        assert_eq!(list(["a"]), "\"a\"");
        assert_eq!(list(["a", "b", "c"]), "\"a\", \"b\" or \"c\"");
        assert_eq!(describe(&Value::Null), "null");
        assert_eq!(describe(&Value::from(3)), "3");
        assert_eq!(describe(&Value::from("x")), "the text \"x\"");
        assert_eq!(describe(&Value::Array(vec![])), "a list");
        assert_eq!(describe(&serde_json::json!({})), "an object");
        assert_eq!(describe_field(&FieldValue::Bool(true)), "true");
        assert_eq!(describe_choice(None), "nothing");
        assert_eq!(describe_choice(Some(&Value::from("float"))), "\"float\"");
        assert_eq!(describe_choice(Some(&Value::from(3))), "3");
    }
}
