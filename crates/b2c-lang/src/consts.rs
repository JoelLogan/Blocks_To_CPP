//! Constant folding (L10, `docs/plans/m3-contracts/types.md` §11.2): the
//! analyser works out the value of every expression built only from
//! literals, `const` variables with a known value and the operator, logic,
//! conditional and conversion blocks, with C++'s semantics on every target
//! Blocks2Cpp supports (`int` is 32-bit two's complement and `double` IEEE
//! 754 binary64 under LP64 Linux and macOS and LLP64 Windows).
//!
//! It reports what such a value makes certain, at the operator that does it:
//!
//! * a whole-number `/` or `%` (also `change … divided by` and `mod`) whose
//!   divisor is a constant zero: `B2C-E0316`, since the program crashes
//!   there (C++ leaves it undefined; g++'s code stops with `SIGFPE`), also
//!   when the left side is not constant;
//! * a constant `int` calculation whose result does not fit an `int` (`+`,
//!   `-`, `*`, `/` and `%` of the smallest `int` by -1, the sign `-`), and a
//!   constant decimal number converted to a whole number that does not fit:
//!   `B2C-E0517`, the code of a literal that does not fit, since C++ leaves
//!   the result undefined;
//! * a decimal division by a constant zero, and a constant decimal
//!   calculation too large for a `double`: `B2C-W0506`, since IEEE 754 gives
//!   infinity or NaN, which is defined but hardly ever meant;
//! * a decimal division by a constant zero whose result goes straight into a
//!   whole number (a `char` or `int` variable, parameter, return value or
//!   count, `convert`, or `change … divided by` on a whole-number variable):
//!   `B2C-E0517` instead of `B2C-W0506`, since infinity and NaN do not fit a
//!   whole number and C++ leaves that conversion undefined.
//!
//! Operands that C++ does not evaluate are not checked: the right side of
//! `&&` and `||` when the constant left side decides, and the choice of `?:`
//! that a constant condition does not pick (g++ does not warn there either).
//! Statements are all checked, except the parts of a block the catalog kept
//! unchecked (ADR-0018 §3): the expressions in its own slots and its own
//! `change` are evaluated without a report, as the lowerer checks nothing
//! there either (a block in one of its inputs is checked as usual). A value
//! that was reported is not a constant further up, so one mistake gives one
//! message, and an expression of type `Error` is never folded.
//!
//! [`constant_value`] exposes the same evaluation without diagnostics, and
//! [`crate::Analysis::constants`] the value of each `const` variable whose
//! starting value is constant, which ADR-0019's initialisation-order rule
//! (modules.md §4.4) reads.
//!
//! The walk also checks what W2 needs fixed when the program is built
//! (w2.md §2.9), since only it knows the item's `const` variables in
//! program order: a `case` label must be a whole number, a character or an
//! enum value that is a constant expression (`B2C-E0424`), and two labels
//! of one switch never have the same value (`B2C-E0425`; `'A'` and `65` are
//! the same, enum values are told apart by their enumerator); the condition
//! of *check at compile time* must be a constant expression
//! (`B2C-E0424`) that is not `false` (`B2C-E0428`).

use std::collections::{BTreeMap, BTreeSet};
use std::num::FpCategory;

use b2c_ir::diag::Location;
use b2c_ir::ids::{BlockId, SymbolId};
use b2c_ir::sast::{
    BinaryOp, Block, CompoundOp, Expr, ExprKind, Item, ItemKind, Stmt, StmtKind, SwitchCase, SymbolKind,
    SymbolTable, UnaryOp,
};
use b2c_ir::types::Type;

use crate::codes;
use crate::messages::Diags;

/// A value the analyser worked out (types.md §11.2).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ConstValue {
    /// A `bool`.
    Bool(bool),
    /// A `char`, by its code: always an ASCII character (0 to 127), since a
    /// `char` outside that range has a value that depends on the target.
    Char(u8),
    /// An `int`.
    Int(i32),
    /// A `double`, always finite.
    Double(f64),
}

impl ConstValue {
    /// The value's type.
    pub fn ty(self) -> Type {
        match self {
            Self::Bool(_) => Type::Bool,
            Self::Char(_) => Type::Char,
            Self::Int(_) => Type::Int,
            Self::Double(_) => Type::Double,
        }
    }

    /// The value after the integral promotions (`bool` and `char` become
    /// `int`); `None` for a `double`.
    fn integer(self) -> Option<i64> {
        match self {
            Self::Bool(value) => Some(i64::from(value)),
            Self::Char(code) => Some(i64::from(code)),
            Self::Int(value) => Some(i64::from(value)),
            Self::Double(_) => None,
        }
    }

    /// The value as a `double`; exact for every `bool`, `char` and `int`.
    fn real(self) -> f64 {
        match self {
            Self::Double(value) => value,
            // A `bool`, `char` or `int` always fits an `i32`.
            other => other.integer().and_then(int_of).map_or(0.0, f64::from),
        }
    }

