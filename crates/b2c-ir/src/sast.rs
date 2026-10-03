//! The Semantic AST (SAST): the language-level program produced by analysis
//! (b2c-lang) and consumed by code generation (b2c-codegen). Spec §6.4.
//!
//! The SAST sits closer to the blocks than to C++ syntax: `Repeat`, `Print` or
//! `Ask` are single nodes that the generator desugars into idiomatic C++. Names
//! live in the [`SymbolTable`]; nodes refer to symbols by [`SymbolId`].
//!
//! Every node that came from a block carries an [`Origin`] so the generator can
//! build source maps and diagnostics can point back at blocks.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::diag::{Location, Part};
use crate::ids::{BlockId, ModuleId, SymbolId};
use crate::text::{CharLit, Comment, Ident, NumLit, StrLit};
use crate::types::Type;

/// The C++ language standard a project targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Default)]
pub enum CppStandard {
    /// C++17.
    #[serde(rename = "c++17")]
    Cpp17,
    /// C++20 (default).
    #[default]
    #[serde(rename = "c++20")]
    Cpp20,
    /// C++23.
    #[serde(rename = "c++23")]
    Cpp23,
    /// C++26.
    #[serde(rename = "c++26")]
    Cpp26,
}

/// Where a node came from.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
pub struct Origin {
    /// The module containing the block.
    pub module: ModuleId,
    /// The block.
    pub block: BlockId,
    /// The part of the block.
    pub part: Part,
}

impl Origin {
    /// The whole of a block.
    pub fn whole(module: ModuleId, block: BlockId) -> Self {
        Self { module, block, part: Part::Whole }
    }

    /// The diagnostic location for this origin.
    pub fn location(&self) -> Location {
        Location { module: Some(self.module.clone()), block: Some(self.block.clone()), part: self.part.clone() }
    }
}

/// A whole analysed program.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Program {
    /// The language standard to generate for.
    pub standard: CppStandard,
    /// Modules in project order. Each becomes a `.cpp` file (and a `.hpp` file
    /// when it shares definitions, from milestone M3).
    pub modules: Vec<Module>,
    /// Every symbol in the program.
    pub symbols: SymbolTable,
}

/// One module.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Module {
    /// Module ID.
    pub id: ModuleId,
    /// Module name, already validated as a file stem (`[a-z][a-z0-9_-]*`).
    pub name: String,
    /// Top-level items, in a deterministic order (by block ID).
    pub items: Vec<Item>,
}

/// A top-level definition.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Item {
    /// What it defines.
    pub kind: ItemKind,
    /// The defining block.
    pub origin: Origin,
    /// The block's comment, emitted above the definition.
    pub comment: Option<Comment>,
}

/// Kinds of top-level definitions.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemKind {
    /// `int main()`.
    Main(MainDef),
    /// A user function.
    Function(FunctionDef),
}

/// The program entry point.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MainDef {
    /// Statements of `main`.
    pub body: Block,
}

/// A user-defined function.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FunctionDef {
    /// The function's symbol (name and return type are in the symbol table).
    pub symbol: SymbolId,
    /// Parameters in order.
    pub params: Vec<Param>,
    /// Return type (`Type::Void` for none).
    pub ret: Type,
    /// Function body.
    pub body: Block,
}

/// A function parameter. Its name and type are in the symbol table.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Param {
    /// The parameter's symbol.
    pub symbol: SymbolId,
    /// How the argument is passed.
    pub mode: PassMode,
    /// Where the parameter was declared (the function block, part = extra row).
    pub origin: Origin,
}

/// How an argument is passed (spec §3.7.7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PassMode {
    /// By value (`T`).
    Copy,
    /// By reference, editable (`T&`).
    Editable,
    /// By reference, read-only (`const T&`).
    ReadOnly,
}

/// A sequence of statements.
#[derive(Debug, Clone, PartialEq, Default, Serialize)]
pub struct Block {
    /// Statements in order.
    pub stmts: Vec<Stmt>,
}

/// One statement.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Stmt {
    /// What it does.
    pub kind: StmtKind,
    /// The block it came from.
    pub origin: Origin,
    /// The block's comment, emitted above the statement.
    pub comment: Option<Comment>,
}

