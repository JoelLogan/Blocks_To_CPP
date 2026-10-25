//! Type signatures (ADR-0017 §5, ADR-0018; `docs/plans/m3-contracts/types.md`
//! §4): the checks a definition's signature keys (`where`, `type`,
//! `assign`, `members`, `type_class`, `headers`) pass when the catalog loads,
//! the [`BlockSig`] built from a valid definition for
//! [`Catalog::sigs`](crate::Catalog::sigs), and what a type field, a
//! parameter row and a header field accept.
//!
//! The rules of the signature itself (binding, the place, checking) are
//! `b2c-ir`'s ([`TypeEnv`]) and `b2c-lang`'s (`type_block`); this module only
//! makes sure every definition gives them something they can evaluate.

use std::collections::{BTreeMap, BTreeSet};

use b2c_ir::Header;
use b2c_ir::sig::{BlockSig, Fit, InputSig, PartName, PartType, SigTable, TypeEnv, TypePattern, TypeVar};
use b2c_ir::types::{NoFacts, TermError, Type, TypeClass, TypeTerm};
use b2c_model::FieldValue;
use serde_json::Value;

use crate::definitions;
use crate::schema::{BlockDef, ExtraDef, ExtraKind, FieldDef, FieldKind, InputDef};

/// Where a pattern sits in a definition.
#[derive(Debug, Clone, Copy)]
enum Place<'a> {
    /// The block's `output`.
    Output,
    /// A field's `type`.
    Field(&'a FieldDef),
    /// A value input's `type`.
    Input(&'a InputDef),
}

impl Place<'_> {
    /// How messages name the pattern.
    fn describe(self) -> String {
        match self {
            Place::Output => String::from("the output type"),
            Place::Field(field) => format!("the type of field {}", field.name),
            Place::Input(input) => format!("the type of input {}", input.name),
        }
    }
}

/// Every problem with a definition's signature keys (types.md §4.3).
pub(crate) fn problems(def: &BlockDef) -> Vec<String> {
    let mut problems = Vec::new();
    check_type_lists(def, &mut problems);
    check_headers(def, &mut problems);
    let vars = declared_vars(def, &mut problems);
    check_keys(def, &vars, &mut problems);
    for (place, pattern) in patterns(def) {
        check_forms(def, place, pattern, true, &vars, &mut problems);
        if let Place::Field(field) = place
            && field.kind == FieldKind::Type
            && !is_type_shape(pattern)
        {
            problems.push(format!(
                "{} can only be made of types, type variables and templates, so that the field knows which types it offers",
                place.describe()
            ));
        }
    }
    check_bindings(def, &vars, &mut problems);
    for input in &def.input {
        if let Some(pattern) = &input.pattern
            && !input.default.is_empty()
            && !pattern.is_fixed()
        {
            problems.push(format!(
                "input {} has a default, but its type {pattern} is not one fixed type, so one default cannot fit every type it may expect",
                input.name
            ));
        }
    }
    problems
}

/// Every pattern of a definition, with where it sits.
fn patterns(def: &BlockDef) -> Vec<(Place<'_>, &TypePattern)> {
    let output = def.output.iter().map(|pattern| (Place::Output, pattern));
    let fields = def
        .field
        .iter()
        .filter_map(|field| Some((Place::Field(field), field.pattern.as_ref()?)));
    let inputs = def
        .input
        .iter()
        .filter_map(|input| Some((Place::Input(input), input.pattern.as_ref()?)));
    output.chain(fields).chain(inputs).collect()
}

/// An entry of a `types` list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Listed {
    /// `auto`, which is not a type.
    Auto,
    /// A type.
    Type(Type),
}

/// Reads an entry of a `types` list: `auto`, or a type written in C++ syntax
/// the canonical way (`int`, `std::string`, `std::map<int, int>`).
pub(crate) fn listed_type(entry: &str) -> Option<Listed> {
    if entry == "auto" {
        return Some(Listed::Auto);
    }
    match TypePattern::parse(entry) {
        Ok(TypePattern::Type(ty)) if ty.cpp(&NoFacts).to_string() == entry => Some(Listed::Type(ty)),
        _ => None,
    }
}

fn check_type_lists(def: &BlockDef, problems: &mut Vec<String>) {
    let fields = def
        .field
        .iter()
        .map(|field| (format!("the type field {}", field.name), &field.types));
    let extras = def
        .extra
        .iter()
        .map(|extra| (format!("params {}", extra.name), &extra.types));
    for (what, types) in fields.chain(extras) {
        for entry in types {
            if listed_type(entry).is_none() {
                problems.push(format!(
                    "{what} lists {entry:?}, which is not a type written the C++ way (such as int, std::string or std::vector<int>)"
                ));
            }
        }
    }
}

/// Every header, of the block and of each dropdown option, must be in the
/// header table, and per-option headers must name options of a dropdown.
fn check_headers(def: &BlockDef, problems: &mut Vec<String>) {
    for header in &def.headers {
        if Header::standard(header).is_none() {
            problems.push(format!(
                "the header {header:?} is not a standard header from the header table, like <iostream>"
            ));
        }
    }
    for field in def.field.iter().filter(|field| !field.headers.is_empty()) {
        let name = &field.name;
        if field.kind != FieldKind::Dropdown {
            problems.push(format!("only dropdown fields have headers per option ({name})"));
            continue;
        }
        for (option, headers) in &field.headers {
            if !field.options.iter().any(|[_, value]| value == option) {
                problems.push(format!(
                    "the dropdown {name} has headers for {option:?}, which is not one of its option values"
                ));
            }
            for header in headers {
                if Header::standard(header).is_none() {
                    problems.push(format!(
                        "the header {header:?} of the option {option:?} of {name} is not a standard header from the header table, like <iostream>"
                    ));
                }
            }
        }
    }
}