    /// The value as a condition: anything but zero is true.
    fn truth(self) -> bool {
        match self {
            Self::Bool(value) => value,
            Self::Double(value) => value.classify() != FpCategory::Zero,
            other => other.integer() != Some(0),
        }
    }

    /// Whether the value is zero (of any type).
    fn is_zero(self) -> bool {
        !self.truth()
    }

    /// The value converted to `to` as C++ converts it (implicitly, or with
    /// `static_cast`), when the result is defined and the same on every
    /// target: `None` for a `double` whose whole part does not fit an `int`,
    /// a `char` outside ASCII, and every type that is not a number or `bool`.
    pub(crate) fn convert(self, to: &Type) -> Option<Self> {
        match to {
            Type::Bool => Some(Self::Bool(self.truth())),
            Type::Double => Some(Self::Double(self.real())),
            Type::Int => match self {
                Self::Double(value) => truncate(value).map(Self::Int),
                other => other.integer().and_then(int_of).map(Self::Int),
            },
            Type::Char => {
                let whole = match self {
                    Self::Double(value) => truncate(value).map(i64::from),
                    other => other.integer(),
                }?;
                u8::try_from(whole).ok().filter(u8::is_ascii).map(Self::Char)
            }
            _ => None,
        }
    }
}

/// An `i64` as an `int`, when it fits.
fn int_of(value: i64) -> Option<i32> {
    i32::try_from(value).ok()
}

/// A `double`'s whole part as an `int` (C++ truncates towards zero), when it
/// fits; C++ leaves the conversion undefined otherwise.
fn truncate(value: f64) -> Option<i32> {
    let whole = value.trunc();
    // Both bounds are exact in a `double`.
    if whole.is_finite() && whole >= f64::from(i32::MIN) && whole <= f64::from(i32::MAX) {
        #[allow(clippy::cast_possible_truncation, reason = "the range is checked above")]
        Some(whole as i32)
    } else {
        None
    }
}

/// What the analyser knows about a constant value (types.md §11.2).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Constant {
    /// The value, of the expression's type (or the variable's).
    pub value: ConstValue,
    /// Whether C++ counts it as a constant expression too. For an expression:
    /// whether it is one (it differs from the analyser's answer only when it
    /// reads a `const double` variable, which C++ allows in constant
    /// expressions only when it is `constexpr`). For a `const` variable:
    /// whether its starting value is one, so that C++ sets it before any code
    /// runs (constant initialisation); such a variable of type `bool`,
    /// `char` or `int` may also be used in constant expressions.
    pub constant_expression: bool,
}

/// The constant value of an expression, if it has one, without reporting
/// anything: the evaluation the analyser runs on every expression. `constants`
/// holds the `const` variables whose value is known
/// ([`crate::Analysis::constants`]).
pub fn constant_value(expr: &Expr, constants: &BTreeMap<SymbolId, Constant>) -> Option<Constant> {
    Folder {
        constants,
        diags: None,
        context: None,
    }
    .eval(expr)
}

/// What the checks read besides the expression: the symbols (for a
/// function's parameter types) and the blocks the catalog kept unchecked,
/// whose own expressions report nothing.
#[derive(Clone, Copy)]
struct Context<'a> {
    symbols: &'a SymbolTable,
    unchecked: &'a BTreeSet<BlockId>,
}

/// Checks one lowered item and records its `const` variables' values in
/// `constants`. The item's nesting must have been checked (`nesting.rs`),
/// which bounds the recursion. `unchecked` is the blocks the catalog kept
/// unchecked: what they hold themselves is not reported (ADR-0018 §3).
pub(crate) fn check_item(
    item: &Item,
    symbols: &SymbolTable,
    unchecked: &BTreeSet<BlockId>,
    diags: &mut Diags,
    constants: &mut BTreeMap<SymbolId, Constant>,
) {
    let (body, ret) = match &item.kind {
        ItemKind::Main(main) => (&main.body, Type::Int),
        ItemKind::Function(function) => (&function.body, function.ret.clone()),
        ItemKind::Global(global) => {
            // A global's starting value converts to its type; recording a
            // constant global's value is W2's globals' (package C2).
            let mut checker = Checker {
                context: Context { symbols, unchecked },
                diags,
                constants,
                ret: Type::Void,
            };
            if let Some(init) = &global.init {
                let ty = symbols.get(&global.symbol).map(|symbol| symbol.ty.clone());
                checker.eval_into(init, ty.as_ref());
            }
            return;
        }
        ItemKind::Struct(def) => {
            let mut checker = Checker {
                context: Context { symbols, unchecked },
                diags,
                constants,
                ret: Type::Void,
            };
            for (field, value) in &def.defaults {
                let ty = symbols.get(field).map(|symbol| symbol.ty.clone());
                checker.eval_into(value, ty.as_ref());
            }
            return;
        }
        ItemKind::Enum(_) | ItemKind::UseHeader { .. } => return,
    };
    let mut checker = Checker {
        context: Context { symbols, unchecked },
        diags,
        constants,
        ret,
    };
    checker.block(body);
}

