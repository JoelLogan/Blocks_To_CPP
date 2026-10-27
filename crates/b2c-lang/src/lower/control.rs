//! Lowering the control blocks of W2 (`docs/plans/m3-contracts/w2.md` §1.6,
//! §2.9): package D.
//!
//! * *switch on* types its value and its case labels with one pattern
//!   (`where = { T = "switchable" }`): a `char`, an `int` or an enum. Each
//!   case and the default part are lowered through
//!   `Lowerer::within(Breakable::Switch, …)`, so a *leave loop* whose
//!   nearest loop or switch is the switch is `B2C-E0401` (`jump`). The
//!   labels' rules (`B2C-E0424`, `B2C-E0425`) are checked in `consts.rs`,
//!   the paths through the switch in `flow.rs` (P0). A switch on an enum
//!   without a default part that misses some of its values is `B2C-W0508`.
//! * *do … then repeat* lowers its body as a loop's, then its condition,
//!   which sees only what is visible before the block.

use b2c_ir::diag::Part;
use b2c_ir::ids::SymbolId;
use b2c_ir::sast::{Expr, ExprKind, StmtKind, SwitchCase, SymbolKind};
use b2c_ir::types::{Type, TypeClass};
use b2c_model::Block;

use super::convert::Subject;
use super::expr::{error_expr, is_number_or_bool};
use super::{Breakable, Lowerer};
use crate::access;
use crate::codes;
use crate::messages::quoted;

impl Lowerer<'_> {
    /// *switch on*, *do … then repeat*.
    pub(super) fn control_statement(&mut self, block: &Block) -> Option<StmtKind> {
        match block.block_type.as_str() {
            "control.switch" => Some(self.switch(block)),
            "control.do_while" => Some(self.do_while(block)),
            _ => None,
        }
    }

    /// *switch on (value) case (label) … default …*: the value and every
    /// label are typed by the block's pattern `T`; an empty label is
    /// reported as an empty input is (`B2C-E0430`) and left out.
    fn switch(&mut self, block: &Block) -> StmtKind {
        let count = access::case_count(block);
        let mut typed = self.typed(self.block_sig(&block.block_type));
        typed.count("CASE", count);
        self.typed_input(block, &mut typed, "VALUE", true);
        let labels: Vec<String> = (0..count).map(|i| format!("CASE{i}")).collect();
        for label in &labels {
            self.typed_input(block, &mut typed, label, true);
        }
        let typing = self.type_parts(block, &mut typed, None);
        let value = typed.take("VALUE");
        if let Some(value) = &value {
            let subject = Subject::Phrase {
                text: String::from("The value switched on"),
                stores: false,
            };
            self.report_fit(&typed, &typing, "VALUE", value, &subject);
        }
        let mut cases = Vec::with_capacity(count);
        for (index, label) in labels.iter().enumerate() {
            let mut taken = typed.take(label);
            if let Some(label_value) = &mut taken {
                let subject = Subject::Phrase {
                    text: format!("The value of case {}", index + 1),
                    stores: false,
                };
                self.report_fit(&typed, &typing, label, label_value, &subject);
                // A label that is no number at all (text, a list) was just
                // refused; `consts.rs` need not say again that it is not a
                // whole number fixed when the program is built.
                if label_value.ty.is_in(TypeClass::Switchable, &self.symbols) == Some(false)
                    && is_number_or_bool(&label_value.ty, &self.symbols) == Some(false)
                {
                    label_value.ty = Type::Error;
                }
            }
            let list = format!("DO{index}");
            let body = self.within(Breakable::Switch, |lowerer| lowerer.body(block, &list, false));
            if let Some(label) = taken {
                cases.push(SwitchCase { label, body });
            }
        }
        let default = access::extra_flag(block, "hasDefault")
            .then(|| self.within(Breakable::Switch, |lowerer| lowerer.body(block, "DEFAULT", false)));
        let value = value.unwrap_or_else(|| {
            error_expr(self.origin(
                &block.id,
                Part::Input {
                    name: String::from("VALUE"),
                },
            ))
        });
        if default.is_none() {
            self.report_missing_cases(block, &value, &cases);
        }
        StmtKind::Switch {
            value,
            cases,
            default,
        }
    }

    /// `B2C-W0508`: a switch on an enum without a default part that has no
    /// case for some of the enum's values (w2.md §2.9). The generator adds
    /// `default: { break; }` to such a switch, so g++'s `-Wswitch` never
    /// fires as well.
    fn report_missing_cases(&mut self, block: &Block, value: &Expr, cases: &[SwitchCase]) {
        if !self.checking() {
            return;
        }
        let Type::Named(owner) = &value.ty else {
            return;
        };
        let Some(SymbolKind::Enum { values }) = self.symbols.get(owner).map(|symbol| &symbol.kind) else {
            return;
        };
        let missing: Vec<SymbolId> = values
            .iter()
            .filter(|enumerator| {
                !cases
                    .iter()
                    .any(|case| matches!(&case.label.kind, ExprKind::Enumerator(sym) if sym == *enumerator))
            })
            .cloned()
            .collect();
        if missing.is_empty() {
            return;
        }
        let names: Vec<String> = missing
            .iter()
            .filter_map(|sym| self.symbols.get(sym))
            .map(|symbol| quoted(symbol.name.as_str()))
            .collect();
        let listed = match names.as_slice() {
            [] => return,
            [one] => format!("for {one}"),
            [rest @ .., last] => format!("for {} and {last}", rest.join(", ")),
        };
        let message = format!(
            "This switch on {} has no case {listed}, and no default part, so for {} none of its parts runs. \
             Add a case for each value, or a default part.",
            self.type_name(&value.ty),
            if names.len() == 1 {
                "that value"
            } else {
                "those values"
            },
        );
        let location = self.loc(&block.id, Part::Whole);
        self.diags.warning(codes::MISSING_CASES, location, message);
    }

    /// *do (body) then repeat while/until (cond)*: the body runs at least
    /// once; the condition is lowered after it, when only what is visible
    /// before the block is in scope again.
    fn do_while(&mut self, block: &Block) -> StmtKind {
        let until = self.choice(block, "MODE", false, &[("while", false), ("until", true)]);
        let body = self.loop_body(block, false);
        let cond = self.required_input(block, "COND");
        let cond = self.condition(cond);
        StmtKind::DoWhile { body, cond, until }
    }
}
