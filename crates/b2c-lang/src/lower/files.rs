//! Lowering the file blocks of W2 (`docs/plans/m3-contracts/w2.md` §1.8,
//! §2.4), *use header* and *check at compile time* (§1.6): package D.
//!
//! * *for each line in file* lowers its path, then its *if it can't be
//!   opened* part, then declares its line (a `std::string` loop variable,
//!   visible in its body only) and lowers its body as a loop's. The
//!   analyser does not restrict paths: a program runs as its user and opens
//!   exactly the paths its blocks hold (decision M3-D3).
//!
//! The other blocks of this module are if-time (w2.md §1.6, §1.8): until
//! they are in the catalog, a statement lowers to nothing and a value to an
//! error placeholder, as a block type the analyser does not know does.

use b2c_ir::sast::{Expr, Item, Origin, StmtKind, SymbolKind};
use b2c_ir::types::Type;
use b2c_model::Block;

use super::convert::Subject;
use super::expr::error_expr;
use super::{Lowerer, field};
use crate::access;
use crate::scope::{ITEM_LIST, ListKey};

impl Lowerer<'_> {
    /// *use header* (`program.use_header`).
    #[allow(
        clippy::unused_self,
        reason = "an if-time block of package D, not in the catalog yet"
    )]
    pub(super) fn use_header(&mut self, block: &Block) -> Option<Item> {
        let _ = block;
        None
    }

    /// *for each line in file*, *open file*, *write … to file*, *check at
    /// compile time*.
    pub(super) fn file_statement(&mut self, block: &Block) -> Option<StmtKind> {
        match block.block_type.as_str() {
            "file.for_each_line" => self.file_lines(block),
            _ => None,
        }
    }

    /// *read line from … into*, *file … exists*.
    #[allow(
        clippy::unused_self,
        reason = "if-time blocks of package D, not in the catalog yet"
    )]
    pub(super) fn file_value(&mut self, block: &Block, origin: Origin) -> Expr {
        let _ = block;
        error_expr(origin)
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
