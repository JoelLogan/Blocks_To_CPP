//! Typed slots in the analyser (ADR-0018 §3, slots.md §3): a block whose
//! catalog signature has patterns is typed by [`type_block`] in four steps.
//!
//! 1. Its arm reads its fields, as before, and lowers its value inputs in
//!    catalog order through [`Lowerer::typed_input`], which puts off an
//!    input holding a place-typed block (the part is then
//!    [`PartType::FromPlace`]).
//! 2. [`Lowerer::type_parts`] runs [`type_block`] with the place the
//!    parent passed down,
//! 3. then lowers the inputs put off, each with its expected type as its
//!    place.
//! 4. The arm reports each part's fit through the checks it always used
//!    ([`Lowerer::report_fit`], which goes through `check_conversion`), so
//!    M2's blocks keep their wording.
//!
//! M2's `create`, `set`, calls and `return` have such signatures in the
//! catalog (slots.md §5) and are always lowered this way: one the table does
//! not know is typed by [`NO_SIGNATURE`], which checks nothing of its values'
//! types. Any other block without such a signature keeps its own rules.
//! Variables are local to one block: between blocks, a value's type flows up
//! to its parent and an expected type flows down into a place-typed block
//! only.

use std::collections::{BTreeMap, BTreeSet};

use b2c_ir::diag::Part;
use b2c_ir::ids::SymbolId;
use b2c_ir::sast::{Expr, ExprKind, SymbolKind, SymbolTable};
use b2c_ir::sig::{BlockParts, BlockSig, PartType, TypePattern};
use b2c_ir::types::{Type, TypeClass};
use b2c_model::{Block, Input};

use super::convert::Subject;
use super::expr::error_expr;
use super::{ItemCtx, Lowerer, field};
use crate::codes;
use crate::messages::{a_class, a_type, quoted};
use crate::slots::{BlockTyping, SlotFit, type_block};
use crate::typing::check_accepts;

/// A value input part of a typed block.
#[derive(Debug, Clone)]
enum Value {
    /// Lowered.
    Lowered(Expr),
    /// Absent, empty or disabled (a required one is reported).
    Empty,
    /// A place-typed block, lowered once its place is known (step 3).
    Deferred,
}

/// The parts of a block with a signature, as its arm reads them.
#[derive(Debug, Clone)]
pub(super) struct Typed<'s> {
    sig: &'s BlockSig,
    fields: BTreeMap<String, PartType>,
    values: BTreeMap<String, Value>,
    counts: BTreeMap<String, usize>,
    /// The parameter types of the function chosen in a field, for `param:`.
    params: Option<(String, Vec<Type>)>,
    /// The field types of the block's own struct rows, by row, for
    /// `member:` on the declaration field.
    rows: Option<(String, Vec<Type>)>,
    /// The symbol chosen in each `members` field.
    members: BTreeMap<String, SymbolId>,
    /// What a `return` gives back where the block is.
    result: PartType,
}

impl Typed<'_> {
    /// Records what a field is.
    pub(super) fn field(&mut self, name: &str, part: PartType) {
        self.fields.insert(name.to_owned(), part);
    }

    /// Records how many parts the repeated input `base` has.
    pub(super) fn count(&mut self, base: &str, count: usize) {
        self.counts.insert(base.to_owned(), count);
    }

    /// Records the parameter types of the function chosen in `field`.
    pub(super) fn params(&mut self, field: &str, params: Vec<Type>) {
        self.params = Some((field.to_owned(), params));
    }

    /// Records the field types of the rows of the struct the block itself
    /// declares in `field`, by row, for `member:` (a damaged row is
    /// `Type::Error`, so later rows keep their own types).
    pub(super) fn rows(&mut self, field: &str, rows: Vec<Type>) {
        self.rows = Some((field.to_owned(), rows));
    }

    /// Takes a lowered input part; `None` when it is absent, empty or
    /// disabled.
    pub(super) fn take(&mut self, part: &str) -> Option<Expr> {
        match self.values.remove(part) {
            Some(Value::Lowered(expr)) => Some(expr),
            Some(Value::Empty | Value::Deferred) | None => None,
        }
    }
}