/// The variables of `where`, each with its class.
fn declared_vars(def: &BlockDef, problems: &mut Vec<String>) -> BTreeMap<TypeVar, TypeClass> {
    let mut vars = BTreeMap::new();
    for (name, class) in &def.type_vars {
        match TypeVar::new(name) {
            Some(var) => {
                vars.insert(var, *class);
            }
            None => problems.push(format!(
                "the type variable {name:?} in where is not an upper-case letter, optionally followed by a digit (T, K, T2)"
            )),
        }
    }
    vars
}

/// `type` on the right fields, `members` on a reference to a struct or enum
/// variable, `assign` with a pattern, `type_class` on parameter rows.
fn check_keys(def: &BlockDef, vars: &BTreeMap<TypeVar, TypeClass>, problems: &mut Vec<String>) {
    for field in &def.field {
        let name = &field.name;
        let patterned = matches!(
            field.kind,
            FieldKind::Type | FieldKind::SymbolRef | FieldKind::SymbolDecl
        );
        if field.pattern.is_some() && !patterned {
            problems.push(format!(
                "only type, symbol_ref and symbol_decl fields have a type pattern ({name})"
            ));
        }
        let Some(members) = &field.members else {
            continue;
        };
        if field.kind != FieldKind::SymbolRef {
            problems.push(format!("only symbol_ref fields list members ({name})"));
        }
        if field.pattern.is_some() {
            problems.push(format!(
                "the field {name} lists members, so it has no type pattern of its own"
            ));
        }
        match TypeVar::new(members).and_then(|var| vars.get(&var)) {
            Some(TypeClass::Struct | TypeClass::Enum) => {}
            Some(class) => problems.push(format!(
                "the field {name} lists the members of {members}, whose class is {}, but only a struct or an enum has members",
                class.name()
            )),
            None => problems.push(format!(
                "the field {name} lists the members of {members:?}, which where does not declare"
            )),
        }
    }
    for input in &def.input {
        if input.assign && input.pattern.is_none() {
            problems.push(format!(
                "input {} is assigned (assign = true) but has no type pattern to be checked against",
                input.name
            ));
        }
    }
    for extra in &def.extra {
        if extra.type_class.is_some() && extra.kind != ExtraKind::Params {
            problems.push(format!("only params extras have a type_class ({})", extra.name));
        }
    }
}

/// Whether a pattern is made only of types, type variables and templates:
/// what a type field's pattern may be.
fn is_type_shape(pattern: &TypePattern) -> bool {
    match pattern {
        TypePattern::Type(_) | TypePattern::Var(_) => true,
        TypePattern::Lib(_, args) => args.iter().all(is_type_shape),
        TypePattern::Any
        | TypePattern::Number
        | TypePattern::Fn(..)
        | TypePattern::Field(_)
        | TypePattern::Symbol(_)
        | TypePattern::Param(_)
        | TypePattern::Member(_)
        | TypePattern::Result => false,
    }
}

/// Checks that every form of a pattern sits where it may (types.md §4.2,
/// §4.3) and names a part the block has. `whole` is true for the whole
/// pattern, false inside a template or a type function.
fn check_forms(
    def: &BlockDef,
    place: Place<'_>,
    pattern: &TypePattern,
    whole: bool,
    vars: &BTreeMap<TypeVar, TypeClass>,
    problems: &mut Vec<String>,
) {
    let has_field = |name: &str, kinds: &[FieldKind]| {
        def.field
            .iter()
            .any(|field| field.name == name && kinds.contains(&field.kind))
    };
    let repeated_input = matches!(place, Place::Input(input) if input.repeat.is_some());
    let at = place.describe();
    let problem = match pattern {
        TypePattern::Type(_) => None,
        TypePattern::Var(var) => (!vars.contains_key(var))
            .then(|| format!("{at} uses the type variable {var}, which where does not declare")),
        TypePattern::Lib(_, args) => {
            for arg in args {
                check_forms(def, place, arg, false, vars, problems);
            }
            None
        }
        TypePattern::Fn(_, arg) => {
            check_forms(def, place, arg, false, vars, problems);
            None
        }
        TypePattern::Any => {
            (!whole).then(|| format!("{at} uses any inside another pattern; any can only be a whole pattern"))
        }
        TypePattern::Number => (!(whole && matches!(place, Place::Output)))
            .then(|| format!("{at} uses number, which only a block's whole output type can be")),
        TypePattern::Field(name) => (!has_field(name, &[FieldKind::Type]))
            .then(|| format!("{at} uses field:{name}, but the block has no type field {name}")),
        TypePattern::Symbol(None) => {
            let refs = def
                .field
                .iter()
                .filter(|field| field.kind == FieldKind::SymbolRef)
                .count();
            (refs != 1).then(|| {
                format!(
                    "{at} uses symbol, which needs exactly one symbol_ref field to take the type from, but the block has {refs}"
                )
            })
        }
        TypePattern::Symbol(Some(name)) => (!has_field(name, &[FieldKind::SymbolRef, FieldKind::SymbolDecl]))
            .then(|| {
                format!(
                    "{at} uses symbol:{name}, but the block has no symbol_ref or symbol_decl field {name}"
                )
            }),
        TypePattern::Param(name) if !repeated_input => Some(format!(
            "{at} uses param:{name}, which only a repeated value input can use"
        )),
        TypePattern::Param(name) => (!has_field(name, &[FieldKind::SymbolRef]))
            .then(|| format!("{at} uses param:{name}, but the block has no symbol_ref field {name}")),
        TypePattern::Member(name) if !repeated_input => Some(format!(
            "{at} uses member:{name}, which only a repeated value input can use"
        )),
        TypePattern::Member(name) => {
            (!has_field(name, &[FieldKind::Type, FieldKind::SymbolRef])).then(|| {
                format!("{at} uses member:{name}, but the block has no type or symbol_ref field {name}")
            })
        }
        TypePattern::Result => (!matches!(place, Place::Input(_)))
            .then(|| format!("{at} uses result, which only a value input can use")),
    };
    problems.extend(problem);
}

