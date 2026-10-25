//! User types and the requirements of types (types.md §1.2, §1.6):
//! references to structs and enums in type terms, the comparable keys of
//! sets, priority queues and maps (`B2C-E0313`), printable values
//! (`B2C-E0314`), and the *comparable* and *printable* ticks of structs.

use std::collections::BTreeMap;

use b2c_ir::diag::{Location, Part};
use b2c_ir::ids::SymbolId;
use b2c_ir::sast::{Expr, SymbolKind, SymbolTable};
use b2c_ir::types::{LibType, Type, TypeClass, TypeFacts, UserKind, UserType};

use super::Lowerer;
use crate::codes;
use crate::collect::DeclKind;
use crate::messages::{a_type, article, culprit, lib_noun, quoted};

/// The field of a struct's block that holds its *comparable* tick, where
/// `B2C-E0313` about the tick is reported.
const COMPARABLE_TICK: &str = "COMPARABLE";
/// The field of a struct's block that holds its *printable* tick.
const PRINTABLE_TICK: &str = "PRINTABLE";

/// What a value is used for when it must be printable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Output {
    /// *print* (and *write to file*).
    Printed,
    /// *join*.
    Joined,
}

/// Facts that answer with the flags being worked out by
/// [`Lowerer::settle_ticks`] instead of those in the table.
struct Settling<'a> {
    symbols: &'a SymbolTable,
    flags: &'a BTreeMap<SymbolId, (bool, bool)>,
}

impl TypeFacts for Settling<'_> {
    fn user_type(&self, id: &SymbolId) -> Option<UserType<'_>> {
        let user = self.symbols.user_type(id)?;
        if user.kind == UserKind::Enum {
            return Some(user);
        }
        let (comparable, printable) = self.flags.get(id).copied().unwrap_or((false, false));
        Some(UserType {
            comparable,
            printable,
            ..user
        })
    }
}

/// The type a struct's field holds, or `Error`.
fn field_type(symbols: &SymbolTable, field: &SymbolId) -> Type {
    symbols.get(field).map_or(Type::Error, |symbol| symbol.ty.clone())
}

