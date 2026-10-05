//! Fresh IDs for pasted and duplicated blocks (spec §5.4, §5.5, §5.12).
//!
//! A paste or a duplicate must give every block a new block ID, and every
//! symbol declared inside the blocks a new symbol ID, rewriting the
//! references to those symbols, or the document would fail to load with
//! `B2C-E0114`/`B2C-E0115`. References to symbols declared outside the
//! blocks are left alone; the paste binds them again by name.
//!
//! The pure crates use no randomness, so the randomness comes from outside:
//! [`SeededIds`] derives IDs from a 256-bit seed that the caller takes from
//! a cryptographic random source (in the editor, `crypto.getRandomValues`).
//! The same seed always gives the same IDs, which keeps tests reproducible.
//!
//! All walks are iterative: statement lists and stacks may hold up to
//! [`crate::limits::MAX_BLOCKS`] blocks, and documents built in memory have
//! no depth limit.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use b2c_ir::{BlockId, IdError, SymbolId};
use sha2::{Digest, Sha256};

use crate::document::{Block, Document, FieldValue, Input, Token};
use crate::walk;

/// How many candidates [`remap_ids`] asks for before it gives up on one ID.
pub const MAX_ID_ATTEMPTS: usize = 64;

/// What a generated ID names, which decides its prefix.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum IdKind {
    /// A block ID (`blk_…`).
    Block,
    /// A symbol ID (`sym_…`).
    Symbol,
}

impl IdKind {
    /// The prefix of IDs of this kind: `blk_` or `sym_`.
    pub fn prefix(self) -> &'static str {
        match self {
            Self::Block => "blk_",
            Self::Symbol => "sym_",
        }
    }

    /// The bytes that stand for this kind in [`SeededIds`]' hash input.
    fn tag(self) -> &'static [u8] {
        match self {
            Self::Block => b"blk",
            Self::Symbol => b"sym",
        }
    }
}

impl fmt::Display for IdKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Block => "block",
            Self::Symbol => "symbol",
        })
    }
}

/// A source of candidate IDs. [`remap_ids`] validates every candidate and
/// asks again when one is already in use, so a source only has to make
/// collisions unlikely.
pub trait IdSource {
    /// The next candidate ID of `kind`.
    fn next_id(&mut self, kind: IdKind) -> String;
}

/// Deterministic IDs derived from a 256-bit seed.
///
/// The ID number `counter` (0, 1, 2, … counted separately for each kind) is
/// the kind's [prefix](IdKind::prefix) followed by 17 base-62 digits
/// (`0`–`9`, `A`–`Z`, `a`–`z`, most significant first) of the first 16
/// bytes, read as a big-endian number modulo 62¹⁷, of
/// `SHA-256(seed ‖ tag ‖ counter)`, where `tag` is the ASCII text `blk` or
/// `sym` and `counter` is 8 bytes, big-endian. 62¹⁷ is above 2¹⁰¹, so with
/// a random seed each ID carries more than the 96 random bits that spec
/// §5.4 asks for, and IDs are 21 characters long.
#[derive(Clone)]
pub struct SeededIds {
    seed: [u8; 32],
    blocks: u64,
    symbols: u64,
}

impl SeededIds {
    /// IDs derived from `seed`, which should come from a cryptographic
    /// random source.
    pub fn new(seed: [u8; 32]) -> Self {
        Self {
            seed,
            blocks: 0,
            symbols: 0,
        }
    }
}

impl fmt::Debug for SeededIds {
    /// Leaves out the seed, which would predict every later ID.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SeededIds")
            .field("blocks", &self.blocks)
            .field("symbols", &self.symbols)
            .finish_non_exhaustive()
    }
}