/// Kinds of statements.
// Expressions are stored inline for simple pattern matching; statement lists are
// small, so the size difference between variants does not matter.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StmtKind {
    /// Declares a local variable (`int score = 0;`).
    VarDecl(VarDecl),
    /// `target = value;`
    Assign {
        /// Variable assigned to.
        target: SymbolId,
        /// New value.
        value: Expr,
    },
    /// `target op= value;` (`change by` is `Add`; the generator prints `++x;`
    /// when the value is the literal 1).
    CompoundAssign {
        /// Variable updated.
        target: SymbolId,
        /// Operator.
        op: CompoundOp,
        /// Right-hand side.
        value: Expr,
    },
    /// `if / else if / else`.
    If {
        /// Condition and body for `if` and each `else if`, in order (at least one).
        branches: Vec<IfBranch>,
        /// The `else` body, if any.
        else_body: Option<Block>,
    },
    /// `while (cond)`, or `repeat until` when `until` is true.
    While {
        /// Loop condition (for `until`, the loop stops when it becomes true).
        cond: Expr,
        /// Whether this is `repeat until`.
        until: bool,
        /// Loop body.
        body: Block,
    },
    /// `repeat (count) times`; the generator chooses a fresh counter name.
    Repeat {
        /// Number of iterations (`int`).
        count: Expr,
        /// Loop body.
        body: Block,
    },
    /// `for [var] from (from) [to|through|down to] (to) [step]`.
    ForRange {
        /// The loop variable's symbol (an `int`).
        var: SymbolId,
        /// Start value.
        from: Expr,
        /// End value.
        to: Expr,
        /// Step (positive; the direction gives the sign). `None` means 1.
        step: Option<Expr>,
        /// Direction and inclusiveness.
        direction: RangeDirection,
        /// Loop body.
        body: Block,
    },
    /// `while (true)`.
    Forever {
        /// Loop body.
        body: Block,
    },
    /// `break;`
    Break,
    /// `continue;`
    Continue,
    /// `return;` or `return value;`
    Return {
        /// Returned value, if any.
        value: Option<Expr>,
    },
    /// Stop the program with an exit code: `return code;` in `main`,
    /// `std::exit(code);` elsewhere.
    Exit {
        /// Exit code (`int`); `None` means 0.
        code: Option<Expr>,
        /// Whether the statement is directly inside `main` (not in a function).
        in_main: bool,
    },
    /// An expression evaluated for its effect (a call whose result is ignored).
    Eval {
        /// The expression.
        expr: Expr,
    },
    /// `print (a) (b) …`
    Print {
        /// Values to print, in order (at least one).
        items: Vec<Expr>,
        /// Text printed between items.
        separator: PrintSeparator,
        /// Whether to end the line.
        newline: bool,
        /// Output stream.
        stream: OutputStream,
    },
    /// `ask (prompt) and save answer in [var]`.
    Ask {
        /// Prompt printed first, if any (`std::string` or `char`).
        prompt: Option<Expr>,
        /// Variable that receives the answer.
        target: SymbolId,
        /// Input handling.
        mode: AskMode,
    },
}

/// A local variable declaration.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct VarDecl {
    /// The variable's symbol (name and resolved type are in the symbol table).
    pub symbol: SymbolId,
    /// Initial value; `None` value-initialises (`int x{};`).
    pub init: Option<Expr>,
    /// Whether it was declared `const`.
    pub is_const: bool,
    /// Whether the user chose `auto` (the generator then prints `auto`).
    pub written_auto: bool,
}

/// One `if` / `else if` arm.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct IfBranch {
    /// Condition (`bool`).
    pub cond: Expr,
    /// Body.
    pub body: Block,
}

/// Compound assignment operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CompoundOp {
    /// `+=`
    Add,
    /// `-=`
    Sub,
    /// `*=`
    Mul,
    /// `/=`
    Div,
    /// `%=`
    Mod,
}

/// `for` loop direction (spec §3.7.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RangeDirection {
    /// `to`: counts up, end excluded (`i < to`).
    UpExclusive,
    /// `through`: counts up, end included (`i <= to`).
    UpInclusive,
    /// `down to`: counts down, end included (`i >= to`).
    DownInclusive,
}

