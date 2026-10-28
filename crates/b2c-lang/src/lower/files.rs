//! Lowering the file blocks of W2 (`docs/plans/m3-contracts/w2.md` §1.8,
//! §2.4), *use header* and *check at compile time* (§1.6): package D, with
//! the if-time blocks of package S.
//!
//! * *for each line in file* lowers its path, then its *if it can't be
//!   opened* part, then declares its line (a `std::string` loop variable,
//!   visible in its body only) and lowers its body as a loop's. The
//!   analyser does not restrict paths: a program runs as its user and opens
//!   exactly the paths its blocks hold (decision M3-D3).
//! * *use header* is a definition: its header, which the catalog checked
//!   against the header table (`B2C-E0607`), joins the module's includes.
//! * *check at compile time* lowers its condition as *if* does and keeps its
//!   message as a string literal; whether the condition is fixed when the
//!   program is built, and true, is `consts.rs`'s check (`B2C-E0424`,
//!   `B2C-E0428`).
//!
//! The other blocks of this module are if-time (w2.md §1.8): until they are
//! in the catalog, a statement lowers to nothing and a value to an error
//! placeholder, as a block type the analyser does not know does.

use b2c_ir::diag::Part;
use b2c_ir::headers::{Header, ItemHeaders};
use b2c_ir::sast::{Expr, Item, ItemKind, Origin, StmtKind, SymbolKind};
use b2c_ir::text::StrLit;
use b2c_ir::types::Type;
use b2c_model::Block;

use super::convert::Subject;
use super::expr::error_expr;
use super::{Lowerer, field};
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
            "program.check" => self.static_check(block),
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
