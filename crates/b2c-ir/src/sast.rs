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
use crate::headers::{Header, ItemHeaders};
use crate::ids::{BlockId, ModuleId, SymbolId};
use crate::layout::Layout;
use crate::text::{CharLit, Comment, Ident, NumLit, StrLit};
use crate::types::{Type, TypeFacts, UserKind, UserType};

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
        Self {
            module,
            block,
            part: Part::Whole,
        }
    }

    /// The diagnostic location for this origin.
    pub fn location(&self) -> Location {
        Location {
            module: Some(self.module.clone()),
            block: Some(self.block.clone()),
            part: self.part.clone(),
        }
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
    /// Which files each module gets, what they include and in which order
    /// their declarations are printed (ADR-0019). Not serialised in the SAST
    /// JSON: the layout has its own snapshots.
    #[serde(skip)]
    pub layout: Layout,
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
    /// The catalog headers of the blocks lowered into it, by where their
    /// code is printed (types.md §5.1). Serialised only when not empty.
    #[serde(skip_serializing_if = "ItemHeaders::is_empty")]
    pub headers: ItemHeaders,
    /// Whether the definition goes into its module's header (ADR-0019).
    /// Always `false` in a single-module project. Serialised only when
    /// `true`.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub shared: bool,
}

/// Kinds of top-level definitions.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemKind {
    /// `int main()`.
    Main(MainDef),
    /// A user function.
    Function(FunctionDef),
    /// A struct (`type.struct`, milestone M3's second wave).
    Struct(StructDef),
    /// An enum (`type.enum`).
    Enum(EnumDef),
    /// A global variable or constant (`var.global`).
    Global(GlobalDef),
    /// A standard header the user asked for (`program.use_header`): merged
    /// into the module's includes, printed nowhere else.
    UseHeader {
        /// The header, from the header table.
        header: Header,
    },
}

/// A struct's definition. Its name, fields and effective ticks are in the
/// symbol table (`SymbolKind::Struct`); the generator prints the fields in
/// row order with the operators the ticks promise (types.md §1.6).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct StructDef {
    /// The struct's symbol.
    pub symbol: SymbolId,
    /// The default member initialisers given in the rows, by field (`int
    /// mark = 0;`); a field without one is value-initialised (`int mark{};`).
    /// Serialised only when not empty.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub defaults: BTreeMap<SymbolId, Expr>,
}

/// An enum's definition (`enum class`). Its name and enumerators are in the
/// symbol table (`SymbolKind::Enum`).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct EnumDef {
    /// The enum's symbol.
    pub symbol: SymbolId,
}

/// A global variable or constant (`int count = 0;`, `constexpr double
/// kGravity = 9.81;`; w2.md §2.7). Its name, type and constness are in the
/// symbol table (`SymbolKind::Global`).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct GlobalDef {
    /// The global's symbol.
    pub symbol: SymbolId,
    /// Its starting value; `None` value-initialises (`int count{};`).
    pub init: Option<Expr>,
    /// A constant of type `bool`, `char`, `int` or `double` whose value is a
    /// constant expression (L10): printed `constexpr` instead of `const`.
    pub constexpr: bool,
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
    /// Its default value (w2.md §2.8), printed in the function's declaration
    /// only: a call may leave the argument out. Serialised only when set.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default: Option<Expr>,
}

