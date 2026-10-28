//! Lowering the file blocks of W2 (`docs/plans/m3-contracts/w2.md` §1.8,
//! §2.4), *use header* and *check at compile time* (§1.6): package D, with
//! the if-time blocks of package S.
//!
//! * *for each line in file* lowers its path, then its *if it can't be
//!   opened* part, then declares its line (a `std::string` loop variable,
//!   visible in its body only) and lowers its body as a loop's. The
//!   analyser does not restrict paths: a program runs as its user and opens
//!   exactly the paths its blocks hold (decision M3-D3).
//! * *open file* lowers its path, then declares its stream (an input file
//!   for *reading*, an output file otherwise; w2.md §2.4), visible in its
//!   *if it can't be opened* part and after the block in its list, then
//!   lowers that part.
//! * *read line from … into* reads a file opened for reading into a text
//!   variable it changes (`B2C-E0308` for one that can't change); *write …
//!   to file* writes printable values (`B2C-E0314`) to a file opened for
//!   writing; *file … exists* takes a path. Each stream part takes only a
//!   stream of its kind (`B2C-E0301`).
//! * *use header* is a definition: its header, which the catalog checked
//!   against the header table (`B2C-E0607`), joins the module's includes.
//! * *check at compile time* lowers its condition as *if* does and keeps its
//!   message as a string literal; whether the condition is fixed when the
//!   program is built, and true, is `consts.rs`'s check (`B2C-E0424`,
//!   `B2C-E0428`).

use b2c_ir::diag::Part;
use b2c_ir::headers::{Header, ItemHeaders};
use b2c_ir::ids::SymbolId;
use b2c_ir::sast::{Expr, ExprKind, FileMode, Item, ItemKind, Origin, StmtKind, SymbolKind};
use b2c_ir::sig::PartType;
use b2c_ir::text::StrLit;
use b2c_ir::types::{LibType, Type};
use b2c_model::Block;

use super::convert::Subject;
use super::expr::error_expr;
use super::typed::Typed;
use super::user_types::Output;
use super::{Lowerer, PendingKind, field};
use crate::scope::{ITEM_LIST, ListKey};
use crate::{access, codes};

/// *use header*'s header when the block has none: the catalog default.
const DEFAULT_HEADER: &str = "<cmath>";

/// *check at compile time*'s message when the block has none: the catalog
/// default.
const DEFAULT_MESSAGE: &str = "check failed";

