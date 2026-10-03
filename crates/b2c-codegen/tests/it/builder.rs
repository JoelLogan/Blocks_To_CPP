//! A small DSL for building SAST programs in tests.
//!
//! Every node gets a fresh block ID (`blk_1`, `blk_2`, …) so source-map tests
//! can tell nodes apart. Expression types are computed the way the analyser
//! would for well-typed programs.

use std::cell::{Cell, RefCell};

use b2c_ir::diag::Part;
use b2c_ir::ids::{BlockId, ModuleId, SymbolId};
use b2c_ir::sast::{
    AskMode, BinaryOp, Block, CppStandard, Expr, ExprKind, FunctionDef, IfBranch, Item, ItemKind, MainDef,
    Module, Origin, OutputStream, Param, PassMode, PrintSeparator, Program, RangeDirection, Stmt, StmtKind,
    Symbol, SymbolKind, SymbolTable, UnaryOp, VarDecl,
};
use b2c_ir::text::{CharLit, Comment, Ident, NumLit, NumType, StrLit};
use b2c_ir::types::Type;

/// Builds one module named `main`.
pub(crate) struct Builder {
    module: ModuleId,
    symbols: RefCell<SymbolTable>,
    next_block: Cell<u32>,
    next_symbol: Cell<u32>,
}

impl Builder {
    pub(crate) fn new() -> Self {
        Self {
            module: ModuleId::new("mod_main").unwrap(),
            symbols: RefCell::new(SymbolTable::default()),
            next_block: Cell::new(0),
            next_symbol: Cell::new(0),
        }
    }

    /// A fresh origin (a new block).
    pub(crate) fn origin(&self) -> Origin {
        self.next_block.set(self.next_block.get() + 1);
        Origin::whole(
            self.module.clone(),
            BlockId::new(&format!("blk_{}", self.next_block.get())).unwrap(),
        )
    }

    /// A fresh origin pointing at a named input of a new block.
    pub(crate) fn input_origin(&self, input: &str) -> Origin {
        let mut origin = self.origin();
        origin.part = Part::Input {
            name: input.to_owned(),
        };
        origin
    }

    fn symbol(&self, name: &str, kind: SymbolKind, ty: Type) -> SymbolId {
        self.next_symbol.set(self.next_symbol.get() + 1);
        let id = SymbolId::new(&format!("sym_{}", self.next_symbol.get())).unwrap();
        let origin = self.origin();
        let symbol = Symbol {
            name: Ident::new(name).unwrap(),
            kind,
            ty,
            origin,
        };
        self.symbols.borrow_mut().symbols.insert(id.clone(), symbol);
        id
    }

    /// A local variable.
    pub(crate) fn var(&self, name: &str, ty: Type) -> SymbolId {
        self.symbol(name, SymbolKind::Variable { is_const: false }, ty)
    }

    /// A `const` local variable.
    pub(crate) fn constant(&self, name: &str, ty: Type) -> SymbolId {
        self.symbol(name, SymbolKind::Variable { is_const: true }, ty)
    }

    /// A `for` loop counter.
    pub(crate) fn loop_var(&self, name: &str) -> SymbolId {
        self.symbol(name, SymbolKind::LoopVariable, Type::Int)
    }

    /// A function with its parameters.
    pub(crate) fn function(&self, name: &str, ret: Type, params: &[(&str, Type, PassMode)]) -> Function {
        let param_ids: Vec<_> = params
            .iter()
            .map(|(pname, ty, mode)| self.symbol(pname, SymbolKind::Parameter { mode: *mode }, ty.clone()))
            .collect();
        let id = self.symbol(
            name,
            SymbolKind::Function {
                params: param_ids.clone(),
            },
            ret.clone(),
        );
        let modes = params.iter().map(|(_, _, mode)| *mode).collect();
        Function {
            id,
            ret,
            params: param_ids,
            modes,
        }
    }

    pub(crate) fn ty(&self, id: &SymbolId) -> Type {
        self.symbols
            .borrow()
            .get(id)
            .map_or(Type::Error, |s| s.ty.clone())
    }

    // ---- expressions ------------------------------------------------------

    pub(crate) fn expr(&self, kind: ExprKind, ty: Type) -> Expr {
        let origin = self.origin();
        Expr { kind, ty, origin }
    }

