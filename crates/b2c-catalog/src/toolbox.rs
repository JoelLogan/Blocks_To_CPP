//! The editor's toolbox (spec §3.11.1, §3.7 and 04 §4.2): the categories in
//! toolbox order, with an icon and a colour token each, and the ordered
//! entries of each category, optionally with a label and a *preset* (field
//! values, `extra` values and input tokens that differ from the catalog
//! defaults). Two categories are *dynamic*: the editor adds the blocks for
//! the open project's variables and functions to them.
//!
//! The toolbox is data, like the catalog: `catalog/toolbox.toml` is embedded
//! with `include_str!` and checked against the catalog here, so the editor
//! (through the generated definitions of `packages/catalog-gen`) only ever
//! sees a toolbox that fits the catalog. The checks: every entry names a
//! block of its own category, every catalog block is reachable through an
//! entry or a dynamic category, and every preset value fits its field,
//! `extra` key or input.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use b2c_ir::text::{CharLit, StrLit, is_invisible};
use b2c_model::Token;
use b2c_model::limits::MAX_EXPR_TOKENS;
use serde::{Deserialize, Serialize};

use crate::Catalog;
use crate::definitions::{count_default, default_token, field_value_fits, repeat_index};
use crate::schema::{BlockDef, Category, ExtraKind, FieldDefault, FieldKind};

/// The built-in toolbox, embedded at compile time.
pub(crate) const CORE_TOOLBOX: &str = include_str!("../../../catalog/toolbox.toml");

/// The largest toolbox file accepted, in bytes.
pub const MAX_TOOLBOX_BYTES: usize = 256 * 1024;
/// The most entries one category may have.
pub const MAX_TOOLBOX_ENTRIES: usize = 256;
/// The longest category name, in characters.
pub const MAX_CATEGORY_NAME_CHARS: usize = 32;
/// The longest category icon, in characters.
pub const MAX_CATEGORY_ICON_CHARS: usize = 4;
/// The longest entry label, in characters.
pub const MAX_ENTRY_LABEL_CHARS: usize = 64;

/// The toolbox: `catalog/toolbox.toml`, a list of `[[category]]` tables.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Toolbox {
    /// The categories, in toolbox order (spec §3.7).
    #[serde(rename = "category", default)]
    pub categories: Vec<ToolboxCategory>,
}

/// One toolbox category.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolboxCategory {
    /// The catalog category whose blocks it shows.
    pub id: Category,
    /// The name shown in the toolbox, e.g. `Input / Output`.
    pub name: String,
    /// A short icon shown with the name, so colour is never the only cue.
    pub icon: String,
    /// The colour token, which is the category's ID; the colour values live
    /// in the editor theme.
    pub colour: String,
    /// Blocks the editor adds for the open project, after the entries.
    #[serde(default)]
    pub dynamic: Option<DynamicCategory>,
    /// The entries, in toolbox order.
    #[serde(rename = "entry", default)]
    pub entries: Vec<ToolboxEntry>,
}

/// The kinds of blocks the editor adds to a category for the open project.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DynamicCategory {
    /// A getter for every variable in scope, and setters preset to it.
    Variables,
    /// *My Blocks*: a call block for every function of the project.
    Functions,
}

impl DynamicCategory {
    /// The catalog blocks this kind adds (the only way to reach them).
    pub const fn blocks(self) -> &'static [&'static str] {
        match self {
            Self::Variables => &["var.get", "var.set", "var.change", "var.update"],
            Self::Functions => &["func.call", "func.call_stmt"],
        }
    }

    /// The category this kind belongs to.
    pub const fn category(self) -> Category {
        match self {
            Self::Variables => Category::Variables,
            Self::Functions => Category::Functions,
        }
    }
}

/// One block in a category.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolboxEntry {
    /// The catalog block ID.
    pub block: String,
    /// A label shown with the block, e.g. `repeat until`.
    #[serde(default)]
    pub label: Option<String>,
    /// Values that differ from the catalog defaults.
    #[serde(default)]
    pub preset: Option<Preset>,
}

