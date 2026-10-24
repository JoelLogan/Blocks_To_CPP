//! Decoding a clipboard payload (spec §5.12) with the same block decoder,
//! limits and text rules as a project file.

use std::collections::BTreeMap;

use b2c_ir::SymbolId;

use super::{Decoder, Placement, Seg};
use crate::clipboard::{Clipboard, ClipboardRef, RefKind};
use crate::codes::{self, Diags};
use crate::json::Json;
use crate::limits::MAX_QUALIFIED_NAME_LEN;
use crate::text_rules::quote;

/// Keys of a clipboard payload, in canonical order.
const CLIPBOARD_KEYS: [&str; 5] = ["format", "formatVersion", "catalog", "blocks", "refs"];

/// Every reference kind, in declaration order.
pub(crate) const REF_KINDS: [RefKind; 4] = [
    RefKind::Variable,
    RefKind::Parameter,
    RefKind::LoopVariable,
    RefKind::Function,
];

impl<'a> Decoder<'a> {
    /// Decodes a whole clipboard payload whose header (`format` and
    /// `formatVersion`) was already checked; returns it with the
    /// diagnostics.
    pub(crate) fn run_clipboard(mut self, root: &'a Json) -> (Option<Clipboard>, Diags) {
        let clipboard = self.clipboard(root);
        (clipboard, self.diags)
    }

    fn clipboard(&mut self, root: &'a Json) -> Option<Clipboard> {
        let entries = self.object(root, &CLIPBOARD_KEYS)?;
        let catalog = self.required(entries, "catalog", Self::string);
        // The copied blocks are top-level blocks of the payload: they may
        // carry a stack, like blocks on a canvas, and a canvas position. The
        // position is checked like one on a canvas and then dropped, because
        // a paste places blocks at the target (spec §5.12). A stacked or
        // nested block with a position is still `B2C-E0128`.
        let blocks = self
            .required(entries, "blocks", |d, v| {
                d.list(v, |d, item| d.block(item, Placement::Canvas))
            })
            .map(|mut blocks| {
                for block in &mut blocks {
                    block.x = None;
                    block.y = None;
                }
                blocks
            });
        let refs = self.required(entries, "refs", Self::refs);
        Some(Clipboard {
            catalog: catalog?,
            blocks: blocks?,
            refs: refs?,
        })
    }

    /// `refs`: symbol ID → qualified name and kind.
    fn refs(&mut self, value: &'a Json) -> Option<BTreeMap<SymbolId, ClipboardRef>> {
        let Json::Object(entries) = value else {
            self.wrong(value, "an object");
            return None;
        };
        let mut out = BTreeMap::new();
        for (key, item) in entries {
            if !self.map_key(key, false) {
                continue;
            }
            let sym = if let Ok(sym) = SymbolId::new(key) {
                Some(sym)
            } else {
                let message = format!(
                    "{} has a key that is not a valid symbol ID: {} is not 1 to 32 characters from A–Z, a–z, 0–9 and _.",
                    self.subject(),
                    quote(key)
                );
                self.report(codes::BAD_ID, message);
                None
            };
            let reference = self.at(Seg::Key(key), |d| d.clipboard_ref(item));
            if let (Some(sym), Some(reference)) = (sym, reference) {
                out.insert(sym, reference);
            }
        }
        Some(out)
    }

    fn clipboard_ref(&mut self, value: &'a Json) -> Option<ClipboardRef> {
        let entries = self.object(value, &["name", "kind"])?;
        let name = self.required(entries, "name", |d, v| {
            let name = d.string(v)?;
            d.check_qualified_name_length(&name).then_some(name)
        });
        let kind = self.required(entries, "kind", |d, v| d.choice(v, &REF_KINDS));
        Some(ClipboardRef {
            name: name?,
            kind: kind?,
        })
    }

    /// Qualified names are limited in characters, like plain names.
    fn check_qualified_name_length(&mut self, name: &str) -> bool {
        let length = name.chars().count();
        if length <= MAX_QUALIFIED_NAME_LEN {
            return true;
        }
        let message = format!(
            "{} is {length} characters long, but a qualified name can be at most {MAX_QUALIFIED_NAME_LEN}.",
            self.subject(),
        );
        self.report(codes::TOO_LONG, message);
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decode::serde_name;

    fn index_of_kind(value: RefKind) -> usize {
        // Fails to compile when a kind is added, so the list stays complete.
        match value {
            RefKind::Variable => 0,
            RefKind::Parameter => 1,
            RefKind::LoopVariable => 2,
            RefKind::Function => 3,
        }
    }

    #[test]
    fn kind_list_is_complete_and_ordered() {
        for (i, kind) in REF_KINDS.iter().enumerate() {
            assert_eq!(index_of_kind(*kind), i);
        }
        assert_eq!(
            serde_name(&RefKind::LoopVariable).as_deref(),
            Some("loopVariable")
        );
    }
}
