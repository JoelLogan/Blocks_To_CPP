//! Desugaring W2's file nodes (`docs/plans/m3-contracts/w2.md` §1.8, §2.4),
//! *use header* and *check at compile time* (§1.6): package D.
//!
//! *for each line in file* prints, in a block of its own,
//!
//! ```cpp
//! {
//!     std::ifstream b2c_file("poem.txt");
//!     if (!b2c_file) {
//!         …
//!     }
//!     std::string line{};
//!     while (std::getline(b2c_file, line)) {
//!         …
//!     }
//! }
//! ```
//!
//! without the `if` when the block has no *if it can't be opened* part (a
//! stream that failed reads no line). The stream's hidden name is unique
//! among the names visible where it is declared, `b2c_file`, `b2c_file2`, …,
//! so nested loops never shadow one another (`-Wshadow`); user names cannot
//! start with `b2c`, so it never clashes with one (w2.md §2.12).
//!
//! *open file* declares its stream where the block is, so the blocks after
//! it can use it, and tests it when the block has its *if it can't be
//! opened* part:
//!
//! ```cpp
//! std::ofstream log("log.txt", std::ios::app);
//! if (!log) {
//!     …
//! }
//! ```
//!
//! *read line from … into* is `std::getline(file, line)` as a condition of
//! its own (`if`, `while`, `do … while`), where the stream converts to
//! `bool`, and `static_cast<bool>(std::getline(file, line))` anywhere else;
//! *write … to file* is `file << a << b << '\n';`, its values written as
//! *print* writes them; *file … exists* is `std::filesystem::exists(path)`.
//!
//! *check at compile time* is `static_assert(c, "message");`, and *use
//! header* adds its header to the module's includes.

use std::collections::BTreeSet;

use b2c_ir::headers::Header;
use b2c_ir::ids::SymbolId;
use b2c_ir::sast::{Block, Expr, ExprKind, FileMode, Item, StmtKind};
use b2c_ir::text::Ident;
use b2c_ir::types::LibType;

use super::{Lowerer, name_expr};
use crate::cast::{BinOp, CBlock, CExpr, CStmt, CStmtKind, CType, Init, UnOp};

/// The stem of the hidden names of the streams *for each line* opens.
const STREAM_STEM: &str = "b2c_file";

/// The first hidden stream name not in `taken`: `b2c_file`, then
/// `b2c_file2`, `b2c_file3`, …
fn stream_name(taken: &BTreeSet<String>) -> Ident {
    let limit = u32::try_from(taken.len()).unwrap_or(u32::MAX).saturating_add(2);
    std::iter::once(STREAM_STEM.to_owned())
        .chain((2..=limit).map(|n| format!("{STREAM_STEM}{n}")))
        .find(|name| !taken.contains(name))
        .and_then(|name| Ident::generated(&name).ok())
        .unwrap_or_else(Ident::main)
}

