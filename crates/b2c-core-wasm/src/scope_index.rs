//! The scope index (docs/plans/m3-contracts/worker.md §4,
//! docs/plans/m3-contracts/w5-performance.md §2.1): what the scope query
//! answers from, as JSON, so that the editor answers it where the analysis is
//! not (the preview worker previews, the page's menus ask).
//!
//! Version 1:
//!
//! ```json
//! {"v": 1, "contentHash": "<hex>", "modules": ["m1", …], "names": ["score", …],
//!  "trail": [parent, name, symbol, …], "functions": [[name, symbol, …], …],
//!  "parked": [node, …], "blocks": [["b1", node, module, "BODY", node, module, …], …],
//!  "members": [[owner, member, …], …], "created": [["b2", symbol], …]}
//! ```
//!
//! Symbols are indexes into the update's `symbols` (the analysis's
//! [`Analysis::symbol_infos`], sorted by name, then ID), -1 for one that
//! list does not have; names are indexes into `names` (the C++ names that
//! hide one another); nodes are indexes into the trail (three numbers per
//! node), -1 for none; modules are indexes into `modules`. A block's
//! position at the block has module -1 when none was recorded there.

use std::collections::HashMap;

use b2c_ir::SymbolId;
use b2c_lang::Analysis;
use serde::Serialize;

/// The format version.
pub const SCOPE_INDEX_VERSION: u32 = 1;

/// One cell of a block's entry: its ID and input names, or a number.
#[derive(Serialize)]
#[serde(untagged)]
enum Cell<'a> {
    Text(&'a str),
    Number(i64),
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Index<'a> {
    v: u32,
    content_hash: &'a str,
    modules: Vec<&'a str>,
    names: Vec<&'a str>,
    trail: Vec<i64>,
    functions: Vec<Vec<i64>>,
    parked: Vec<i64>,
    blocks: Vec<Vec<Cell<'a>>>,
    members: Vec<Vec<i64>>,
    created: Vec<(&'a str, i64)>,
}

/// Interns strings in first use.
#[derive(Default)]
struct Names<'a> {
    list: Vec<&'a str>,
    seen: HashMap<&'a str, i64>,
}

impl<'a> Names<'a> {
    fn of(&mut self, name: &'a str) -> i64 {
        if let Some(&index) = self.seen.get(name) {
            return index;
        }
        let index = number(self.list.len());
        self.list.push(name);
        self.seen.insert(name, index);
        index
    }
}

/// A count or index as a JSON number (an analysis never holds 2^63 of anything).
fn number(value: usize) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

fn node(value: Option<usize>) -> i64 {
    value.map_or(-1, number)
}

/// The scope index of `analysis`, the analysis of the document whose content
/// hash is `content_hash` (hex), as JSON text.
pub(crate) fn encode(analysis: &Analysis, content_hash: &str) -> String {
    let symbols: HashMap<SymbolId, i64> = analysis
        .symbol_infos()
        .into_iter()
        .enumerate()
        .map(|(index, info)| (info.id, number(index)))
        .collect();
    let symbol = |id: Option<&SymbolId>| id.and_then(|id| symbols.get(id)).copied().unwrap_or(-1);
    let parts = analysis.scope_index_parts();
    let mut names = Names::default();

    let mut trail = Vec::new();
    for (parent, name, sym) in parts.trail() {
        trail.extend([node(parent), names.of(name), symbol(sym)]);
    }
    let functions = parts
        .functions()
        .map(|list| {
            list.iter()
                .flat_map(|(name, sym)| [names.of(name), symbol(Some(sym))])
                .collect()
        })
        .collect();
    let blocks = parts
        .blocks()
        .map(|block| {
            let mut cells = vec![Cell::Text(block.id.as_str())];
            match block.at {
                Some(at) => cells.extend([Cell::Number(node(at.node)), Cell::Number(number(at.module))]),
                None => cells.extend([Cell::Number(-1), Cell::Number(-1)]),
            }
            for (name, at) in block.inputs {
                cells.extend([
                    Cell::Text(name),
                    Cell::Number(node(at.node)),
                    Cell::Number(number(at.module)),
                ]);
            }
            cells
        })
        .collect();
    let members = parts
        .members()
        .map(|(owner, members)| {
            std::iter::once(symbol(Some(owner)))
                .chain(members.into_iter().map(|member| symbol(Some(member))))
                .collect()
        })
        .collect();
    let created = parts
        .created()
        .into_iter()
        .map(|(block, sym)| (block.as_str(), symbol(Some(sym))))
        .collect();
    let index = Index {
        v: SCOPE_INDEX_VERSION,
        content_hash,
        modules: parts.modules().map(b2c_ir::ModuleId::as_str).collect(),
        names: names.list,
        trail,
        functions,
        parked: parts.parked().map(node).collect(),
        blocks,
        members,
        created,
    };
    serde_json::to_string(&index).unwrap_or_else(|_| String::from("null"))
}