/// How an argument is passed (spec §3.7.7).
///
/// Its JSON is `"copy"`, `"editable"` or `"read_only"`, also inside a
/// [`SymbolInfo`](crate::scope_info::SymbolInfo), which the editor reads back.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
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
    /// `switch (value) { case …: { … } default: { … } }` (*switch on*;
    /// w2.md §2.9). Each case is its own `{ }` scope; the generator adds the
    /// `break;` a case needs.
    Switch {
        /// The value switched on (`char`, `int` or an enum).
        value: Expr,
        /// The cases, in order.
        cases: Vec<SwitchCase>,
        /// The `default` part, if the block has one.
        default: Option<Block>,
    },
    /// `do { … } while (cond);`, or with `until` the loop that stops when
    /// `cond` becomes true (*do … then repeat*).
    DoWhile {
        /// Loop body: runs at least once.
        body: Block,
        /// Loop condition, which sees only what is visible before the block.
        cond: Expr,
        /// Whether the loop repeats *until* the condition holds.
        until: bool,
    },
    /// A range-`for` over a collection or text (*for each*, *for each key
    /// value*): `for (const auto& x : c)`, `for (const auto& [k, v] : m)`.
    RangeFor {
        /// The loop's variables: the item, or a map's key and value.
        vars: Vec<SymbolId>,
        /// The collection or text gone through.
        range: Expr,
        /// How each item is bound.
        mode: RangeMode,
        /// Loop body.
        body: Block,
    },
    /// `try { … } catch (const E& e) { … } …` (w2.md §2.5).
    Try {
        /// The `try` part.
        body: Block,
        /// The catch parts, tried in order (at least one).
        catches: Vec<Catch>,
    },
    /// `throw E(message);` (*throw … with message*): ends its path.
    Throw {
        /// The error type: `std::runtime_error`, `std::invalid_argument` or
        /// `std::out_of_range`.
        error: Type,
        /// The message (`std::string`).
        message: Expr,
    },
    /// `throw;` (*throw the error again*), inside a catch part: ends its
    /// path.
    Rethrow,
    /// `[[fallthrough]];` (*go on into the next case*), the last statement
    /// of a case.
    FallThrough,
    /// Reads a file line by line (*for each line in file*; w2.md §2.4): a
    /// stream with a hidden name, the `failed` part when it cannot be
    /// opened, then `while (std::getline(stream, line)) { … }`.
    FileLines {
        /// The line variable (`std::string`), visible in `body` only.
        line: SymbolId,
        /// The file's path (`std::string`).
        path: Expr,
        /// Loop body.
        body: Block,
        /// The *if it can't be opened* part, if the block has one.
        failed: Option<Block>,
    },
    /// `std::ifstream file("x");` or `std::ofstream file("x");`, then the
    /// `failed` part under `if (!file)` (*open file*).
    OpenFile {
        /// The stream variable, visible after the block and in `failed`.
        file: SymbolId,
        /// The file's path (`std::string`).
        path: Expr,
        /// Reading, writing or adding to the end.
        mode: FileMode,
        /// The *if it can't be opened* part, if the block has one.
        failed: Option<Block>,
    },
    /// `file << a << b << '\n';` (*write … to file*).
    WriteFile {
        /// The output stream variable.
        file: SymbolId,
        /// Values written, in order (at least one).
        items: Vec<Expr>,
        /// Whether to end the line.
        newline: bool,
    },
    /// A change to a collection variable (*add*, *set item*, *remove*,
    /// *clear*, *sort*; w2.md §1.2).
    CollectionOp {
        /// The collection changed.
        collection: SymbolId,
        /// What is done to it.
        op: CollectionChange,
    },
    /// `target.field = value;` (*set field of*).
    SetField {
        /// The struct variable changed.
        target: SymbolId,
        /// The field set.
        field: SymbolId,
        /// The new value.
        value: Expr,
    },
    /// `std::cout << std::fixed << std::setprecision(n);` (*show numbers
    /// with … decimal places*).
    Decimals {
        /// The number of decimal places (`int`).
        digits: Expr,
    },
    /// `std::this_thread::sleep_for(…);` (*wait … seconds*).
    Wait {
        /// The time to wait, in seconds (`double`).
        seconds: Expr,
    },
    /// `target = !target;` (*flip*).
    Flip {
        /// The `bool` variable flipped.
        target: SymbolId,
    },
    /// `static_assert(cond, "message");` (*check at compile time*).
    StaticCheck {
        /// The condition, a constant expression.
        cond: Expr,
        /// The message shown when it fails.
        message: StrLit,
    },
}

/// One `case` of a [`StmtKind::Switch`].
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SwitchCase {
    /// The label: a literal, an enumerator or a `const` whole number or
    /// character with a constant value.
    pub label: Expr,
    /// The statements under it.
    pub body: Block,
}

