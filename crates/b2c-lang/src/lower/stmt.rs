//! Lowering statement blocks (spec §3.7).

use b2c_ir::diag::Part;
use b2c_ir::ids::SymbolId;
use b2c_ir::sast::{
    self, AskMode, CompoundOp, Expr, IfBranch, OutputStream, PrintSeparator, RangeDirection, Stmt, StmtKind,
    Symbol, SymbolKind, VarDecl,
};
use b2c_ir::types::Type;
use b2c_model::Block;

use super::convert::{Subject, constant_int};
use super::{ItemCtx, Lowerer, PendingKind, field};
use crate::access;
use crate::codes;
use crate::messages::{a_type, quoted};
use crate::scope::{COUNTER_LIST, ListKey};
use crate::typing::is_integral;

impl Lowerer<'_> {
    /// Lowers the statement list `list` of `owner` in a new scope (`joined`
    /// when C++ treats it as the same scope as the enclosing frame).
    pub(super) fn body(&mut self, owner: &Block, list: &str, joined: bool) -> sast::Block {
        self.scopes.push(ListKey::new(&owner.id, list), joined);
        self.record_input(&owner.id, list);
        let mut stmts = Vec::new();
        for block in access::statements(owner, list) {
            if block.disabled {
                continue;
            }
            if let Some(stmt) = self.statement(block) {
                stmts.push(stmt);
            }
        }
        self.scopes.pop();
        sast::Block { stmts }
    }

    /// A loop body: like [`Self::body`], inside a loop.
    fn loop_body(&mut self, owner: &Block, joined: bool) -> sast::Block {
        self.loop_depth += 1;
        let body = self.body(owner, "BODY", joined);
        self.loop_depth -= 1;
        body
    }

    fn statement(&mut self, block: &Block) -> Option<Stmt> {
        if !self.enter(block) {
            return None;
        }
        self.record_block(&block.id);
        let kind = self.statement_kind(block);
        self.leave();
        let kind = kind?;
        let origin = self.origin(&block.id, Part::Whole);
        Some(Stmt {
            kind,
            origin,
            comment: self.comment(block),
        })
    }

    /// Lowers one statement; `None` for blocks that are not statements (the
    /// catalog check reports those) or that lack what they need (reported).
    fn statement_kind(&mut self, block: &Block) -> Option<StmtKind> {
        Some(match block.block_type.as_str() {
            "var.declare" => return self.declare(block),
            "var.set" => return self.set(block),
            "var.change" => return self.compound(block, CompoundOp::Add, "BY"),
            "var.update" => {
                let options = [
                    ("add", CompoundOp::Add),
                    ("sub", CompoundOp::Sub),
                    ("mul", CompoundOp::Mul),
                    ("div", CompoundOp::Div),
                    ("mod", CompoundOp::Mod),
                ];
                let op = self.choice(block, "OP", CompoundOp::Add, &options);
                return self.compound(block, op, "VALUE");
            }
            "control.if" => self.if_else(block),
            "control.while" => self.while_loop(block),
            "control.repeat" => {
                let count = self.required_input(block, "TIMES");
                let count = self.integer(count, &Subject::RepeatCount);
                StmtKind::Repeat {
                    count,
                    body: self.loop_body(block, false),
                }
            }
            "control.for_range" => return self.for_range(block),
            "control.forever" => StmtKind::Forever {
                body: self.loop_body(block, false),
            },
            "control.break" => self.jump(block, StmtKind::Break, "'leave loop'"),
            "control.continue" => self.jump(block, StmtKind::Continue, "'skip to next round'"),
            "io.print" => self.print(block),
            "io.ask" => return self.ask(block),
            "func.call_stmt" => StmtKind::Eval {
                expr: self.call_block(block, true)?,
            },
            "func.return" => self.return_value(block),
            "program.exit" => {
                let code = self
                    .optional_input(block, "CODE")
                    .map(|code| self.integer(code, &Subject::ExitCode));
                StmtKind::Exit {
                    code,
                    in_main: matches!(self.item, ItemCtx::Main),
                }
            }
            _ => return None,
        })
    }

    fn declare(&mut self, block: &Block) -> Option<StmtKind> {
        let name_loc = self.loc(&block.id, field("NAME"));
        let decl = access::decl_field(block, "NAME");
        let type_text = access::text_field(block, "TYPE", Some("int")).ok();
        let written_auto = type_text == Some("auto");
        let declared = if written_auto {
            None
        } else {
            Some(self.type_setting(block, "TYPE", "int", false))
        };
        let is_const = self.checkbox(block, "CONST", false);
        let Ok(decl) = decl else {
            self.incomplete(name_loc, "This 'create variable' block needs a name.");
            let _ = self.optional_input(block, "VALUE");
            return None;
        };
        let ident = self.ident(&decl.name, &name_loc, false);
        self.check_new_name(&decl.name, &name_loc);
        let kind = PendingKind::Initialiser(declared.clone());
        self.push_pending(&decl.name, &decl.sym, kind);
        self.record_input(&block.id, "VALUE");
        let init = self.optional_input(block, "VALUE");
        self.pop_pending();
        let ty = match (declared, &init) {
            (Some(ty), Some(value)) => {
                self.check_conversion(value, &ty, &Subject::Variable(decl.name.clone()));
                ty
            }
            (Some(ty), None) => ty,
            (None, Some(value)) => value.ty.clone(),
            (None, None) => {
                let message = format!(
                    "{} has the type 'auto', so it needs a starting value to take its type from. Give it a \
                     value, or choose a type.",
                    quoted(&decl.name)
                );
                self.diags
                    .error(codes::AUTO_WITHOUT_VALUE, name_loc.clone(), message);
                Type::Error
            }
        };
        let origin = self.origin(&block.id, field("NAME"));
        let symbol = Symbol {
            name: ident,
            kind: SymbolKind::Variable { is_const },
            ty,
            origin,
        };
        if !self.declare_local(&decl.sym, &decl.name, symbol, name_loc) {
            // A damaged file declares this symbol twice; the first one stays.
            return None;
        }
        Some(StmtKind::VarDecl(VarDecl {
            symbol: decl.sym.clone(),
            init,
            is_const,
            written_auto,
        }))
    }

    /// Adds a local symbol to the table and the innermost scope; `false`
    /// (after reporting) when another declaration already has its ID.
    fn declare_local(
        &mut self,
        sym: &SymbolId,
        name: &str,
        symbol: Symbol,
        location: b2c_ir::diag::Location,
    ) -> bool {
        let inserted = self.insert_symbol(sym, name, symbol, location);
        if inserted {
            self.scopes.declare(name, sym);
        }
        inserted
    }

    /// The variable a statement changes (field `VAR`): its symbol and, when
    /// it resolves, its type.
    fn target(&mut self, block: &Block) -> Option<(SymbolId, Option<Type>)> {
        let location = self.loc(&block.id, field("VAR"));
        if let Ok(sym) = access::ref_field(block, "VAR") {
            let ty = self.write_target(sym, &location);
            Some((sym.clone(), ty))
        } else {
            self.incomplete(location, "Choose a variable in this block.");
            None
        }
    }

    fn set(&mut self, block: &Block) -> Option<StmtKind> {
        let target = self.target(block);
        let value = self.required_input(block, "VALUE");
        let (sym, ty) = target?;
        // `std::string` can be assigned a single `char` (`s = 'a';`), although
        // it can't be initialised with one, so this is fine for `set`.
        if let Some(ty) = ty.filter(|ty| !(*ty == Type::String && value.ty == Type::Char)) {
            self.check_conversion(&value, &ty, &Subject::Variable(self.raw_name(&sym)));
        }
        Some(StmtKind::Assign { target: sym, value })
    }

    fn compound(&mut self, block: &Block, op: CompoundOp, input: &str) -> Option<StmtKind> {
        let target = self.target(block);
        let value = self.required_input(block, input);
        let (sym, ty) = target?;
        if let Some(ty) = ty {
            self.check_compound(block, &sym, &ty, op, &value);
        }
        Some(StmtKind::CompoundAssign {
            target: sym,
            op,
            value,
        })
    }

    /// Type rules of `change by` and the compound operators.
    fn check_compound(&mut self, block: &Block, sym: &SymbolId, target: &Type, op: CompoundOp, value: &Expr) {
        if *target == Type::Error || value.ty == Type::Error {
            return;
        }
        let location = self.loc(&block.id, Part::Whole);
        let value_loc = value.origin.location();
        let name = self.name_of(sym);
        match target {
            Type::String => {
                let message = format!(
                    "{name} holds text, so it can't be changed by a number. To add to text, use a 'set' block \
                     with a 'join' block."
                );
                self.diags.error(codes::BAD_OPERANDS, location, message);
                return;
            }
            Type::Bool => {
                let message =
                    format!("{name} is a true/false value; changing it by a number treats it as 1 or 0.");
                self.diags.warning(codes::BOOL_NUMBER, location.clone(), message);
            }
            _ => {}
        }
        match value.ty {
            Type::String => {
                self.diags.error(
                    codes::CONVERSION,
                    value_loc,
                    "The amount must be a number, but this is text.",
                );
                return;
            }
            Type::Bool => {
                let message = "This is a true/false value, used as a number (1 or 0).";
                self.diags.warning(codes::BOOL_NUMBER, value_loc.clone(), message);
            }
            _ => {}
        }
        if op == CompoundOp::Mod && (*target == Type::Double || value.ty == Type::Double) {
            let message = "'mod' only works with whole numbers, but this uses a decimal number. Use the \
                           'convert' block to make it a whole number first.";
            self.diags.error(codes::MOD_DECIMAL, location, message);
        } else if is_integral(target) && value.ty == Type::Double {
            let message = format!(
                "{name} is {}, so the part after the decimal point of the result will be dropped. Use the \
                 'convert' block to show that this is intended.",
                a_type(target)
            );
            self.diags.warning(codes::NARROWING, value_loc, message);
        } else if *target == Type::Double {
            self.warn_integer_division(value);
        }
    }

    fn if_else(&mut self, block: &Block) -> StmtKind {
        let count = access::else_if_count(block);
        let mut branches = Vec::with_capacity(count + 1);
        for i in 0..=count {
            let cond = self.required_input(block, &format!("COND{i}"));
            let cond = self.condition(cond);
            let body = self.body(block, &format!("DO{i}"), false);
            branches.push(IfBranch { cond, body });
        }
        let else_body = access::extra_flag(block, "hasElse").then(|| self.body(block, "ELSE", false));
        StmtKind::If { branches, else_body }
    }

    fn while_loop(&mut self, block: &Block) -> StmtKind {
        let until = self.choice(block, "MODE", false, &[("while", false), ("until", true)]);
        let cond = self.required_input(block, "COND");
        let cond = self.condition(cond);
        StmtKind::While {
            cond,
            until,
            body: self.loop_body(block, false),
        }
    }

    fn for_range(&mut self, block: &Block) -> Option<StmtKind> {
        let name_loc = self.loc(&block.id, field("VAR"));
        let options = [
            ("to", RangeDirection::UpExclusive),
            ("through", RangeDirection::UpInclusive),
            ("down_to", RangeDirection::DownInclusive),
        ];
        let direction = self.choice(block, "DIRECTION", RangeDirection::UpExclusive, &options);
        let decl = access::decl_field(block, "VAR").ok();
        if let Some(decl) = decl {
            self.push_pending(&decl.name, &decl.sym, PendingKind::Counter);
            for input in ["FROM", "TO", "STEP"] {
                self.record_input(&block.id, input);
            }
        }
        let from = self.required_input(block, "FROM");
        let from = self.integer(from, &Subject::LoopStart);
        let to = self.required_input(block, "TO");
        let to = self.integer(to, &Subject::LoopEnd);
        let step = self
            .optional_input(block, "STEP")
            .map(|step| self.loop_step(step));
        if decl.is_some() {
            self.pop_pending();
        }
        let Some(decl) = decl else {
            self.incomplete(name_loc, "This 'for' loop needs a name for its counter.");
            return None;
        };
        let ident = self.ident(&decl.name, &name_loc, false);
        self.scopes.push(ListKey::new(&block.id, COUNTER_LIST), false);
        self.check_new_name(&decl.name, &name_loc);
        let origin = self.origin(&block.id, field("VAR"));
        let symbol = Symbol {
            name: ident,
            kind: SymbolKind::LoopVariable,
            ty: Type::Int,
            origin,
        };
        let _ = self.declare_local(&decl.sym, &decl.name, symbol, name_loc);
        let body = self.loop_body(block, true);
        self.scopes.pop();
        Some(StmtKind::ForRange {
            var: decl.sym.clone(),
            from,
            to,
            step,
            direction,
            body,
        })
    }

    fn loop_step(&mut self, step: Expr) -> Expr {
        let step = self.integer(step, &Subject::LoopStep);
        if constant_int(&step).is_some_and(|value| value <= 0) {
            self.diags.warning(
                codes::BAD_STEP,
                step.origin.location(),
                "The step must be more than 0: the direction ('to', 'through' or 'down to') decides whether the \
                 loop counts up or down. With this step the loop would never finish or never start.",
            );
        }
        step
    }

    fn jump(&mut self, block: &Block, kind: StmtKind, label: &str) -> StmtKind {
        if self.loop_depth == 0 {
            let location = self.loc(&block.id, Part::Whole);
            self.diags.error(
                codes::OUTSIDE_LOOP,
                location,
                format!("{label} can only be used inside a loop."),
            );
        }
        kind
    }

    fn print(&mut self, block: &Block) -> StmtKind {
        let separators = [
            ("none", PrintSeparator::None),
            ("space", PrintSeparator::Space),
            ("comma", PrintSeparator::Comma),
        ];
        let separator = self.choice(block, "SEP", PrintSeparator::None, &separators);
        let newline = self.checkbox(block, "NEWLINE", true);
        let stream = self.choice(
            block,
            "STREAM",
            OutputStream::Out,
            &[("out", OutputStream::Out), ("err", OutputStream::Err)],
        );
        let count = access::extra_count(block, "itemCount", 1, 1);
        let items = (0..count)
            .map(|i| self.required_input(block, &format!("ITEM{i}")))
            .collect();
        StmtKind::Print {
            items,
            separator,
            newline,
            stream,
        }
    }

    fn ask(&mut self, block: &Block) -> Option<StmtKind> {
        let target = self.target(block);
        let modes = [("keep_asking", AskMode::KeepAsking), ("simple", AskMode::Simple)];
        let mode = self.choice(block, "MODE", AskMode::KeepAsking, &modes);
        let prompt = self.optional_input(block, "PROMPT");
        if let Some(prompt) = &prompt
            && !matches!(prompt.ty, Type::String | Type::Char | Type::Error)
        {
            let message = format!(
                "The question to ask must be text, but this is {}. Use the 'join' block to turn it into text.",
                a_type(&prompt.ty)
            );
            self.diags
                .error(codes::CONVERSION, prompt.origin.location(), message);
        }
        let (target, _) = target?;
        Some(StmtKind::Ask { prompt, target, mode })
    }

    fn return_value(&mut self, block: &Block) -> StmtKind {
        let value = self.optional_input(block, "VALUE");
        let location = self.loc(&block.id, Part::Whole);
        match (self.item.clone(), &value) {
            (ItemCtx::Main, None) => self.diags.error(
                codes::RETURN_NEEDS_VALUE,
                location,
                "In 'when program starts', 'return' needs an exit code (0 means success). You can also use \
                 'stop program with exit code'.",
            ),
            (ItemCtx::Main, Some(value)) => self.check_conversion(value, &Type::Int, &Subject::ExitCode),
            (
                ItemCtx::Function {
                    name,
                    ret: Type::Void,
                },
                Some(_),
            ) => {
                let message = format!(
                    "{} gives back nothing, so this 'return' can't have a value. Remove the value, or change \
                     what the function gives back.",
                    quoted(&name)
                );
                self.diags.error(codes::RETURN_HAS_VALUE, location, message);
            }
            (
                ItemCtx::Function {
                    ret: Type::Void | Type::Error,
                    ..
                },
                None,
            )
            | (ItemCtx::Function { ret: Type::Error, .. }, Some(_)) => {}
            (ItemCtx::Function { name, ret }, None) => {
                let message = format!(
                    "{} must give back {}, so this 'return' needs a value.",
                    quoted(&name),
                    a_type(&ret)
                );
                self.diags.error(codes::RETURN_NEEDS_VALUE, location, message);
            }
            (ItemCtx::Function { name, ret }, Some(value)) => {
                self.check_conversion(value, &ret, &Subject::Result(name));
            }
        }
        StmtKind::Return { value }
    }
}