/// What tells two `case` labels apart (w2.md §2.9): the value of a whole
/// number or character after the integral promotions, or an enum value's
/// enumerator.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum LabelKey {
    /// A whole number or a character.
    Integer(i64),
    /// An enum value.
    Enumerator(SymbolId),
}

/// The statement walk of [`check_item`].
struct Checker<'a> {
    context: Context<'a>,
    diags: &'a mut Diags,
    constants: &'a mut BTreeMap<SymbolId, Constant>,
    /// The item's return type: what a `return` converts its value to.
    ret: Type,
}

impl Checker<'_> {
    fn eval(&mut self, expr: &Expr) -> Option<Constant> {
        self.eval_into(expr, None)
    }

    /// [`Self::eval`] for a value C++ converts to `into`.
    fn eval_into(&mut self, expr: &Expr, into: Option<&Type>) -> Option<Constant> {
        Folder {
            constants: &*self.constants,
            diags: Some(&mut *self.diags),
            context: Some(self.context),
        }
        .eval_into(expr, into)
    }

    /// The type a statement converts a value to, read from `symbol`: not
    /// known for the statement of a block the catalog kept unchecked.
    fn type_of(&self, stmt: &Stmt, symbol: &SymbolId) -> Option<Type> {
        if self.context.unchecked.contains(&stmt.origin.block) {
            return None;
        }
        self.context.symbols.get(symbol).map(|symbol| symbol.ty.clone())
    }

    /// `ty`, unless `stmt` is the statement of a block the catalog kept
    /// unchecked.
    fn checked_type(&self, stmt: &Stmt, ty: Type) -> Option<Type> {
        (!self.context.unchecked.contains(&stmt.origin.block)).then_some(ty)
    }

    fn block(&mut self, block: &Block) {
        for stmt in &block.stmts {
            self.stmt(stmt);
        }
    }

    #[allow(clippy::too_many_lines, reason = "one arm per statement")]
    fn stmt(&mut self, stmt: &Stmt) {
        match &stmt.kind {
            StmtKind::VarDecl(decl) => {
                let Some(init) = &decl.init else { return };
                let value = self.eval_into(init, self.type_of(stmt, &decl.symbol).as_ref());
                let ty = self
                    .context
                    .symbols
                    .get(&decl.symbol)
                    .map(|symbol| symbol.ty.clone());
                if decl.is_const
                    && let (Some(value), Some(ty)) = (value, ty)
                    && let Some(converted) = value.value.convert(&ty)
                {
                    self.constants.insert(
                        decl.symbol.clone(),
                        Constant {
                            value: converted,
                            constant_expression: value.constant_expression,
                        },
                    );
                }
            }
            StmtKind::Assign { target, value } => {
                let ty = self.type_of(stmt, target);
                self.eval_into(value, ty.as_ref());
            }
            StmtKind::CompoundAssign { target, op, value } => self.compound(stmt, target, *op, value),
            StmtKind::If { branches, else_body } => {
                for branch in branches {
                    self.eval(&branch.cond);
                    self.block(&branch.body);
                }
                if let Some(body) = else_body {
                    self.block(body);
                }
            }
            StmtKind::While { cond, body, .. } => {
                self.eval(cond);
                self.block(body);
            }
            StmtKind::Repeat { count, body } => {
                let ty = self.checked_type(stmt, Type::Int);
                self.eval_into(count, ty.as_ref());
                self.block(body);
            }
            StmtKind::ForRange {
                var,
                from,
                to,
                step,
                body,
                ..
            } => {
                let ty = self.type_of(stmt, var);
                for value in [Some(from), Some(to), step.as_ref()].into_iter().flatten() {
                    self.eval_into(value, ty.as_ref());
                }
                self.block(body);
            }
            StmtKind::Forever { body } => self.block(body),
            StmtKind::Break
            | StmtKind::Continue
            | StmtKind::Rethrow
            | StmtKind::FallThrough
            | StmtKind::Flip { .. } => {}
            StmtKind::Return { value } => {
                if let Some(value) = value {
                    let ty = self.checked_type(stmt, self.ret.clone());
                    self.eval_into(value, ty.as_ref());
                }
            }
            StmtKind::Exit { code, .. } => {
                if let Some(code) = code {
                    let ty = self.checked_type(stmt, Type::Int);
                    self.eval_into(code, ty.as_ref());
                }
            }
            StmtKind::Eval { expr } => {
                self.eval(expr);
            }
            StmtKind::Print { items, .. } | StmtKind::WriteFile { items, .. } => {
                for item in items {
                    self.eval(item);
                }
            }
            StmtKind::Ask { prompt, .. } => {
                if let Some(prompt) = prompt {
                    self.eval(prompt);
                }
            }
            StmtKind::Switch {
                value,
                cases,
                default,
            } => {
                self.eval(value);
                self.switch_labels(value, cases);
                for case in cases {
                    self.block(&case.body);
                }
                if let Some(body) = default {
                    self.block(body);
                }
            }
            StmtKind::StaticCheck { cond, .. } => self.static_check(cond),
            StmtKind::DoWhile { body, cond, .. } => {
                self.block(body);
                self.eval(cond);
            }
            StmtKind::RangeFor { range, body, .. } => {
                self.eval(range);
                self.block(body);
            }
            StmtKind::Try { body, catches } => {
                self.block(body);
                for catch in catches {
                    self.block(&catch.body);
                }
            }
            StmtKind::FileLines {
                path, body, failed, ..
            } => {
                self.eval(path);
                if let Some(failed) = failed {
                    self.block(failed);
                }
                self.block(body);
            }
            StmtKind::OpenFile { path, failed, .. } => {
                self.eval(path);
                if let Some(failed) = failed {
                    self.block(failed);
                }
            }
            StmtKind::CollectionOp { op, .. } => {
                for value in op.operands() {
                    self.eval(value);
                }
            }
            StmtKind::SetField { field, value, .. } => {
                let ty = self.type_of(stmt, field);
                self.eval_into(value, ty.as_ref());
            }
            StmtKind::Decimals { digits: value }
            | StmtKind::Wait { seconds: value }
            | StmtKind::Throw { message: value, .. } => {
                self.eval(value);
            }
        }
    }

    /// The labels of a switch: each a constant expression of a whole
    /// number or a character, or an enum value (`B2C-E0424`), and no two
    /// with the same value (`B2C-E0425`). A label of type `Error` was
    /// reported already.
    fn switch_labels(&mut self, value: &Expr, cases: &[SwitchCase]) {
        let mut seen: BTreeSet<LabelKey> = BTreeSet::new();
        let into = (value.ty != Type::Error).then_some(&value.ty);
        for case in cases {
            let label = &case.label;
            if label.ty == Type::Error {
                continue;
            }
            let key = match &label.kind {
                ExprKind::Enumerator(sym) => Some(LabelKey::Enumerator(sym.clone())),
                _ => self
                    .eval_into(label, into)
                    .filter(|constant| constant.constant_expression)
                    .and_then(|constant| match constant.value {
                        ConstValue::Int(_) | ConstValue::Char(_) => constant.value.integer(),
                        ConstValue::Bool(_) | ConstValue::Double(_) => None,
                    })
                    .map(LabelKey::Integer),
            };
            let Some(key) = key else {
                self.diags.error(
                    codes::NOT_CONSTANT,
                    label.origin.location(),
                    "A 'case' label must be a value that is fixed when the program is built: a whole number, a \
                     character, a value of an enum, or a constant whole number or character with a fixed value.",
                );
                continue;
            };
            if seen.contains(&key) {
                let shown = match &key {
                    LabelKey::Integer(number) => number.to_string(),
                    LabelKey::Enumerator(sym) => self
                        .context
                        .symbols
                        .get(sym)
                        .map(|symbol| symbol.name.as_str().to_owned())
                        .unwrap_or_default(),
                };
                self.diags.error(
                    codes::DUPLICATE_CASE,
                    label.origin.location(),
                    format!(
                        "Another 'case' of this switch has the same value ({shown}). Each case needs its own value: \
                         remove one of them, or put their blocks together in one case."
                    ),
                );
            } else {
                seen.insert(key);
            }
        }
    }

    /// *check at compile time*: a condition fixed when the program is built
    /// (`B2C-E0424`) that is not `false` (`B2C-E0428`).
    fn static_check(&mut self, cond: &Expr) {
        if cond.ty == Type::Error {
            return;
        }
        match self.eval_into(cond, Some(&Type::Bool)) {
            Some(constant) if constant.constant_expression => {
                if !constant.value.truth() {
                    self.diags.error(
                        codes::CHECK_FAILED,
                        cond.origin.location(),
                        "This check fails when the program is built: its condition is false. Fix the values it \
                         checks, or remove the check.",
                    );
                }
            }
            _ => self.diags.error(
                codes::NOT_CONSTANT,
                cond.origin.location(),
                "The condition of 'check at compile time' must be fixed when the program is built: use numbers, \
                 characters, true/false values and constants with fixed values, not variables or the answers \
                 of functions.",
            ),
        }
    }
}