/// The values a toolbox entry gives its block instead of the catalog
/// defaults. Symbol fields cannot be preset: the editor fills them in.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Preset {
    /// Field values by field name.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub fields: BTreeMap<String, FieldDefault>,
    /// `extra` values by key: a count or a flag.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra: BTreeMap<String, PresetExtra>,
    /// Value-input tokens by input name (for a repeated input, a numbered
    /// name such as `ITEM0`), written like catalog defaults:
    /// `[{ str = "Your answer: " }]`.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub inputs: BTreeMap<String, Vec<toml::Value>>,
}

impl Preset {
    /// Whether the preset sets nothing.
    pub fn is_empty(&self) -> bool {
        self.fields.is_empty() && self.extra.is_empty() && self.inputs.is_empty()
    }
}

/// A preset `extra` value, as a project file stores it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PresetExtra {
    /// A `flag`.
    Flag(bool),
    /// A `count`.
    Count(u32),
}

/// A problem with a toolbox.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolboxProblem {
    /// The category, when the problem is about one.
    pub category: Option<Category>,
    /// The entry's position in its category (from 0), when the problem is
    /// about one entry.
    pub entry: Option<usize>,
    /// What is wrong.
    pub message: String,
}

impl fmt::Display for ToolboxProblem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match (self.category, self.entry) {
            (Some(category), Some(entry)) => {
                write!(f, "category {category}, entry {}: {}", entry + 1, self.message)
            }
            (Some(category), None) => write!(f, "category {category}: {}", self.message),
            (None, _) => f.write_str(&self.message),
        }
    }
}

/// Why a toolbox file cannot be used.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ToolboxError {
    /// The file is larger than [`MAX_TOOLBOX_BYTES`].
    #[error("the toolbox file has {size} bytes, more than the limit of {MAX_TOOLBOX_BYTES}")]
    TooLarge {
        /// The file's size in bytes.
        size: usize,
    },
    /// The file is not TOML of the toolbox schema (syntax, an unknown key, a
    /// value of the wrong kind).
    #[error("the toolbox file is not valid: {0}")]
    Syntax(String),
    /// The toolbox does not fit the catalog.
    #[error("the toolbox does not fit the catalog: {}", join(.0))]
    Invalid(Vec<ToolboxProblem>),
}

fn join(problems: &[ToolboxProblem]) -> String {
    let texts: Vec<String> = problems.iter().map(ToString::to_string).collect();
    texts.join("; ")
}

impl Toolbox {
    /// Parses a toolbox file, checking its syntax and schema only (see
    /// [`Toolbox::check`] for the rest).
    ///
    /// # Errors
    /// [`ToolboxError::TooLarge`] or [`ToolboxError::Syntax`].
    pub fn parse(text: &str) -> Result<Self, ToolboxError> {
        if text.len() > MAX_TOOLBOX_BYTES {
            return Err(ToolboxError::TooLarge { size: text.len() });
        }
        toml::from_str(text).map_err(|error| ToolboxError::Syntax(error.to_string()))
    }

    /// Parses a toolbox file and checks it against a catalog.
    ///
    /// # Errors
    /// Any problem of [`Toolbox::parse`], or [`ToolboxError::Invalid`] with
    /// every problem [`Toolbox::check`] finds.
    pub fn load(text: &str, catalog: &Catalog) -> Result<Self, ToolboxError> {
        let toolbox = Self::parse(text)?;
        let problems = toolbox.check(catalog);
        if problems.is_empty() {
            Ok(toolbox)
        } else {
            Err(ToolboxError::Invalid(problems))
        }
    }

    /// Every problem of this toolbox against a catalog, in toolbox order,
    /// with the catalog blocks that cannot be reached last.
    pub fn check(&self, catalog: &Catalog) -> Vec<ToolboxProblem> {
        Checker::run(self, catalog).problems
    }

    /// The block IDs the toolbox can show: every entry's block and the
    /// blocks of its dynamic categories.
    pub fn reachable_blocks(&self) -> BTreeSet<&str> {
        let mut reachable = BTreeSet::new();
        for category in &self.categories {
            reachable.extend(category.entries.iter().map(|e| e.block.as_str()));
            if let Some(dynamic) = category.dynamic {
                reachable.extend(dynamic.blocks().iter().copied());
            }
        }
        reachable
    }
}