    pub(crate) fn int(&self, text: &str) -> Expr {
        self.expr(
            ExprKind::Int(NumLit::parse(text, NumType::Int).unwrap()),
            Type::Int,
        )
    }

    pub(crate) fn float(&self, text: &str) -> Expr {
        self.expr(
            ExprKind::Float(NumLit::parse(text, NumType::Double).unwrap()),
            Type::Double,
        )
    }

    pub(crate) fn boolean(&self, value: bool) -> Expr {
        self.expr(ExprKind::Bool(value), Type::Bool)
    }

    pub(crate) fn str(&self, text: &str) -> Expr {
        self.expr(ExprKind::Str(StrLit::new(text).unwrap()), Type::String)
    }

    pub(crate) fn chr(&self, text: &str) -> Expr {
        self.expr(ExprKind::Char(CharLit::new(text).unwrap()), Type::Char)
    }

    pub(crate) fn get(&self, id: &SymbolId) -> Expr {
        let ty = self.ty(id);
        self.expr(ExprKind::Var(id.clone()), ty)
    }

    pub(crate) fn un(&self, op: UnaryOp, operand: Expr) -> Expr {
        let ty = match op {
            UnaryOp::Not => Type::Bool,
            UnaryOp::Neg | UnaryOp::Plus if operand.ty == Type::Double => Type::Double,
            UnaryOp::Neg | UnaryOp::Plus => Type::Int,
        };
        self.expr(
            ExprKind::Unary {
                op,
                operand: Box::new(operand),
            },
            ty,
        )
    }

    pub(crate) fn bin(&self, op: BinaryOp, lhs: Expr, rhs: Expr) -> Expr {
        let ty = match op {
            BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div | BinaryOp::Mod => {
                if lhs.ty == Type::String || rhs.ty == Type::String {
                    Type::String
                } else if lhs.ty == Type::Double || rhs.ty == Type::Double {
                    Type::Double
                } else {
                    Type::Int
                }
            }
            _ => Type::Bool,
        };
        self.expr(
            ExprKind::Binary {
                op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            },
            ty,
        )
    }

    pub(crate) fn cond(&self, cond: Expr, then_value: Expr, else_value: Expr) -> Expr {
        let ty = then_value.ty.clone();
        self.expr(
            ExprKind::Conditional {
                cond: Box::new(cond),
                then_value: Box::new(then_value),
                else_value: Box::new(else_value),
            },
            ty,
        )
    }

    pub(crate) fn call(&self, function: &Function, args: Vec<Expr>) -> Expr {
        self.expr(
            ExprKind::Call {
                function: function.id.clone(),
                args,
            },
            function.ret.clone(),
        )
    }

    pub(crate) fn join(&self, items: Vec<Expr>) -> Expr {
        self.expr(ExprKind::Join(items), Type::String)
    }

    pub(crate) fn random(&self, low: Expr, high: Expr) -> Expr {
        self.expr(
            ExprKind::RandomInt {
                low: Box::new(low),
                high: Box::new(high),
            },
            Type::Int,
        )
    }

    pub(crate) fn convert(&self, to: Type, value: Expr) -> Expr {
        self.expr(
            ExprKind::Convert {
                to: to.clone(),
                value: Box::new(value),
            },
            to,
        )
    }

    // ---- statements -------------------------------------------------------

    pub(crate) fn stmt(&self, kind: StmtKind) -> Stmt {
        let origin = self.origin();
        Stmt {
            kind,
            origin,
            comment: None,
        }
    }

    pub(crate) fn declare(&self, symbol: &SymbolId, init: Option<Expr>) -> Stmt {
        self.stmt(StmtKind::VarDecl(VarDecl {
            symbol: symbol.clone(),
            init,
            is_const: false,
            written_auto: false,
        }))
    }

    pub(crate) fn declare_with(
        &self,
        symbol: &SymbolId,
        init: Option<Expr>,
        is_const: bool,
        auto: bool,
    ) -> Stmt {
        self.stmt(StmtKind::VarDecl(VarDecl {
            symbol: symbol.clone(),
            init,
            is_const,
            written_auto: auto,
        }))
    }

    pub(crate) fn set(&self, target: &SymbolId, value: Expr) -> Stmt {
        self.stmt(StmtKind::Assign {
            target: target.clone(),
            value,
        })
    }

