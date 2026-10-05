//! The resolve stage (spec §6.3): checks every block of a document against
//! its catalog definition and completes the document with catalog defaults.
//!
//! Blocks saved with an older version of their definition are upgraded with
//! the block migrations ([`crate::migrate`]) before they are checked.
//!
//! Defaults filled in: absent fields with a default, absent `count`/`flag`
//! extras with a default, and absent value inputs with default tokens (the
//! "shadow" blocks of the editor). Statement inputs are never filled: an
//! absent one simply means an empty list.
//!
//! A block directly on the canvas may carry a loose statement stack (spec
//! §5.4, ADR-0011): the blocks attached below it. Stacked blocks are checked
//! like blocks in a statement list. The stack itself is in the wrong place
//! (`B2C-E0604`), reported once, on its head: a statement head is not inside
//! `main` or a function, and nothing can be attached below any other shape.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use b2c_ir::{Diagnostic, Location, ModuleId, Part, SymbolId};
use b2c_model::{Block, Document, ExprInput, FieldValue, Input};
use serde_json::Value;

use crate::Catalog;
use crate::codes::{self, Diags};
use crate::definitions::{self, PARAM_MODES, count_default, default_tokens, repeat_index};
use crate::migrate::{self, BLOCK_MIGRATIONS, BlockMigration, MigrationError};
use crate::schema::{BlockDef, ExtraDef, ExtraKind, FieldDef, FieldDefault, FieldKind, Shape};
use crate::stack;

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
    Params,
    /// Missing or invalid (already reported).
    Invalid,
}

/// Runs the resolve stage with the shipped block migrations.
pub(crate) fn run(document: &Document, catalog: &Catalog) -> (Document, Vec<Diagnostic>) {
    run_with(document, catalog, BLOCK_MIGRATIONS)
}