/// Parses and checks a toolbox, leaving out every category and entry with a
/// problem. Used for the embedded toolbox, which tests keep free of
/// problems, so that a broken one never panics.
pub(crate) fn build(text: &str, catalog: &Catalog) -> (Toolbox, Vec<ToolboxProblem>) {
    let toolbox = match Toolbox::parse(text) {
        Ok(toolbox) => toolbox,
        Err(error) => {
            let problem = ToolboxProblem {
                category: None,
                entry: None,
                message: error.to_string(),
            };
            return (Toolbox::default(), vec![problem]);
        }
    };
    let checked = Checker::run(&toolbox, catalog);
    let categories = toolbox
        .categories
        .into_iter()
        .enumerate()
        .filter(|(index, _)| !checked.bad_categories.contains(index))
        .map(|(index, mut category)| {
            category.entries = std::mem::take(&mut category.entries)
                .into_iter()
                .enumerate()
                .filter(|(entry, _)| !checked.bad_entries.contains(&(index, *entry)))
                .map(|(_, entry)| entry)
                .collect();
            category
        })
        .collect();
    (Toolbox { categories }, checked.problems)
}

/// The checks, collecting problems and the parts they make unusable.
#[derive(Debug, Default)]
struct Checker {
    problems: Vec<ToolboxProblem>,
    /// Indexes of unusable categories.
    bad_categories: BTreeSet<usize>,
    /// (category index, entry index) of unusable entries.
    bad_entries: BTreeSet<(usize, usize)>,
}

impl Checker {
    fn run(toolbox: &Toolbox, catalog: &Catalog) -> Self {
        let mut checker = Self::default();
        if toolbox.categories.is_empty() {
            checker.problem(None, None, String::from("the toolbox has no categories"));
        }
        let mut previous: Option<Category> = None;
        for (index, category) in toolbox.categories.iter().enumerate() {
            let mut problems = Vec::new();
            match previous {
                Some(before) if before == category.id => {
                    problems.push(String::from("the category is listed twice"));
                }
                Some(before) if before > category.id => problems.push(format!(
                    "the category must come before {before} (toolbox order is the order of spec §3.7)"
                )),
                _ => previous = Some(category.id),
            }
            problems.extend(check_category(category, catalog));
            if !problems.is_empty() {
                checker.bad_categories.insert(index);
            }
            for message in problems {
                checker.problem(Some(category.id), None, message);
            }
            for (position, entry) in category.entries.iter().enumerate() {
                let mut problems = check_entry(category.id, entry, catalog);
                if category.entries[..position].contains(entry) {
                    problems.push(String::from(
                        "the same block with the same label and preset is listed twice",
                    ));
                }
                if !problems.is_empty() {
                    checker.bad_entries.insert((index, position));
                }
                for message in problems {
                    checker.problem(Some(category.id), Some(position), message);
                }
            }
        }
        let reachable = toolbox.reachable_blocks();
        for id in catalog.blocks.keys() {
            if !reachable.contains(id.as_str()) {
                checker.problem(
                    None,
                    None,
                    format!("the block {id} is not in the toolbox: add an entry for it"),
                );
            }
        }
        checker
    }

    fn problem(&mut self, category: Option<Category>, entry: Option<usize>, message: String) {
        self.problems.push(ToolboxProblem {
            category,
            entry,
            message,
        });
    }
}

/// The problems of a category's own keys.
fn check_category(category: &ToolboxCategory, catalog: &Catalog) -> Vec<String> {
    let mut problems = Vec::new();
    check_text("name", &category.name, MAX_CATEGORY_NAME_CHARS, &mut problems);
    check_text("icon", &category.icon, MAX_CATEGORY_ICON_CHARS, &mut problems);
    if category.colour != category.id.as_str() {
        problems.push(format!(
            "the colour token must be the category ID {:?}, not {:?}",
            category.id.as_str(),
            category.colour
        ));
    }
    if let Some(dynamic) = category.dynamic {
        let ids = if dynamic.category() == category.id {
            dynamic.blocks()
        } else {
            problems.push(format!(
                "dynamic {dynamic:?} blocks belong to the category {}",
                dynamic.category()
            ));
            &[]
        };
        for id in ids {
            match catalog.blocks.get(*id) {
                None => problems.push(format!(
                    "dynamic {dynamic:?} blocks include {id}, which the catalog does not define"
                )),
                Some(def) if def.category != category.id => problems.push(format!(
                    "dynamic {dynamic:?} blocks include {id}, which belongs to the category {}",
                    def.category
                )),
                Some(_) => {}
            }
        }
    }
    if category.entries.is_empty() && category.dynamic.is_none() {
        problems.push(String::from("the category has no entries"));
    }
    if category.entries.len() > MAX_TOOLBOX_ENTRIES {
        problems.push(format!(
            "the category has {} entries, more than the limit of {MAX_TOOLBOX_ENTRIES}",
            category.entries.len()
        ));
    }
    problems
}

