//! Pass 1: find every declaration in the document, including those in
//! disabled or unattached blocks.
//!
//! The lowering pass declares symbols as it walks, so it only knows what is
//! visible at each point. This table lets it explain *why* a reference does
//! not resolve: the symbol is declared later, out of scope, in a disabled
//! block, in another module, or nowhere at all.

use std::collections::BTreeMap;

use b2c_ir::diag::{Location, Part};
use b2c_ir::ids::{ModuleId, SymbolId};
use b2c_model::{Block, Document, Input};

use crate::access::{decl_field, is_statement_type, param_rows, statement_lists};
use crate::scope::{COUNTER_LIST, ListKey, PARAMS_LIST};

/// Deepest block nesting the analyser follows (statement and value nesting
/// combined). A loaded project file cannot get close: its JSON nesting limit
/// allows about 40 levels of blocks.
pub(crate) const MAX_BLOCK_DEPTH: usize = 64;

/// What a declaration declares.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DeclKind {
    /// A local variable (`var.declare`).
    Variable,
    /// A function parameter.
    Parameter,
    /// A `for` loop counter.
    Counter,
    /// A function.
    Function,
}

/// Whether a declaration is part of the program.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DeclStatus {
    /// Part of the program.
    Active,
    /// Inside a disabled block.
    Disabled,
    /// In a block that is not attached to `main` or a function.
    Detached,
}

/// A declaration found anywhere in the document.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DeclInfo {
    /// The user's name (not validated).
    pub(crate) name: String,
    /// What it declares.
    pub(crate) kind: DeclKind,
    /// Whether it is part of the program.
    pub(crate) status: DeclStatus,
    /// Index of its module in the document.
    pub(crate) module: usize,
    /// Where it is declared (the declaring field).
    pub(crate) location: Location,
    /// The statement lists enclosing it, outermost first (empty for functions).
    pub(crate) path: Vec<ListKey>,
    /// For parameters: the function's name.
    pub(crate) owner: Option<String>,
}

/// Every declaration in a document, by symbol ID (the first one wins when a
/// damaged file declares an ID twice).
pub(crate) fn collect(document: &Document) -> BTreeMap<SymbolId, DeclInfo> {
    let mut collector = Collector {
        decls: BTreeMap::new(),
        module: 0,
        module_id: None,
    };
    for (index, module) in document.modules.iter().enumerate() {
        collector.module = index;
        collector.module_id = Some(module.id.clone());
        let mut blocks: Vec<&Block> = module.workspace.blocks.iter().collect();
        blocks.sort_by(|a, b| a.id.cmp(&b.id));
        for block in blocks {
            collector.top_level(block);
        }
    }
    collector.decls
}

struct Collector {
    decls: BTreeMap<SymbolId, DeclInfo>,
    module: usize,
    module_id: Option<ModuleId>,
}

impl Collector {
    fn record(&mut self, block: &Block, field: &str, kind: DeclKind, status: DeclStatus, path: &[ListKey]) {
        if let Ok(decl) = decl_field(block, field) {
            self.record_symbol(&decl.sym, &decl.name, block, field, kind, status, path, None);
        }
    }

    #[allow(clippy::too_many_arguments)] // a private helper; a struct would only rename the arguments
    fn record_symbol(
        &mut self,
        sym: &SymbolId,
        name: &str,
        block: &Block,
        part: &str,
        kind: DeclKind,
        status: DeclStatus,
        path: &[ListKey],
        owner: Option<&str>,
    ) {
        if self.decls.contains_key(sym) {
            return;
        }
        let location = Location {
            module: self.module_id.clone(),
            block: Some(block.id.clone()),
            part: Part::Field {
                name: part.to_owned(),
            },
        };
        self.decls.insert(
            sym.clone(),
            DeclInfo {
                name: name.to_owned(),
                kind,
                status,
                module: self.module,
                location,
                path: path.to_vec(),
                owner: owner.map(str::to_owned),
            },
        );
    }

    fn top_level(&mut self, block: &Block) {
        let status = if block.disabled {
            DeclStatus::Disabled
        } else {
            DeclStatus::Active
        };
        match block.block_type.as_str() {
            "program.main" => {
                let path = [ListKey::new(&block.id, "BODY")];
                self.list(block, "BODY", &path, status, 1);
                self.other_statements(block, &["BODY"], 1, status);
            }
            "func.define" => self.function(block, status),
            _ => self.anywhere(block, Self::off_program(status), 0),
        }
        // A loose stack (05 §5.4) is outside the program, like its head.
        for stacked in &block.stack {
            self.anywhere(stacked, Self::off_program(status), 0);
        }
    }

