//! Block-structured scopes, following C++ (spec §3.6).
//!
//! Each statement list is a frame. A frame can be *joined* to its parent when
//! C++ treats both as one scope: a function body shares the scope of the
//! parameters, and a `for` body shares the scope of its counter, so declaring
//! the same name there is an error rather than shadowing.
//!
//! Lookups are by symbol ID (what blocks store) and by name (what C++ sees);
//! both are logarithmic so that large programs stay fast.
//!
//! The stack also keeps a [`Trail`] for the editor's scope query
//! (`crate::query`): every declaration appends a node that points back to the
//! node that was current before it, and [`Scopes::here`] names the current
//! node. Walking back from a node visits exactly the names that are visible
//! at that point, innermost first, so remembering one node per position is
//! enough to answer "what is visible here" later, in memory linear in the
//! number of declarations.

use std::collections::BTreeMap;

use b2c_ir::ids::{BlockId, SymbolId};

/// A node of the [`Trail`], by index.
pub(crate) type NodeId = usize;

/// One step of the trail: a name that becomes visible, or a name that is
/// hidden without becoming usable (a declaration whose starting value or loop
/// header is being lowered, see [`Scopes::hide`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TrailNode {
    /// The node that was current before this one (always a smaller index).
    pub(crate) parent: Option<NodeId>,
    /// The name C++ sees.
    pub(crate) name: String,
    /// The symbol it names, or `None` for a hidden name.
    pub(crate) sym: Option<SymbolId>,
}

/// Every declaration of the analysis in the order the lowering made them,
/// linked back to what was visible before each one (a persistent stack).
pub(crate) type Trail = Vec<TrailNode>;

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
    /// The trail node that was current when the frame was opened.
    entry: Option<NodeId>,
}

/// The stack of scopes at the current point of the analysis.
#[derive(Debug, Default)]
pub(crate) struct Scopes {
    frames: Vec<Frame>,
    by_sym: BTreeMap<SymbolId, usize>,
    by_name: BTreeMap<String, Vec<(usize, SymbolId)>>,
    /// Every declaration so far, kept across [`Scopes::reset`].
    trail: Trail,
    /// The current node of the trail (`None`: nothing is visible).
    head: Option<NodeId>,
}

/// What [`Scopes::hide`] replaced, to give back to [`Scopes::unhide`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct HideMark(Option<NodeId>);

impl Scopes {
    /// Empties the stack for the next item, keeping the trail.
    pub(crate) fn reset(&mut self) {
        self.frames.clear();
        self.by_sym.clear();
        self.by_name.clear();
        self.head = None;
    }

    /// The current trail node: what is visible here (see [`Trail`]).
    pub(crate) fn here(&self) -> Option<NodeId> {
        self.head
    }

    /// Takes the trail out, leaving an empty one.
    pub(crate) fn take_trail(&mut self) -> Trail {
        self.head = None;
        std::mem::take(&mut self.trail)
    }

    /// Records a name that C++ already binds to a declaration that is not
    /// usable yet (a variable inside its own starting value, a `for` counter
    /// inside the loop's header). It hides outer symbols with the same name
    /// from the trail until [`Self::unhide`]; name lookups by the lowering are
    /// unaffected (the lowering checks those names itself).
    pub(crate) fn hide(&mut self, name: &str) -> HideMark {
        let mark = HideMark(self.head);
        self.append(name, None);
        mark
    }

    /// Ends what [`Self::hide`] started.
    pub(crate) fn unhide(&mut self, mark: HideMark) {
        self.head = mark.0;
    }

    fn append(&mut self, name: &str, sym: Option<&SymbolId>) {
        let id = self.trail.len();
        self.trail.push(TrailNode {
            parent: self.head,
            name: name.to_owned(),
            sym: sym.cloned(),
        });
        self.head = Some(id);
    }

    /// Opens a scope for a statement list.
    pub(crate) fn push(&mut self, key: ListKey, joined: bool) {
        self.frames.push(Frame {
            key,
            joined,
            declared: Vec::new(),
            entry: self.head,
        });
    }

    /// Closes the innermost scope.
    pub(crate) fn pop(&mut self) {
        let Some(frame) = self.frames.pop() else {
            return;
        };
        self.head = frame.entry;
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
        self.append(name, Some(sym));
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

    /// The names visible from a trail node, innermost first (`-name` for a
    /// hidden one).
    fn walk(trail: &Trail, mut node: Option<NodeId>) -> Vec<String> {
        let mut names = Vec::new();
        while let Some(index) = node {
            let n = &trail[index];
            names.push(if n.sym.is_some() {
                n.name.clone()
            } else {
                format!("-{}", n.name)
            });
            assert!(n.parent.is_none_or(|p| p < index), "parents come first");
            node = n.parent;
        }
        names
    }

    #[test]
    fn the_trail_follows_the_stack() {
        let mut s = Scopes::default();
        assert_eq!(s.here(), None);
        s.declare("outside", &sym("o"));
        assert_eq!(s.here(), None, "nothing is declared outside a scope");

        s.push(key("m", "BODY"), false);
        s.declare("a", &sym("a"));
        let after_a = s.here();
        s.push(key("if", "DO0"), false);
        assert_eq!(s.here(), after_a, "opening a list declares nothing");
        s.declare("b", &sym("b"));
        let mark = s.hide("c");
        let hiding = s.here();
        s.unhide(mark);
        assert_eq!(walk(&s.trail, s.here()), ["b", "a"]);
        s.pop();
        assert_eq!(s.here(), after_a, "closing a list forgets its declarations");
        s.declare("d", &sym("d"));
        assert_eq!(walk(&s.trail, s.here()), ["d", "a"]);
        assert_eq!(walk(&s.trail, hiding), ["-c", "b", "a"]);

        s.reset();
        assert_eq!(s.here(), None);
        assert!(!s.contains(&sym("a")));
        s.push(key("f", PARAMS_LIST), false);
        s.declare("p", &sym("p"));
        assert_eq!(walk(&s.trail, s.here()), ["p"], "a new item starts empty");
        let trail = s.take_trail();
        assert_eq!(trail.len(), 5, "a, b, -c, d and p; nothing for `outside`");
        assert_eq!(s.here(), None);
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