/// Names, icons and labels: shown as plain text, so they must be visible,
/// short and on one line.
fn check_text(what: &str, text: &str, max_chars: usize, problems: &mut Vec<String>) {
    if text.trim().is_empty() {
        problems.push(format!("the {what} is empty"));
    } else if text.trim() != text {
        problems.push(format!("the {what} {text:?} starts or ends with a space"));
    }
    if text.chars().count() > max_chars {
        problems.push(format!(
            "the {what} {text:?} is longer than {max_chars} characters"
        ));
    }
    if text.chars().any(|c| c.is_control() || is_invisible(c)) {
        problems.push(format!(
            "the {what} {text:?} contains a control or invisible character"
        ));
    }
}

/// The problems of one entry.
fn check_entry(category: Category, entry: &ToolboxEntry, catalog: &Catalog) -> Vec<String> {
    let mut problems = Vec::new();
    if let Some(label) = &entry.label {
        check_text("label", label, MAX_ENTRY_LABEL_CHARS, &mut problems);
    }
    let Some(def) = catalog.blocks.get(&entry.block) else {
        problems.push(format!("the catalog has no block {:?}", entry.block));
        return problems;
    };
    if def.category != category {
        problems.push(format!(
            "the block {} belongs to the category {}",
            def.id, def.category
        ));
    }
    if let Some(preset) = &entry.preset {
        if preset.is_empty() {
            problems.push(String::from("the preset sets nothing (leave it out instead)"));
        }
        check_preset(def, preset, &mut problems);
    }
    problems
}

/// Every preset value must fit its field, `extra` key or input.
fn check_preset(def: &BlockDef, preset: &Preset, problems: &mut Vec<String>) {
    for (name, value) in &preset.fields {
        let Some(field) = def.field.iter().find(|f| &f.name == name) else {
            problems.push(format!("the block {} has no field {name}", def.id));
            continue;
        };
        if matches!(field.kind, FieldKind::SymbolDecl | FieldKind::SymbolRef) {
            problems.push(format!(
                "the field {name} names a variable or function, which the editor fills in, so it cannot be preset"
            ));
        } else if !field_value_fits(field, value) {
            problems.push(format!(
                "the preset value of the field {name} does not fit its kind or choices"
            ));
        }
    }
    // The counts the block has with the preset: these decide which numbered
    // inputs exist.
    let mut counts = BTreeMap::new();
    for extra in &def.extra {
        let preset_value = preset.extra.get(&extra.name);
        let count = match (extra.kind, preset_value) {
            (ExtraKind::Count, Some(PresetExtra::Count(n))) if (extra.min..=extra.max).contains(n) => {
                Some(*n)
            }
            (ExtraKind::Count, None) => extra.default.as_ref().and_then(|d| count_default(extra, d)),
            (ExtraKind::Count, Some(_)) => {
                problems.push(format!(
                    "the preset value of {} must be a whole number from {} to {}",
                    extra.name, extra.min, extra.max
                ));
                None
            }
            (ExtraKind::Flag, Some(PresetExtra::Flag(_)) | None) | (ExtraKind::Params, None) => None,
            (ExtraKind::Flag, Some(PresetExtra::Count(_))) => {
                problems.push(format!(
                    "the preset value of {} must be true or false",
                    extra.name
                ));
                None
            }
            (ExtraKind::Params, Some(_)) => {
                problems.push(format!("the parameters {} cannot be preset", extra.name));
                None
            }
        };
        if let Some(count) = count {
            counts.insert(extra.name.as_str(), count);
        }
    }
    for name in preset.extra.keys() {
        if !def.extra.iter().any(|e| &e.name == name) {
            problems.push(format!("the block {} has no extra {name}", def.id));
        }
    }
    for (name, tokens) in &preset.inputs {
        if !input_exists(def, name, &counts) {
            problems.push(format!(
                "the block {} has no input {name} (with the preset's counts)",
                def.id
            ));
        }
        check_tokens(name, tokens, problems);
    }
}