/// How a range-`for` binds each item (w2.md §1.2, *for each*'s `MODE`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RangeMode {
    /// `const auto& x`: the item cannot be changed.
    ReadOnly,
    /// `auto x`: a copy the body may change.
    Copy,
    /// `auto& x`: changing it changes the item in the collection.
    Editable,
}

/// One catch part of a [`StmtKind::Try`].
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Catch {
    /// The error type caught (`std::exception` or one derived from it).
    pub error: Type,
    /// The caught error's variable, visible in `body` only.
    pub var: SymbolId,
    /// The part's statements.
    pub body: Block,
    /// Where the part is on its block (its type field).
    pub origin: Origin,
}

/// How *open file* opens its file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FileMode {
    /// `std::ifstream`.
    Read,
    /// `std::ofstream`, emptying the file.
    Write,
    /// `std::ofstream` with `std::ios::app`.
    Append,
}

/// What a [`StmtKind::CollectionOp`] does to its collection.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CollectionChange {
    /// `c.push_back(x)`, `c.insert(x)` or `c.push(x)` (*add … to*).
    Add(Expr),
    /// `c.at(i) = x`, or `c[k] = x` for a map (*set item*).
    SetItem {
        /// The position or key.
        index: Expr,
        /// The new value.
        value: Expr,
    },
    /// `c.clear()`, or `c = {}` for an adaptor (*clear*).
    Clear,
    /// `b2c::remove_at(c, i)` (*remove item at*).
    RemoveAt(Expr),
    /// `c.erase(x)` (*remove … from*).
    RemoveValue(Expr),
    /// `c.pop()` (*remove next from*).
    RemoveNext,
    /// `std::sort` or `std::ranges::sort` (*sort*).
    Sort {
        /// Largest first.
        descending: bool,
    },
}

impl CollectionChange {
    /// The values the change uses, in the block's order.
    pub fn operands(&self) -> Vec<&Expr> {
        match self {
            Self::Add(value) | Self::RemoveAt(value) | Self::RemoveValue(value) => vec![value],
            Self::SetItem { index, value } => vec![index, value],
            Self::Clear | Self::RemoveNext | Self::Sort { .. } => Vec::new(),
        }
    }
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
    /// `+=` on text (*add to the end*).
    Append,
    /// `&=`
    BitAnd,
    /// `|=`
    BitOr,
    /// `^=`
    BitXor,
    /// `<<=`
    Shl,
    /// `>>=`
    Shr,
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
    /// An item of a collection by position or key (*item of*): `c.at(i)`.
    Item {
        /// The collection.
        collection: Box<Expr>,
        /// The position (`int`) or the map's key.
        index: Box<Expr>,
    },
    /// The number of items (*length of*): `static_cast<int>(c.size())`.
    Size(Box<Expr>),
    /// Whether a collection has no items (*is empty*): `c.empty()`.
    IsEmpty(Box<Expr>),
    /// Whether a collection holds a value (*contains*).
    Contains {
        /// The collection.
        collection: Box<Expr>,
        /// The value or key looked for.
        value: Box<Expr>,
    },
    /// The next item of a queue, stack or priority queue (*next item of*):
    /// `c.front()` or `c.top()`.
    Next(Box<Expr>),
    /// A brace list `{a, b}` (*list of*), typed by its place.
    ListOf(Vec<Expr>),
    /// A field of a struct value (*of*): `obj.field`.
    Field {
        /// The struct value.
        object: Box<Expr>,
        /// The field.
        field: SymbolId,
    },
    /// A struct value from its fields (*new … with*): `Student{…}`.
    New {
        /// The struct.
        ty: SymbolId,
        /// One part per field, in row order; `None` for an empty part.
        fields: Vec<Option<Expr>>,
    },
    /// A value of an enum: `Color::Red`.
    Enumerator(SymbolId),
    /// A character of a text (*letter of*): `s.at(i)`.
    Letter {
        /// The text.
        text: Box<Expr>,
        /// The position (`int`).
        index: Box<Expr>,
    },
    /// The length of a text (*length of*): `static_cast<int>(s.size())`.
    TextLength(Box<Expr>),
    /// A text operation of the support helpers or of `std::string`.
    TextCall {
        /// The operation.
        op: TextOp,
        /// Its operands, in the block's order.
        args: Vec<Expr>,
    },
    /// Text read as a number (*text as number*): `std::stoi`, `std::stod`.
    ToNumber {
        /// `int` or `double`.
        to: Type,
        /// The text.
        text: Box<Expr>,
    },
    /// A value as text (*as text*).
    AsText(Box<Expr>),
    /// The message of a caught error (*message of*): `e.what()`.
    ErrorMessage(SymbolId),
    /// A function of `<cmath>`, `<cstdlib>` or `<algorithm>`.
    MathCall {
        /// The function.
        op: MathOp,
        /// Its operands, in the block's order.
        args: Vec<Expr>,
    },
    /// A uniformly random decimal number from `low` to `high` (support
    /// helper).
    RandomReal {
        /// Lower bound.
        low: Box<Expr>,
        /// Upper bound.
        high: Box<Expr>,
    },
    /// Reads a line of an input file into a variable, giving whether it did
    /// (*read line from … into*): `std::getline(file, line)`.
    ReadLine {
        /// The input stream variable.
        file: SymbolId,
        /// The `std::string` variable the line goes into.
        line: SymbolId,
    },
    /// Whether a file exists (*file … exists*): `std::filesystem::exists`.
    FileExists(Box<Expr>),
}