impl Checker<'_> {
    /// `change … by`: a division or `mod` by a constant zero.
    fn compound(&mut self, stmt: &Stmt, target: &SymbolId, op: CompoundOp, value: &Expr) {
        let divisor = self.eval(value);
        let divides = matches!(op, CompoundOp::Div | CompoundOp::Mod);
        if !divides || !divisor.is_some_and(|divisor| divisor.value.is_zero()) {
            return;
        }
        // `x /= v` divides as `x / v` does: as whole numbers when both are,
        // else as decimal numbers, whose infinity or NaN is then stored in
        // `x`. The `change` of a block the catalog kept unchecked follows a
        // definition this catalog does not have: its target type is not
        // known here.
        let location = value.origin.location();
        let whole = |ty: &Type| matches!(ty, Type::Int | Type::Char | Type::Bool);
        match self.type_of(stmt, target) {
            Some(ty) if whole(&ty) && whole(&value.ty) => {
                whole_division(self.diags, location, op == CompoundOp::Mod);
            }
            Some(Type::Int | Type::Char) if op == CompoundOp::Div => {
                decimal_division_into_whole(self.diags, location);
            }
            Some(Type::Double) if op == CompoundOp::Div => decimal_division(self.diags, location),
            _ => {}
        }
    }
}

/// Reports a whole-number division (or `mod`) by a constant zero.
fn whole_division(diags: &mut Diags, location: Location, remainder: bool) {
    let what = if remainder {
        "This works out the remainder ('mod') of a division by zero"
    } else {
        "This divides a whole number by zero"
    };
    diags.error(
        codes::DIVISION_BY_ZERO,
        location,
        format!(
            "{what}: the value it divides by is always 0. That stops the program with a crash. Check the value \
             you divide by."
        ),
    );
}