/// Whether a value input of this name exists, given the block's counts.
fn input_exists(def: &BlockDef, name: &str, counts: &BTreeMap<&str, u32>) -> bool {
    def.input.iter().any(|input| match &input.repeat {
        None => input.name == name,
        Some(repeat) => repeat_index(name, &input.name).is_some_and(|index| {
            counts
                .get(repeat.count.as_str())
                .is_some_and(|count| index < count.saturating_add(repeat.plus))
        }),
    })
}

/// Preset input tokens: literal and operator tokens, as for catalog
/// defaults, with string and character values a C++ literal can hold.
fn check_tokens(name: &str, tokens: &[toml::Value], problems: &mut Vec<String>) {
    if tokens.is_empty() {
        problems.push(format!(
            "the preset of the input {name} has no tokens (leave the input out instead)"
        ));
    }
    if tokens.len() > MAX_EXPR_TOKENS {
        problems.push(format!(
            "the preset of the input {name} has more than {MAX_EXPR_TOKENS} tokens"
        ));
    }
    for token in tokens {
        let problem = match default_token(token) {
            Err(error) => Some(error),
            Ok(Token::Str(text)) => StrLit::new(&text).err().map(|e| e.to_string()),
            Ok(Token::Chr(text)) => CharLit::new(&text).err().map(|e| e.to_string()),
            Ok(Token::Num(text)) if text.trim().is_empty() => {
                Some(String::from("a number token needs digits"))
            }
            Ok(_) => None,
        };
        if let Some(problem) = problem {
            problems.push(format!("the preset of the input {name}: {problem}"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn core() -> &'static Catalog {
        crate::core_catalog()
    }

    #[test]
    fn the_core_toolbox_is_valid_and_reaches_every_block() {
        let (toolbox, problems) = build(CORE_TOOLBOX, core());
        assert_eq!(problems, []);
        assert_eq!(toolbox, Toolbox::load(CORE_TOOLBOX, core()).unwrap());
        let ids: Vec<Category> = toolbox.categories.iter().map(|c| c.id).collect();
        assert_eq!(ids, Category::ALL);
        let reachable = toolbox.reachable_blocks();
        let all: BTreeSet<&str> = core().blocks.keys().map(String::as_str).collect();
        assert_eq!(reachable, all);
        assert_eq!(reachable.len(), 32);
    }

    #[test]
    fn problems_name_their_place() {
        let problem = |category, entry| ToolboxProblem {
            category,
            entry,
            message: String::from("bad"),
        };
        assert_eq!(problem(None, None).to_string(), "bad");
        assert_eq!(problem(Some(Category::Io), None).to_string(), "category io: bad");
        assert_eq!(
            problem(Some(Category::Io), Some(0)).to_string(),
            "category io, entry 1: bad"
        );
        let error = ToolboxError::Invalid(vec![problem(None, None), problem(Some(Category::Math), None)]);
        assert_eq!(
            error.to_string(),
            "the toolbox does not fit the catalog: bad; category math: bad"
        );
    }

    #[test]
    fn broken_parts_are_left_out() {
        let text = r#"
[[category]]
id = "program"
name = "Program"
icon = "P"
colour = "program"

[[category.entry]]
block = "program.main"

[[category.entry]]
block = "program.nope"

[[category]]
id = "math"
name = ""
icon = "M"
colour = "math"

[[category.entry]]
block = "math.number"
"#;
        let (toolbox, problems) = build(text, core());
        assert_eq!(toolbox.categories.len(), 1);
        assert_eq!(toolbox.categories[0].entries.len(), 1);
        assert_eq!(toolbox.categories[0].entries[0].block, "program.main");
        assert_eq!(problems[0].category, Some(Category::Program));
        assert_eq!(problems[0].entry, Some(1));
        assert_eq!(problems[1].category, Some(Category::Math));
        assert_eq!(problems[1].entry, None);
        // Then every block that cannot be reached (math.number's category
        // was left out, but the toolbox still names it).
        assert_eq!(problems.len(), 2 + 32 - 2);

        let (toolbox, problems) = build("[[category]]\nid = \"nope\"\n", core());
        assert_eq!(toolbox, Toolbox::default());
        assert_eq!(problems.len(), 1);
        assert!(
            problems[0].message.starts_with("the toolbox file is not valid"),
            "{problems:?}"
        );
    }
}