impl Lowerer<'_> {
    /// *for each line in file*, *open file*, *write … to file* and *check at
    /// compile time*.
    pub(super) fn file_stmt(&mut self, kind: &StmtKind) -> CStmtKind {
        match kind {
            StmtKind::FileLines {
                line,
                path,
                body,
                failed,
            } => self.file_lines(line, path, body, failed.as_ref()),
            StmtKind::OpenFile {
                file,
                path,
                mode,
                failed,
            } => self.open_file(file, path, *mode, failed.as_ref()),
            StmtKind::WriteFile { file, items, newline } => self.write_file(file, items, *newline),
            StmtKind::StaticCheck { cond, message } => CStmtKind::StaticAssert {
                cond: self.expr(cond),
                message: message.clone(),
            },
            _ => CStmtKind::Error,
        }
    }

    /// *read line from … into* and *file … exists*.
    pub(super) fn file_expr(&mut self, expr: &Expr) -> CExpr {
        match &expr.kind {
            ExprKind::ReadLine { .. } => match self.read_line(expr) {
                Some(read) => CExpr::call_fixed("static_cast<bool>", vec![read]),
                None => CExpr::error(),
            },
            // `<filesystem>` is the block's catalog header.
            ExprKind::FileExists(path) => {
                let path = self.expr(path);
                CExpr::call_fixed("std::filesystem::exists", vec![path])
            }
            _ => CExpr::error(),
        }
    }

    /// A condition of its own (`if`, `while`, `do … while`): *read line from
    /// … into* is the bare `std::getline(file, line)` there, whose stream
    /// converts to `bool`; any other condition is lowered as usual.
    pub(super) fn condition(&mut self, cond: &Expr) -> CExpr {
        if matches!(cond.kind, ExprKind::ReadLine { .. })
            && let Some(read) = self.read_line(cond)
        {
            return read.with_origin(cond.origin.clone());
        }
        self.expr(cond)
    }

    /// `std::getline(file, line)`, or `None` for a stream or line the
    /// program does not have.
    fn read_line(&mut self, expr: &Expr) -> Option<CExpr> {
        let ExprKind::ReadLine { file, line } = &expr.kind else {
            return None;
        };
        let file = self.variable(file)?.name.clone();
        let line = self.variable(line)?.name.clone();
        self.include("<string>");
        Some(CExpr::call_fixed(
            "std::getline",
            vec![name_expr(file), name_expr(line)],
        ))
    }

    /// *open file*: the stream, declared where the block is, then the part
    /// for a file that can't be opened.
    fn open_file(
        &mut self,
        file: &SymbolId,
        path: &Expr,
        mode: FileMode,
        failed: Option<&Block>,
    ) -> CStmtKind {
        let Some(name) = self.variable(file).map(|symbol| symbol.name.clone()) else {
            return CStmtKind::Error;
        };
        let stream = match mode {
            FileMode::Read => LibType::IfStream,
            FileMode::Write | FileMode::Append => LibType::OfStream,
        };
        let ty = CType::Lib(stream, Vec::new());
        self.note_type(&ty);
        let mut args = vec![self.expr(path)];
        if mode == FileMode::Append {
            self.include("<ios>");
            args.push(CExpr::fixed("std::ios::app"));
        }
        // The stream is visible from its declarator on, also in the part
        // for a file that can't be opened.
        self.declare(name.clone());
        let decl = CStmt::new(CStmtKind::Decl {
            is_const: false,
            ty,
            name: name.clone(),
            init: Init::Args(args),
        });
        let Some(failed) = failed else {
            return decl.kind;
        };
        let test = CStmt::new(CStmtKind::If {
            branches: vec![(CExpr::unary(UnOp::Not, name_expr(name)), self.block(failed))],
            else_body: None,
        });
        CStmtKind::Seq(vec![decl, test])
    }

    /// *write … to file*: `file << a << b << '\n';`.
    fn write_file(&mut self, file: &SymbolId, items: &[Expr], newline: bool) -> CStmtKind {
        let Some(name) = self.variable(file).map(|symbol| symbol.name.clone()) else {
            return CStmtKind::Error;
        };
        // `operator<<` for text and numbers is declared in `<ostream>`.
        self.include("<ostream>");
        let mut chain = name_expr(name);
        for item in items {
            let operand = self.print_operand(item);
            chain = CExpr::binary(BinOp::Shl, chain, operand);
        }
        if newline {
            chain = CExpr::binary(BinOp::Shl, chain, CExpr::fixed("'\\n'"));
        }
        CStmtKind::Expr(chain)
    }

    /// *use header* (`ItemKind::UseHeader`): the header joins the module's
    /// includes, once (w2.md §1.6).
    pub(super) fn use_header(&mut self, header: Header, item: &Item) {
        let _ = item;
        self.usage.includes.insert(header);
    }

    /// The file loop of w2.md §2.4.
    fn file_lines(
        &mut self,
        line: &SymbolId,
        path: &Expr,
        body: &Block,
        failed: Option<&Block>,
    ) -> CStmtKind {
        let Some(line) = self.variable(line).map(|symbol| symbol.name.clone()) else {
            return CStmtKind::Error;
        };
        let stream = stream_name(&self.visible_names());
        let stream_type = CType::Lib(LibType::IfStream, Vec::new());
        self.note_type(&stream_type);
        let path = self.expr(path);
        // The stream, the failed part and the line are in a scope of their
        // own, which ends with the loop: the stream closes there.
        self.scopes.push(vec![stream.clone()]);
        let mut stmts = vec![CStmt::new(CStmtKind::Decl {
            is_const: false,
            ty: stream_type,
            name: stream.clone(),
            init: Init::Args(vec![path]),
        })];
        if let Some(failed) = failed {
            stmts.push(CStmt::new(CStmtKind::If {
                branches: vec![(
                    CExpr::unary(UnOp::Not, name_expr(stream.clone())),
                    self.block(failed),
                )],
                else_body: None,
            }));
        }
        self.note_type(&CType::String);
        self.declare(line.clone());
        stmts.push(CStmt::new(CStmtKind::Decl {
            is_const: false,
            ty: CType::String,
            name: line.clone(),
            init: Init::Braces,
        }));
        let read = CExpr::call_fixed("std::getline", vec![name_expr(stream), name_expr(line)]);
        let body = self.block(body);
        stmts.push(CStmt::new(CStmtKind::While { cond: read, body }));
        self.scopes.pop();
        CStmtKind::Block(CBlock { stmts })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stream_names_skip_the_taken_ones() {
        let taken = |names: &[&str]| names.iter().map(|n| (*n).to_owned()).collect::<BTreeSet<_>>();
        assert_eq!(stream_name(&taken(&[])).as_str(), "b2c_file");
        assert_eq!(stream_name(&taken(&["line", "b2c_file"])).as_str(), "b2c_file2");
        assert_eq!(
            stream_name(&taken(&["b2c_file", "b2c_file2"])).as_str(),
            "b2c_file3"
        );
    }
}