impl ExprKind {
    /// The direct sub-expressions, in the order C++ shows them.
    pub fn children(&self) -> Vec<&Expr> {
        match self {
            Self::Int(_)
            | Self::Float(_)
            | Self::Bool(_)
            | Self::Str(_)
            | Self::Char(_)
            | Self::Var(_)
            | Self::Enumerator(_)
            | Self::ErrorMessage(_)
            | Self::ReadLine { .. } => Vec::new(),
            Self::Unary { operand: value, .. }
            | Self::Convert { value, .. }
            | Self::Size(value)
            | Self::IsEmpty(value)
            | Self::Next(value)
            | Self::Field { object: value, .. }
            | Self::TextLength(value)
            | Self::ToNumber { text: value, .. }
            | Self::AsText(value)
            | Self::FileExists(value) => vec![value],
            Self::Binary { lhs, rhs, .. }
            | Self::RandomInt { low: lhs, high: rhs }
            | Self::RandomReal { low: lhs, high: rhs }
            | Self::Item {
                collection: lhs,
                index: rhs,
            }
            | Self::Contains {
                collection: lhs,
                value: rhs,
            }
            | Self::Letter {
                text: lhs,
                index: rhs,
            } => vec![lhs, rhs],
            Self::Conditional {
                cond,
                then_value,
                else_value,
            } => vec![cond, then_value, else_value],
            Self::Call { args, .. }
            | Self::Join(args)
            | Self::ListOf(args)
            | Self::TextCall { args, .. }
            | Self::MathCall { args, .. } => args.iter().collect(),
            Self::New { fields, .. } => fields.iter().flatten().collect(),
        }
    }
}

/// The operations of [`ExprKind::TextCall`] (w2.md §1.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TextOp {
    /// `b2c::to_lower(s)`.
    ToLower,
    /// `b2c::to_upper(s)`.
    ToUpper,
    /// `b2c::split(s, separator)`.
    Split,
    /// `s.find(t) != std::string::npos`.
    Contains,
    /// `s.starts_with(t)`, `b2c::starts_with(s, t)` in C++17.
    StartsWith,
    /// `s.ends_with(t)`, `b2c::ends_with(s, t)` in C++17.
    EndsWith,
    /// `s.substr(start, length)`.
    Part,
    /// `b2c::trim(s)`.
    Trim,
    /// `b2c::replace_all(s, old, new)`.
    ReplaceAll,
}