/// [`Typed`] as [`type_block`] reads it: the members of user types come from
/// the symbol table.
struct View<'a, 's> {
    typed: &'a Typed<'s>,
    symbols: &'a SymbolTable,
}

impl BlockParts for View<'_, '_> {
    fn field(&self, name: &str) -> PartType {
        self.typed.fields.get(name).cloned().unwrap_or(PartType::Empty)
    }

    fn input(&self, part: &str) -> PartType {
        match self.typed.values.get(part) {
            Some(Value::Lowered(expr)) => PartType::Known(expr.ty.clone()),
            Some(Value::Deferred) => PartType::FromPlace,
            Some(Value::Empty) | None => PartType::Empty,
        }
    }

    fn count(&self, base: &str) -> usize {
        self.typed.counts.get(base).copied().unwrap_or(0)
    }

    fn param(&self, field: &str, index: usize) -> PartType {
        match &self.typed.params {
            Some((name, params)) if name == field => params
                .get(index)
                .map_or(PartType::Empty, |ty| PartType::Known(ty.clone())),
            _ => PartType::Unknown,
        }
    }

    fn member(&self, field: &str, index: usize) -> PartType {
        // The block's own rows (a struct's default values).
        if let Some((name, rows)) = &self.typed.rows
            && name == field
        {
            return rows
                .get(index)
                .map_or(PartType::Empty, |ty| PartType::Known(ty.clone()));
        }
        // The struct chosen in a type or reference field: its fields in order.
        let Some(PartType::Known(Type::Named(owner))) = self.typed.fields.get(field) else {
            return PartType::Unknown;
        };
        match self.symbols.get(owner).map(|symbol| &symbol.kind) {
            Some(SymbolKind::Struct { fields, .. }) => fields
                .get(index)
                .and_then(|member| self.symbols.get(member))
                .map_or(PartType::Empty, |member| PartType::Known(member.ty.clone())),
            _ => PartType::Unknown,
        }
    }

    fn chosen_member(&self, field: &str, owner: &Type) -> PartType {
        let Some(sym) = self.typed.members.get(field) else {
            return PartType::Empty;
        };
        member_type(self.symbols, owner, sym).map_or(PartType::Unknown, PartType::Known)
    }

    fn result(&self) -> PartType {
        self.typed.result.clone()
    }
}

/// The type of `sym` when it is a member of the user type `owner`: a field
/// of a struct, or a value of an enum.
pub(super) fn member_type(symbols: &SymbolTable, owner: &Type, sym: &SymbolId) -> Option<Type> {
    let Type::Named(owner) = owner else {
        return None;
    };
    let members = match &symbols.get(owner)?.kind {
        SymbolKind::Struct { fields, .. } => fields,
        SymbolKind::Enum { values } => values,
        _ => return None,
    };
    members
        .contains(sym)
        .then(|| symbols.get(sym).map(|member| member.ty.clone()))
        .flatten()
}

/// The signature of a block that is always typed but that the table does not
/// know (a table other than the catalog's): no variables and no patterns, so
/// its parts are lowered and read as usual and nothing of their types is
/// checked.
static NO_SIGNATURE: BlockSig = BlockSig {
    vars: BTreeMap::new(),
    output: None,
    fields: BTreeMap::new(),
    inputs: BTreeMap::new(),
    order: Vec::new(),
    place_var: None,
    members: BTreeMap::new(),
    symbol_refs: BTreeSet::new(),
    symbol_decls: BTreeSet::new(),
    repeated: BTreeSet::new(),
    repeated_fields: BTreeSet::new(),
    headers: Vec::new(),
    option_headers: BTreeMap::new(),
};