/// A part that can bind type variables (types.md §4.3, rule 1): a `type` or
/// `symbol_ref` field with a pattern (not a `members` field), or a value
/// input with a pattern that is not `assign`. `required` is false for an
/// optional input and for a repeated input whose count can be 0.
struct Binder<'a> {
    pattern: &'a TypePattern,
    required: bool,
}

fn binders(def: &BlockDef) -> Vec<Binder<'_>> {
    let fields = def.field.iter().filter_map(|field| {
        let binds = matches!(field.kind, FieldKind::Type | FieldKind::SymbolRef) && field.members.is_none();
        Some(Binder {
            pattern: field.pattern.as_ref().filter(|_| binds)?,
            required: true,
        })
    });
    let inputs = def.input.iter().filter_map(|input| {
        let pattern = input.pattern.as_ref().filter(|_| !input.assign)?;
        let never_empty = input.repeat.as_ref().is_none_or(|repeat| {
            let min = def
                .extra
                .iter()
                .find(|extra| extra.name == repeat.count)
                .map_or(0, |extra| extra.min);
            min.saturating_add(repeat.plus) > 0
        });
        Some(Binder {
            pattern,
            required: !input.optional && never_empty,
        })
    });
    fields.chain(inputs).collect()
}

/// The output's variable when no part binds it: the block takes its type
/// from its place (types.md §4.3, rule 3).
fn place_var<'a>(def: &'a BlockDef, binders: &[Binder<'_>]) -> Option<&'a TypeVar> {
    match &def.output {
        Some(TypePattern::Var(var))
            if !binders
                .iter()
                .any(|binder| binder.pattern.binding_vars().contains(&var)) =>
        {
            Some(var)
        }
        _ => None,
    }
}

/// Every variable of `where` is used and can be bound: an output variable
/// (other than a place-typed block's) by a part that must be filled, any
/// other by some part.
fn check_bindings(def: &BlockDef, vars: &BTreeMap<TypeVar, TypeClass>, problems: &mut Vec<String>) {
    let binders = binders(def);
    let place = place_var(def, &binders);
    let bound = |var: &TypeVar, required_only: bool| {
        binders
            .iter()
            .any(|binder| (binder.required || !required_only) && binder.pattern.binding_vars().contains(&var))
    };
    let mut used: BTreeSet<&str> = patterns(def)
        .into_iter()
        .flat_map(|(_, pattern)| pattern.vars().map(TypeVar::as_str))
        .collect();
    used.extend(def.field.iter().filter_map(|field| field.members.as_deref()));
    let in_output: BTreeSet<&TypeVar> = def.output.iter().flat_map(TypePattern::vars).collect();
    for var in vars.keys() {
        if place == Some(var) {
            continue;
        }
        if !used.contains(var.as_str()) {
            problems.push(format!("the type variable {var} in where is not used"));
        } else if in_output.contains(var) {
            if !bound(var, true) {
                problems.push(format!(
                    "the output type uses the type variable {var}, but no part that must be filled binds it (a part binds a variable its type names outside elem, index and key)"
                ));
            }
        } else if !bound(var, false) {
            problems.push(format!(
                "nothing can bind the type variable {var}: no type field, reference or value input names it outside elem, index and key"
            ));
        }
    }
}

/// The signature of a definition that passed every check.
pub(crate) fn build(def: &BlockDef) -> BlockSig {
    let mut refs = def
        .field
        .iter()
        .filter(|field| field.kind == FieldKind::SymbolRef);
    let symbol_field = match (refs.next(), refs.next()) {
        (Some(field), None) => Some(field.name.as_str()),
        _ => None,
    };
    let named = |pattern: &TypePattern| name_symbol(pattern, symbol_field);
    let order = def
        .field
        .iter()
        .filter(|field| field.pattern.is_some())
        .map(|field| PartName::Field(field.name.clone()))
        .chain(
            def.input
                .iter()
                .filter(|input| input.pattern.is_some())
                .map(|input| PartName::Input(input.name.clone())),
        )
        .collect();
    let headers = |names: &[String]| names.iter().filter_map(|name| Header::standard(name)).collect();
    BlockSig {
        vars: vars_of(def),
        output: def.output.as_ref().map(named),
        fields: def
            .field
            .iter()
            .filter_map(|field| Some((field.name.clone(), named(field.pattern.as_ref()?))))
            .collect(),
        inputs: def
            .input
            .iter()
            .map(|input| {
                let sig = InputSig {
                    check: input.check,
                    pattern: input.pattern.as_ref().map(named),
                    assign: input.assign,
                };
                (input.name.clone(), sig)
            })
            .collect(),
        order,
        place_var: place_var(def, &binders(def)).cloned(),
        members: def
            .field
            .iter()
            .filter_map(|field| Some((field.name.clone(), TypeVar::new(field.members.as_deref()?)?)))
            .collect(),
        symbol_refs: def
            .field
            .iter()
            .filter(|field| field.kind == FieldKind::SymbolRef && field.pattern.is_some())
            .map(|field| field.name.clone())
            .collect(),
        symbol_decls: def
            .field
            .iter()
            .filter(|field| field.kind == FieldKind::SymbolDecl && field.pattern.is_some())
            .map(|field| field.name.clone())
            .collect(),
        repeated: def
            .input
            .iter()
            .filter(|input| input.repeat.is_some())
            .map(|input| input.name.clone())
            .collect(),
        headers: headers(&def.headers),
        option_headers: def
            .field
            .iter()
            .filter(|field| !field.headers.is_empty())
            .map(|field| {
                let options = field
                    .headers
                    .iter()
                    .map(|(option, names)| (option.clone(), headers(names)))
                    .collect();
                (field.name.clone(), options)
            })
            .collect(),
    }
}

/// The variables of `where` that are variables, with their classes.
fn vars_of(def: &BlockDef) -> BTreeMap<TypeVar, TypeClass> {
    def.type_vars
        .iter()
        .filter_map(|(name, class)| Some((TypeVar::new(name)?, *class)))
        .collect()
}

/// M2's `symbol` named after the block's one `symbol_ref` field, as
/// [`TypeEnv::resolve`] reads it.
fn name_symbol(pattern: &TypePattern, field: Option<&str>) -> TypePattern {
    match pattern {
        TypePattern::Symbol(None) => TypePattern::Symbol(field.map(str::to_owned)),
        TypePattern::Lib(ty, args) => {
            TypePattern::Lib(*ty, args.iter().map(|arg| name_symbol(arg, field)).collect())
        }
        TypePattern::Fn(function, arg) => TypePattern::Fn(*function, Box::new(name_symbol(arg, field))),
        other => other.clone(),
    }
}

/// The signatures of every definition that passes the catalog's checks, by
/// block type. A broken definition gets none: its blocks are kept unchecked
/// (`B2C-E0620`).
pub(crate) fn table(blocks: &BTreeMap<String, BlockDef>) -> SigTable {
    let mut table = SigTable::empty();
    for (id, def) in blocks {
        if *id == def.id && definitions::check_block(def).is_empty() {
            table.insert(id.clone(), build(def));
        }
    }
    table
}

// ---------------------------------------------------------------------------
// What fields and rows accept
// ---------------------------------------------------------------------------

/// How a value fits what a type field or a parameter row accepts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TypeFit {
    /// It is accepted.
    Fits,
    /// A type, or `auto`, that the field or row does not accept: a newer
    /// catalog may have widened it.
    Outside,
    /// A well-formed name that is no type this catalog knows: a newer
    /// catalog may have added it.
    Unknown,
    /// No type at all: a value of another shape, a name that breaks the
    /// name rule, or a template with the wrong arguments.
    Invalid,
}