    pub(crate) fn print(&self, items: Vec<Expr>) -> Stmt {
        self.print_with(items, PrintSeparator::None, true, OutputStream::Out)
    }

    pub(crate) fn print_with(
        &self,
        items: Vec<Expr>,
        separator: PrintSeparator,
        newline: bool,
        stream: OutputStream,
    ) -> Stmt {
        self.stmt(StmtKind::Print {
            items,
            separator,
            newline,
            stream,
        })
    }

    pub(crate) fn print_text(&self, text: &str) -> Stmt {
        let item = self.str(text);
        self.print(vec![item])
    }

    pub(crate) fn ask(&self, prompt: Option<Expr>, target: &SymbolId, mode: AskMode) -> Stmt {
        self.stmt(StmtKind::Ask {
            prompt,
            target: target.clone(),
            mode,
        })
    }

    pub(crate) fn if_else(&self, branches: Vec<(Expr, Vec<Stmt>)>, else_body: Option<Vec<Stmt>>) -> Stmt {
        let branches = branches
            .into_iter()
            .map(|(cond, body)| IfBranch {
                cond,
                body: block(body),
            })
            .collect();
        self.stmt(StmtKind::If {
            branches,
            else_body: else_body.map(block),
        })
    }

    pub(crate) fn repeat(&self, count: Expr, body: Vec<Stmt>) -> Stmt {
        self.stmt(StmtKind::Repeat {
            count,
            body: block(body),
        })
    }

    pub(crate) fn for_range(
        &self,
        var: &SymbolId,
        from: Expr,
        to: Expr,
        step: Option<Expr>,
        direction: RangeDirection,
        body: Vec<Stmt>,
    ) -> Stmt {
        self.stmt(StmtKind::ForRange {
            var: var.clone(),
            from,
            to,
            step,
            direction,
            body: block(body),
        })
    }

    pub(crate) fn eval(&self, expr: Expr) -> Stmt {
        self.stmt(StmtKind::Eval { expr })
    }

    pub(crate) fn ret(&self, value: Option<Expr>) -> Stmt {
        self.stmt(StmtKind::Return { value })
    }

    // ---- items ------------------------------------------------------------

    pub(crate) fn main(&self, body: Vec<Stmt>) -> Item {
        let origin = self.origin();
        Item {
            kind: ItemKind::Main(MainDef { body: block(body) }),
            origin,
            comment: None,
        }
    }

    pub(crate) fn define(&self, function: &Function, body: Vec<Stmt>) -> Item {
        let origin = self.origin();
        let params = function
            .params
            .iter()
            .zip(&function.modes)
            .enumerate()
            .map(|(index, (symbol, mode))| {
                let mut param_origin = origin.clone();
                param_origin.part = Part::Field {
                    name: format!("PARAM{index}"),
                };
                Param {
                    symbol: symbol.clone(),
                    mode: *mode,
                    origin: param_origin,
                }
            })
            .collect();
        let def = FunctionDef {
            symbol: function.id.clone(),
            params,
            ret: function.ret.clone(),
            body: block(body),
        };
        Item {
            kind: ItemKind::Function(def),
            origin,
            comment: None,
        }
    }

    pub(crate) fn program(self, items: Vec<Item>) -> Program {
        let module = Module {
            id: self.module,
            name: String::from("main"),
            items,
        };
        Program {
            standard: CppStandard::Cpp20,
            modules: vec![module],
            symbols: self.symbols.into_inner(),
        }
    }
}

/// A function symbol with what calls need to know.
#[derive(Clone)]
pub(crate) struct Function {
    pub(crate) id: SymbolId,
    pub(crate) ret: Type,
    pub(crate) params: Vec<SymbolId>,
    pub(crate) modes: Vec<PassMode>,
}

pub(crate) fn block(stmts: Vec<Stmt>) -> Block {
    Block { stmts }
}

/// Attaches a comment to a statement.
pub(crate) fn commented(mut stmt: Stmt, text: &str) -> Stmt {
    stmt.comment = Some(Comment::new(text).unwrap());
    stmt
}

/// Attaches a comment to an item.
pub(crate) fn commented_item(mut item: Item, text: &str) -> Item {
    item.comment = Some(Comment::new(text).unwrap());
    item
}