impl IdSource for SeededIds {
    fn next_id(&mut self, kind: IdKind) -> String {
        const DIGITS: &[u8; 62] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
        const LENGTH: usize = 17;
        let counter = match kind {
            IdKind::Block => &mut self.blocks,
            IdKind::Symbol => &mut self.symbols,
        };
        let number = *counter;
        *counter = counter.wrapping_add(1);
        let digest = Sha256::new()
            .chain_update(self.seed)
            .chain_update(kind.tag())
            .chain_update(number.to_be_bytes())
            .finalize();
        let mut first = [0u8; 16];
        first.copy_from_slice(digest.get(..16).unwrap_or(&[0; 16]));
        let mut value = u128::from_be_bytes(first);
        let mut digits = [b'0'; LENGTH];
        for slot in digits.iter_mut().rev() {
            let index = usize::try_from(value % 62).unwrap_or(0);
            *slot = DIGITS.get(index).copied().unwrap_or(b'0');
            value /= 62;
        }
        let mut id = String::with_capacity(kind.prefix().len() + LENGTH);
        id.push_str(kind.prefix());
        id.extend(digits.iter().map(|&b| char::from(b)));
        id
    }
}

/// Why [`remap_ids`] could not give the blocks fresh IDs. The blocks may be
/// partly changed; callers discard them.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RemapError {
    /// The ID source returned text that is not a valid ID.
    #[error("the ID source returned an invalid ID: {0}")]
    InvalidId(#[from] IdError),
    /// The ID source kept returning IDs that are already in use.
    #[error("the ID source returned no unused {kind} ID in {attempts} attempts")]
    Exhausted {
        /// The kind of ID that was asked for.
        kind: IdKind,
        /// How many candidates were tried.
        attempts: usize,
    },
    /// The blocks declare the same symbol twice (a valid payload never
    /// does: the loader reports it as `B2C-E0115`).
    #[error("the symbol `{0}` is declared more than once in the blocks")]
    DuplicateSymbol(SymbolId),
}

/// Gives every block in `blocks` (at any depth, in stacks too) a fresh block
/// ID, and every symbol declared inside them a fresh symbol ID, and points
/// the references to those symbols (field references, expression tokens) at
/// the new IDs. References to symbols declared elsewhere are not changed.
///
/// A fresh ID is one that is not in `taken` and was not given earlier in
/// the same call; [`used_ids`] gives the `taken` set of a document. Symbols
/// get their IDs first (in walk order), then blocks.
///
/// Returns the map from each declared symbol's old ID to its new one.
///
/// # Errors
/// See [`RemapError`].
pub fn remap_ids(
    blocks: &mut [Block],
    taken: &BTreeSet<String>,
    ids: &mut dyn IdSource,
) -> Result<BTreeMap<SymbolId, SymbolId>, RemapError> {
    let mut fresh = Fresh {
        taken,
        given: BTreeSet::new(),
        ids,
    };
    // First every declaration, because a reference can come before the
    // declaration it names (a call above the function's definition).
    let mut declared = Vec::new();
    walk::blocks(blocks, |block| declared.extend(declarations(block)));
    let mut symbols = BTreeMap::new();
    for old in declared {
        if symbols.contains_key(&old) {
            return Err(RemapError::DuplicateSymbol(old));
        }
        let new = SymbolId::new(&fresh.next(IdKind::Symbol)?)?;
        symbols.insert(old, new);
    }
    let mut result = Ok(());
    walk::blocks_mut(blocks, |block| {
        if result.is_err() {
            return;
        }
        match fresh
            .next(IdKind::Block)
            .and_then(|id| BlockId::new(&id).map_err(RemapError::from))
        {
            Ok(id) => block.id = id,
            Err(error) => result = Err(error),
        }
        rename_symbols(block, &symbols, true);
    });
    result.map(|()| symbols)
}

/// Points every reference to a symbol in `map` (field references and
/// expression tokens, at any depth) at the symbol's new ID. Declarations
/// are not changed. A paste uses this to bind references to symbols
/// declared outside the pasted blocks to the target's symbols.
pub fn rewrite_refs(blocks: &mut [Block], map: &BTreeMap<SymbolId, SymbolId>) {
    if map.is_empty() {
        return;
    }
    walk::blocks_mut(blocks, |block| rename_symbols(block, map, false));
}