impl Lowerer<'_> {
    /// Checks a reference to a user type in a type term: it must name a
    /// struct or an enum (`B2C-E0201` for a symbol that does not exist,
    /// `B2C-E0207` for one that is not a type). Returns whether it does.
    pub(super) fn check_type_reference(&mut self, id: &SymbolId, location: &Location) -> bool {
        let symbol = self.symbols.get(id);
        if symbol
            .is_some_and(|symbol| matches!(symbol.kind, SymbolKind::Struct { .. } | SymbolKind::Enum { .. }))
        {
            return true;
        }
        if let Some(decl) = self.decls.get(id) {
            let kind = match decl.kind {
                DeclKind::Variable => "a variable",
                DeclKind::Parameter => "an input of a function",
                DeclKind::Counter => "the counter of a 'for' loop",
                DeclKind::Function => "a function",
            };
            let message = format!(
                "{} is {kind}, not a type: choose a struct or an enum here.",
                quoted(&decl.name)
            );
            self.diags.error(codes::WRONG_KIND, location.clone(), message);
            return false;
        }
        if let Some(symbol) = symbol {
            let message = format!(
                "{} is not a type: choose a struct or an enum here.",
                quoted(symbol.name.as_str())
            );
            self.diags.error(codes::WRONG_KIND, location.clone(), message);
            return false;
        }
        self.diags.error(
            codes::NOT_DECLARED,
            location.clone(),
            "This type refers to a struct or enum that doesn't exist (any more). Choose another type.",
        );
        false
    }

    /// Reports the first set or priority queue whose items, or map whose
    /// keys, are not comparable in a type where it is written (`B2C-E0313`):
    /// such a variable can be declared, but every insert, lookup or
    /// ordering fails to compile.
    pub(super) fn check_requirements(&mut self, ty: &Type, location: &Location) {
        let mut stack = vec![ty];
        while let Some(ty) = stack.pop() {
            let Type::Lib(app) = ty else {
                continue;
            };
            let what = match app.ty() {
                LibType::Set => "A set keeps its items in order, so they",
                LibType::PriorityQueue => "A priority queue keeps its items in order, so they",
                LibType::Map => "A map keeps its keys in order, so they",
                _ => {
                    stack.extend(app.args().iter().rev());
                    continue;
                }
            };
            let Some(key) = app.args().first() else {
                continue;
            };
            if key.is_in(TypeClass::Comparable, &self.symbols) != Some(false) {
                stack.extend(app.args().iter().rev());
                continue;
            }
            let fix = if app.ty() == LibType::Map {
                "choose another type for the keys"
            } else {
                "use a list"
            };
            let hint = match culprit(key, TypeClass::Comparable, &self.symbols) {
                Some(name) => format!("Tick 'comparable' on {}, or {fix}.", quoted(name)),
                None => format!("Choose another type, or {fix}."),
            };
            let message = format!(
                "{what} must be comparable, and {} is not. {hint}",
                self.type_name(key)
            );
            self.diags.error(codes::NOT_COMPARABLE, location.clone(), message);
            return;
        }
    }

    /// Reports a value that cannot be printed or joined in one go
    /// (`B2C-E0314`): a container, a stream, an error, or a struct not
    /// ticked *printable*.
    pub(super) fn check_printable(&mut self, value: &Expr, output: Output) {
        if !self.checking() || value.ty.is_in(TypeClass::Printable, &self.symbols) != Some(false) {
            return;
        }
        let verb = match output {
            Output::Printed => "printed",
            Output::Joined => "joined into text",
        };
        let message = if let Type::Lib(app) = &value.ty {
            let noun = lib_noun(app.ty());
            let one_go = format!("{} {noun} can't be {verb}", capitalised(article(noun)));
            match app.ty() {
                LibType::Vector | LibType::Deque | LibType::Set => {
                    format!("{one_go} in one go. Use 'for each' to go through its items.")
                }
                LibType::Map => {
                    format!("{one_go} in one go. Use 'for each key, value' to go through its items.")
                }
                LibType::Queue | LibType::Stack | LibType::PriorityQueue => {
                    format!("{one_go} in one go. Take its items out one by one instead.")
                }
                _ => format!("{one_go}."),
            }
        } else {
            let name = self.type_name(&value.ty);
            match culprit(&value.ty, TypeClass::Printable, &self.symbols) {
                Some(user) => format!(
                    "{name} can't be {verb}. Tick 'printable' on {}, or use its fields one by one.",
                    quoted(user)
                ),
                None => format!(
                    "{} can't be {verb}.",
                    capitalised(&a_type(&value.ty, &self.symbols))
                ),
            }
        };
        self.diags
            .error(codes::NOT_PRINTABLE, value.origin.location(), message);
    }

    /// Works out the effective *comparable* and *printable* flags of every
    /// struct (types.md §1.6) and stores them in the symbol table, which
    /// the type rules read through [`TypeFacts`]. A struct declares the
    /// ticks it was given; a tick counts only when every field's type
    /// qualifies, and a struct reached again while its flags are worked out
    /// (a cycle through containers) counts as neither. A tick whose fields
    /// do not support it is `B2C-E0313` or `B2C-E0314` at the tick, naming
    /// the first such field.
    ///
    /// The flags are the least fixed point of that rule, found by rounds
    /// that only ever turn a flag on: at most twice as many rounds as there
    /// are structs.
    pub(super) fn settle_ticks(&mut self) {
        let structs: Vec<(SymbolId, Vec<SymbolId>, bool, bool)> = self
            .symbols
            .symbols
            .iter()
            .filter_map(|(id, symbol)| match &symbol.kind {
                SymbolKind::Struct {
                    fields,
                    comparable,
                    printable,
                } => Some((id.clone(), fields.clone(), *comparable, *printable)),
                _ => None,
            })
            .collect();
        if structs.is_empty() {
            return;
        }
        let holds = |facts: &dyn TypeFacts, fields: &[SymbolId], class: TypeClass| {
            fields
                .iter()
                .all(|field| field_type(&self.symbols, field).is_in(class, facts) != Some(false))
        };
        let mut flags: BTreeMap<SymbolId, (bool, bool)> = structs
            .iter()
            .map(|(id, ..)| (id.clone(), (false, false)))
            .collect();
        loop {
            let facts = Settling {
                symbols: &self.symbols,
                flags: &flags,
            };
            let next: BTreeMap<SymbolId, (bool, bool)> = structs
                .iter()
                .map(|(id, fields, comparable, printable)| {
                    let comparable = *comparable && holds(&facts, fields, TypeClass::Comparable);
                    let printable = *printable && holds(&facts, fields, TypeClass::Printable);
                    (id.clone(), (comparable, printable))
                })
                .collect();
            if next == flags {
                break;
            }
            flags = next;
        }
        for (id, fields, comparable, printable) in &structs {
            let (effective_comparable, effective_printable) =
                flags.get(id).copied().unwrap_or((false, false));
            if *comparable && !effective_comparable {
                self.report_tick(id, fields, &flags, TypeClass::Comparable);
            }
            if *printable && !effective_printable {
                self.report_tick(id, fields, &flags, TypeClass::Printable);
            }
        }
        for (id, (comparable, printable)) in flags {
            if let Some(SymbolKind::Struct {
                comparable: stored_comparable,
                printable: stored_printable,
                ..
            }) = self.symbols.symbols.get_mut(&id).map(|symbol| &mut symbol.kind)
            {
                *stored_comparable = comparable;
                *stored_printable = printable;
            }
        }
    }

    /// Reports a tick its struct's fields do not support.
    fn report_tick(
        &mut self,
        id: &SymbolId,
        fields: &[SymbolId],
        flags: &BTreeMap<SymbolId, (bool, bool)>,
        class: TypeClass,
    ) {
        let facts = Settling {
            symbols: &self.symbols,
            flags,
        };
        let Some((field, ty)) = fields
            .iter()
            .map(|field| (field, field_type(&self.symbols, field)))
            .find(|(_, ty)| ty.is_in(class, &facts) == Some(false))
        else {
            return;
        };
        let (tick, code, adjective, part) = match class {
            TypeClass::Comparable => (
                "comparable",
                codes::NOT_COMPARABLE,
                "is not comparable",
                COMPARABLE_TICK,
            ),
            _ => (
                "printable",
                codes::NOT_PRINTABLE,
                "can't be printed",
                PRINTABLE_TICK,
            ),
        };
        let name = |sym: &SymbolId| {
            self.symbols
                .get(sym)
                .map(|symbol| quoted(symbol.name.as_str()))
                .unwrap_or_default()
        };
        let held = match &ty {
            Type::Named(owner) => name(owner),
            _ => a_type(&ty, &facts),
        };
        let hint = match culprit(&ty, class, &facts) {
            Some(user) => format!("Tick '{tick}' on {} too, or untick it here.", quoted(user)),
            None => String::from("Untick it here."),
        };
        let message = format!(
            "{} is ticked '{tick}', but its field {} holds {held}, which {adjective}. {hint}",
            name(id),
            name(field)
        );
        let Some(origin) = self.symbols.get(id).map(|symbol| symbol.origin.clone()) else {
            return;
        };
        let location = Location {
            module: Some(origin.module),
            block: Some(origin.block),
            part: Part::Field {
                name: part.to_owned(),
            },
        };
        self.diags.error(code, location, message);
    }
}

