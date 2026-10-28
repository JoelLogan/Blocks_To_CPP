//! Lowering the struct and enum blocks of W2 (`docs/plans/m3-contracts/w2.md`
//! §1.5, §2.2): *struct*, *enum*, *new … with*, *… of …*, *set … of … to*
//! and an enum's value.
//!
//! Structs and enums are definitions of their module, like functions:
//! [`Lowerer::declare_user_types`] declares every one of the program, with its
//! fields or values, before any function or body is lowered, so canvas order
//! never matters. A struct's or enum's name is a `SymbolKind::Struct` or
//! `SymbolKind::Enum`, each row a `SymbolKind::Field` or
//! `SymbolKind::Enumerator` with its owner, in row order.
//!
//! The value blocks are typed by their catalog signatures (ADR-0018): *new …
//! with*'s parts take the types of the chosen struct's fields in order
//! (`member:TYPE`), and a `members` field names a member of the type its
//! variable is bound to (`B2C-E0207` for one it lacks, reported by
//! `type_parts`).

use std::collections::BTreeMap;

use b2c_ir::diag::{Location, Part};
use b2c_ir::ids::SymbolId;
use b2c_ir::sast::{
    CppStandard, EnumDef, Expr, ExprKind, Item, ItemKind, Origin, StmtKind, StructDef, Symbol, SymbolKind,
};
use b2c_ir::sig::PartType;
use b2c_ir::types::{Type, TypeTerm};
use b2c_model::Block;

use super::convert::Subject;
use super::expr::error_expr;
use super::{ItemCtx, Lowerer, TypeUse, field, holds_value, sorted_blocks};
use crate::access;
use crate::codes;
use crate::messages::quoted;

/// A field type whose requirements (`B2C-E0313`) are checked once every
/// struct's flags are known.
pub(super) struct PendingField {
    ty: Type,
    location: Location,
}

/// What a definition is, for messages.
fn kind_word(block_type: &str) -> &'static str {
    if block_type == "type.enum" {
        "the enum"
    } else {
        "the struct"
    }
}

/// A phrase subject for messages ("The `mark` field of `Student`").
fn phrase(text: String) -> Subject {
    Subject::Phrase { text, stores: true }
}