/// Separator between printed items.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PrintSeparator {
    /// Nothing between items.
    None,
    /// A single space.
    Space,
    /// `", "`.
    Comma,
}

/// Output stream for `print`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OutputStream {
    /// `std::cout`.
    Out,
    /// `std::cerr`.
    Err,
}

/// How `ask` reads input (spec §3.7.6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AskMode {
    /// Re-prompt until the input is valid; exit with "Input ended" at end of input.
    KeepAsking,
    /// Plain `std::cin >> x;` (or `std::getline` for text).
    Simple,
}

/// An expression with its type (filled in by type checking).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Expr {
    /// The expression.
    pub kind: ExprKind,
    /// Its static type; [`Type::Error`] after an error.
    pub ty: Type,
    /// Where it came from (a block, or a token range in an expression slot).
    pub origin: Origin,
}

/// Kinds of expressions.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExprKind {
    /// Integer literal.
    Int(NumLit),
    /// Floating-point literal.
    Float(NumLit),
    /// `true` / `false`.
    Bool(bool),
    /// String literal.
    Str(StrLit),
    /// Character literal.
    Char(CharLit),
    /// A variable or parameter.
    Var(SymbolId),
    /// Unary operator.
    Unary {
        /// Operator.
        op: UnaryOp,
        /// Operand.
        operand: Box<Expr>,
    },
    /// Binary operator (arithmetic, comparison or logical).
    Binary {
        /// Operator.
        op: BinaryOp,
        /// Left operand.
        lhs: Box<Expr>,
        /// Right operand.
        rhs: Box<Expr>,
    },
    /// `cond ? then : otherwise`.
    Conditional {
        /// Condition.
        cond: Box<Expr>,
        /// Value when true.
        then_value: Box<Expr>,
        /// Value when false.
        else_value: Box<Expr>,
    },
    /// Call of a user function.
    Call {
        /// The function's symbol.
        function: SymbolId,
        /// Arguments in order.
        args: Vec<Expr>,
    },
    /// Text join (`join (a) (b) …`); the result is `std::string`.
    Join(Vec<Expr>),
    /// A uniformly random integer in `[low, high]` (support helper).
    RandomInt {
        /// Lower bound (inclusive).
        low: Box<Expr>,
        /// Upper bound (inclusive).
        high: Box<Expr>,
    },
    /// Explicit numeric conversion (`static_cast<to>(value)`).
    Convert {
        /// Target type.
        to: Type,
        /// Value converted.
        value: Box<Expr>,
    },
}

/// Unary operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UnaryOp {
    /// `-x`
    Neg,
    /// `+x`
    Plus,
    /// `!x`
    Not,
}

/// Binary operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BinaryOp {
    /// `+`
    Add,
    /// `-`
    Sub,
    /// `*`
    Mul,
    /// `/`
    Div,
    /// `%`
    Mod,
    /// `<`
    Lt,
    /// `<=`
    Le,
    /// `>`
    Gt,
    /// `>=`
    Ge,
    /// `==`
    Eq,
    /// `!=`
    Ne,
    /// `&&`
    And,
    /// `||`
    Or,
}

/// Every symbol in a program, by ID.
#[derive(Debug, Clone, PartialEq, Default, Serialize)]
pub struct SymbolTable {
    /// Symbols by ID (sorted, for deterministic iteration).
    pub symbols: BTreeMap<SymbolId, Symbol>,
}

impl SymbolTable {
    /// Looks up a symbol.
    pub fn get(&self, id: &SymbolId) -> Option<&Symbol> {
        self.symbols.get(id)
    }
}

/// A named entity.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Symbol {
    /// The C++ name.
    pub name: Ident,
    /// What it is.
    pub kind: SymbolKind,
    /// Its type (for functions: the return type).
    pub ty: Type,
    /// Where it was declared.
    pub origin: Origin,
}

/// Kinds of symbols.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SymbolKind {
    /// A local variable.
    Variable {
        /// Declared `const`.
        is_const: bool,
    },
    /// A function parameter.
    Parameter {
        /// Passing mode.
        mode: PassMode,
    },
    /// A `for` loop counter.
    LoopVariable,
    /// A user function.
    Function {
        /// Parameter symbols in order.
        params: Vec<SymbolId>,
    },
}