/// Reports a decimal division by a constant zero.
fn decimal_division(diags: &mut Diags, location: Location) {
    diags.warning(
        codes::NOT_FINITE,
        location,
        "This divides by zero: the value it divides by is always 0, so the result is infinity or 'nan' (not a \
         number) instead of a number you can use. Check the value you divide by.",
    );
}

/// Reports a decimal division by a constant zero whose result goes straight
/// into a whole number: infinity and NaN do not fit one, and C++ leaves the
/// conversion undefined (as for `convert`, `B2C-E0517`).
fn decimal_division_into_whole(diags: &mut Diags, location: Location) {
    diags.error(
        codes::LITERAL_OVERFLOW,
        location,
        "This divides by zero: the value it divides by is always 0, so the result is infinity or 'nan' (not a \
         number), and it is then stored as a whole number, which cannot hold either, so C++ gives an \
         unpredictable result. Check the value you divide by.",
    );
}

/// Whether C++ converts a value to `ty` as a whole number (a `bool` takes
/// infinity and NaN as `true`, which is defined).
fn whole_number(ty: Option<&Type>) -> bool {
    matches!(ty, Some(Type::Int | Type::Char))
}

/// A whole number for a message.
fn whole_limit(value: i64) -> String {
    if value > i64::from(i32::MAX) {
        format!("{value}, which is too large for a whole number (the largest is 2147483647)")
    } else {
        format!("{value}, which is too small for a whole number (the smallest is -2147483648)")
    }
}

/// A decimal number for a message: plain digits up to 15 of them, else with
/// an exponent (`1e300`).
fn decimal_shown(value: f64) -> String {
    if value.abs() < 1e15 {
        format!("{value}")
    } else {
        format!("{value:e}")
    }
}

/// The evaluation of one expression.
struct Folder<'a, 'd> {
    constants: &'a BTreeMap<SymbolId, Constant>,
    /// Where reports go; `None` evaluates without reporting.
    diags: Option<&'d mut Diags>,
    /// The symbols and unchecked blocks while checking; `None` evaluates
    /// without them.
    context: Option<Context<'a>>,
}

impl Folder<'_, '_> {
    /// The value of `expr`, after checking it and the operands C++
    /// evaluates.
    fn eval(&mut self, expr: &Expr) -> Option<Constant> {
        self.eval_into(expr, None)
    }