    fn function(&mut self, block: &Block, status: DeclStatus) {
        self.record(block, "NAME", DeclKind::Function, status, &[]);
        let name = decl_field(block, "NAME").map(|d| d.name.clone()).ok();
        let params = [ListKey::new(&block.id, PARAMS_LIST)];
        for (i, row) in param_rows(block).into_iter().enumerate() {
            if let Ok(row) = row {
                let part = format!("params[{i}]");
                let kind = DeclKind::Parameter;
                self.record_symbol(
                    &row.sym,
                    &row.name,
                    block,
                    &part,
                    kind,
                    status,
                    &params,
                    name.as_deref(),
                );
            }
        }
        let body = [params[0].clone(), ListKey::new(&block.id, "BODY")];
        self.list(block, "BODY", &body, status, 1);
        self.other_statements(block, &["BODY"], 1, status);
    }

    /// The statements of one list of `owner`, at `path`.
    fn list(&mut self, owner: &Block, name: &str, path: &[ListKey], status: DeclStatus, depth: usize) {
        for block in crate::access::statements(owner, name) {
            self.statement(block, path, status, depth);
        }
    }

    fn statement(&mut self, block: &Block, path: &[ListKey], status: DeclStatus, depth: usize) {
        if depth > MAX_BLOCK_DEPTH {
            return;
        }
        let status = if block.disabled && status == DeclStatus::Active {
            DeclStatus::Disabled
        } else {
            status
        };
        if !is_statement_type(&block.block_type) {
            self.anywhere(block, Self::off_program(status), depth);
            return;
        }
        let mut inner: Vec<ListKey> = path.to_vec();
        match block.block_type.as_str() {
            "var.declare" => self.record(block, "NAME", DeclKind::Variable, status, path),
            "control.for_range" => {
                inner.push(ListKey::new(&block.id, COUNTER_LIST));
                self.record(block, "VAR", DeclKind::Counter, status, &inner);
            }
            _ => {}
        }
        let lists = statement_lists(block);
        let names: Vec<&str> = lists.iter().map(|(name, _)| name.as_str()).collect();
        for (name, _) in &lists {
            let mut list_path = inner.clone();
            list_path.push(ListKey::new(&block.id, name));
            self.list(block, name, &list_path, status, depth + 1);
        }
        self.other_statements(block, &names, depth, status);
        self.inputs(block, depth, status);
    }

    /// Statement inputs that the lowering ignores: their declarations are detached.
    fn other_statements(&mut self, block: &Block, lowered: &[&str], depth: usize, status: DeclStatus) {
        for (name, list) in &block.statements {
            if !lowered.contains(&name.as_str()) {
                for nested in list {
                    self.anywhere(nested, Self::off_program(status), depth + 1);
                }
            }
        }
    }

    /// Nested reporter blocks (they declare nothing in M1, but a misplaced
    /// statement block may hide there).
    fn inputs(&mut self, block: &Block, depth: usize, status: DeclStatus) {
        for input in block.inputs.values() {
            if let Input::Block(nested) = input {
                self.anywhere(&nested.block, Self::off_program(status), depth + 1);
            }
        }
    }

    /// Records declarations of any block that is not part of the program.
    fn anywhere(&mut self, block: &Block, status: DeclStatus, depth: usize) {
        if depth > MAX_BLOCK_DEPTH {
            return;
        }
        let status = if block.disabled {
            DeclStatus::Disabled
        } else {
            status
        };
        match block.block_type.as_str() {
            "var.declare" | "func.define" => self.record(block, "NAME", Self::kind_of(block), status, &[]),
            "control.for_range" => self.record(block, "VAR", DeclKind::Counter, status, &[]),
            _ => {}
        }
        for list in block.statements.values() {
            for nested in list {
                self.anywhere(nested, status, depth + 1);
            }
        }
        for input in block.inputs.values() {
            if let Input::Block(nested) = input {
                self.anywhere(&nested.block, status, depth + 1);
            }
        }
    }

    fn kind_of(block: &Block) -> DeclKind {
        if block.block_type == "func.define" {
            DeclKind::Function
        } else {
            DeclKind::Variable
        }
    }