/// The symbols that `blocks` refer to (in field references or expression
/// tokens) but do not declare: what a clipboard payload's `refs` describes
/// (spec §5.12).
pub fn outside_refs(blocks: &[Block]) -> BTreeSet<SymbolId> {
    let mut declared = BTreeSet::new();
    let mut referenced = BTreeSet::new();
    walk::blocks(blocks, |block| {
        declared.extend(declarations(block));
        for value in block.fields.values() {
            if let FieldValue::Ref(reference) = value {
                referenced.insert(reference.target.clone());
            }
        }
        for input in block.inputs.values() {
            if let Input::Expr(expr) = input {
                for token in &expr.expr {
                    if let Token::Ref(sym) = token {
                        referenced.insert(sym.clone());
                    }
                }
            }
        }
    });
    referenced.retain(|sym| !declared.contains(sym));
    referenced
}

/// Every block, frame, note and declared symbol ID of a document: the IDs a
/// paste into it must not reuse (the `taken` set of [`remap_ids`]).
pub fn used_ids(document: &Document) -> BTreeSet<String> {
    let mut used = BTreeSet::new();
    walk::document(document, |block| {
        used.insert(block.id.as_str().to_owned());
        used.extend(declarations(block).map(|sym| sym.as_str().to_owned()));
    });
    for module in &document.modules {
        let workspace = &module.workspace;
        used.extend(workspace.frames.iter().map(|f| f.id.as_str().to_owned()));
        used.extend(workspace.notes.iter().map(|n| n.id.as_str().to_owned()));
    }
    used
}

/// Hands out fresh IDs: validated, not taken and not given before.
struct Fresh<'a> {
    taken: &'a BTreeSet<String>,
    given: BTreeSet<String>,
    ids: &'a mut dyn IdSource,
}

impl Fresh<'_> {
    fn next(&mut self, kind: IdKind) -> Result<String, RemapError> {
        for _ in 0..MAX_ID_ATTEMPTS {
            let id = self.ids.next_id(kind);
            // Validates the text the same way for both kinds.
            BlockId::new(&id)?;
            if !self.taken.contains(&id) && self.given.insert(id.clone()) {
                return Ok(id);
            }
        }
        Err(RemapError::Exhausted {
            kind,
            attempts: MAX_ID_ATTEMPTS,
        })
    }
}

/// The symbols a block declares, as the loader counts them: symbol
/// declaration fields, and the `sym` of each row of `extra.params`
/// (function parameters).
fn declarations(block: &Block) -> impl Iterator<Item = SymbolId> + '_ {
    let fields = block.fields.values().filter_map(|value| match value {
        FieldValue::Decl(decl) => Some(decl.sym.clone()),
        _ => None,
    });
    let params = block
        .extra
        .get("params")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|row| row.get("sym")?.as_str())
        .filter_map(|sym| SymbolId::new(sym).ok());
    fields.chain(params)
}