impl Lowerer<'_> {
    // --- Declarations -----------------------------------------------------------

    /// Declares every struct and enum of the program with its fields or values
    /// (w2.md §2.2), before any function: first their names, so that a field
    /// may name any of them, then their rows. Returns the field types whose
    /// requirements are checked once the structs' flags are settled
    /// ([`Lowerer::check_field_requirements`]).
    pub(super) fn declare_user_types(&mut self) -> Vec<PendingField> {
        let document = self.document;
        let mut names: BTreeMap<String, SymbolId> = BTreeMap::new();
        let mut declared: Vec<(usize, &Block, SymbolId)> = Vec::new();
        for (index, module) in document.modules.iter().enumerate() {
            self.enter_module(index, &module.id);
            for block in sorted_blocks(module) {
                if block.disabled || !matches!(block.block_type.as_str(), "type.struct" | "type.enum") {
                    continue;
                }
                if let Some(sym) = self.declare_user_type(block, &mut names) {
                    declared.push((index, block, sym));
                }
            }
        }
        self.user_types = names;
        let mut pending = Vec::new();
        for (index, block, sym) in declared {
            self.enter_module(index, &document.modules[index].id);
            if block.block_type == "type.struct" {
                self.declare_fields(block, &sym, &mut pending);
            } else {
                self.declare_values(block, &sym);
            }
        }
        pending
    }

    /// Declares a struct's or an enum's name, with no rows yet. Returns its
    /// symbol when it is in the table.
    fn declare_user_type(
        &mut self,
        block: &Block,
        names: &mut BTreeMap<String, SymbolId>,
    ) -> Option<SymbolId> {
        let what = kind_word(&block.block_type);
        let name_loc = self.loc(&block.id, field("NAME"));
        let Ok(decl) = access::decl_field(block, "NAME") else {
            let noun = if block.block_type == "type.enum" {
                "enum"
            } else {
                "struct"
            };
            self.incomplete(name_loc, format!("This {noun} needs a name."));
            return None;
        };
        let ident = self.ident(&decl.name, &name_loc, true);
        if let Some(other) = names.get(&decl.name).cloned() {
            let message = format!(
                "There is already a struct or enum called {}. Rename one of them.",
                quoted(&decl.name)
            );
            self.error_with_decl(
                codes::DUPLICATE_NAME,
                name_loc.clone(),
                message,
                &other,
                "the other definition",
            );
        } else {
            names.insert(decl.name.clone(), decl.sym.clone());
        }
        let function = self
            .module_functions
            .iter()
            .find_map(|functions| functions.get(&decl.name))
            .cloned();
        if let Some(function) = function {
            let message = format!(
                "There is already a function called {}, and C++ would not know which of the two the name \
                 means. Rename {what} or the function.",
                quoted(&decl.name)
            );
            self.error_with_decl(
                codes::DUPLICATE_NAME,
                name_loc.clone(),
                message,
                &function,
                "the function",
            );
        }
        let kind = if block.block_type == "type.enum" {
            SymbolKind::Enum { values: Vec::new() }
        } else {
            SymbolKind::Struct {
                fields: Vec::new(),
                comparable: self.checkbox(block, "COMPARABLE", false),
                printable: self.checkbox(block, "PRINTABLE", false),
            }
        };
        let symbol = Symbol {
            name: ident,
            kind,
            ty: Type::Named(decl.sym.clone()),
            origin: self.origin(&block.id, field("NAME")),
        };
        self.insert_symbol(&decl.sym, &decl.name, symbol, name_loc)
            .then(|| decl.sym.clone())
    }

    /// Declares a struct's fields, in row order: each a valid name, unique in
    /// the struct and other than the struct's own (C++ refuses a member named
    /// like its class), of a type a variable can hold.
    fn declare_fields(&mut self, block: &Block, owner: &SymbolId, pending: &mut Vec<PendingField>) {
        let owner_name = self.raw_name(owner);
        let mut seen: BTreeMap<String, SymbolId> = BTreeMap::new();
        let mut fields = Vec::new();
        for (i, row) in access::rows(block, "fields").into_iter().enumerate() {
            let part = field(&format!("fields[{i}]"));
            let loc = self.loc(&block.id, part.clone());
            let Ok(row) = row else {
                self.incomplete(
                    loc,
                    "This field of the struct is damaged. Remove it and add it again.",
                );
                continue;
            };
            let ident = self.ident(&row.name, &loc, false);
            self.check_row_name(&row.name, &row.sym, &owner_name, "field", &mut seen, &loc);
            let ty = match row.ty {
                Some(Ok(term)) => self.field_type(&term, &loc),
                Some(Err(text)) => {
                    let message = format!("{} is not a type that a field can have.", quoted(&text));
                    self.incomplete(loc.clone(), message);
                    Type::Error
                }
                None => Type::Error,
            };
            if ty != Type::Error {
                pending.push(PendingField {
                    ty: ty.clone(),
                    location: loc.clone(),
                });
            }
            let symbol = Symbol {
                name: ident,
                kind: SymbolKind::Field { owner: owner.clone() },
                ty,
                origin: self.origin(&block.id, part),
            };
            if self.insert_symbol(&row.sym, &row.name, symbol, loc) {
                if holds_value(block, &format!("DEFAULT{i}")) {
                    self.defaulted_fields.insert(row.sym.clone());
                }
                fields.push(row.sym);
            }
        }
        if let Some(SymbolKind::Struct { fields: stored, .. }) =
            self.symbols.symbols.get_mut(owner).map(|symbol| &mut symbol.kind)
        {
            *stored = fields;
        }
    }

    /// Declares an enum's values, in row order: each a valid name, unique in
    /// the enum.
    fn declare_values(&mut self, block: &Block, owner: &SymbolId) {
        let owner_name = self.raw_name(owner);
        let mut seen: BTreeMap<String, SymbolId> = BTreeMap::new();
        let mut values = Vec::new();
        for (i, row) in access::rows(block, "values").into_iter().enumerate() {
            let part = field(&format!("values[{i}]"));
            let loc = self.loc(&block.id, part.clone());
            let Ok(row) = row else {
                self.incomplete(
                    loc,
                    "This value of the enum is damaged. Remove it and add it again.",
                );
                continue;
            };
            let ident = self.ident(&row.name, &loc, false);
            self.check_row_name(&row.name, &row.sym, &owner_name, "value", &mut seen, &loc);
            let symbol = Symbol {
                name: ident,
                kind: SymbolKind::Enumerator { owner: owner.clone() },
                ty: Type::Named(owner.clone()),
                origin: self.origin(&block.id, part),
            };
            if self.insert_symbol(&row.sym, &row.name, symbol, loc) {
                values.push(row.sym);
            }
        }
        if let Some(SymbolKind::Enum { values: stored }) =
            self.symbols.symbols.get_mut(owner).map(|symbol| &mut symbol.kind)
        {
            *stored = values;
        }
    }

    /// Reports a row named like an earlier row of the same definition, or
    /// (a struct's field) like the struct itself or any other struct or enum
    /// of the program (`B2C-E0210`): inside the struct, C++ would find the
    /// field by that name instead of the type.
    fn check_row_name(
        &mut self,
        name: &str,
        sym: &SymbolId,
        owner: &str,
        row: &str,
        seen: &mut BTreeMap<String, SymbolId>,
        location: &Location,
    ) {
        let (owner_kind, rows) = if row == "field" {
            ("struct", "fields")
        } else {
            ("enum", "values")
        };
        if let Some(other) = seen.get(name).cloned() {
            let message = format!(
                "Two {rows} of {} are called {}. Rename one of them.",
                quoted(owner),
                quoted(name)
            );
            self.error_with_decl(
                codes::DUPLICATE_NAME,
                location.clone(),
                message,
                &other,
                &format!("the other {row}"),
            );
        } else {
            seen.insert(name.to_owned(), sym.clone());
            if row == "field" && name == owner {
                let message = format!(
                    "A field can't have the name of its {owner_kind} ({}). Rename the field.",
                    quoted(owner)
                );
                self.diags.error(codes::DUPLICATE_NAME, location.clone(), message);
            } else if row == "field"
                && let Some((other, what)) = self.user_type_named(name)
            {
                let message = format!(
                    "A field can't have the name of a struct or an enum: inside {}, C++ would find the field \
                     {} instead of the {what}. Rename the field.",
                    quoted(owner),
                    quoted(name)
                );
                self.error_with_decl(
                    codes::DUPLICATE_NAME,
                    location.clone(),
                    message,
                    &other,
                    &format!("the {what}"),
                );
            }
        }
    }

    /// A field's type: as a variable's type is looked up (types.md §2.2),
    /// without the requirements of sets and maps, which need every struct's
    /// flags.
    fn field_type(&mut self, term: &TypeTerm, location: &Location) -> Type {
        self.lookup_term(term, location, TypeUse::Variable)
    }

    /// Checks the requirements of the field types (`B2C-E0313`), once the
    /// structs' flags are settled.
    pub(super) fn check_field_requirements(&mut self, pending: Vec<PendingField>) {
        for field in pending {
            self.check_requirements(&field.ty, &field.location);
        }
    }

    // --- Items ------------------------------------------------------------------

    /// A struct or an enum definition (`type.struct`, `type.enum`): its item,
    /// when the pre-pass declared it. Its rows and ticks are in the symbol
    /// table; a struct's default values (W2's package C2, w2.md §1.9) are in
    /// its item.
    pub(super) fn type_definition(&mut self, block: &Block) -> Option<Item> {
        let symbol = access::decl_field(block, "NAME").ok()?.sym.clone();
        let declared = self.symbols.get(&symbol)?;
        if declared.origin.block != block.id {
            return None;
        }
        let is_struct = match declared.kind {
            SymbolKind::Struct { .. } => true,
            SymbolKind::Enum { .. } => false,
            _ => return None,
        };
        self.begin_item(ItemCtx::Main);
        self.record_block(&block.id);
        self.add_headers(block, true);
        let kind = if is_struct {
            let defaults = self.struct_defaults(block, &symbol);
            ItemKind::Struct(StructDef { symbol, defaults })
        } else {
            ItemKind::Enum(EnumDef { symbol })
        };
        Some(Item {
            kind,
            origin: self.origin(&block.id, Part::Whole),
            comment: self.comment(block),
            headers: std::mem::take(&mut self.headers),
            shared: false,
        })
    }

    /// The default values of a struct's fields (`int mark = 0;`), one
    /// optional part per row (`DEFAULT<i>`), each typed by its own row's
    /// field (`member:NAME`). A default sees the module's definitions, as a
    /// parameter's default does; a field without one starts empty.
    fn struct_defaults(&mut self, block: &Block, owner: &SymbolId) -> BTreeMap<SymbolId, Expr> {
        let rows: Vec<Option<SymbolId>> = access::rows(block, "fields")
            .into_iter()
            .map(|row| row.ok().map(|row| row.sym))
            .collect();
        let mut defaults = BTreeMap::new();
        if !(0..rows.len()).any(|i| access::input(block, &format!("DEFAULT{i}")).is_some()) {
            return defaults;
        }
        // A row whose field is not the struct's (damaged, or a name taken
        // twice) types its default as an error, which is not reported again.
        let member = |sym: &Option<SymbolId>| {
            sym.as_ref().and_then(|sym| {
                self.symbols.get(sym).and_then(|symbol| match &symbol.kind {
                    SymbolKind::Field { owner: of } if of == owner => Some((sym.clone(), symbol.ty.clone())),
                    _ => None,
                })
            })
        };
        let members: Vec<Option<(SymbolId, Type)>> = rows.iter().map(member).collect();
        let mut typed = self.typed(self.block_sig(&block.block_type));
        typed.field("NAME", PartType::Known(Type::Named(owner.clone())));
        typed.rows(
            "NAME",
            members
                .iter()
                .map(|member| member.as_ref().map_or(Type::Error, |(_, ty)| ty.clone()))
                .collect(),
        );
        typed.count("DEFAULT", rows.len());
        for i in 0..rows.len() {
            let part = format!("DEFAULT{i}");
            self.record_input(&block.id, &part);
            self.typed_input(block, &mut typed, &part, false);
        }
        let typing = self.type_parts(block, &mut typed, None);
        let fields: Vec<(String, SymbolId)> = members
            .iter()
            .flatten()
            .map(|(sym, _)| (self.raw_name(sym), sym.clone()))
            .collect();
        for (i, member) in members.into_iter().enumerate() {
            let part = format!("DEFAULT{i}");
            let Some(value) = typed.take(&part) else {
                continue;
            };
            let Some((sym, _)) = member else {
                continue;
            };
            self.check_field_default_names(&value, owner, &fields);
            let subject = phrase(format!("The default value of {}", self.member_name(&sym)));
            self.report_fit(&typed, &typing, &part, &value, &subject);
            defaults.insert(sym, value);
        }
        defaults
    }

    /// Reports a global or function that a field's default value names and
    /// that is called like a field of the struct `owner`: inside a struct
    /// C++ finds the field by that name, also in default values (a complete-
    /// class context), so the value would read the field, or fail to call it.
    fn check_field_default_names(&mut self, value: &Expr, owner: &SymbolId, fields: &[(String, SymbolId)]) {
        for (sym, called, location) in self.names_looked_up(value) {
            let name = self.raw_name(&sym);
            let Some((_, field)) = fields.iter().find(|(field, _)| *field == name) else {
                continue;
            };
            let (uses, other) = if called {
                ("calls the function", "function")
            } else {
                ("uses the global", "global")
            };
            let message = format!(
                "This {uses} {name}, but {} has a field called {name}, and inside a struct C++ finds the field by \
                 that name, also in default values. Rename the field or the {other}.",
                quoted(&self.raw_name(owner)),
                name = quoted(&name)
            );
            let field = field.clone();
            self.error_with_decl(codes::NAME_HIDDEN, location, message, &field, "the field");
        }
    }

    // --- Blocks -----------------------------------------------------------------

    /// *set … of … to*.
    pub(super) fn type_statement(&mut self, block: &Block) -> Option<StmtKind> {
        if block.block_type != "type.set_field" {
            return None;
        }
        let mut typed = self.typed(self.block_sig(&block.block_type));
        // In catalog order: the field, then the variable.
        let member = self.chosen_member(block, "MEMBER", "Choose a field in this block.");
        let location = self.loc(&block.id, field("VAR"));
        let target = if let Ok(sym) = access::ref_field(block, "VAR") {
            let ty = self.write_target(sym, &location).unwrap_or(Type::Error);
            typed.field("VAR", PartType::Known(ty));
            Some(sym.clone())
        } else {
            typed.field("VAR", PartType::Empty);
            self.incomplete(location, "Choose a struct variable in this block.");
            None
        };
        self.typed_input(block, &mut typed, "VALUE", true);
        let typing = self.type_parts(block, &mut typed, None);
        let value = typed.take("VALUE").unwrap_or_else(|| {
            error_expr(self.origin(
                &block.id,
                Part::Input {
                    name: String::from("VALUE"),
                },
            ))
        });
        let (target, member) = (target?, member?);
        self.report_field_fit(block, &typed, &typing, "VAR");
        let subject = phrase(format!(
            "The {} field of {}",
            self.member_name(&member),
            self.name_of(&target)
        ));
        self.report_fit(&typed, &typing, "VALUE", &value, &subject);
        Some(StmtKind::SetField {
            target,
            field: member,
            value,
        })
    }

    /// *new … with*, *… of …*, an enum value.
    pub(super) fn type_value(&mut self, block: &Block, origin: Origin) -> Expr {
        match block.block_type.as_str() {
            "type.new" => self.new_struct(block, origin),
            "type.field" => self.field_of(block, origin),
            "type.enum_value" => self.enum_value(block, origin),
            _ => error_expr(origin),
        }
    }

    /// The symbol chosen in a member field, or `None` after reporting that
    /// none is.
    fn chosen_member(&mut self, block: &Block, name: &str, missing: &str) -> Option<SymbolId> {
        if let Ok(sym) = access::ref_field(block, name) {
            return Some(sym.clone());
        }
        let location = self.loc(&block.id, field(name));
        self.incomplete(location, missing);
        None
    }

    /// A member's name for messages, in backticks.
    fn member_name(&self, sym: &SymbolId) -> String {
        self.symbols
            .get(sym)
            .map_or_else(|| self.name_of(sym), |symbol| quoted(symbol.name.as_str()))
    }

    /// In C++17, an empty part of *new … with* before a filled one whose
    /// field has a default value (w2.md §1.5, §1.9): C++17 has no
    /// designated initialisers, so the part is printed as `{}`, which
    /// value-initialises the field and skips its default (`P{{}, 5}` makes
    /// `x` 0 where `int x = 10;`), while C++20 leaves the field out and
    /// keeps it. An empty part after the last filled one keeps it in both.
    fn check_cpp17_gaps(&mut self, block: &Block, parts: &[Option<Expr>], fields: &[SymbolId]) {
        if self.document.project.language.standard >= CppStandard::Cpp20 || !self.checking() {
            return;
        }
        let Some(last) = parts.iter().rposition(Option::is_some) else {
            return;
        };
        for (i, member) in fields.iter().enumerate().take(last) {
            if parts.get(i).is_some_and(Option::is_none) && self.defaulted_fields.contains(member) {
                let message = format!(
                    "In C++17, an empty part before a filled one sets its field to an empty value, so {} would not \
                     get its default value. Fill in this part, or choose C++20 in the project's settings.",
                    self.member_name(member)
                );
                let location = self.loc(
                    &block.id,
                    Part::Input {
                        name: format!("FIELD{i}"),
                    },
                );
                self.incomplete(location, message);
            }
        }
    }

    /// *new [S] with (a) (b) …*: a struct value from one part per field, in
    /// row order; an empty part leaves its field value-initialised, or in
    /// C++20 at its default value, if it has one (in C++17 that is
    /// `B2C-E0430` before a filled part). A number of parts other than the
    /// struct's fields is `B2C-E0306`.
    fn new_struct(&mut self, block: &Block, origin: Origin) -> Expr {
        let mut typed = self.typed(self.block_sig(&block.block_type));
        let location = self.loc(&block.id, field("TYPE"));
        let ty = match block.fields.get("TYPE") {
            None => {
                self.incomplete(location, "Choose a struct in this block.");
                None
            }
            Some(_) => Some(self.type_setting(block, "TYPE", "int", TypeUse::Variable)),
        };
        typed.field("TYPE", ty.clone().map_or(PartType::Empty, PartType::Known));
        let count = access::extra_count(block, "fieldCount", 0, 0);
        typed.count("FIELD", count);
        for i in 0..count {
            self.typed_input(block, &mut typed, &format!("FIELD{i}"), false);
        }
        let typing = self.type_parts(block, &mut typed, None);
        let parts: Vec<Option<Expr>> = (0..count).map(|i| typed.take(&format!("FIELD{i}"))).collect();
        let Some(ty) = ty else {
            return error_expr(origin);
        };
        self.report_field_fit(block, &typed, &typing, "TYPE");
        let Type::Named(id) = &ty else {
            return error_expr(origin);
        };
        let Some(SymbolKind::Struct { fields, .. }) = self.symbols.get(id).map(|symbol| symbol.kind.clone())
        else {
            return error_expr(origin);
        };
        let struct_name = self.type_name(&ty);
        if parts.len() != fields.len() && self.checking() {
            let have = match fields.len() {
                1 => String::from("1 field"),
                n => format!("{n} fields"),
            };
            let given = match parts.len() {
                1 => String::from("1 part"),
                n => format!("{n} parts"),
            };
            let message = format!(
                "{struct_name} has {have}, but this block has {given}. Choose the struct again so that the block \
                 has one part for each field."
            );
            self.diags
                .error(codes::ARGUMENT_COUNT, origin.location(), message);
        }
        for (i, (part, member)) in parts.iter().zip(&fields).enumerate() {
            if let Some(value) = part {
                let subject = phrase(format!("The {} field of {struct_name}", self.member_name(member)));
                self.report_fit(&typed, &typing, &format!("FIELD{i}"), value, &subject);
            }
        }
        self.check_cpp17_gaps(block, &parts, &fields);
        Expr {
            kind: ExprKind::New {
                ty: id.clone(),
                fields: parts,
            },
            ty: typing.output.unwrap_or(Type::Error),
            origin,
        }
    }

    /// *[field] of (obj)*: `obj.field`.
    fn field_of(&mut self, block: &Block, origin: Origin) -> Expr {
        let mut typed = self.typed(self.block_sig(&block.block_type));
        let member = self.chosen_member(block, "MEMBER", "Choose a field in this block.");
        self.typed_input(block, &mut typed, "OBJ", true);
        let typing = self.type_parts(block, &mut typed, None);
        let object = typed.take("OBJ");
        let (Some(object), Some(member)) = (object, member) else {
            return error_expr(origin);
        };
        let subject = Subject::Phrase {
            text: String::from("The value whose field is read"),
            stores: false,
        };
        self.report_fit(&typed, &typing, "OBJ", &object, &subject);
        Expr {
            kind: ExprKind::Field {
                object: Box::new(object),
                field: member,
            },
            ty: typing.output.unwrap_or(Type::Error),
            origin,
        }
    }

    /// *[enum] [value]*: `Action::Deposit`. `ENUM` lists enum definitions
    /// (w2.md §2.3); a variable of an enum type there is `B2C-E0207`.
    fn enum_value(&mut self, block: &Block, origin: Origin) -> Expr {
        let mut typed = self.typed(self.block_sig(&block.block_type));
        let location = self.loc(&block.id, field("ENUM"));
        let owner = if let Ok(sym) = access::ref_field(block, "ENUM") {
            self.enum_definition(sym, &location)
        } else {
            self.incomplete(location, "Choose an enum in this block.");
            None
        };
        typed.field(
            "ENUM",
            owner.as_ref().map_or(PartType::Empty, |owner| {
                PartType::Known(Type::Named(owner.clone()))
            }),
        );
        let value = self.chosen_member(block, "VALUE", "Choose a value of the enum in this block.");
        let typing = self.type_parts(block, &mut typed, None);
        let (Some(_), Some(value)) = (owner, value) else {
            return error_expr(origin);
        };
        Expr {
            kind: ExprKind::Enumerator(value),
            ty: typing.output.unwrap_or(Type::Error),
            origin,
        }
    }

    /// The enum `sym` names, or `None` after reporting a symbol that is not
    /// an enum definition (`B2C-E0207`) or does not exist (`B2C-E0201`).
    fn enum_definition(&mut self, sym: &SymbolId, location: &Location) -> Option<SymbolId> {
        match self.symbols.get(sym).map(|symbol| &symbol.kind) {
            Some(SymbolKind::Enum { .. }) => return Some(sym.clone()),
            Some(SymbolKind::Struct { .. }) => {
                let message = format!(
                    "{} is a struct, not an enum: choose an enum here.",
                    self.name_of(sym)
                );
                self.diags.error(codes::WRONG_KIND, location.clone(), message);
                return None;
            }
            _ => {}
        }
        if self.decls.contains_key(sym) {
            let message = format!(
                "{} is not an enum: choose an enum here, not a variable or value of one.",
                self.name_of(sym)
            );
            self.diags.error(codes::WRONG_KIND, location.clone(), message);
        } else {
            self.diags.error(
                codes::NOT_DECLARED,
                location.clone(),
                "This block refers to an enum that doesn't exist (any more). Choose another enum.",
            );
        }
        None
    }
}