/// The text with its first letter in upper case.
fn capitalised(text: &str) -> String {
    let mut chars = text.chars();
    chars
        .next()
        .map(|first| first.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

#[cfg(test)]
pub(super) mod tests {
    use std::collections::BTreeSet;

    use b2c_ir::ids::{BlockId, ModuleId};
    use b2c_ir::sast::{Origin, Symbol};
    use b2c_ir::sig::SigTable;
    use b2c_ir::text::Ident;
    use b2c_model::Document;

    use super::*;

    /// A document with one empty module.
    pub(in crate::lower) fn document() -> Document {
        serde_json::from_value(serde_json::json!({
            "format": "blocks2cpp/project",
            "formatVersion": 1,
            "generator": {"app": "0.1.0", "catalog": "1.0.0"},
            "project": {"id": "prj_test", "name": "Test", "language": {"standard": "c++20"}},
            "modules": [{"id": "mod_0", "name": "main", "workspace": {"blocks": []}}]
        }))
        .expect("document")
    }

    pub(in crate::lower) fn module() -> ModuleId {
        ModuleId::new("mod_0").expect("id")
    }

    pub(in crate::lower) fn id(name: &str) -> SymbolId {
        SymbolId::new(&format!("sym_{name}")).expect("id")
    }

    pub(in crate::lower) fn named(name: &str) -> Type {
        Type::Named(id(name))
    }

    /// Adds a symbol declared by the block `b_<name>`.
    pub(in crate::lower) fn add(symbols: &mut SymbolTable, name: &str, kind: SymbolKind, ty: Type) {
        let origin = Origin::whole(module(), BlockId::new(&format!("b_{name}")).expect("id"));
        let symbol = Symbol {
            name: Ident::new(name).expect("name"),
            kind,
            ty,
            origin,
        };
        symbols.symbols.insert(id(name), symbol);
    }

    /// Adds a struct with these fields (name, type), ticked as given.
    pub(in crate::lower) fn add_struct(
        symbols: &mut SymbolTable,
        name: &str,
        fields: &[(&str, Type)],
        comparable: bool,
        printable: bool,
    ) {
        for (field, ty) in fields {
            add(symbols, field, SymbolKind::Field { owner: id(name) }, ty.clone());
        }
        let kind = SymbolKind::Struct {
            fields: fields.iter().map(|(field, _)| id(field)).collect(),
            comparable,
            printable,
        };
        add(symbols, name, kind, named(name));
    }

    fn list(of: Type) -> Type {
        Type::lib(LibType::Vector, vec![of]).expect("type")
    }

    #[test]
    fn ticks_count_only_when_the_fields_qualify() {
        let document = document();
        let sigs = SigTable::empty();
        let unchecked = BTreeSet::new();
        let mut lowerer = Lowerer::new(&document, module(), &sigs, &unchecked);
        let symbols = &mut lowerer.symbols;
        add_struct(
            symbols,
            "Point",
            &[("x", Type::Int), ("y", Type::Int)],
            true,
            true,
        );
        add_struct(symbols, "Player", &[("name", Type::String)], false, false);
        add_struct(symbols, "Team", &[("members", list(named("Player")))], true, true);
        add_struct(symbols, "Squad", &[("lead", named("Player"))], true, false);
        add_struct(symbols, "Node", &[("next", list(named("Node")))], true, false);
        add_struct(symbols, "Path", &[("points", list(named("Point")))], true, false);
        lowerer.settle_ticks();

        let flags = |name: &str| {
            let user = lowerer.symbols.user_type(&id(name)).expect("struct");
            (user.comparable, user.printable)
        };
        assert_eq!(flags("Point"), (true, true));
        assert_eq!(flags("Player"), (false, false));
        assert_eq!(flags("Team"), (false, false));
        assert_eq!(flags("Squad"), (false, false));
        // A struct reached again while its flags are worked out counts as
        // not comparable.
        assert_eq!(flags("Node"), (false, false));
        assert_eq!(flags("Path"), (true, false));

        let found: Vec<(&str, &str)> = lowerer
            .diags
            .list
            .iter()
            .map(|d| (d.code.0.as_str(), d.message.as_str()))
            .collect();
        assert_eq!(
            found,
            [
                (
                    codes::NOT_COMPARABLE,
                    "`Node` is ticked 'comparable', but its field `next` holds a list of Node, which is not \
                     comparable. Tick 'comparable' on `Node` too, or untick it here."
                ),
                (
                    codes::NOT_COMPARABLE,
                    "`Squad` is ticked 'comparable', but its field `lead` holds `Player`, which is not \
                     comparable. Tick 'comparable' on `Player` too, or untick it here."
                ),
                (
                    codes::NOT_COMPARABLE,
                    "`Team` is ticked 'comparable', but its field `members` holds a list of Player, which is not \
                     comparable. Tick 'comparable' on `Player` too, or untick it here."
                ),
                (
                    codes::NOT_PRINTABLE,
                    "`Team` is ticked 'printable', but its field `members` holds a list of Player, which can't \
                     be printed. Untick it here."
                ),
            ]
        );
        let tick = &lowerer.diags.list[0].primary;
        assert_eq!(tick.block.as_ref().map(BlockId::as_str), Some("b_Node"));
        assert_eq!(
            tick.part,
            Part::Field {
                name: String::from(COMPARABLE_TICK)
            }
        );
    }

    #[test]
    fn requirements_name_the_struct_to_tick() {
        let document = document();
        let sigs = SigTable::empty();
        let unchecked = BTreeSet::new();
        let mut lowerer = Lowerer::new(&document, module(), &sigs, &unchecked);
        add_struct(&mut lowerer.symbols, "Point", &[("x", Type::Int)], false, false);
        let location = Location::project();
        let set = Type::lib(LibType::Set, vec![named("Point")]).expect("type");
        lowerer.check_requirements(&set, &location);
        let map = Type::lib(LibType::Map, vec![list(named("Point")), Type::Int]).expect("type");
        lowerer.check_requirements(&map, &location);
        let value = Expr {
            kind: b2c_ir::sast::ExprKind::Var(id("p")),
            ty: named("Point"),
            origin: Origin::whole(module(), BlockId::new("b").expect("id")),
        };
        lowerer.check_printable(&value, Output::Printed);
        let messages: Vec<&str> = lowerer.diags.list.iter().map(|d| d.message.as_str()).collect();
        assert_eq!(
            messages,
            [
                "A set keeps its items in order, so they must be comparable, and `Point` is not. Tick \
                 'comparable' on `Point`, or use a list.",
                "A map keeps its keys in order, so they must be comparable, and a list of Point is not. Tick \
                 'comparable' on `Point`, or choose another type for the keys.",
                "`Point` can't be printed. Tick 'printable' on `Point`, or use its fields one by one.",
            ]
        );
    }
}