/// The functions of [`ExprKind::MathCall`] (w2.md §1.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MathOp {
    /// `std::floor` (*round down*).
    Floor,
    /// `std::ceil` (*round up*).
    Ceil,
    /// `std::round`.
    Round,
    /// `std::sqrt`.
    Sqrt,
    /// `std::cbrt`.
    Cbrt,
    /// `std::exp`.
    Exp,
    /// `std::log` (natural log).
    Log,
    /// `std::log10`.
    Log10,
    /// `std::sin`.
    Sin,
    /// `std::cos`.
    Cos,
    /// `std::tan`.
    Tan,
    /// `std::asin`.
    Asin,
    /// `std::acos`.
    Acos,
    /// `std::atan`.
    Atan,
    /// `std::abs`.
    Abs,
    /// `std::max<T>`.
    Max,
    /// `std::min<T>`.
    Min,
    /// `std::pow`.
    Pow,
    /// `std::atan2`.
    Atan2,
    /// `std::hypot`.
    Hypot,
    /// `std::clamp<T>`.
    Clamp,
}

impl MathOp {
    /// The standard function's qualified name.
    pub const fn cpp_name(self) -> &'static str {
        match self {
            Self::Floor => "std::floor",
            Self::Ceil => "std::ceil",
            Self::Round => "std::round",
            Self::Sqrt => "std::sqrt",
            Self::Cbrt => "std::cbrt",
            Self::Exp => "std::exp",
            Self::Log => "std::log",
            Self::Log10 => "std::log10",
            Self::Sin => "std::sin",
            Self::Cos => "std::cos",
            Self::Tan => "std::tan",
            Self::Asin => "std::asin",
            Self::Acos => "std::acos",
            Self::Atan => "std::atan",
            Self::Abs => "std::abs",
            Self::Max => "std::max",
            Self::Min => "std::min",
            Self::Pow => "std::pow",
            Self::Atan2 => "std::atan2",
            Self::Hypot => "std::hypot",
            Self::Clamp => "std::clamp",
        }
    }
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

/// Whether a count is 0 (serde's `skip_serializing_if`).
#[allow(clippy::trivially_copy_pass_by_ref, reason = "serde passes a reference")]
fn is_zero(count: &usize) -> bool {
    *count == 0
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

/// The program's structs and enums, with the effective flags the analyser
/// worked out when it declared them.
impl TypeFacts for SymbolTable {
    fn user_type(&self, id: &SymbolId) -> Option<UserType<'_>> {
        let symbol = self.get(id)?;
        let (kind, comparable, printable) = match &symbol.kind {
            SymbolKind::Struct {
                comparable,
                printable,
                ..
            } => (UserKind::Struct, *comparable, *printable),
            SymbolKind::Enum { .. } => (UserKind::Enum, true, true),
            SymbolKind::Variable { .. }
            | SymbolKind::Parameter { .. }
            | SymbolKind::LoopVariable
            | SymbolKind::Global { .. }
            | SymbolKind::Function { .. }
            | SymbolKind::Field { .. }
            | SymbolKind::Enumerator { .. } => return None,
        };
        Some(UserType {
            name: symbol.name.as_str(),
            kind,
            comparable,
            printable,
        })
    }
}

/// A named entity.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Symbol {
    /// The C++ name.
    pub name: Ident,
    /// What it is.
    pub kind: SymbolKind,
    /// Its type: for a function the return type, for a struct or enum
    /// [`Type::Named`] of itself, for an enumerator [`Type::Named`] of its
    /// enum, and for a field its declared type.
    pub ty: Type,
    /// Where it was declared.
    pub origin: Origin,
}