impl TypeFit {
    /// Whether a document from a newer catalog may hold it legitimately, so
    /// that it is `B2C-E0602` there instead of `E0607` or `E0614`.
    pub(crate) fn may_be_newer(self) -> bool {
        matches!(self, TypeFit::Outside | TypeFit::Unknown)
    }
}

/// What a type field accepts (types.md §4.1): its `types` (the only way to
/// `auto` and `void`) and, when it has a pattern, every type the pattern
/// matches within its variables' classes. A user type always matches a
/// variable, since the catalog cannot know its kind: the analyser checks
/// references.
pub(crate) fn field_type_fit(def: &BlockDef, field: &FieldDef, value: &FieldValue) -> TypeFit {
    if let FieldValue::Text(text) = value
        && field.types.contains(text)
    {
        return TypeFit::Fits;
    }
    let Some(term) = value.type_term() else {
        return TypeFit::Invalid;
    };
    accepts(&field.types, &term, |ty| {
        field
            .pattern
            .as_ref()
            .is_some_and(|pattern| pattern_matches(def, pattern, ty))
    })
}

/// What a parameter row's `type` accepts: the row key's `types` and, with
/// `type_class`, every type in that class (a user type always: the analyser
/// checks references).
pub(crate) fn row_type_fit(extra: &ExtraDef, value: &Value) -> TypeFit {
    if let Value::String(text) = value
        && extra.types.contains(text)
    {
        return TypeFit::Fits;
    }
    let Ok(term) = serde_json::from_value::<TypeTerm>(value.clone()) else {
        return TypeFit::Invalid;
    };
    accepts(&extra.types, &term, |ty| {
        extra
            .type_class
            .is_some_and(|class| ty.is_in(class, &NoFacts) != Some(false))
    })
}

/// A term against a `types` list and a wider rule, which never admits
/// `void`.
fn accepts(types: &[String], term: &TypeTerm, wider: impl Fn(&Type) -> bool) -> TypeFit {
    let ty = match Type::from_term(term) {
        Ok(ty) => ty,
        Err(TermError::Auto) => return TypeFit::Outside,
        Err(TermError::Unknown) => return TypeFit::Unknown,
        Err(_) => return TypeFit::Invalid,
    };
    let listed = types
        .iter()
        .any(|entry| listed_type(entry) == Some(Listed::Type(ty.clone())));
    if listed || (ty != Type::Void && wider(&ty)) {
        TypeFit::Fits
    } else {
        TypeFit::Outside
    }
}