    /// [`Self::eval`] for a value C++ then converts to `into` (`None`: not
    /// converted, or not known): a decimal division by zero that goes into a
    /// whole number is an error, not a warning.
    #[allow(clippy::too_many_lines, reason = "one arm per expression")]
    fn eval_into(&mut self, expr: &Expr, into: Option<&Type>) -> Option<Constant> {
        let foldable = matches!(expr.ty, Type::Bool | Type::Char | Type::Int | Type::Double);
        match &expr.kind {
            ExprKind::Int(literal) if expr.ty == Type::Int => literal
                .as_str()
                .parse::<i32>()
                .ok()
                .map(|value| exact(ConstValue::Int(value))),
            ExprKind::Float(literal) if expr.ty == Type::Double => literal
                .as_str()
                .parse::<f64>()
                .ok()
                .filter(|value| value.is_finite())
                .map(|value| exact(ConstValue::Double(value))),
            ExprKind::Bool(value) if expr.ty == Type::Bool => Some(exact(ConstValue::Bool(*value))),
            ExprKind::Char(literal) if expr.ty == Type::Char => u8::try_from(literal.value())
                .ok()
                .filter(u8::is_ascii)
                .map(|code| exact(ConstValue::Char(code))),
            ExprKind::Var(sym) if foldable => self.variable(sym, &expr.ty),
            ExprKind::Unary { op, operand } => {
                // A sign keeps infinity and NaN what they are.
                let into = if *op == UnaryOp::Not { None } else { into };
                let value = self.eval_into(operand, into);
                if foldable {
                    self.unary(*op, value?, expr)
                } else {
                    None
                }
            }
            ExprKind::Binary { op, lhs, rhs } => self.binary(*op, lhs, rhs, expr, whole_number(into)),
            ExprKind::Conditional {
                cond,
                then_value,
                else_value,
            } => {
                // A constant condition: only the chosen value is evaluated.
                let Some(cond) = self.eval(cond) else {
                    self.eval(then_value);
                    self.eval(else_value);
                    return None;
                };
                let chosen = if cond.value.truth() {
                    then_value
                } else {
                    else_value
                };
                let value = self.eval_into(chosen, into)?;
                if !foldable {
                    return None;
                }
                Some(Constant {
                    value: value.value.convert(&expr.ty)?,
                    constant_expression: cond.constant_expression && value.constant_expression,
                })
            }
            ExprKind::Convert { to, value } => self.convert(to, value, expr),
            ExprKind::Call { function, args } => {
                if self.diags.is_some() {
                    let params = if self.unchecked(expr) {
                        None
                    } else {
                        self.parameter_types(function)
                    };
                    for (index, arg) in args.iter().enumerate() {
                        let ty = params.as_ref().and_then(|params| params.get(index));
                        self.eval_into(arg, ty);
                    }
                }
                None
            }
            ExprKind::Join(items) => {
                if self.diags.is_some() {
                    for item in items {
                        self.eval(item);
                    }
                }
                None
            }
            ExprKind::RandomInt { low, high } => {
                if self.diags.is_some() {
                    let into = (!self.unchecked(expr)).then_some(&Type::Int);
                    self.eval_into(low, into);
                    self.eval_into(high, into);
                }
                None
            }
            ExprKind::Int(_)
            | ExprKind::Float(_)
            | ExprKind::Bool(_)
            | ExprKind::Char(_)
            | ExprKind::Str(_)
            | ExprKind::Var(_) => None,
            // W2's values are never constants here (an enum value is told
            // apart by its enumerator); their operands are checked.
            ExprKind::Item { .. }
            | ExprKind::Size(_)
            | ExprKind::IsEmpty(_)
            | ExprKind::Contains { .. }
            | ExprKind::Next(_)
            | ExprKind::ListOf(_)
            | ExprKind::Field { .. }
            | ExprKind::New { .. }
            | ExprKind::Enumerator(_)
            | ExprKind::Letter { .. }
            | ExprKind::TextLength(_)
            | ExprKind::TextCall { .. }
            | ExprKind::ToNumber { .. }
            | ExprKind::AsText(_)
            | ExprKind::ErrorMessage(_)
            | ExprKind::MathCall { .. }
            | ExprKind::RandomReal { .. }
            | ExprKind::ReadLine { .. }
            | ExprKind::FileExists(_) => {
                if self.diags.is_some() {
                    for child in expr.kind.children() {
                        self.eval(child);
                    }
                }
                None
            }
        }
    }

    /// *convert*: a constant decimal number that does not fit an `int`.
    fn convert(&mut self, to: &Type, value: &Expr, expr: &Expr) -> Option<Constant> {
        let into = (!self.unchecked(expr)).then_some(to);
        let value = self.eval_into(value, into)?;
        if *to == Type::Int
            && let ConstValue::Double(real) = value.value
            && truncate(real).is_none()
        {
            if let Some(diags) = self.diags_at(expr) {
                diags.error(
                    codes::LITERAL_OVERFLOW,
                    expr.origin.location(),
                    format!(
                        "This turns {} into a whole number, but it does not fit in one (whole numbers go from \
                         -2147483648 to 2147483647), so C++ gives an unpredictable result. Use smaller values, or \
                         keep it a decimal number.",
                        decimal_shown(real)
                    ),
                );
            }
            return None;
        }
        Some(Constant {
            value: value.value.convert(to)?,
            constant_expression: value.constant_expression,
        })
    }

    /// Where a report about `expr` goes: nowhere while evaluating without
    /// reports, and nowhere for an expression of a block the catalog kept
    /// unchecked (a block nested in one is checked as usual).
    fn diags_at(&mut self, expr: &Expr) -> Option<&mut Diags> {
        if self.unchecked(expr) {
            return None;
        }
        self.diags.as_deref_mut()
    }

    /// Whether `expr` belongs to a block the catalog kept unchecked, whose
    /// types (a conversion's, a call's parameters) are not to be trusted.
    fn unchecked(&self, expr: &Expr) -> bool {
        self.context
            .is_some_and(|context| context.unchecked.contains(&expr.origin.block))
    }