/// Runs the resolve stage with the given block migrations.
fn run_with(
    document: &Document,
    catalog: &Catalog,
    migrations: &[BlockMigration],
) -> (Document, Vec<Diagnostic>) {
    let mut completed = document.clone();
    let mut resolver = Resolver {
        catalog,
        migrations,
        broken: broken_definitions(catalog),
        reported_broken: BTreeSet::new(),
        module: None,
        diags: Diags::default(),
    };
    let has_blocks = completed.modules.iter().any(|m| !m.workspace.blocks.is_empty());
    if catalog.blocks.is_empty() {
        if has_blocks {
            resolver.diags.error(
                codes::EMPTY_CATALOG,
                Location::project(),
                "No block definitions are available (the block catalog is empty or could not be loaded), so no block can be checked. This is a bug in Blocks2Cpp.",
            );
        }
        return (completed, resolver.diags.finish());
    }
    for module in &mut completed.modules {
        resolver.module = Some(module.id.clone());
        for block in &mut module.workspace.blocks {
            let mut stacked = stack::take(block);
            resolver.top_level(block, &mut stacked);
            stack::restore(block, stacked);
        }
    }
    (completed, resolver.diags.finish())
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
    migrations: &'c [BlockMigration],
    broken: BTreeMap<String, String>,
    reported_broken: BTreeSet<String>,
    module: Option<ModuleId>,
    diags: Diags,
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
            self.diags.error(
                codes::UNKNOWN_BLOCK,
                location,
                unknown_block_message(&block.block_type),
            );
            self.children(block);
            return;
        };
        if let Some(problem) = self.broken.get(&def.id) {
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
            let message = format!(
                "This block was made with a newer version of Blocks2Cpp: it is version {} of {}, and this version knows only up to version {}. Update Blocks2Cpp to use it.",
                block.v,
                quote(&def.id),
                def.version
            );
            self.diags.error(codes::NEWER_BLOCK, location, message);
            self.children(block);
            return;
        }
        if block.v < def.version {
            let saved = block.v;
            if let Err(error) = migrate::upgrade(block, def.version, self.migrations) {
                let message = match error {
                    MigrationError::NoPath { .. } => format!(
                        "This block is version {saved} of {}, which this version of Blocks2Cpp cannot upgrade to version {}.",
                        quote(&def.id),
                        def.version
                    ),
                    MigrationError::Failed { from, reason } => format!(
                        "This block is version {saved} of {}, and upgrading it from version {from} failed: {reason}.",
                        quote(&def.id)
                    ),
                };
                self.diags.error(codes::OLD_BLOCK, location, message);
                self.children(block);
                return;
            }
        }
        self.check_place(def, position, &location);
        let extras = self.extras(def, block, &location);
        self.fields(def, block, &location);
        self.inputs(def, block, &extras, &location);
        self.statements(def, block, &extras, &location);
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

    fn check_place(&mut self, def: &BlockDef, position: Position, location: &Location) {
        let kind = quote(&def.id);
        if let Position::TopLevel { stacked } = position
            && stacked > 0
            && def.shape != Shape::Statement
        {
            // Only a statement has a connection below it (spec §3.3).
            let message = if stacked == 1 {
                format!(
                    "1 block is stacked below this {kind} block, but nothing can be attached below it, so the stacked block would never run. Move it inside a block, or delete it."
                )
            } else {
                format!(
                    "{stacked} blocks are stacked below this {kind} block, but nothing can be attached below it, so the stacked blocks would never run. Move them inside a block, or delete them."
                )
            };
            self.diags.error(codes::WRONG_PLACE, location.clone(), message);
        }
        let message = match (position, def.shape) {
            (Position::TopLevel { .. }, Shape::Hat | Shape::Definition)
            | (Position::Statement, Shape::Statement)
            | (Position::Value, Shape::Reporter | Shape::Predicate) => return,
            (Position::TopLevel { stacked: 0 }, Shape::Statement) => format!(
                "This {kind} block is not inside \"when program starts\" or a function, so it would never run. Move it inside one, or delete it."
            ),
            (Position::TopLevel { stacked }, Shape::Statement) => format!(
                "This {kind} block and the {} below it are not inside \"when program starts\" or a function, so they would never run. Move them inside one, or delete them.",
                if stacked == 1 {
                    String::from("block")
                } else {
                    format!("{stacked} blocks")
                }
            ),
            (Position::TopLevel { .. }, Shape::Reporter | Shape::Predicate) => format!(
                "This {kind} block gives a value, but it is not plugged into another block. Put it into an input, or delete it."
            ),
            (Position::Statement | Position::Value, Shape::Hat | Shape::Definition) => {
                format!("A {kind} block must sit directly on the canvas, not inside another block.")
            }
            (Position::Statement, Shape::Reporter | Shape::Predicate) => format!(
                "This {kind} block gives a value and cannot be a step on its own. Put it into an input of another block."
            ),
            (Position::Value, Shape::Statement) => {
                format!("This {kind} block is a step, not a value, so it cannot be plugged into an input.")
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
            ExtraKind::Params => match value {
                Value::Array(rows) => {
                    let limit = usize::try_from(extra.max).unwrap_or(usize::MAX);
                    if rows.len() > limit {
                        let message = format!(
                            "This block has {} parameters, but at most {} are allowed.",
                            rows.len(),
                            extra.max
                        );
                        self.diags.error(codes::BAD_EXTRA, location.clone(), message);
                    }
                    for (index, row) in rows.iter().enumerate() {
                        self.param_row(index + 1, row, extra, location);
                    }
                    (Resolved::Params, None)
                }
                _ => (
                    Resolved::Invalid,
                    Some(format!(
                        "\"extra.{name}\" of this block should be a list of parameters, but it is {}.",
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

    fn param_row(&mut self, number: usize, row: &Value, extra: &ExtraDef, location: &Location) {
        let mut problems = Vec::new();
        if let Value::Object(map) = row {
            for key in map.keys() {
                if !["sym", "name", "type", "mode"].contains(&key.as_str()) {
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
            match map.get("type") {
                Some(Value::String(t)) if extra.types.contains(t) => {}
                other => problems.push(format!(
                    "has the type {}, but parameters can only be {}",
                    describe_choice(other),
                    list(extra.types.iter().map(String::as_str))
                )),
            }
            match map.get("mode") {
                Some(Value::String(m)) if PARAM_MODES.contains(&m.as_str()) => {}
                other => problems.push(format!(
                    "has the mode {}, but the mode must be {}",
                    describe_choice(other),
                    list(PARAM_MODES)
                )),
            }
        } else {
            problems.push(format!(
                "should be an object with \"sym\", \"name\", \"type\" and \"mode\", but it is {}",
                describe(row)
            ));
        }
        for problem in problems {
            self.diags.error(
                codes::BAD_PARAM,
                location.clone(),
                format!("Parameter {number} of this block {problem}."),
            );
        }
    }

    // -----------------------------------------------------------------------
    // Fields
    // -----------------------------------------------------------------------

    fn fields(&mut self, def: &BlockDef, block: &mut Block, location: &Location) {
        for name in block.fields.keys() {
            if !def.field.iter().any(|f| &f.name == name) {
                let message = format!(
                    "This block has no field {}. Remove it or check its spelling.",
                    quote(name)
                );
                self.diags
                    .error(codes::UNKNOWN_FIELD, field_location(location, name), message);
            }
        }
        for field in &def.field {
            if let Some(value) = block.fields.get(&field.name) {
                self.field(field, value, location);
                continue;
            }
            match &field.default {
                Some(FieldDefault::Bool(flag)) => {
                    block.fields.insert(field.name.clone(), FieldValue::Bool(*flag));
                }
                Some(FieldDefault::Text(text)) => {
                    block
                        .fields
                        .insert(field.name.clone(), FieldValue::Text(text.clone()));
                }
                None => {
                    let name = &field.name;
                    let message = match field.kind {
                        FieldKind::SymbolDecl => {
                            format!("This block needs a name: its field {name} is missing.")
                        }
                        FieldKind::SymbolRef => format!(
                            "This block needs to know which variable or function it uses: its field {name} is missing."
                        ),
                        _ => format!("This block is missing its field {name}."),
                    };
                    self.diags
                        .error(codes::MISSING_FIELD, field_location(location, name), message);
                }
            }
        }
    }

    fn field(&mut self, field: &FieldDef, value: &FieldValue, location: &Location) {
        let name = &field.name;
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
            (FieldKind::Type, FieldValue::Text(text)) if field.types.contains(text) => return,
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
            (FieldKind::Type, _) => format!(
                "The field {name} is {}, but it must be one of the types {}.",
                describe_field(value),
                list(field.types.iter().map(String::as_str))
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
            Some(Resolved::Count(n)) => Some(n.saturating_add(plus)),
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

/// Describes a field value for a message.
fn describe_field(value: &FieldValue) -> String {
    match value {
        FieldValue::Bool(b) => b.to_string(),
        FieldValue::Text(text) => quote(text),
        FieldValue::Decl(_) => String::from("a name declaration"),
        FieldValue::Ref(_) => String::from("a reference to a variable or function"),
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
fn quote(text: &str) -> String {
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
    use super::*;

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

    const CHAIN: &[BlockMigration] = &[BlockMigration {
        block: "io.print",
        from: 1,
        apply: rename_separator,
    }];

    #[test]
    fn older_blocks_are_upgraded_before_they_are_checked() {
        let catalog = catalog_v2();
        let (completed, diagnostics) = run_with(&document("space"), &catalog, CHAIN);
        assert_eq!(diagnostics, []);
        let print = &completed.modules[0].workspace.blocks[0].statements["BODY"][0];
        assert_eq!(print.v, 2);
        assert_eq!(print.fields["SEP"], FieldValue::Text("spaces".into()));
        // The upgraded block is complete, and resolving it again is a no-op.
        assert_eq!(print.fields["STREAM"], FieldValue::Text("out".into()));
        assert_eq!(run_with(&completed, &catalog, CHAIN), (completed.clone(), vec![]));
    }

    #[test]
    fn blocks_that_cannot_be_upgraded_are_reported() {
        let catalog = catalog_v2();
        let (completed, diagnostics) = run_with(&document("bad"), &catalog, CHAIN);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code.0, codes::OLD_BLOCK);
        assert_eq!(
            diagnostics[0].message,
            "This block is version 1 of \"io.print\", and upgrading it from version 1 failed: the separator is unknown."
        );
        let print = &completed.modules[0].workspace.blocks[0].statements["BODY"][0];
        assert_eq!(print.v, 1, "a failed upgrade leaves the block as it was");

        let (_, diagnostics) = run_with(&document("space"), &catalog, &[]);
        assert_eq!(
            diagnostics[0].message,
            "This block is version 1 of \"io.print\", which this version of Blocks2Cpp cannot upgrade to version 2."
        );
    }

    /// A block from its JSON form (tests only; files go through the loader).
    fn block(value: Value) -> Block {
        serde_json::from_value(value).unwrap()
    }

    /// Resolves one top-level block with a loose stack against the core
    /// catalog, as the stage does for every block on a canvas.
    fn resolve_stack(head: Value, stacked: Vec<Value>) -> (Block, Vec<Block>, Vec<Diagnostic>) {
        let catalog = crate::core_catalog();
        let mut resolver = Resolver {
            catalog,
            migrations: BLOCK_MIGRATIONS,
            broken: broken_definitions(catalog),
            reported_broken: BTreeSet::new(),
            module: ModuleId::new("m").ok(),
            diags: Diags::default(),
        };
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
    fn a_loose_stack_is_reported_once_on_its_head() {
        let (head, stacked, diagnostics) = resolve_stack(print("b1"), vec![print("b2"), print("b3")]);
        assert_eq!(
            codes_and_blocks(&diagnostics),
            [(codes::WRONG_PLACE.to_owned(), "b1".to_owned())]
        );
        assert_eq!(
            diagnostics[0].message,
            "This \"io.print\" block and the 2 blocks below it are not inside \"when program starts\" or a function, so they would never run. Move them inside one, or delete them."
        );
        // Stacked blocks are completed like any other block.
        assert!(head.inputs.contains_key("ITEM0"));
        assert!(stacked.iter().all(|b| b.inputs.contains_key("ITEM0")));

        let (_, _, diagnostics) = resolve_stack(print("b1"), vec![print("b2")]);
        assert_eq!(
            diagnostics[0].message,
            "This \"io.print\" block and the block below it are not inside \"when program starts\" or a function, so they would never run. Move them inside one, or delete them."
        );
        // Without a stack, the message is about the block alone.
        let (_, _, diagnostics) = resolve_stack(print("b1"), vec![]);
        assert_eq!(
            diagnostics[0].message,
            "This \"io.print\" block is not inside \"when program starts\" or a function, so it would never run. Move it inside one, or delete it."
        );
    }

    #[test]
    fn stacked_blocks_are_checked_like_statements() {
        let bad_field =
            serde_json::json!({"id": "b3", "type": "io.print", "v": 1, "fields": {"SEP": "tabs"}});
        let reporter = serde_json::json!({"id": "b4", "type": "math.number", "v": 1});
        let hat = serde_json::json!({"id": "b5", "type": "program.main", "v": 1});
        let unknown = serde_json::json!({"id": "b6", "type": "x.nope", "v": 1});
        let (_, _, diagnostics) =
            resolve_stack(print("b1"), vec![print("b2"), bad_field, reporter, hat, unknown]);
        assert_eq!(
            codes_and_blocks(&diagnostics),
            [
                (codes::WRONG_PLACE.to_owned(), "b1".to_owned()),
                (codes::BAD_FIELD.to_owned(), "b3".to_owned()),
                // A value or a hat is not a statement, so it cannot be
                // stacked either.
                (codes::WRONG_PLACE.to_owned(), "b4".to_owned()),
                (codes::WRONG_PLACE.to_owned(), "b5".to_owned()),
                (codes::UNKNOWN_BLOCK.to_owned(), "b6".to_owned()),
            ]
        );
        assert!(diagnostics[0].message.contains("the 5 blocks below it"));
        assert!(diagnostics[2].message.contains("cannot be a step on its own"));
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
            "1 block is stacked below this \"program.main\" block, but nothing can be attached below it, so the stacked block would never run. Move it inside a block, or delete it."
        );
        let (_, _, diagnostics) = resolve_stack(main, vec![print("b2"), print("b3")]);
        assert!(
            diagnostics[0]
                .message
                .starts_with("2 blocks are stacked below this \"program.main\" block")
        );
        // A loose value block with a stack: both problems, on the head.
        let number = serde_json::json!({"id": "b1", "type": "math.number", "v": 1});
        let (_, _, diagnostics) = resolve_stack(number, vec![print("b2")]);
        assert_eq!(codes_and_blocks(&diagnostics).len(), 2);
        assert!(diagnostics.iter().all(|d| d.code.0 == codes::WRONG_PLACE));
        assert!(diagnostics[1].message.contains("gives a value"));
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
    fn documents_without_stacks_resolve_as_before() {
        // The seam: until `b2c_model::Block` has its `stack`, every block
        // has an empty one and resolving is unchanged.
        let mut head = block(print("b1"));
        let stacked = stack::take(&mut head);
        assert!(stacked.is_empty());
        let before = head.clone();
        stack::restore(&mut head, stacked);
        assert_eq!(head, before);
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