/// Whether a type field's pattern matches a type within the classes of the
/// block's variables (a type whose membership the catalog cannot know, a
/// user type, matches).
fn pattern_matches(def: &BlockDef, pattern: &TypePattern, ty: &Type) -> bool {
    if let TypePattern::Type(fixed) = pattern {
        return fixed == ty;
    }
    let sig = BlockSig {
        vars: vars_of(def),
        ..BlockSig::default()
    };
    let part = PartType::Known(ty.clone());
    let fit = TypeEnv::new(&sig).bind(pattern, &part, PartName::Field(String::new()), &NoFacts);
    matches!(fit, Fit::Fits | Fit::Unknown)
}

/// What a type field's pattern or a row's `type_class` adds to its `types`,
/// for messages; `None` when it adds nothing.
pub(crate) fn field_wider(def: &BlockDef, field: &FieldDef) -> Option<String> {
    match field.pattern.as_ref()? {
        TypePattern::Var(var) => {
            let class = vars_of(def).get(var).copied().unwrap_or(TypeClass::Any);
            Some(class_phrase(class).to_owned())
        }
        TypePattern::Type(ty) => Some(format!("{}", ty.cpp(&NoFacts))),
        TypePattern::Lib(ty, _) => Some(format!("a {} of a type the block allows", ty.cpp_name())),
        _ => None,
    }
}

/// A class of types as messages name it: what a value of the class may be.
pub(crate) fn class_phrase(class: TypeClass) -> &'static str {
    match class {
        TypeClass::Any => "any type but void",
        TypeClass::Number => "a number type (int, double or char)",
        TypeClass::Integer => "a whole-number type (int or char)",
        TypeClass::Bool => "bool",
        TypeClass::Text => "a text type (std::string or char)",
        TypeClass::Value => "another type a variable can hold",
        TypeClass::Collection => "a list, deque, set, queue, stack, priority queue or map",
        TypeClass::Sequence => "a list or deque",
        TypeClass::Indexable => "a list, deque or map",
        TypeClass::Insertable => "a list, deque, set, queue, stack or priority queue",
        TypeClass::Searchable => "a list, deque, set or map",
        TypeClass::Iterable => "a list, deque, set or std::string",
        TypeClass::Keyed => "a set or map",
        TypeClass::Adaptor => "a queue, stack or priority queue",
        TypeClass::Listable => "a list, deque or set",
        TypeClass::Sortable => "a list or deque of comparable items",
        TypeClass::Switchable => "char, int or an enum",
        TypeClass::Comparable => "a comparable type",
        TypeClass::Printable => "a printable type",
        TypeClass::Struct => "a struct",
        TypeClass::Enum => "an enum",
        TypeClass::InputFile => "an input file (std::ifstream)",
        TypeClass::OutputFile => "an output file (std::ofstream)",
        TypeClass::Exception => "an error type",
    }
}

/// Whether text names a header of the header table.
pub(crate) fn header_fits(text: &str) -> bool {
    Header::standard(text).is_some()
}