/// Renames the symbols in `map` in one block (not its children): its
/// references, and its declarations too when `declarations` is set.
fn rename_symbols(block: &mut Block, map: &BTreeMap<SymbolId, SymbolId>, declarations: bool) {
    for value in block.fields.values_mut() {
        match value {
            FieldValue::Ref(reference) => {
                if let Some(new) = map.get(&reference.target) {
                    reference.target = new.clone();
                }
            }
            FieldValue::Decl(decl) if declarations => {
                if let Some(new) = map.get(&decl.sym) {
                    decl.sym = new.clone();
                }
            }
            _ => {}
        }
    }
    for input in block.inputs.values_mut() {
        if let Input::Expr(expr) = input {
            for token in &mut expr.expr {
                if let Token::Ref(sym) = token
                    && let Some(new) = map.get(sym)
                {
                    *sym = new.clone();
                }
            }
        }
    }
    if declarations && let Some(serde_json::Value::Array(rows)) = block.extra.get_mut("params") {
        for row in rows {
            if let Some(serde_json::Value::String(text)) = row.get_mut("sym")
                && let Ok(sym) = SymbolId::new(text)
                && let Some(new) = map.get(&sym)
            {
                new.as_str().clone_into(text);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Pinned outputs, computed independently from the documented
    /// derivation (a few lines of Python with `hashlib`): changing the
    /// derivation changes every ID a seed gives, so it must be deliberate.
    #[test]
    fn seeded_ids_are_pinned() {
        let mut ids = SeededIds::new([0; 32]);
        let first = ids.next_id(IdKind::Block);
        let symbol = ids.next_id(IdKind::Symbol);
        let second = ids.next_id(IdKind::Block);
        assert_eq!(
            [first.as_str(), second.as_str(), symbol.as_str()],
            [
                "blk_0iVy0jeAAlTRzCwTE",
                "blk_3eG0dQWvSE8NLMbt9",
                "sym_Gzg8AZLXY0B7MYgxT"
            ]
        );
    }

    #[test]
    fn seeded_ids_are_valid_distinct_and_reproducible() {
        let seed = [42; 32];
        let mut ids = SeededIds::new(seed);
        let mut given = BTreeSet::new();
        for _ in 0..2000 {
            for kind in [IdKind::Block, IdKind::Symbol] {
                let id = ids.next_id(kind);
                assert_eq!(id.len(), 21);
                assert!(id.starts_with(kind.prefix()));
                assert!(BlockId::new(&id).is_ok());
                assert!(given.insert(id));
            }
        }
        let mut again = SeededIds::new(seed);
        let mut other = SeededIds::new([43; 32]);
        let a = again.next_id(IdKind::Block);
        assert_eq!(a, SeededIds::new(seed).next_id(IdKind::Block));
        assert_ne!(a, other.next_id(IdKind::Block));
        assert!(!format!("{again:?}").contains("42"));
    }

    /// A source that replays a fixed list.
    struct Script(Vec<String>);

    impl IdSource for Script {
        fn next_id(&mut self, _: IdKind) -> String {
            if self.0.is_empty() {
                String::from("x")
            } else {
                self.0.remove(0)
            }
        }
    }

    fn block(id: &str) -> Block {
        Block {
            id: BlockId::new(id).unwrap(),
            block_type: "t".into(),
            v: 1,
            x: None,
            y: None,
            collapsed: false,
            disabled: false,
            comment: None,
            extra: BTreeMap::new(),
            fields: BTreeMap::new(),
            inputs: BTreeMap::new(),
            statements: BTreeMap::new(),
            stack: Vec::new(),
        }
    }

    #[test]
    fn taken_and_repeated_ids_are_skipped() {
        let mut blocks = vec![block("a"), block("b")];
        let taken: BTreeSet<String> = ["t1".to_owned()].into();
        let mut script = Script(["t1", "n1", "n1", "n2"].map(String::from).to_vec());
        remap_ids(&mut blocks, &taken, &mut script).unwrap();
        assert_eq!(blocks[0].id.as_str(), "n1");
        assert_eq!(blocks[1].id.as_str(), "n2");
    }

    #[test]
    fn source_errors_are_reported() {
        let mut blocks = vec![block("a")];
        let taken = BTreeSet::new();
        let error = remap_ids(&mut blocks, &taken, &mut Script(vec!["bad id".into()])).unwrap_err();
        assert!(matches!(error, RemapError::InvalidId(_)));
        let taken: BTreeSet<String> = ["x".to_owned()].into();
        let error = remap_ids(&mut blocks, &taken, &mut Script(Vec::new())).unwrap_err();
        assert_eq!(
            error,
            RemapError::Exhausted {
                kind: IdKind::Block,
                attempts: MAX_ID_ATTEMPTS
            }
        );
        assert_eq!(
            error.to_string(),
            "the ID source returned no unused block ID in 64 attempts"
        );
    }
}