/// Kinds of symbols.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SymbolKind {
    /// A local variable: also a catch part's error (`const`), a *for each*
    /// item that is copied or editable, and an opened file.
    Variable {
        /// Declared `const`.
        is_const: bool,
    },
    /// A function parameter.
    Parameter {
        /// Passing mode.
        mode: PassMode,
    },
    /// A loop's own variable, which the blocks inside cannot change: a `for`
    /// counter, a read-only *for each* item, a map's key and value, and a
    /// file's line.
    LoopVariable,
    /// A module-level variable or constant (`var.global`; w2.md §2.7).
    Global {
        /// A constant (*constant* in its `KIND`).
        is_const: bool,
    },
    /// A user function.
    Function {
        /// Parameter symbols in order.
        params: Vec<SymbolId>,
        /// How many of the last parameters have a default value, so that a
        /// call may leave their arguments out (w2.md §2.8). Serialised only
        /// when not 0.
        #[serde(skip_serializing_if = "is_zero")]
        optional: usize,
    },
    /// A user struct (from milestone M3).
    Struct {
        /// Field symbols in order.
        fields: Vec<SymbolId>,
        /// Ticked *comparable*, and every field's type is comparable
        /// (types.md §1.6).
        comparable: bool,
        /// Ticked *printable*, and every field's type is printable.
        printable: bool,
    },
    /// A field of a struct.
    Field {
        /// The struct.
        owner: SymbolId,
    },
    /// A user enum (`enum class`).
    Enum {
        /// Enumerator symbols in order.
        values: Vec<SymbolId>,
    },
    /// A value of an enum.
    Enumerator {
        /// The enum.
        owner: SymbolId,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::TypeClass;

    fn sym(id: &str) -> SymbolId {
        SymbolId::new(id).unwrap()
    }

    fn symbol(name: &str, kind: SymbolKind, ty: Type) -> Symbol {
        Symbol {
            name: Ident::new(name).unwrap(),
            kind,
            ty,
            origin: Origin::whole(ModuleId::new("m").unwrap(), BlockId::new("b").unwrap()),
        }
    }

    #[test]
    fn the_symbol_table_knows_its_user_types() {
        let mut table = SymbolTable::default();
        table.symbols.insert(
            sym("s_pt"),
            symbol(
                "Point",
                SymbolKind::Struct {
                    fields: vec![sym("f_x")],
                    comparable: true,
                    printable: false,
                },
                Type::Named(sym("s_pt")),
            ),
        );
        table.symbols.insert(
            sym("f_x"),
            symbol("x", SymbolKind::Field { owner: sym("s_pt") }, Type::Int),
        );
        table.symbols.insert(
            sym("s_col"),
            symbol(
                "Color",
                SymbolKind::Enum {
                    values: vec![sym("e_red")],
                },
                Type::Named(sym("s_col")),
            ),
        );
        table.symbols.insert(
            sym("v_n"),
            symbol("n", SymbolKind::Variable { is_const: false }, Type::Int),
        );
        let point = table.user_type(&sym("s_pt")).unwrap();
        assert_eq!(
            (point.name, point.kind, point.comparable, point.printable),
            ("Point", UserKind::Struct, true, false)
        );
        let color = table.user_type(&sym("s_col")).unwrap();
        assert_eq!(
            (color.name, color.kind, color.comparable, color.printable),
            ("Color", UserKind::Enum, true, true)
        );
        for other in ["f_x", "v_n", "missing"] {
            assert_eq!(table.user_type(&sym(other)), None, "{other}");
        }
        assert_eq!(
            Type::Named(sym("s_pt")).is_in(TypeClass::Printable, &table),
            Some(false)
        );
        assert_eq!(Type::Named(sym("s_col")).label(&table).to_string(), "Color");
    }

    #[test]
    fn new_item_keys_are_written_only_when_set() {
        let mut item = Item {
            kind: ItemKind::Main(MainDef {
                body: Block::default(),
            }),
            origin: Origin::whole(ModuleId::new("m").unwrap(), BlockId::new("b").unwrap()),
            comment: None,
            headers: ItemHeaders::default(),
            shared: false,
        };
        let plain = serde_json::to_value(&item).unwrap();
        assert_eq!(plain.as_object().unwrap().len(), 3, "{plain}");
        item.shared = true;
        item.headers
            .decl
            .insert(crate::headers::Header::standard("<string>").unwrap());
        let json = serde_json::to_value(&item).unwrap();
        assert_eq!(json["shared"], true);
        assert_eq!(json["headers"]["decl"], serde_json::json!(["<string>"]));
    }
}