/// Whether text is shaped like a standard header (`<name>` with lower-case
/// letters, digits and `_`), which a newer catalog's table may have.
pub(crate) fn looks_like_header(text: &str) -> bool {
    text.strip_prefix('<')
        .and_then(|inner| inner.strip_suffix('>'))
        .is_some_and(|inner| {
            !inner.is_empty()
                && inner
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::CatalogFile;

    /// The one definition of a catalog file.
    fn def(toml: &str) -> BlockDef {
        let header = "[[block]]\nid = \"test.block\"\nversion = 1\ncategory = \"control\"\nlowering = \"builtin\"\nhelp = \"Help.\"\n";
        let file: CatalogFile = toml::from_str(&format!("{header}{toml}")).unwrap();
        file.block.into_iter().next().unwrap()
    }

    /// A reporter with the given output, `where` and parts.
    fn reporter(output: &str, vars: &str, parts: &str) -> BlockDef {
        def(&format!(
            "shape = \"reporter\"\noutput = \"{output}\"\nwhere = {{ {vars} }}\nlabel = {{ friendly = \"x\", cpp = \"x\" }}\n{parts}"
        ))
    }

    /// `coll.item`: `elem(C)` from a list or map in `LIST`, at `INDEX`.
    fn item() -> BlockDef {
        reporter(
            "elem(C)",
            "C = \"indexable\"",
            "[[block.field]]\nname = \"LIST\"\nkind = \"symbol_ref\"\ntype = \"C\"\n\
             [[block.input]]\nname = \"INDEX\"\ncheck = \"any\"\ntype = \"index(C)\"\n",
        )
    }

    #[test]
    fn a_collection_block_passes_and_builds_its_signature() {
        let item = item();
        assert_eq!(problems(&item), Vec::<String>::new());
        let sig = build(&item);
        let c = TypeVar::new("C").unwrap();
        assert_eq!(sig.vars, BTreeMap::from([(c.clone(), TypeClass::Indexable)]));
        assert_eq!(sig.output, Some(TypePattern::parse("elem(C)").unwrap()));
        assert_eq!(sig.fields["LIST"], TypePattern::Var(c));
        assert_eq!(
            sig.inputs["INDEX"].pattern,
            Some(TypePattern::parse("index(C)").unwrap())
        );
        assert_eq!(
            sig.order,
            [PartName::Field("LIST".into()), PartName::Input("INDEX".into())]
        );
        assert_eq!(sig.place_var, None);
    }

    #[test]
    fn a_default_needs_a_fixed_pattern() {
        let with_default = |ty: &str| {
            reporter(
                "elem(C)",
                "C = \"indexable\"",
                &format!(
                    "[[block.field]]\nname = \"LIST\"\nkind = \"symbol_ref\"\ntype = \"C\"\n\
                     [[block.input]]\nname = \"INDEX\"\ncheck = \"any\"\ntype = \"{ty}\"\ndefault = [{{ num = \"0\" }}]\n"
                ),
            )
        };
        assert_eq!(
            problems(&with_default("index(C)")),
            [
                "input INDEX has a default, but its type index(C) is not one fixed type, so one default cannot fit every type it may expect"
            ]
        );
        assert_eq!(problems(&with_default("int")), Vec::<String>::new());
    }

    #[test]
    fn variables_are_declared_used_and_bound() {
        // Declared but unused, and used but not declared.
        let block = reporter(
            "int",
            "T = \"value\", U = \"value\"",
            "[[block.input]]\nname = \"A\"\ncheck = \"any\"\ntype = \"T\"\n\
             [[block.input]]\nname = \"B\"\ncheck = \"any\"\ntype = \"V\"\n",
        );
        assert_eq!(
            problems(&block),
            [
                "the type of input B uses the type variable V, which where does not declare",
                "the type variable U in where is not used",
            ]
        );
        // A variable only inside a function, and not in the output, can never be bound.
        let block = reporter(
            "int",
            "C = \"indexable\"",
            "[[block.input]]\nname = \"I\"\ncheck = \"any\"\ntype = \"index(C)\"\n",
        );
        assert_eq!(
            problems(&block),
            [
                "nothing can bind the type variable C: no type field, reference or value input names it outside elem, index and key"
            ]
        );
        // An output variable needs a binder that must be filled.
        let block = reporter(
            "elem(C)",
            "C = \"indexable\"",
            "[[block.input]]\nname = \"L\"\ncheck = \"any\"\noptional = true\ntype = \"C\"\n",
        );
        assert_eq!(
            problems(&block),
            [
                "the output type uses the type variable C, but no part that must be filled binds it (a part binds a variable its type names outside elem, index and key)"
            ]
        );
        // An assign input never binds.
        let block = reporter(
            "T",
            "T = \"value\"",
            "[[block.input]]\nname = \"A\"\ncheck = \"any\"\ntype = \"T\"\nassign = true\n",
        );
        assert_eq!(build(&block).place_var, TypeVar::new("T"));
        // Not a variable name.
        let block = reporter("int", "TT = \"value\"", "");
        assert_eq!(
            problems(&block),
            [
                "the type variable \"TT\" in where is not an upper-case letter, optionally followed by a digit (T, K, T2)"
            ]
        );
    }

    #[test]
    fn a_place_typed_block_has_a_place_variable() {
        // `list of`: items of `elem(C)`; nothing binds C, so the place does.
        let list = def(
            "shape = \"reporter\"\noutput = \"C\"\nwhere = { C = \"listable\" }\n\
             label = { friendly = \"list of %ITEM …\", cpp = \"{…}\" }\n\
             [[block.extra]]\nname = \"itemCount\"\nkind = \"count\"\nmin = 0\nmax = 8\ndefault = \"2\"\n\
             [[block.input]]\nname = \"ITEM\"\ncheck = \"any\"\nrepeat = { count = \"itemCount\" }\ntype = \"elem(C)\"\n",
        );
        assert_eq!(problems(&list), Vec::<String>::new());
        assert_eq!(build(&list).place_var, TypeVar::new("C"));
        // A repeated input whose count can be 0 binds, but not for sure.
        let maybe = def(
            "shape = \"reporter\"\noutput = \"elem(C)\"\nwhere = { C = \"listable\" }\n\
             label = { friendly = \"first of %L …\", cpp = \"x\" }\n\
             [[block.extra]]\nname = \"n\"\nkind = \"count\"\nmin = 0\nmax = 8\ndefault = \"1\"\n\
             [[block.input]]\nname = \"L\"\ncheck = \"any\"\nrepeat = { count = \"n\" }\ntype = \"C\"\n",
        );
        assert_eq!(problems(&maybe).len(), 1, "{:?}", problems(&maybe));
        let mut sure = maybe.clone();
        sure.extra[0].min = 1;
        assert_eq!(problems(&sure), Vec::<String>::new());
        assert_eq!(build(&sure).place_var, None);
    }

    #[test]
    fn forms_sit_where_they_may() {
        let block = def(
            "shape = \"statement\"\nwhere = { S = \"struct\", E = \"enum\" }\n\
             label = { friendly = \"x %F %T %M %V %A %B %C\", cpp = \"x\" }\n\
             [[block.field]]\nname = \"F\"\nkind = \"symbol_ref\"\n\
             [[block.field]]\nname = \"T\"\nkind = \"type\"\ntypes = [\"int\"]\ntype = \"std::vector<S>\"\n\
             [[block.field]]\nname = \"M\"\nkind = \"symbol_ref\"\nmembers = \"S\"\n\
             [[block.field]]\nname = \"V\"\nkind = \"dropdown\"\noptions = [[\"a\", \"a\"]]\ntype = \"int\"\n\
             [[block.input]]\nname = \"A\"\ncheck = \"any\"\ntype = \"param:F\"\n\
             [[block.input]]\nname = \"B\"\ncheck = \"any\"\ntype = \"std::vector<number>\"\n\
             [[block.input]]\nname = \"C\"\ncheck = \"any\"\ntype = \"field:F\"\n\
             [[block.input]]\nname = \"D\"\ncheck = \"any\"\ntype = \"E\"\nassign = true\n",
        );
        assert_eq!(
            problems(&block),
            [
                "only type, symbol_ref and symbol_decl fields have a type pattern (V)",
                "the type of input A uses param:F, which only a repeated value input can use",
                "the type of input B uses number, which only a block's whole output type can be",
                "the type of input C uses field:F, but the block has no type field F",
                "nothing can bind the type variable E: no type field, reference or value input names it outside elem, index and key",
            ]
        );
        let block = reporter(
            "result",
            "",
            "[[block.field]]\nname = \"T\"\nkind = \"type\"\ntypes = [\"int\"]\ntype = \"elem(std::vector<int>)\"\n\
             [[block.input]]\nname = \"A\"\ncheck = \"any\"\ntype = \"symbol\"\n",
        );
        assert_eq!(
            problems(&block),
            [
                "the output type uses result, which only a value input can use",
                "the type of field T can only be made of types, type variables and templates, so that the field knows which types it offers",
                "the type of input A uses symbol, which needs exactly one symbol_ref field to take the type from, but the block has 0",
            ]
        );
    }

    #[test]
    fn members_need_a_struct_or_enum_variable() {
        let member = |class: &str, extra: &str| {
            reporter(
                "symbol:MEMBER",
                &format!("S = \"{class}\""),
                &format!(
                    "[[block.input]]\nname = \"OBJ\"\ncheck = \"any\"\ntype = \"S\"\n\
                     [[block.field]]\nname = \"MEMBER\"\nkind = \"symbol_ref\"\nmembers = \"S\"\n{extra}"
                ),
            )
        };
        let good = member("struct", "");
        assert_eq!(problems(&good), Vec::<String>::new());
        let sig = build(&good);
        assert_eq!(sig.members["MEMBER"], TypeVar::new("S").unwrap());
        // A members field binds nothing and has no pattern of its own.
        assert_eq!(sig.order, [PartName::Input("OBJ".into())]);
        assert_eq!(problems(&member("enum", "")), Vec::<String>::new());
        assert_eq!(
            problems(&member("value", "")),
            [
                "the field MEMBER lists the members of S, whose class is value, but only a struct or an enum has members"
            ]
        );
        assert_eq!(
            problems(&member("struct", "type = \"S\"\n")),
            ["the field MEMBER lists members, so it has no type pattern of its own"]
        );
    }

    #[test]
    fn headers_come_from_the_table() {
        let block = def(
            "shape = \"statement\"\nheaders = [\"<cmath>\", \"<bits/stdc++.h>\"]\n\
             label = { friendly = \"x %FN %N\", cpp = \"x\" }\n\
             [[block.field]]\nname = \"FN\"\nkind = \"dropdown\"\noptions = [[\"root\", \"sqrt\"], [\"max\", \"max\"]]\n\
             headers = { sqrt = [\"<cmath>\"], max = [\"<algorithm>\", \"<math.h>\"], min = [] }\n\
             [[block.field]]\nname = \"N\"\nkind = \"text\"\nheaders = { a = [] }\n",
        );
        assert_eq!(
            problems(&block),
            [
                "the header \"<bits/stdc++.h>\" is not a standard header from the header table, like <iostream>",
                "the header \"<math.h>\" of the option \"max\" of FN is not a standard header from the header table, like <iostream>",
                "the dropdown FN has headers for \"min\", which is not one of its option values",
                "only dropdown fields have headers per option (N)",
            ]
        );
        let mut good = block.clone();
        good.headers = vec!["<cmath>".into()];
        good.field[0].headers.remove("min");
        good.field[0]
            .headers
            .insert("max".into(), vec!["<algorithm>".into()]);
        good.field[1].headers.clear();
        assert_eq!(problems(&good), Vec::<String>::new());
        let sig = build(&good);
        assert_eq!(sig.headers, [Header::standard("<cmath>").unwrap()]);
        assert_eq!(
            sig.option_headers["FN"]["max"],
            [Header::standard("<algorithm>").unwrap()]
        );
    }

    #[test]
    fn types_lists_are_written_the_cpp_way() {
        for (entry, ok) in [
            ("int", true),
            ("auto", true),
            ("void", true),
            ("std::string", true),
            ("std::vector<int>", true),
            ("std::map<int, std::vector<int>>", true),
            ("std::map<int,int>", false),
            ("string", false),
            ("T", false),
            ("long", false),
            ("std::vector<T>", false),
        ] {
            assert_eq!(listed_type(entry).is_some(), ok, "{entry}");
        }
    }

    fn text(value: &str) -> FieldValue {
        FieldValue::Text(value.into())
    }

    fn term(json: Value) -> FieldValue {
        FieldValue::Type(serde_json::from_value(json).unwrap())
    }

    #[test]
    fn type_fields_accept_their_types_and_their_pattern() {
        let block = def(
            "shape = \"statement\"\nwhere = { T = \"value\" }\nlabel = { friendly = \"x %TYPE\", cpp = \"x\" }\n\
             [[block.field]]\nname = \"TYPE\"\nkind = \"type\"\ntypes = [\"int\", \"auto\", \"std::vector<int>\"]\ntype = \"T\"\n",
        );
        let field = &block.field[0];
        let fit = |value: &FieldValue| field_type_fit(&block, field, value);
        assert_eq!(fit(&text("int")), TypeFit::Fits);
        assert_eq!(fit(&text("auto")), TypeFit::Fits);
        assert_eq!(fit(&text("double")), TypeFit::Fits);
        assert_eq!(
            fit(&term(serde_json::json!({"std::vector": "int"}))),
            TypeFit::Fits
        );
        assert_eq!(
            fit(&term(
                serde_json::json!({"std::map": ["std::string", {"std::vector": "char"}]})
            )),
            TypeFit::Fits
        );
        assert_eq!(
            fit(&FieldValue::Ref(
                serde_json::from_value(serde_json::json!({"ref": "sym_Pt"})).unwrap()
            )),
            TypeFit::Fits
        );
        // Not a value type, not a type of this catalog, not a type at all.
        assert_eq!(fit(&text("std::ifstream")), TypeFit::Outside);
        assert_eq!(fit(&text("void")), TypeFit::Outside);
        assert_eq!(fit(&text("float")), TypeFit::Unknown);
        assert_eq!(
            fit(&term(serde_json::json!({"std::list": "int"}))),
            TypeFit::Unknown
        );
        assert_eq!(fit(&text("int; x")), TypeFit::Invalid);
        assert_eq!(fit(&FieldValue::Bool(true)), TypeFit::Invalid);
        assert_eq!(
            fit(&term(serde_json::json!({"std::vector": "std::ifstream"}))),
            TypeFit::Invalid
        );

        // Without a pattern only the list counts, including its templates.
        let mut listed = block.clone();
        listed.field[0].pattern = None;
        let field = &listed.field[0];
        let fit = |value: &FieldValue| field_type_fit(&listed, field, value);
        assert_eq!(
            fit(&term(serde_json::json!({"std::vector": "int"}))),
            TypeFit::Fits
        );
        assert_eq!(fit(&text("double")), TypeFit::Outside);
        assert_eq!(fit(&term(serde_json::json!({"ref": "sym_Pt"}))), TypeFit::Outside);

        // A template pattern takes only its own shape.
        let mut lists = block.clone();
        lists.field[0].pattern = Some(TypePattern::parse("std::vector<T>").unwrap());
        let field = &lists.field[0];
        let fit = |value: &FieldValue| field_type_fit(&lists, field, value);
        assert_eq!(
            fit(&term(serde_json::json!({"std::vector": "double"}))),
            TypeFit::Fits
        );
        assert_eq!(
            fit(&term(serde_json::json!({"std::set": "double"}))),
            TypeFit::Outside
        );
        assert_eq!(fit(&text("double")), TypeFit::Outside);
        assert!(TypeFit::Outside.may_be_newer() && TypeFit::Unknown.may_be_newer());
        assert!(!TypeFit::Invalid.may_be_newer() && !TypeFit::Fits.may_be_newer());
    }

    #[test]
    fn rows_accept_their_types_and_their_class() {
        use serde_json::json;
        let row = |types: &[&str], class: Option<TypeClass>| ExtraDef {
            name: "params".into(),
            kind: ExtraKind::Params,
            min: 0,
            max: 4,
            default: None,
            types: types.iter().map(|t| (*t).to_owned()).collect(),
            type_class: class,
        };
        let plain = row(&["int", "std::string"], None);
        let wide = row(&["int", "std::string"], Some(TypeClass::Value));
        assert_eq!(row_type_fit(&plain, &json!("int")), TypeFit::Fits);
        assert_eq!(row_type_fit(&plain, &json!("double")), TypeFit::Outside);
        assert_eq!(row_type_fit(&wide, &json!("double")), TypeFit::Fits);
        assert_eq!(row_type_fit(&wide, &json!({"std::vector": "int"})), TypeFit::Fits);
        assert_eq!(row_type_fit(&wide, &json!({"ref": "sym_Pt"})), TypeFit::Fits);
        assert_eq!(row_type_fit(&wide, &json!("std::ofstream")), TypeFit::Outside);
        assert_eq!(row_type_fit(&wide, &json!("void")), TypeFit::Outside);
        assert_eq!(row_type_fit(&wide, &json!("auto")), TypeFit::Outside);
        assert_eq!(row_type_fit(&wide, &json!("float")), TypeFit::Unknown);
        assert_eq!(row_type_fit(&wide, &json!(3)), TypeFit::Invalid);
        assert_eq!(
            row_type_fit(&wide, &json!({"std::vector": ["int"]})),
            TypeFit::Invalid
        );
        assert_eq!(row_type_fit(&wide, &json!("int; x")), TypeFit::Invalid);
    }

    #[test]
    fn headers_fit_by_the_table_and_look_like_headers() {
        assert!(header_fits("<vector>"));
        assert!(!header_fits("<future_header>"));
        assert!(looks_like_header("<future_header>"));
        for bad in ["vector", "<Vector>", "<math.h>", "<>", "<a b>", "\"x\""] {
            assert!(!header_fits(bad) && !looks_like_header(bad), "{bad}");
        }
    }

    #[test]
    fn every_class_has_a_phrase() {
        let phrases: BTreeSet<&str> = TypeClass::ALL.into_iter().map(class_phrase).collect();
        assert_eq!(phrases.len(), TypeClass::ALL.len());
    }
}
