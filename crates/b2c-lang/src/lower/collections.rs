//! Lowering the collection blocks of W2 (`docs/plans/m3-contracts/w2.md`
//! §1.2): *add*, *item*, *set item*, *length*, *is empty*, *list of*,
//! *contains*, *clear*, *remove item at*, *remove … from*, *next item*,
//! *remove next*, *sort*, *for each* and *for each key value*.
//!
//! Each block is typed by its catalog signature (ADR-0018, slots.md §3): the
//! collection in `LIST` (or `MAP`) binds its type variable, and the other
//! parts are checked against the patterns it resolves (`elem(C)`,
//! `index(C)`, `key(C)`). A block that changes its collection reads `LIST`
//! as a variable it changes, so a constant, a read-only parameter or a
//! read-only loop item there is `B2C-E0308`.

use b2c_ir::diag::Part;
use b2c_ir::ids::SymbolId;
use b2c_ir::sast::{CollectionChange, Expr, ExprKind, Origin, RangeMode, StmtKind, Symbol, SymbolKind};
use b2c_ir::sig::PartType;
use b2c_ir::types::{LibType, Type, TypeClass};
use b2c_model::Block;

use super::convert::Subject;
use super::expr::error_expr;
use super::typed::Typed;
use super::{Lowerer, field};
use crate::access;
use crate::codes;
use crate::messages::{a_type, culprit, quoted};
use crate::scope::{ITEM_LIST, ListKey};
use crate::slots::BlockTyping;

/// The collection a block names in a field, as its arm reads it.
struct Named {
    /// The chosen variable.
    sym: SymbolId,
    /// Its type; `Error` when it cannot be used here (reported).
    ty: Type,
    /// Where the field is, for the value's origin.
    origin: Origin,
}

impl Named {
    /// The variable as a value of the block (`ExprKind::Var`).
    fn value(&self) -> Expr {
        Expr {
            kind: ExprKind::Var(self.sym.clone()),
            ty: self.ty.clone(),
            origin: self.origin.clone(),
        }
    }
}

/// Whether a type is a map (`std::map<K, V>`).
fn is_map(ty: &Type) -> bool {
    matches!(ty, Type::Lib(app) if app.ty() == LibType::Map)
}

/// Whether a type is a list of `bool` (`std::vector<bool>`), whose items are
/// not real objects: `auto&` cannot bind to them.
fn is_bool_list(ty: &Type) -> bool {
    matches!(ty, Type::Lib(app) if app.ty() == LibType::Vector && app.args() == [Type::Bool])
}

/// A phrase subject for messages ("An item of `scores` needs …").
fn phrase(text: String, stores: bool) -> Subject {
    Subject::Phrase { text, stores }
}