    /// The types of a function's parameters, in order, when they are known.
    fn parameter_types(&self, function: &SymbolId) -> Option<Vec<Type>> {
        let symbols = self.context?.symbols;
        let SymbolKind::Function { params } = &symbols.get(function)?.kind else {
            return None;
        };
        params
            .iter()
            .map(|param| symbols.get(param).map(|param| param.ty.clone()))
            .collect()
    }

    /// A `const` variable with a known value. Only an integral one is a
    /// constant expression in C++ (a `const double` needs `constexpr`).
    fn variable(&self, sym: &SymbolId, ty: &Type) -> Option<Constant> {
        let constant = self.constants.get(sym)?;
        Some(Constant {
            value: constant.value.convert(ty)?,
            constant_expression: constant.constant_expression
                && matches!(ty, Type::Bool | Type::Char | Type::Int),
        })
    }

    fn unary(&mut self, op: UnaryOp, operand: Constant, expr: &Expr) -> Option<Constant> {
        let value = match (op, &expr.ty) {
            (UnaryOp::Not, Type::Bool) => ConstValue::Bool(!operand.value.truth()),
            (UnaryOp::Plus, ty) => operand.value.convert(ty)?,
            (UnaryOp::Neg, Type::Double) => ConstValue::Double(-operand.value.real()),
            (UnaryOp::Neg, Type::Int) => {
                let negated = -operand.value.integer()?;
                self.int_result(negated, expr)?
            }
            _ => return None,
        };
        Some(Constant {
            value,
            constant_expression: operand.constant_expression,
        })
    }

    /// A binary operation; `into_whole` when its value goes straight into a
    /// whole number.
    fn binary(
        &mut self,
        op: BinaryOp,
        lhs: &Expr,
        rhs: &Expr,
        expr: &Expr,
        into_whole: bool,
    ) -> Option<Constant> {
        if matches!(op, BinaryOp::And | BinaryOp::Or) {
            return self.logical(op, lhs, rhs, expr);
        }
        let left = self.eval(lhs);
        let right = self.eval(rhs);
        if matches!(op, BinaryOp::Div | BinaryOp::Mod) && right.is_some_and(|right| right.value.is_zero()) {
            if let Some(diags) = self.diags_at(expr) {
                match expr.ty {
                    Type::Int => whole_division(diags, expr.origin.location(), op == BinaryOp::Mod),
                    Type::Double if into_whole => decimal_division_into_whole(diags, expr.origin.location()),
                    Type::Double => decimal_division(diags, expr.origin.location()),
                    _ => {}
                }
            }
            return None;
        }
        let (left, right) = (left?, right?);
        let constant_expression = left.constant_expression && right.constant_expression;
        let value = match (op, &expr.ty) {
            (BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div, Type::Double) => {
                let (a, b) = (left.value.real(), right.value.real());
                let result = match op {
                    BinaryOp::Add => a + b,
                    BinaryOp::Sub => a - b,
                    BinaryOp::Mul => a * b,
                    _ => a / b,
                };
                if !result.is_finite() {
                    if let Some(diags) = self.diags_at(expr) {
                        diags.warning(
                            codes::NOT_FINITE,
                            expr.origin.location(),
                            "This calculation gives a result too large for a decimal number (the largest is about \
                             1.8e308), so it becomes infinity. Check the values.",
                        );
                    }
                    return None;
                }
                ConstValue::Double(result)
            }
            (BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div | BinaryOp::Mod, Type::Int) => {
                let (a, b) = (left.value.integer()?, right.value.integer()?);
                if op == BinaryOp::Mod && b == -1 && a == i64::from(i32::MIN) {
                    // `a / b` does not fit, so C++ leaves `a % b` undefined too.
                    self.report_overflow(
                        expr,
                        String::from(
                            "This works out -2147483648 mod -1, which C++ leaves undefined because \
                             -2147483648 / -1 (2147483648) is too large for a whole number (the largest is \
                             2147483647). Use smaller values.",
                        ),
                    );
                    return None;
                }
                // Exact in an `i64`; `/` and `%` truncate towards zero, as in C++.
                let result = match op {
                    BinaryOp::Add => a + b,
                    BinaryOp::Sub => a - b,
                    BinaryOp::Mul => a * b,
                    BinaryOp::Div => a / b,
                    _ => a % b,
                };
                self.int_result(result, expr)?
            }
            (
                BinaryOp::Lt | BinaryOp::Le | BinaryOp::Gt | BinaryOp::Ge | BinaryOp::Eq | BinaryOp::Ne,
                Type::Bool,
            ) => ConstValue::Bool(compare(op, left.value, right.value)?),
            _ => return None,
        };
        Some(Constant {
            value,
            constant_expression,
        })
    }

