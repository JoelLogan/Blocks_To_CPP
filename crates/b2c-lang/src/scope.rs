//! Block-structured scopes, following C++ (spec §3.6).
//!
//! Each statement list is a frame. A frame can be *joined* to its parent when
//! C++ treats both as one scope: a function body shares the scope of the
//! parameters, and a `for` body shares the scope of its counter, so declaring
//! the same name there is an error rather than shadowing.
//!
//! Lookups are by symbol ID (what blocks store) and by name (what C++ sees);
//! both are logarithmic so that large programs stay fast.

use std::collections::BTreeMap;

use b2c_ir::ids::{BlockId, SymbolId};

/// Identifies a statement list: the block that owns it and the input name
/// (`BODY`, `DO0`, …), or a pseudo-list such as `#params`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ListKey {
    /// The owning block.
    pub(crate) block: BlockId,
    /// The statement input name or pseudo-list name.
    pub(crate) list: String,
}

impl ListKey {
    /// A key for a list of a block.
    pub(crate) fn new(block: &BlockId, list: &str) -> Self {
        Self {
            block: block.clone(),
            list: list.to_owned(),
        }
    }
}

/// Pseudo-list holding a function's parameters.
pub(crate) const PARAMS_LIST: &str = "#params";
/// Pseudo-list holding a `for` loop's counter.
pub(crate) const COUNTER_LIST: &str = "#counter";

#[derive(Debug)]
struct Frame {
    key: ListKey,
    joined: bool,
    declared: Vec<(String, SymbolId)>,
}

/// The stack of scopes at the current point of the analysis.
#[derive(Debug, Default)]
pub(crate) struct Scopes {
    frames: Vec<Frame>,
    by_sym: BTreeMap<SymbolId, usize>,
    by_name: BTreeMap<String, Vec<(usize, SymbolId)>>,
}

impl Scopes {
    /// Opens a scope for a statement list.
    pub(crate) fn push(&mut self, key: ListKey, joined: bool) {
        self.frames.push(Frame {
            key,
            joined,
            declared: Vec::new(),
        });
    }

    /// Closes the innermost scope.
    pub(crate) fn pop(&mut self) {
        let Some(frame) = self.frames.pop() else {
            return;
        };
        let index = self.frames.len();
        for (name, sym) in frame.declared {
            if self.by_sym.get(&sym) == Some(&index) {
                self.by_sym.remove(&sym);
            }
            if let Some(stack) = self.by_name.get_mut(&name) {
                stack.pop();
                if stack.is_empty() {
                    self.by_name.remove(&name);
                }
            }
        }
    }

    /// Declares a symbol in the innermost scope. Does nothing outside any scope.
    pub(crate) fn declare(&mut self, name: &str, sym: &SymbolId) {
        let Some(index) = self.frames.len().checked_sub(1) else {
            return;
        };
        if let Some(frame) = self.frames.get_mut(index) {
            frame.declared.push((name.to_owned(), sym.clone()));
        }
        self.by_sym.insert(sym.clone(), index);
        self.by_name
            .entry(name.to_owned())
            .or_default()
            .push((index, sym.clone()));
    }

    /// Whether a symbol is visible here.
    pub(crate) fn contains(&self, sym: &SymbolId) -> bool {
        self.by_sym.contains_key(sym)
    }

    /// The symbol C++ finds for a name here (the innermost declaration).
    pub(crate) fn lookup(&self, name: &str) -> Option<&SymbolId> {
        self.by_name
            .get(name)
            .and_then(|stack| stack.last())
            .map(|(_, sym)| sym)
    }

    /// A symbol with this name declared in the same C++ scope as the
    /// innermost frame (a redeclaration would be an error).
    pub(crate) fn in_same_scope(&self, name: &str) -> Option<&SymbolId> {
        let top = self.frames.len().checked_sub(1)?;
        let joined = self.frames.get(top).is_some_and(|f| f.joined);
        let (index, sym) = self.by_name.get(name)?.last()?;
        (*index == top || (joined && *index + 1 == top)).then_some(sym)
    }

    /// Whether the statement lists in `path` enclose the current position
    /// (so a declaration in that list would be visible here if it came first).
    pub(crate) fn encloses(&self, path: &[ListKey]) -> bool {
        path.len() <= self.frames.len()
            && path
                .iter()
                .zip(&self.frames)
                .all(|(key, frame)| *key == frame.key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sym(s: &str) -> SymbolId {
        SymbolId::new(s).expect("id")
    }

    fn key(b: &str, l: &str) -> ListKey {
        ListKey::new(&BlockId::new(b).expect("id"), l)
    }

    #[test]
    fn nesting_and_shadowing() {
        let mut s = Scopes::default();
        assert!(s.in_same_scope("x").is_none());
        s.declare("ignored", &sym("i0"));
        assert!(!s.contains(&sym("i0")));

        s.push(key("m", "BODY"), false);
        s.declare("x", &sym("x1"));
        assert_eq!(s.lookup("x"), Some(&sym("x1")));
        assert_eq!(s.in_same_scope("x"), Some(&sym("x1")));

        s.push(key("if", "DO0"), false);
        assert!(s.contains(&sym("x1")));
        assert!(
            s.in_same_scope("x").is_none(),
            "outer x is shadowed, not redeclared"
        );
        s.declare("x", &sym("x2"));
        assert_eq!(s.lookup("x"), Some(&sym("x2")));
        assert!(s.encloses(&[key("m", "BODY")]));
        assert!(s.encloses(&[key("m", "BODY"), key("if", "DO0")]));
        assert!(!s.encloses(&[key("m", "BODY"), key("if", "DO1")]));
        s.pop();

        assert!(!s.contains(&sym("x2")));
        assert_eq!(s.lookup("x"), Some(&sym("x1")));
        s.pop();
        assert!(s.lookup("x").is_none());
        assert!(!s.contains(&sym("x1")));
        s.pop(); // popping an empty stack is harmless
    }

    #[test]
    fn joined_frames_share_a_scope() {
        let mut s = Scopes::default();
        s.push(key("f", PARAMS_LIST), false);
        s.declare("n", &sym("p"));
        s.push(key("f", "BODY"), true);
        assert_eq!(s.in_same_scope("n"), Some(&sym("p")));
        s.push(key("w", "BODY"), false);
        assert!(s.in_same_scope("n").is_none());
    }
}