    /// The status of declarations in blocks the lowering does not reach.
    fn off_program(status: DeclStatus) -> DeclStatus {
        if status == DeclStatus::Disabled {
            DeclStatus::Disabled
        } else {
            DeclStatus::Detached
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use b2c_ir::ids::BlockId;
    use serde_json::json;

    fn doc(blocks: &serde_json::Value) -> Document {
        serde_json::from_value(json!({
            "format": "blocks2cpp/project", "formatVersion": 1,
            "generator": {"app": "0.1.0", "catalog": "1.0.0"},
            "project": {"id": "p", "name": "T", "language": {"standard": "c++20"}},
            "modules": [{"id": "m", "name": "main", "workspace": {"blocks": blocks}}]
        }))
        .expect("document")
    }

    fn declare(id: &str, sym: &str, disabled: bool) -> serde_json::Value {
        json!({"id": id, "type": "var.declare", "v": 1, "disabled": disabled,
               "fields": {"TYPE": "int", "NAME": {"sym": sym, "name": sym}}})
    }

    fn get(decls: &BTreeMap<SymbolId, DeclInfo>, s: &str) -> DeclInfo {
        decls
            .get(&SymbolId::new(s).expect("id"))
            .cloned()
            .unwrap_or_else(|| panic!("{s} not collected"))
    }

    #[test]
    fn statuses_and_paths() {
        let d = doc(&json!([
            {"id": "main", "type": "program.main", "v": 1, "statements": {"BODY": [
                declare("d1", "a", false),
                declare("d2", "b", true),
                {"id": "if", "type": "control.if", "v": 1, "disabled": true, "statements": {"DO0": [declare("d3", "c", false)]}},
                {"id": "for", "type": "control.for_range", "v": 1,
                 "fields": {"VAR": {"sym": "i", "name": "i"}},
                 "statements": {"BODY": [declare("d4", "d", false)], "JUNK": [declare("d5", "e", false)]}},
                {"id": "num", "type": "math.number", "v": 1, "statements": {"X": [declare("d6", "f6", false)]}}
            ]}},
            declare("loose", "g", false),
            {"id": "fn", "type": "func.define", "v": 1,
             "fields": {"NAME": {"sym": "f", "name": "f"}},
             "extra": {"params": [{"sym": "p", "name": "p", "type": "int", "mode": "copy"}]},
             "statements": {"BODY": [declare("d7", "h", false)]}},
            {"id": "off", "type": "func.define", "v": 1, "disabled": true,
             "fields": {"NAME": {"sym": "g2", "name": "g2"}},
             "statements": {"BODY": [declare("d8", "k", false)]}}
        ]));
        let decls = collect(&d);
        let main = ListKey::new(&BlockId::new("main").expect("id"), "BODY");
        let a = get(&decls, "a");
        assert_eq!((a.kind, a.status), (DeclKind::Variable, DeclStatus::Active));
        assert_eq!(a.path, vec![main.clone()]);
        assert_eq!(get(&decls, "b").status, DeclStatus::Disabled);
        assert_eq!(get(&decls, "c").status, DeclStatus::Disabled);
        let i = get(&decls, "i");
        assert_eq!((i.kind, i.status), (DeclKind::Counter, DeclStatus::Active));
        assert_eq!(i.path.len(), 2);
        assert_eq!(get(&decls, "d").path.len(), 3);
        assert_eq!(get(&decls, "e").status, DeclStatus::Detached);
        assert_eq!(get(&decls, "f6").status, DeclStatus::Detached);
        assert_eq!(get(&decls, "g").status, DeclStatus::Detached);
        let func = get(&decls, "f");
        assert_eq!((func.kind, func.status), (DeclKind::Function, DeclStatus::Active));
        assert!(func.path.is_empty());
        let p = get(&decls, "p");
        assert_eq!((p.kind, p.owner.as_deref()), (DeclKind::Parameter, Some("f")));
        assert_eq!(
            p.location.part,
            Part::Field {
                name: String::from("params[0]")
            }
        );
        assert_eq!(get(&decls, "h").path.len(), 2);
        assert_eq!(get(&decls, "g2").status, DeclStatus::Disabled);
        assert_eq!(get(&decls, "k").status, DeclStatus::Disabled);
    }

    #[test]
    fn declarations_in_a_loose_stack_are_detached() {
        let mut head = declare("head", "h", false);
        head["x"] = json!(0);
        head["y"] = json!(0);
        head["stack"] = json!([
            declare("s1", "s", false),
            {"id": "w", "type": "control.while", "v": 1,
             "statements": {"BODY": [declare("s2", "t", false)]}},
            declare("s3", "u", true)
        ]);
        let decls = collect(&doc(&json!([head])));
        assert_eq!(get(&decls, "h").status, DeclStatus::Detached);
        assert_eq!(get(&decls, "s").status, DeclStatus::Detached);
        assert_eq!(get(&decls, "t").status, DeclStatus::Detached);
        assert_eq!(get(&decls, "u").status, DeclStatus::Disabled);
    }

    #[test]
    fn deep_trees_are_cut_off() {
        let mut block = declare("leaf", "deep", false);
        for i in 0..(MAX_BLOCK_DEPTH + 10) {
            block = json!({"id": format!("w{i}"), "type": "control.forever", "v": 1, "statements": {"BODY": [block]}});
        }
        let d =
            doc(&json!([{"id": "main", "type": "program.main", "v": 1, "statements": {"BODY": [block]}}]));
        let decls = collect(&d);
        assert!(decls.is_empty());
    }
}