impl<'d> Lowerer<'d> {
    /// The signature that types the parts of a block of this type (ADR-0018),
    /// if it has one with patterns: a pattern on a part, a `members` field or
    /// a place-typed output. Every other block keeps its own rules.
    pub(super) fn typed_sig(&self, block_type: &str) -> Option<&'d BlockSig> {
        let sigs: &'d b2c_ir::sig::SigTable = self.sigs;
        sigs.get(block_type)
            .filter(|sig| !sig.order.is_empty() || !sig.members.is_empty() || sig.place_var.is_some())
    }

    /// The signature of a block that is always typed (M2's `create`, `set`,
    /// calls and `return`): its own, or [`NO_SIGNATURE`] when the table has
    /// none with patterns.
    pub(super) fn block_sig(&self, block_type: &str) -> &'d BlockSig {
        self.typed_sig(block_type).unwrap_or(&NO_SIGNATURE)
    }

    /// Whether a block takes its type from its place: its signature's output
    /// is a variable that no part binds.
    pub(super) fn is_place_typed(&self, block: &Block) -> bool {
        self.sigs
            .get(&block.block_type)
            .is_some_and(|sig| sig.place_var.is_some())
    }

    /// Starts typing a block with this signature.
    pub(super) fn typed<'s>(&self, sig: &'s BlockSig) -> Typed<'s> {
        let result = match &self.item {
            ItemCtx::Main => PartType::Known(Type::Int),
            ItemCtx::Function { ret: Type::Void, .. } => PartType::Empty,
            ItemCtx::Function { ret, .. } => PartType::Known(ret.clone()),
        };
        Typed {
            sig,
            fields: BTreeMap::new(),
            values: BTreeMap::new(),
            counts: BTreeMap::new(),
            params: None,
            rows: None,
            members: BTreeMap::new(),
            result,
        }
    }

    /// Lowers a value input part of a typed block (step 1), or puts it off
    /// when it holds a place-typed block. A required part that is absent or
    /// disabled is reported as [`Lowerer::required_input`] reports it.
    pub(super) fn typed_input(&mut self, block: &Block, typed: &mut Typed<'_>, part: &str, required: bool) {
        let place_typed = matches!(
            crate::access::input(block, part),
            Some(Input::Block(nested)) if !nested.block.disabled && self.is_place_typed(&nested.block)
        );
        let value = if place_typed {
            Value::Deferred
        } else if required {
            match self.required_value(block, part) {
                Ok(expr) => Value::Lowered(expr),
                Err(()) => Value::Empty,
            }
        } else {
            self.optional_input(block, part)
                .map_or(Value::Empty, Value::Lowered)
        };
        typed.values.insert(part.to_owned(), value);
    }

    /// Types the block (step 2) with the place its parent passed down, then
    /// lowers the inputs put off with their expected types as places (step
    /// 3). The inputs of a block the catalog kept unchecked check nothing,
    /// so their places are `Error`.
    pub(super) fn type_parts(
        &mut self,
        block: &Block,
        typed: &mut Typed<'_>,
        place: Option<&Type>,
    ) -> BlockTyping {
        // A `members` field names a member of the type its variable is bound
        // to, not a symbol in scope: it is read here, not by the arm.
        for name in typed.sig.members.keys() {
            if let Ok(sym) = crate::access::ref_field(block, name) {
                typed.members.insert(name.clone(), sym.clone());
            }
        }
        let typing = {
            let view = View {
                typed,
                symbols: &self.symbols,
            };
            type_block(typed.sig, &view, place, &self.symbols)
        };
        let checking = self.checking();
        for (part, value) in &mut typed.values {
            if !matches!(value, Value::Deferred) {
                continue;
            }
            let expects = if checking {
                typing.inputs.get(part).and_then(|input| input.expects.clone())
            } else {
                Some(Type::Error)
            };
            let lowered = match crate::access::input(block, part) {
                Some(Input::Block(nested)) => self.value_block(&nested.block, expects.as_ref()),
                _ => error_expr(self.origin(&block.id, Part::Input { name: part.clone() })),
            };
            *value = Value::Lowered(lowered);
        }
        self.report_members(block, typed, &typing);
        typing
    }

    /// Reports a `members` field whose chosen symbol is not a member of the
    /// user type bound to its variable (`B2C-E0207`). When the variable is
    /// unbound or `Error`, nothing more is said.
    fn report_members(&mut self, block: &Block, typed: &Typed<'_>, typing: &BlockTyping) {
        if !self.checking() {
            return;
        }
        for (name, var) in &typed.sig.members {
            let Some((owner, _)) = typing.bound.get(var) else {
                continue;
            };
            let Some(sym) = typed.members.get(name) else {
                continue;
            };
            if *owner == Type::Error || member_type(&self.symbols, owner, sym).is_some() {
                continue;
            }
            let kind = match owner {
                Type::Named(id)
                    if matches!(
                        self.symbols.get(id).map(|s| &s.kind),
                        Some(SymbolKind::Enum { .. })
                    ) =>
                {
                    "value"
                }
                _ => "field",
            };
            let member = self
                .symbols
                .get(sym)
                .map_or_else(|| self.name_of(sym), |s| quoted(s.name.as_str()));
            let message = format!("{member} is not a {kind} of {}.", self.type_name(owner));
            let location = self.loc(&block.id, field(name));
            self.diags.error(codes::WRONG_KIND, location, message);
        }
    }

    /// A type named for a message: a user type by its name in backticks,
    /// any other type with its article.
    pub(super) fn type_name(&self, ty: &Type) -> String {
        match ty {
            Type::Named(id) => self.symbols.get(id).map_or_else(
                || a_type(ty, &self.symbols),
                |symbol| quoted(symbol.name.as_str()),
            ),
            _ => a_type(ty, &self.symbols),
        }
    }

    /// Reports how a lowered value fits its input (step 4): a conversion
    /// through `check_conversion` and its messages, as M2's blocks always
    /// did; a value outside the class its input requires as `B2C-E0301`.
    /// `assign` inputs also take a `char` for a `std::string`.
    pub(super) fn report_fit(
        &mut self,
        typed: &Typed<'_>,
        typing: &BlockTyping,
        part: &str,
        value: &Expr,
        subject: &Subject,
    ) {
        let Some(input) = typing.inputs.get(part) else {
            return;
        };
        let base = part.trim_end_matches(|c: char| c.is_ascii_digit());
        let sig_input = typed.sig.inputs.get(part).or_else(|| typed.sig.inputs.get(base));
        let assign = sig_input.is_some_and(|input| input.assign);
        let expects = input.expects.as_ref().filter(|expects| **expects != Type::Error);
        match (input.fit, expects) {
            (SlotFit::Unknown, _) | (SlotFit::Converts(_), None) => {}
            (SlotFit::OutsideClass, _) => {
                let class = sig_input.map_or(TypeClass::Any, |input| {
                    self.failed_class(typed.sig, input.check, input.pattern.as_ref(), &value.ty)
                });
                self.report_outside_class(value, class);
            }
            (SlotFit::Converts(_) | SlotFit::Invalid, Some(expects)) => {
                // `s = 'a';` compiles although `std::string s = 'a';` does not.
                if !(assign && value.ty == Type::Char && *expects == Type::String) {
                    self.check_conversion(value, expects, subject);
                }
            }
            (SlotFit::Invalid, None) => self.report_shape(value),
        }
    }

    /// Reports how a `type` or `symbol_ref` field fits (step 4): a type
    /// outside the class of its variable, a wrong shape, or a reference to a
    /// value of another type than an earlier part gave.
    pub(super) fn report_field_fit(
        &mut self,
        block: &Block,
        typed: &Typed<'_>,
        typing: &BlockTyping,
        name: &str,
    ) {
        if !self.checking() {
            return;
        }
        let (Some(fit), Some(PartType::Known(ty))) = (typing.fields.get(name), typed.fields.get(name)) else {
            return;
        };
        let location = self.loc(&block.id, field(name));
        let have = self.type_name(ty);
        let message = match fit {
            SlotFit::OutsideClass => {
                let pattern = typed.sig.fields.get(name);
                let class = self.failed_class(typed.sig, TypeClass::Any, pattern, ty);
                format!("This is {have}, but this block needs {} here.", a_class(class))
            }
            SlotFit::Invalid => match typed
                .sig
                .fields
                .get(name)
                .map(|pattern| resolved_for_message(pattern, typing))
            {
                Some(Some(expected)) if expected != *ty && expected != Type::Error => format!(
                    "This is {have}, but this block needs {} here, as its other parts say.",
                    self.type_name(&expected)
                ),
                _ => format!("This is {have}, which doesn't fit this block here."),
            },
            SlotFit::Unknown | SlotFit::Converts(_) => return,
        };
        // A user type of a kind the class does not hold is a wrong
        // reference (types.md §1.7); any other type is a value that does not
        // fit.
        let type_reference = !typed.sig.symbol_refs.contains(name);
        let code = if type_reference && matches!(ty, Type::Named(_)) && *fit == SlotFit::OutsideClass {
            codes::WRONG_KIND
        } else {
            codes::CONVERSION
        };
        self.diags.error(code, location, message);
    }

    /// The class a value of type `ty` is outside of: the `check` class when
    /// it is that one, else the class of the first variable of the pattern
    /// that does not hold it.
    fn failed_class(
        &self,
        sig: &BlockSig,
        check: TypeClass,
        pattern: Option<&TypePattern>,
        ty: &Type,
    ) -> TypeClass {
        if check_accepts(check, ty, &self.symbols) == Some(false) {
            return check;
        }
        pattern
            .map(TypePattern::binding_vars)
            .unwrap_or_default()
            .into_iter()
            .filter_map(|var| sig.vars.get(var).copied())
            .find(|class| ty.is_in(*class, &self.symbols) == Some(false))
            .unwrap_or(check)
    }

    /// `B2C-E0301` for a value outside the class its input requires
    /// (types.md §1.7).
    fn report_outside_class(&mut self, value: &Expr, class: TypeClass) {
        if !self.checking() {
            return;
        }
        let have = a_type(&value.ty, &self.symbols);
        let message = match &value.kind {
            ExprKind::Var(sym) => format!(
                "{} is {have}, but this block needs {}.",
                self.name_of(sym),
                a_class(class)
            ),
            _ => format!("This is {have}, but this block needs {}.", a_class(class)),
        };
        self.diags
            .error(codes::CONVERSION, value.origin.location(), message);
    }

    /// `B2C-E0301` for a value whose type does not have the shape its input
    /// needs (`std::set` where a `std::vector` is needed).
    fn report_shape(&mut self, value: &Expr) {
        if !self.checking() {
            return;
        }
        let message = format!(
            "This is {}, which doesn't fit this block here.",
            a_type(&value.ty, &self.symbols)
        );
        self.diags
            .error(codes::CONVERSION, value.origin.location(), message);
    }

    /// The type of a place-typed block (slots.md §3): its place's type, or
    /// `Error` with `B2C-E0315` when it has no place, with `B2C-E0301` when
    /// its place is outside its class, and silently when its place is
    /// `Error` or not known. A block the catalog kept unchecked reports
    /// nothing.
    pub(super) fn place_type(&mut self, block: &Block, sig: &BlockSig, place: Option<&Type>) -> Type {
        let Some(var) = &sig.place_var else {
            return Type::Error;
        };
        let class = sig.vars.get(var).copied().unwrap_or(TypeClass::Any);
        let location = self.loc(&block.id, Part::Whole);
        match place {
            _ if !self.checking() => Type::Error,
            None => {
                self.diags.error(
                    codes::NO_PLACE_TYPE,
                    location,
                    "This block takes its type from where it is plugged in, but nothing there says which type \
                     it should have. Plug it in where the type is known, such as the value of a 'create' block \
                     with a chosen type.",
                );
                Type::Error
            }
            Some(place) => match place.is_in(class, &self.symbols) {
                Some(true) => place.clone(),
                Some(false) => {
                    let message = format!(
                        "This block makes {}, but {} is needed here.",
                        a_class(class),
                        a_type(place, &self.symbols)
                    );
                    self.diags.error(codes::CONVERSION, location, message);
                    Type::Error
                }
                None => Type::Error,
            },
        }
    }
}