impl Lowerer<'_> {
    /// *use header* (`program.use_header`). A header the table does not
    /// hold was reported by the catalog (`B2C-E0607`), which also keeps the
    /// field's value as it is: such a block adds nothing.
    pub(super) fn use_header(&mut self, block: &Block) -> Option<Item> {
        let header = access::text_field(block, "HEADER", Some(DEFAULT_HEADER))
            .ok()
            .and_then(Header::standard)?;
        Some(Item {
            kind: ItemKind::UseHeader { header },
            origin: self.origin(&block.id, Part::Whole),
            comment: self.comment(block),
            headers: ItemHeaders::default(),
            shared: false,
        })
    }

    /// *for each line in file*, *open file*, *write … to file*, *check at
    /// compile time*.
    pub(super) fn file_statement(&mut self, block: &Block) -> Option<StmtKind> {
        match block.block_type.as_str() {
            "file.for_each_line" => self.file_lines(block),
            "file.open" => self.open_file(block),
            "file.write" => self.write_file(block),
            "program.check" => self.static_check(block),
            _ => None,
        }
    }

    /// *read line from … into*, *file … exists*.
    pub(super) fn file_value(&mut self, block: &Block, origin: Origin) -> Expr {
        match block.block_type.as_str() {
            "file.read_line" => self.read_line(block, origin),
            "file.exists" => {
                let [path] = self.typed_values(
                    block,
                    [(
                        "PATH",
                        Subject::Phrase {
                            text: String::from("The path of the file"),
                            stores: false,
                        },
                    )],
                );
                Expr {
                    kind: ExprKind::FileExists(Box::new(path)),
                    ty: Type::Bool,
                    origin,
                }
            }
            _ => error_expr(origin),
        }
    }

    /// The stream or text variable a `symbol_ref` part names, typed for the
    /// block's signature: `None` (after reporting) when none is chosen or it
    /// does not resolve. A part that `changes` its variable must name one
    /// that can change.
    fn file_part(
        &mut self,
        block: &Block,
        typed: &mut Typed<'_>,
        name: &str,
        changes: bool,
        missing: &str,
    ) -> Option<SymbolId> {
        let location = self.loc(&block.id, field(name));
        let Ok(sym) = access::ref_field(block, name) else {
            typed.field(name, PartType::Empty);
            self.incomplete(location, missing);
            return None;
        };
        let ty = if changes {
            self.write_target(sym, &location)
        } else {
            self.read_variable(sym, &location)
        };
        typed.field(name, PartType::Known(ty.clone().unwrap_or(Type::Error)));
        ty.map(|_| sym.clone())
    }

    /// *open file (path) for [mode] as [file] … if it can't be opened …*
    /// (w2.md §2.4).
    fn open_file(&mut self, block: &Block) -> Option<StmtKind> {
        // C++ declares the stream from its name on, also in its own
        // initialiser (`std::ifstream data(data);` reads the new stream), so
        // the path is lowered with the name taken, as *create*'s value is.
        let decl = access::decl_field(block, "FILE").ok().cloned();
        if let Some(decl) = &decl {
            self.push_pending(&decl.name, &decl.sym, PendingKind::OpenedFile);
        }
        let [path] = self.typed_values(
            block,
            [(
                "PATH",
                Subject::Phrase {
                    text: String::from("The path of the file"),
                    stores: false,
                },
            )],
        );
        if decl.is_some() {
            self.pop_pending();
        }
        let modes = [
            ("read", FileMode::Read),
            ("write", FileMode::Write),
            ("append", FileMode::Append),
        ];
        let mode = self.choice(block, "MODE", FileMode::Read, &modes);
        let name_loc = self.loc(&block.id, field("FILE"));
        let Some(decl) = decl else {
            self.incomplete(name_loc, "This 'open file' block needs a name for its file.");
            return None;
        };
        let stream = if mode == FileMode::Read {
            LibType::IfStream
        } else {
            LibType::OfStream
        };
        let ty = Type::lib(stream, Vec::new()).unwrap_or(Type::Error);
        // Declared in the list the block is in: the part for a file that
        // can't be opened sees it, and so do the blocks after this one.
        self.declare_loop_item(block, "FILE", &decl, SymbolKind::Variable { is_const: false }, ty);
        let failed = access::extra_flag(block, "hasFailed").then(|| self.body(block, "FAILED", false));
        Some(StmtKind::OpenFile {
            file: decl.sym,
            path,
            mode,
            failed,
        })
    }

    /// *read line from [file] into [line]*: whether a line was read.
    fn read_line(&mut self, block: &Block, origin: Origin) -> Expr {
        let mut typed = self.typed(self.block_sig(&block.block_type));
        let file = self.file_part(
            block,
            &mut typed,
            "FILE",
            false,
            "Choose a file opened for reading in this block.",
        );
        let line = self.file_part(
            block,
            &mut typed,
            "LINE",
            true,
            "Choose a text variable for the line in this block.",
        );
        let typing = self.type_parts(block, &mut typed, None);
        self.report_field_fit(block, &typed, &typing, "FILE");
        self.report_field_fit(block, &typed, &typing, "LINE");
        let (Some(file), Some(line)) = (file, line) else {
            return error_expr(origin);
        };
        Expr {
            kind: ExprKind::ReadLine { file, line },
            ty: Type::Bool,
            origin,
        }
    }

    /// *write (item) … to [file] new line [newline]*: each item must be
    /// printable, as *print*'s are.
    fn write_file(&mut self, block: &Block) -> Option<StmtKind> {
        let mut typed = self.typed(self.block_sig(&block.block_type));
        let file = self.file_part(
            block,
            &mut typed,
            "FILE",
            false,
            "Choose a file opened for writing in this block.",
        );
        let typing = self.type_parts(block, &mut typed, None);
        self.report_field_fit(block, &typed, &typing, "FILE");
        let newline = self.checkbox(block, "NEWLINE", true);
        let count = access::extra_count(block, "itemCount", 1, 1);
        let items: Vec<Expr> = (0..count)
            .map(|i| self.required_input(block, &format!("ITEM{i}")))
            .collect();
        for item in &items {
            self.check_printable(item, Output::Printed);
        }
        Some(StmtKind::WriteFile {
            file: file?,
            items,
            newline,
        })
    }

    /// *check at compile time (condition) message [message]*: the message is
    /// printed as a string literal, escaped as *print*'s quoted text is.
    fn static_check(&mut self, block: &Block) -> Option<StmtKind> {
        let cond = self.required_input(block, "COND");
        let cond = self.condition(cond);
        let location = self.loc(&block.id, field("MESSAGE"));
        let Ok(text) = access::text_field(block, "MESSAGE", Some(DEFAULT_MESSAGE)) else {
            self.incomplete(location, "The message of this check is damaged. Type it again.");
            return None;
        };
        match StrLit::new(text) {
            Ok(message) => Some(StmtKind::StaticCheck { cond, message }),
            Err(error) => {
                self.diags.error(
                    codes::BAD_LITERAL,
                    location,
                    format!("This message can't be used: {error}."),
                );
                None
            }
        }
    }

    /// *for each line [line] in file (path) … if it can't be opened …*
    /// (w2.md §2.4).
    fn file_lines(&mut self, block: &Block) -> Option<StmtKind> {
        let [path] = self.typed_values(
            block,
            [(
                "PATH",
                Subject::Phrase {
                    text: String::from("The path of the file"),
                    stores: false,
                },
            )],
        );
        // The part that runs when the file can't be opened comes before the
        // line exists, as in the C++.
        let failed = access::extra_flag(block, "hasFailed").then(|| self.body(block, "FAILED", false));
        let name_loc = self.loc(&block.id, field("LINE"));
        let Ok(decl) = access::decl_field(block, "LINE") else {
            self.incomplete(name_loc, "This 'for each line' loop needs a name for its line.");
            return None;
        };
        let decl = decl.clone();
        self.scopes.push(ListKey::new(&block.id, ITEM_LIST), false);
        self.declare_loop_item(block, "LINE", &decl, SymbolKind::LoopVariable, Type::String);
        let body = self.loop_body(block, true);
        self.scopes.pop();
        Some(StmtKind::FileLines {
            line: decl.sym,
            path,
            body,
            failed,
        })
    }
}