    /// `&&` and `||`: the right side is evaluated (and checked) only when the
    /// left side does not decide, as in C++.
    fn logical(&mut self, op: BinaryOp, lhs: &Expr, rhs: &Expr, expr: &Expr) -> Option<Constant> {
        let left = self.eval(lhs);
        let decides = op == BinaryOp::Or;
        if let Some(left) = left
            && left.value.truth() == decides
        {
            return (expr.ty == Type::Bool).then_some(Constant {
                value: ConstValue::Bool(decides),
                constant_expression: left.constant_expression,
            });
        }
        let right = self.eval(rhs);
        let (left, right) = (left?, right?);
        (expr.ty == Type::Bool).then_some(Constant {
            value: ConstValue::Bool(right.value.truth()),
            constant_expression: left.constant_expression && right.constant_expression,
        })
    }

    /// An exact whole-number result as an `int`, or a report that it does
    /// not fit.
    fn int_result(&mut self, value: i64, expr: &Expr) -> Option<ConstValue> {
        if let Some(value) = int_of(value) {
            return Some(ConstValue::Int(value));
        }
        self.report_overflow(
            expr,
            format!(
                "This calculation gives {}, so C++ gives an unpredictable result. Use smaller values, or decimal \
                 numbers.",
                whole_limit(value)
            ),
        );
        None
    }

    fn report_overflow(&mut self, expr: &Expr, message: String) {
        if let Some(diags) = self.diags_at(expr) {
            diags.error(codes::LITERAL_OVERFLOW, expr.origin.location(), message);
        }
    }
}

/// A literal's value: always a constant expression.
fn exact(value: ConstValue) -> Constant {
    Constant {
        value,
        constant_expression: true,
    }
}

/// A comparison after the usual arithmetic conversions: as `double`s when
/// one side is a `double`, else as whole numbers.
fn compare(op: BinaryOp, a: ConstValue, b: ConstValue) -> Option<bool> {
    let ordering = match (a.integer(), b.integer()) {
        (Some(x), Some(y)) => Some(x.cmp(&y)),
        _ => a.real().partial_cmp(&b.real()),
    };
    // A NaN compares unequal to everything; folded values are never NaN.
    let ordering = ordering?;
    Some(match op {
        BinaryOp::Lt => ordering.is_lt(),
        BinaryOp::Le => ordering.is_le(),
        BinaryOp::Gt => ordering.is_gt(),
        BinaryOp::Ge => ordering.is_ge(),
        BinaryOp::Eq => ordering.is_eq(),
        _ => ordering.is_ne(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conversions_follow_cpp() {
        use ConstValue as V;
        assert_eq!(V::Double(2.9).convert(&Type::Int), Some(V::Int(2)));
        assert_eq!(V::Double(-2.9).convert(&Type::Int), Some(V::Int(-2)));
        assert_eq!(
            V::Double(2_147_483_647.5).convert(&Type::Int),
            Some(V::Int(i32::MAX))
        );
        assert_eq!(V::Double(2_147_483_648.0).convert(&Type::Int), None);
        assert_eq!(
            V::Double(-2_147_483_648.9).convert(&Type::Int),
            Some(V::Int(i32::MIN))
        );
        assert_eq!(V::Double(-2_147_483_649.0).convert(&Type::Int), None);
        assert_eq!(V::Int(65).convert(&Type::Char), Some(V::Char(b'A')));
        assert_eq!(V::Int(200).convert(&Type::Char), None, "depends on the target");
        assert_eq!(V::Int(-1).convert(&Type::Char), None);
        assert_eq!(V::Char(b'a').convert(&Type::Int), Some(V::Int(97)));
        assert_eq!(V::Bool(true).convert(&Type::Double), Some(V::Double(1.0)));
        assert_eq!(V::Double(-0.0).convert(&Type::Bool), Some(V::Bool(false)));
        assert_eq!(V::Int(3).convert(&Type::Bool), Some(V::Bool(true)));
        assert_eq!(V::Int(3).convert(&Type::String), None);
        assert_eq!(V::Char(b'x').ty(), Type::Char);
    }

    #[test]
    fn comparisons_use_the_usual_conversions() {
        use ConstValue as V;
        assert_eq!(compare(BinaryOp::Lt, V::Int(1), V::Double(1.5)), Some(true));
        assert_eq!(compare(BinaryOp::Eq, V::Char(b'A'), V::Int(65)), Some(true));
        assert_eq!(compare(BinaryOp::Ge, V::Bool(true), V::Int(1)), Some(true));
        assert_eq!(compare(BinaryOp::Ne, V::Double(0.5), V::Double(0.5)), Some(false));
        assert_eq!(compare(BinaryOp::Gt, V::Double(f64::NAN), V::Int(0)), None);
    }

    #[test]
    fn messages_show_numbers_plainly() {
        assert_eq!(decimal_shown(1e10), "10000000000");
        assert_eq!(decimal_shown(1e300), "1e300");
        assert!(whole_limit(2_147_483_648).contains("too large"));
        assert!(whole_limit(-2_147_483_649).contains("too small"));
    }
}