/// A field's pattern resolved against the typing's bindings, for messages
/// (only fixed types, variables and templates over them).
fn resolved_for_message(pattern: &TypePattern, typing: &BlockTyping) -> Option<Type> {
    match pattern {
        TypePattern::Type(ty) => Some(ty.clone()),
        TypePattern::Var(var) => typing.bound.get(var).map(|(ty, _)| ty.clone()),
        TypePattern::Lib(lib, args) => {
            let args = args
                .iter()
                .map(|arg| resolved_for_message(arg, typing))
                .collect::<Option<Vec<_>>>()?;
            Type::lib(*lib, args).ok()
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use b2c_ir::sig::{PartName, SigTable, TypeVar};
    use b2c_ir::types::TypeClass;

    use super::super::user_types::tests::{add, add_struct, document, id, module, named};
    use super::*;
    use crate::slots::SlotFit;

    /// `[OBJ] . [MEMBER]`: `where = { S = "struct" }`, output `symbol:MEMBER`.
    fn member_sig() -> BlockSig {
        let s = TypeVar::new("S").expect("variable");
        BlockSig {
            vars: BTreeMap::from([(s.clone(), TypeClass::Struct)]),
            output: Some(TypePattern::parse("symbol:MEMBER").expect("pattern")),
            fields: BTreeMap::from([(String::from("OBJ"), TypePattern::Var(s.clone()))]),
            order: vec![PartName::Field(String::from("OBJ"))],
            members: BTreeMap::from([(String::from("MEMBER"), s)]),
            symbol_refs: BTreeSet::from([String::from("OBJ")]),
            ..BlockSig::default()
        }
    }

    fn block(member: &str) -> Block {
        serde_json::from_value(serde_json::json!({
            "id": "b1", "type": "struct.member", "v": 1,
            "fields": {"OBJ": {"ref": "sym_s"}, "MEMBER": {"ref": format!("sym_{member}")}}
        }))
        .expect("block")
    }

    #[test]
    fn a_members_field_is_read_among_the_members_of_its_bound_type() {
        let document = document();
        let sigs = SigTable::empty();
        let unchecked = BTreeSet::new();
        let mut lowerer = Lowerer::new(&document, module(), &sigs, &unchecked);
        add_struct(
            &mut lowerer.symbols,
            "Student",
            &[("mark", Type::Int)],
            false,
            false,
        );
        add(
            &mut lowerer.symbols,
            "speed",
            SymbolKind::Variable { is_const: false },
            Type::Int,
        );
        let sig = member_sig();

        let mut typed = lowerer.typed(&sig);
        typed.field("OBJ", PartType::Known(named("Student")));
        let typing = lowerer.type_parts(&block("mark"), &mut typed, None);
        assert!(lowerer.diags.list.is_empty());
        assert_eq!(
            typing.fields["MEMBER"],
            SlotFit::Converts(crate::Conversion::Same)
        );
        assert_eq!(typing.output, Some(Type::Int));

        let mut typed = lowerer.typed(&sig);
        typed.field("OBJ", PartType::Known(named("Student")));
        let typing = lowerer.type_parts(&block("speed"), &mut typed, None);
        assert_eq!(typing.fields["MEMBER"], SlotFit::Unknown);
        assert_eq!(typing.output, Some(Type::Error));
        let messages: Vec<(&str, &str)> = lowerer
            .diags
            .list
            .iter()
            .map(|d| (d.code.0.as_str(), d.message.as_str()))
            .collect();
        assert_eq!(
            messages,
            [(codes::WRONG_KIND, "`speed` is not a field of `Student`.")]
        );

        // Nothing more is said while the variable is unbound or `Error`.
        lowerer.diags.list.clear();
        for obj in [PartType::Empty, PartType::Known(Type::Error), PartType::Unknown] {
            let mut typed = lowerer.typed(&sig);
            typed.field("OBJ", obj);
            let _ = lowerer.type_parts(&block("speed"), &mut typed, None);
        }
        assert!(lowerer.diags.list.is_empty());
        assert_eq!(
            member_type(&lowerer.symbols, &named("Student"), &id("mark")),
            Some(Type::Int)
        );
        assert_eq!(member_type(&lowerer.symbols, &Type::Int, &id("mark")), None);
    }
}