impl Lowerer<'_> {
    /// A collection statement: *add*, *set item*, *for each*, *for each key
    /// value*, *clear*, *remove*, *sort*.
    pub(super) fn collection_statement(&mut self, block: &Block) -> Option<StmtKind> {
        match block.block_type.as_str() {
            "coll.add" => self.add_to(block),
            "coll.set_item" => self.set_item(block),
            "coll.for_each" => self.for_each(block),
            "coll.for_each_pair" => self.for_each_pair(block),
            "coll.clear" => self.plain_change(block, CollectionChange::Clear),
            "coll.remove_next" => self.plain_change(block, CollectionChange::RemoveNext),
            "coll.sort" => {
                let order = [("ascending", false), ("descending", true)];
                let descending = self.choice(block, "ORDER", false, &order);
                self.plain_change(block, CollectionChange::Sort { descending })
            }
            "coll.remove_at" => self.remove_at(block),
            "coll.remove_value" => self.remove_value(block),
            _ => None,
        }
    }

    /// A collection value: *item of*, *length of*, *is empty*, *list of*,
    /// *contains*, *next item of*. `place` is the type the value's place
    /// expects, which only *list of* reads.
    pub(super) fn collection_value(&mut self, block: &Block, origin: Origin, place: Option<&Type>) -> Expr {
        match block.block_type.as_str() {
            "coll.item" => self.item_of(block, origin),
            "coll.length" => self.whole_collection(block, origin, ExprKind::Size),
            "coll.is_empty" => self.whole_collection(block, origin, ExprKind::IsEmpty),
            "coll.next" => self.whole_collection(block, origin, ExprKind::Next),
            "coll.contains" => self.contains(block, origin),
            "coll.list_of" => self.list_of(block, origin, place),
            _ => error_expr(origin),
        }
    }

    // --- Parts ------------------------------------------------------------------

    /// The collection a block names in the field `name` (`LIST`, `MAP`),
    /// recorded as the field's part. `changes` when the block changes it: a
    /// variable that may not be changed is then `B2C-E0308` (its type still
    /// counts). `None` after reporting when nothing is chosen.
    fn named_collection(
        &mut self,
        block: &Block,
        typed: &mut Typed<'_>,
        name: &str,
        changes: bool,
    ) -> Option<Named> {
        let location = self.loc(&block.id, field(name));
        let Ok(sym) = access::ref_field(block, name) else {
            typed.field(name, PartType::Empty);
            let what = if name == "MAP" { "a map" } else { "a collection" };
            self.incomplete(location, format!("Choose {what} in this block."));
            return None;
        };
        let ty = if changes {
            self.write_target(sym, &location)
        } else {
            self.read_variable(sym, &location)
        }
        .unwrap_or(Type::Error);
        typed.field(name, PartType::Known(ty.clone()));
        Some(Named {
            sym: sym.clone(),
            ty,
            origin: self.origin(&block.id, field(name)),
        })
    }

    /// The lowered value of an input part, or a placeholder for one that is
    /// absent (reported by [`Lowerer::typed_input`]).
    fn taken(&self, block: &Block, typed: &mut Typed<'_>, part: &str) -> Expr {
        typed.take(part).unwrap_or_else(|| {
            error_expr(self.origin(
                &block.id,
                Part::Input {
                    name: part.to_owned(),
                },
            ))
        })
    }

    /// The name of a collection for messages, in backticks.
    fn collection_name(&self, named: Option<&Named>) -> String {
        named.map_or_else(
            || String::from("the collection"),
            |named| self.name_of(&named.sym),
        )
    }

    /// What a position or key part of `named` is called in messages: a
    /// map's key, any other collection's position.
    fn position_subject(&self, named: Option<&Named>) -> Subject {
        let name = self.collection_name(named);
        if named.is_some_and(|named| is_map(&named.ty)) {
            phrase(format!("A key of {name}"), true)
        } else {
            phrase(format!("The position in {name}"), false)
        }
    }

    /// What an item part of `named` is called in messages.
    fn item_subject(&self, named: Option<&Named>) -> Subject {
        let name = self.collection_name(named);
        if named.is_some_and(|named| is_map(&named.ty)) {
            phrase(format!("A value of {name}"), true)
        } else {
            phrase(format!("An item of {name}"), true)
        }
    }

    // --- Statements -------------------------------------------------------------

    /// *add (x) to [c]*: `push_back`, `insert` or `push` (w2.md §1.2).
    fn add_to(&mut self, block: &Block) -> Option<StmtKind> {
        let mut typed = self.typed(self.block_sig(&block.block_type));
        let list = self.named_collection(block, &mut typed, "LIST", true);
        self.typed_input(block, &mut typed, "VALUE", true);
        let typing = self.type_parts(block, &mut typed, None);
        let value = self.taken(block, &mut typed, "VALUE");
        let list = list?;
        self.report_field_fit(block, &typed, &typing, "LIST");
        self.check_not_walked(&list);
        let subject = self.item_subject(Some(&list));
        self.report_fit(&typed, &typing, "VALUE", &value, &subject);
        Some(StmtKind::CollectionOp {
            collection: list.sym,
            op: CollectionChange::Add(value),
        })
    }

    /// *set item (i) of [c] to (x)*: `c.at(i) = x;`, `c[k] = x;` for a map.
    fn set_item(&mut self, block: &Block) -> Option<StmtKind> {
        let mut typed = self.typed(self.block_sig(&block.block_type));
        let list = self.named_collection(block, &mut typed, "LIST", true);
        self.typed_input(block, &mut typed, "INDEX", true);
        self.typed_input(block, &mut typed, "VALUE", true);
        let typing = self.type_parts(block, &mut typed, None);
        let index = self.taken(block, &mut typed, "INDEX");
        let value = self.taken(block, &mut typed, "VALUE");
        let list = list?;
        self.report_field_fit(block, &typed, &typing, "LIST");
        let position = self.position_subject(Some(&list));
        self.report_fit(&typed, &typing, "INDEX", &index, &position);
        let item = self.item_subject(Some(&list));
        self.report_fit(&typed, &typing, "VALUE", &value, &item);
        Some(StmtKind::CollectionOp {
            collection: list.sym,
            op: CollectionChange::SetItem { index, value },
        })
    }

    /// A change that takes nothing but the collection: *clear*, *remove
    /// next*, *sort*.
    fn plain_change(&mut self, block: &Block, op: CollectionChange) -> Option<StmtKind> {
        let mut typed = self.typed(self.block_sig(&block.block_type));
        let list = self.named_collection(block, &mut typed, "LIST", true);
        let typing = self.type_parts(block, &mut typed, None);
        let list = list?;
        self.report_field_fit(block, &typed, &typing, "LIST");
        // Sorting moves items, but frees nothing a loop goes through.
        if !matches!(op, CollectionChange::Sort { .. }) {
            self.check_not_walked(&list);
        }
        Some(StmtKind::CollectionOp {
            collection: list.sym,
            op,
        })
    }

    /// *remove item at (i) from [c]*: `b2c::remove_at(c, i);`.
    fn remove_at(&mut self, block: &Block) -> Option<StmtKind> {
        let mut typed = self.typed(self.block_sig(&block.block_type));
        let list = self.named_collection(block, &mut typed, "LIST", true);
        self.typed_input(block, &mut typed, "INDEX", true);
        let typing = self.type_parts(block, &mut typed, None);
        let index = self.taken(block, &mut typed, "INDEX");
        let list = list?;
        self.report_field_fit(block, &typed, &typing, "LIST");
        self.check_not_walked(&list);
        let position = self.position_subject(Some(&list));
        self.report_fit(&typed, &typing, "INDEX", &index, &position);
        Some(StmtKind::CollectionOp {
            collection: list.sym,
            op: CollectionChange::RemoveAt(index),
        })
    }

    /// *remove (x) from [c]*: `c.erase(x);` on a set or a map.
    fn remove_value(&mut self, block: &Block) -> Option<StmtKind> {
        let mut typed = self.typed(self.block_sig(&block.block_type));
        let list = self.named_collection(block, &mut typed, "LIST", true);
        self.typed_input(block, &mut typed, "VALUE", true);
        let typing = self.type_parts(block, &mut typed, None);
        let value = self.taken(block, &mut typed, "VALUE");
        let list = list?;
        self.report_field_fit(block, &typed, &typing, "LIST");
        self.check_not_walked(&list);
        let subject = phrase(String::from("The value to remove"), true);
        self.report_fit(&typed, &typing, "VALUE", &value, &subject);
        Some(StmtKind::CollectionOp {
            collection: list.sym,
            op: CollectionChange::RemoveValue(value),
        })
    }

    /// *for each [x] in (c) [mode]*: a range-`for` over a list, deque, set or
    /// text (w2.md §1.2). A read-only item is a loop variable; a copy or an
    /// editable item is a variable the body may change.
    fn for_each(&mut self, block: &Block) -> Option<StmtKind> {
        let modes = [
            ("read_only", RangeMode::ReadOnly),
            ("copy", RangeMode::Copy),
            ("editable", RangeMode::Editable),
        ];
        let mode = self.choice(block, "MODE", RangeMode::ReadOnly, &modes);
        let mut typed = self.typed(self.block_sig(&block.block_type));
        self.typed_input(block, &mut typed, "LIST", true);
        let typing = self.type_parts(block, &mut typed, None);
        let list = self.taken(block, &mut typed, "LIST");
        self.report_range(&typed, &typing, &list);
        if mode == RangeMode::Editable {
            self.check_editable_range(&list);
        }
        let item_ty = typing.declares.get("ITEM").cloned().unwrap_or(Type::Error);
        let kind = match mode {
            RangeMode::ReadOnly => SymbolKind::LoopVariable,
            RangeMode::Copy | RangeMode::Editable => SymbolKind::Variable { is_const: false },
        };
        let name_loc = self.loc(&block.id, field("ITEM"));
        let Ok(decl) = access::decl_field(block, "ITEM") else {
            self.incomplete(name_loc, "This 'for each' loop needs a name for its item.");
            return None;
        };
        let decl = decl.clone();
        self.scopes.push(ListKey::new(&block.id, ITEM_LIST), false);
        self.declare_loop_item(block, "ITEM", &decl, kind, item_ty);
        // A copy or an editable item is a variable, which the scope query
        // would otherwise count as visible after the loop.
        self.index.record_part_variable(&decl.sym);
        let walked = match &list.kind {
            ExprKind::Var(sym) => Some(sym.clone()),
            _ => None,
        };
        let body = self.walking(walked, |lowerer| lowerer.loop_body(block, true));
        self.scopes.pop();
        Some(StmtKind::RangeFor {
            vars: vec![decl.sym],
            range: list,
            mode,
            body,
        })
    }

    /// How *for each*'s collection fits: a map gets its own hint (types.md
    /// §1.5: a map's items are pairs, which M3 cannot name).
    fn report_range(&mut self, typed: &Typed<'_>, typing: &BlockTyping, list: &Expr) {
        if is_map(&list.ty) && self.checking() {
            let have = match &list.kind {
                ExprKind::Var(sym) => format!("{} is {}", self.name_of(sym), a_type(&list.ty, &self.symbols)),
                _ => format!("This is {}", a_type(&list.ty, &self.symbols)),
            };
            let message = format!(
                "{have}, but this block goes through a list, a deque, a set or text. Use 'for each key, value' \
                 for a map."
            );
            self.diags
                .error(codes::CONVERSION, list.origin.location(), message);
            return;
        }
        let subject = phrase(String::from("The collection to go through"), false);
        self.report_fit(typed, typing, "LIST", list, &subject);
    }

    /// *editable* changes the items of the collection, so its collection must
    /// be a variable that may be changed (`B2C-E0308`), and a list or deque
    /// whose items are real objects (`B2C-E0301`): `auto&` cannot bind to an
    /// item of a list of `bool`, or change an item of a set.
    fn check_editable_range(&mut self, list: &Expr) {
        if !self.checking() || list.ty == Type::Error {
            return;
        }
        let location = list.origin.location();
        let ExprKind::Var(sym) = &list.kind else {
            self.diags.error(
                codes::NOT_ASSIGNABLE,
                location,
                "'editable' changes the items of a variable, so this block needs a variable here, such as a \
                 list. Choose 'read-only' or 'copy' to go through a value.",
            );
            return;
        };
        if let Some(reason) = self.not_changeable(sym) {
            self.diags.error(codes::NOT_ASSIGNABLE, location.clone(), reason);
        }
        let message = if is_bool_list(&list.ty) {
            Some(String::from(
                "The items of a list of true/false values can't be changed one by one in a loop. Choose \
                 'copy', and use 'set item' to change an item.",
            ))
        } else if list.ty.is_in(TypeClass::Sequence, &self.symbols) == Some(false) {
            Some(format!(
                "'editable' can only change the items of a list or a deque, but this is {}. Choose 'read-only' \
                 or 'copy'.",
                a_type(&list.ty, &self.symbols)
            ))
        } else {
            None
        };
        if let Some(message) = message {
            self.diags.error(codes::CONVERSION, location, message);
        }
    }

    /// *for each key [k] value [v] in [m]*: `for (const auto& [k, v] : m)`.
    /// Both names are read-only.
    fn for_each_pair(&mut self, block: &Block) -> Option<StmtKind> {
        let mut typed = self.typed(self.block_sig(&block.block_type));
        let map = self.named_collection(block, &mut typed, "MAP", false);
        let typing = self.type_parts(block, &mut typed, None);
        if map.is_some() {
            self.report_field_fit(block, &typed, &typing, "MAP");
        }
        let mut decls = Vec::new();
        for (part, label) in [("KEY", "key"), ("VALUE", "value")] {
            if let Ok(decl) = access::decl_field(block, part) {
                decls.push((part, decl.clone()));
            } else {
                let location = self.loc(&block.id, field(part));
                let message = format!("This 'for each key, value' loop needs a name for its {label}.");
                self.incomplete(location, message);
            }
        }
        let map = map?;
        let [(_, key), (_, value)] = <[_; 2]>::try_from(decls).ok()?;
        self.scopes.push(ListKey::new(&block.id, ITEM_LIST), false);
        for (part, decl) in [("KEY", &key), ("VALUE", &value)] {
            let ty = typing.declares.get(part).cloned().unwrap_or(Type::Error);
            self.declare_loop_item(block, part, decl, SymbolKind::LoopVariable, ty);
        }
        let body = self.walking(Some(map.sym.clone()), |lowerer| lowerer.loop_body(block, true));
        self.scopes.pop();
        Some(StmtKind::RangeFor {
            vars: vec![key.sym, value.sym],
            range: map.value(),
            mode: RangeMode::ReadOnly,
            body,
        })
    }

    /// Lowers a loop's body with the collection variable it goes through
    /// (if it goes through one) on the walked stack.
    fn walking<T>(&mut self, collection: Option<SymbolId>, body: impl FnOnce(&mut Self) -> T) -> T {
        let pushed = collection.is_some();
        self.walked.extend(collection);
        let lowered = body(self);
        if pushed {
            self.walked.pop();
        }
        lowered
    }

    /// Reports a change that adds or removes items of a collection that a
    /// *for each* around the block goes through (`B2C-E0308`): adding to a
    /// list may move its items, and removing frees the item the loop is at,
    /// so the loop would read freed memory (undefined behaviour). Setting
    /// an item or sorting keeps every item where the loop can reach it.
    fn check_not_walked(&mut self, list: &Named) {
        if !self.checking() || !self.walked.contains(&list.sym) {
            return;
        }
        let message = format!(
            "{} can't gain or lose items inside a 'for each' loop that goes through it: the loop would lose \
             its place. Collect the changes in another collection and make them after the loop.",
            self.name_of(&list.sym)
        );
        self.diags
            .error(codes::NOT_ASSIGNABLE, list.origin.location(), message);
    }

    /// Declares a name a loop over items gives its body, in the loop's item
    /// scope.
    pub(super) fn declare_loop_item(
        &mut self,
        block: &Block,
        part: &str,
        decl: &b2c_model::SymbolDecl,
        kind: SymbolKind,
        ty: Type,
    ) {
        let location = self.loc(&block.id, field(part));
        let ident = self.ident(&decl.name, &location, false);
        self.check_new_name(&decl.name, &location);
        let symbol = Symbol {
            name: ident,
            kind,
            ty,
            origin: self.origin(&block.id, field(part)),
        };
        if self.insert_symbol(&decl.sym, &decl.name, symbol, location) {
            self.scopes.declare(&decl.name, &decl.sym);
        }
    }

    // --- Values -----------------------------------------------------------------

    /// *item (i) of [c]*: `c.at(i)` (w2.md §1.2).
    fn item_of(&mut self, block: &Block, origin: Origin) -> Expr {
        let mut typed = self.typed(self.block_sig(&block.block_type));
        let list = self.named_collection(block, &mut typed, "LIST", false);
        self.typed_input(block, &mut typed, "INDEX", true);
        let typing = self.type_parts(block, &mut typed, None);
        let index = self.taken(block, &mut typed, "INDEX");
        let Some(list) = list else {
            return error_expr(origin);
        };
        self.report_field_fit(block, &typed, &typing, "LIST");
        let position = self.position_subject(Some(&list));
        self.report_fit(&typed, &typing, "INDEX", &index, &position);
        Expr {
            kind: ExprKind::Item {
                collection: Box::new(list.value()),
                index: Box::new(index),
            },
            ty: typing.output.unwrap_or(Type::Error),
            origin,
        }
    }

    /// A value of the whole collection: *length of*, *is empty*, *next item
    /// of*.
    fn whole_collection(&mut self, block: &Block, origin: Origin, kind: fn(Box<Expr>) -> ExprKind) -> Expr {
        let mut typed = self.typed(self.block_sig(&block.block_type));
        let list = self.named_collection(block, &mut typed, "LIST", false);
        let typing = self.type_parts(block, &mut typed, None);
        let Some(list) = list else {
            return error_expr(origin);
        };
        self.report_field_fit(block, &typed, &typing, "LIST");
        Expr {
            kind: kind(Box::new(list.value())),
            ty: typing.output.unwrap_or(Type::Error),
            origin,
        }
    }

    /// *[c] contains (x)*: the items of a list or deque must be comparable
    /// to be looked for (`B2C-E0313`); a set's and a map's always are
    /// (types.md §1.6).
    fn contains(&mut self, block: &Block, origin: Origin) -> Expr {
        let mut typed = self.typed(self.block_sig(&block.block_type));
        let list = self.named_collection(block, &mut typed, "LIST", false);
        self.typed_input(block, &mut typed, "VALUE", true);
        let typing = self.type_parts(block, &mut typed, None);
        let value = self.taken(block, &mut typed, "VALUE");
        let Some(list) = list else {
            return error_expr(origin);
        };
        self.report_field_fit(block, &typed, &typing, "LIST");
        let subject = phrase(String::from("The value to look for"), false);
        self.report_fit(&typed, &typing, "VALUE", &value, &subject);
        self.check_items_comparable(block, &list);
        Expr {
            kind: ExprKind::Contains {
                collection: Box::new(list.value()),
                value: Box::new(value),
            },
            ty: typing.output.unwrap_or(Type::Error),
            origin,
        }
    }

    /// `B2C-E0313` for *contains* on a list or deque whose items cannot be
    /// compared: `std::find` needs `==`.
    fn check_items_comparable(&mut self, block: &Block, list: &Named) {
        let Type::Lib(app) = &list.ty else {
            return;
        };
        if !self.checking() || !matches!(app.ty(), LibType::Vector | LibType::Deque) {
            return;
        }
        let Some(item) = app.args().first() else {
            return;
        };
        if item.is_in(TypeClass::Comparable, &self.symbols) != Some(false) {
            return;
        }
        let hint = culprit(item, TypeClass::Comparable, &self.symbols).map_or_else(String::new, |name| {
            format!(" Tick comparable on {}.", quoted(name))
        });
        let message = format!(
            "To look for an item, the items of {} must be comparable, and {} is not.{hint}",
            self.name_of(&list.sym),
            self.type_name(item)
        );
        let location = self.loc(&block.id, field("LIST"));
        self.diags.error(codes::NOT_COMPARABLE, location, message);
    }

    /// *list of (a) (b) …*: a brace list typed by its place (slots.md §3);
    /// each item is checked against the place's element type. Its own type,
    /// and `B2C-E0315` or `B2C-E0301` for its place, come from
    /// [`Lowerer::value_block`].
    fn list_of(&mut self, block: &Block, origin: Origin, place: Option<&Type>) -> Expr {
        let mut typed = self.typed(self.block_sig(&block.block_type));
        let count = access::extra_count(block, "itemCount", 0, 2);
        typed.count("ITEM", count);
        let parts: Vec<String> = (0..count).map(|i| format!("ITEM{i}")).collect();
        for part in &parts {
            self.typed_input(block, &mut typed, part, true);
        }
        let typing = self.type_parts(block, &mut typed, place);
        let subject = phrase(String::from("An item of the list"), true);
        let items: Vec<Expr> = parts
            .iter()
            .map(|part| {
                let item = self.taken(block, &mut typed, part);
                self.report_fit(&typed, &typing, part, &item, &subject);
                item
            })
            .collect();
        Expr {
            kind: ExprKind::ListOf(items),
            ty: typing.output.unwrap_or(Type::Error),
            origin,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use b2c_ir::sig::SigTable;

    use super::super::user_types::tests::{add_struct, id, module, named};
    use super::*;

    /// A document whose `main` creates `pts`, so that messages can name it.
    fn document() -> b2c_model::Document {
        serde_json::from_value(serde_json::json!({
            "format": "blocks2cpp/project",
            "formatVersion": 2,
            "generator": {"app": "0.1.0", "catalog": "1.3.0"},
            "project": {"id": "prj_test", "name": "Test", "language": {"standard": "c++20"}},
            "modules": [{"id": "mod_0", "name": "main", "workspace": {"blocks": [
                {"id": "main", "type": "program.main", "v": 1, "statements": {"BODY": [
                    {"id": "b1", "type": "var.declare", "v": 1,
                        "fields": {"TYPE": "int", "NAME": {"sym": "sym_pts", "name": "pts"}}}
                ]}}
            ]}}]
        }))
        .expect("document")
    }

    /// Structs cannot be declared from blocks before package C1, so the
    /// check is called on a table that holds them.
    #[test]
    fn contains_needs_items_that_can_be_compared() {
        let document = document();
        let sigs = SigTable::empty();
        let unchecked = BTreeSet::new();
        let mut lowerer = Lowerer::new(&document, module(), &sigs, &unchecked);
        add_struct(&mut lowerer.symbols, "Point", &[("x", Type::Int)], false, false);
        add_struct(&mut lowerer.symbols, "Mark", &[("m", Type::Int)], true, false);
        let block: Block = serde_json::from_value(serde_json::json!({
            "id": "b2", "type": "coll.contains", "v": 1, "fields": {"LIST": {"ref": "sym_pts"}}
        }))
        .expect("block");
        let list = |ty: LibType, item: Type| Named {
            sym: id("pts"),
            ty: Type::lib(ty, vec![item]).expect("type"),
            origin: Origin::whole(module(), block.id.clone()),
        };
        for (named_list, reported) in [
            (list(LibType::Vector, named("Point")), true),
            (list(LibType::Deque, named("Point")), true),
            (list(LibType::Vector, named("Mark")), false),
            (list(LibType::Vector, Type::Double), false),
        ] {
            lowerer.diags.list.clear();
            lowerer.check_items_comparable(&block, &named_list);
            assert_eq!(
                lowerer.diags.list.len(),
                usize::from(reported),
                "{:?}",
                named_list.ty
            );
        }
        lowerer.diags.list.clear();
        lowerer.check_items_comparable(&block, &list(LibType::Vector, named("Point")));
        let messages: Vec<(&str, &str)> = lowerer
            .diags
            .list
            .iter()
            .map(|d| (d.code.0.as_str(), d.message.as_str()))
            .collect();
        assert_eq!(
            messages,
            [(
                codes::NOT_COMPARABLE,
                "To look for an item, the items of `pts` must be comparable, and `Point` is not. Tick \
                 comparable on `Point`."
            )]
        );
    }
}
