//! Binding references to symbols (spec §3.6, §6.5).
//!
//! Blocks store symbol IDs, but the generated C++ refers to names. A reference
//! is therefore checked twice: the ID must be visible here, and C++ name
//! lookup of its name must find the same symbol (not a newer one that hides
//! it). When a reference does not resolve, the declaration table from pass 1
//! explains why.

use b2c_ir::diag::Location;
use b2c_ir::ids::SymbolId;
use b2c_ir::sast::{PassMode, SymbolKind};
use b2c_ir::types::Type;

use super::{FuncSig, Lowerer, PendingKind};
use crate::codes;
use crate::collect::{DeclKind, DeclStatus};
use crate::messages::quoted;

impl Lowerer<'_> {
    /// The user's name of a symbol (raw; quote it for messages).
    pub(super) fn raw_name(&self, sym: &SymbolId) -> String {
        self.decls.get(sym).map(|d| d.name.clone()).unwrap_or_default()
    }

    /// The symbol's name for messages, in backticks.
    pub(super) fn name_of(&self, sym: &SymbolId) -> String {
        self.decls
            .get(sym)
            .map_or_else(|| String::from("an unknown name"), |d| quoted(&d.name))
    }

    /// The symbol that C++ name lookup finds for `name` here.
    fn binding(&self, name: &str) -> Option<&SymbolId> {
        self.pending
            .iter()
            .rev()
            .find(|p| p.name == name)
            .map(|p| &p.sym)
            .or_else(|| self.scopes.lookup(name))
    }

    /// Whether a function of the current module is declared with this ID.
    fn local_function(&self, sym: &SymbolId) -> Option<&FuncSig> {
        self.functions.get(sym).filter(|f| f.module == self.module_index)
    }

    /// Reads a variable, parameter or counter: its type, or `None` after
    /// reporting why it cannot be used here.
    pub(super) fn read_variable(&mut self, sym: &SymbolId, location: &Location) -> Option<Type> {
        if let Some(pending) = self.pending.iter().rev().find(|p| &p.sym == sym) {
            let name = quoted(&pending.name);
            return match &pending.kind {
                PendingKind::Initialiser(Some(ty)) => Some(ty.clone()),
                PendingKind::Initialiser(None) => {
                    let message = format!(
                        "{name} can't be used in its own starting value: its type ('auto') comes from that value."
                    );
                    self.diags
                        .error(codes::USED_BEFORE_DECLARATION, location.clone(), message);
                    None
                }
                PendingKind::Counter => {
                    let message = format!(
                        "{name} only exists inside its 'for' loop, so it can't be used for the loop's own start, \
                         end or step."
                    );
                    self.diags.error(codes::OUT_OF_SCOPE, location.clone(), message);
                    None
                }
            };
        }
        if self.scopes.contains(sym) {
            if !self.check_not_hidden(sym, location) {
                return None;
            }
            return Some(self.symbols.get(sym).map_or(Type::Error, |s| s.ty.clone()));
        }
        if self.local_function(sym).is_some() {
            let message = format!(
                "{} is a function, not a value. To run it and use its result, call it: use its call block or \
                 write {}() in an expression.",
                self.name_of(sym),
                crate::messages::shown(&self.raw_name(sym))
            );
            self.diags.error(codes::WRONG_KIND, location.clone(), message);
            return None;
        }
        self.report_unavailable(sym, location);
        None
    }

    /// Resolves the variable a statement changes (`set`, `change`, `ask`):
    /// its type, or `None` after reporting. A variable that may not be changed
    /// is reported but still gives its type, so the value is checked too.
    pub(super) fn write_target(&mut self, sym: &SymbolId, location: &Location) -> Option<Type> {
        if self.scopes.contains(sym) {
            if !self.check_not_hidden(sym, location) {
                return None;
            }
            if let Some(reason) = self.not_changeable(sym) {
                self.diags.error(codes::NOT_ASSIGNABLE, location.clone(), reason);
            }
            return Some(self.symbols.get(sym).map_or(Type::Error, |s| s.ty.clone()));
        }
        if self.local_function(sym).is_some() {
            let message = format!(
                "{} is a function; only variables can be changed.",
                self.name_of(sym)
            );
            self.diags.error(codes::WRONG_KIND, location.clone(), message);
            return None;
        }
        self.read_variable(sym, location)
    }

    /// Why a visible symbol may not be changed, as a full message.
    pub(super) fn not_changeable(&self, sym: &SymbolId) -> Option<String> {
        let name = self.name_of(sym);
        match self.symbols.get(sym).map(|s| &s.kind)? {
            SymbolKind::Variable { is_const: true } => Some(format!(
                "{name} was created as a constant, so it can't be changed. Untick 'const' in its 'create' block \
                 if it needs to change."
            )),
            SymbolKind::LoopVariable => Some(format!(
                "{name} is the counter of a 'for' loop: the loop changes it, so the blocks inside can't. \
                 Use another variable."
            )),
            SymbolKind::Parameter {
                mode: PassMode::ReadOnly,
            } => Some(format!(
                "{name} is a read-only input of this function, so it can't be changed. Change its mode to \
                 'copy' or 'editable'."
            )),
            _ => None,
        }
    }

    /// Resolves the function of a call, or reports why it cannot be called.
    pub(super) fn resolve_function(&mut self, sym: &SymbolId, location: &Location) -> Option<FuncSig> {
        if self.pending.iter().any(|p| &p.sym == sym) || self.scopes.contains(sym) {
            let message = format!(
                "{} is a variable, not a function, so it can't be called.",
                self.name_of(sym)
            );
            self.diags.error(codes::WRONG_KIND, location.clone(), message);
            return None;
        }
        if let Some(sig) = self.local_function(sym).cloned() {
            if let Some(hiding) = self.binding(&sig.name).cloned() {
                let message = format!(
                    "This calls the function {name}, but a variable called {name} hides it here, so C++ would \
                     use the variable. Rename the variable.",
                    name = quoted(&sig.name)
                );
                self.error_with_decl(
                    codes::NAME_HIDDEN,
                    location.clone(),
                    message,
                    &hiding,
                    "the variable",
                );
                return None;
            }
            return Some(sig);
        }
        self.report_unavailable(sym, location);
        None
    }

    /// Checks that C++ name lookup finds `sym` itself here; reports otherwise.
    fn check_not_hidden(&mut self, sym: &SymbolId, location: &Location) -> bool {
        let name = self.raw_name(sym);
        match self.binding(&name).cloned() {
            Some(found) if &found != sym => {
                let message = format!(
                    "This refers to an outer {name}, but a newer variable also called {name} hides it here, so \
                     C++ would use the newer one. Rename one of them.",
                    name = quoted(&name)
                );
                self.error_with_decl(
                    codes::NAME_HIDDEN,
                    location.clone(),
                    message,
                    &found,
                    "the newer variable",
                );
                false
            }
            _ => true,
        }
    }

    /// Explains why a symbol is not available at this point.
    fn report_unavailable(&mut self, sym: &SymbolId, location: &Location) {
        let Some(decl) = self.decls.get(sym).cloned() else {
            self.diags.error(
                codes::NOT_DECLARED,
                location.clone(),
                "This refers to a variable or function that doesn't exist (any more). Choose another one, or \
                 create it again.",
            );
            return;
        };
        let name = quoted(&decl.name);
        let (code, message) = match (decl.status, decl.kind) {
            (DeclStatus::Disabled, _) => (
                codes::UNAVAILABLE,
                format!(
                    "{name} is created in a disabled block. Enable that block, or choose something else."
                ),
            ),
            (DeclStatus::Detached, _) => (
                codes::UNAVAILABLE,
                format!(
                    "{name} is created in a block that isn't attached to 'when program starts' or a function, \
                     so it doesn't exist when the program runs."
                ),
            ),
            (DeclStatus::Active, DeclKind::Function) if decl.module != self.module_index => {
                let module = self
                    .document
                    .modules
                    .get(decl.module)
                    .map(|m| quoted(&m.name))
                    .unwrap_or_default();
                (
                    codes::OTHER_MODULE,
                    format!(
                        "{name} is a function of another module ({module}). Using functions from other modules \
                         isn't supported yet, so move it into this module."
                    ),
                )
            }
            (DeclStatus::Active, DeclKind::Variable) if self.scopes.encloses(&decl.path) => (
                codes::USED_BEFORE_DECLARATION,
                format!(
                    "{name} is used before it is created. Move this block below the block that creates {name}."
                ),
            ),
            (DeclStatus::Active, DeclKind::Variable | DeclKind::Function) => (
                codes::OUT_OF_SCOPE,
                format!(
                    "{name} can't be used here: it only exists from the block that creates it to the end of \
                     that group of blocks (including the blocks nested inside them)."
                ),
            ),
            (DeclStatus::Active, DeclKind::Parameter) => (
                codes::OUT_OF_SCOPE,
                format!(
                    "{name} is an input of the function {} and can only be used inside that function.",
                    quoted(decl.owner.as_deref().unwrap_or_default())
                ),
            ),
            (DeclStatus::Active, DeclKind::Counter) => (
                codes::OUT_OF_SCOPE,
                format!("{name} is the counter of a 'for' loop and only exists inside that loop."),
            ),
        };
        let diagnostic =
            b2c_ir::Diagnostic::error(code, b2c_ir::DiagSource::Analyser, location.clone(), message)
                .with_related(decl.location, format!("{name} is created here"));
        self.diags.push(diagnostic);
    }

    /// Checks a new local name (variable or counter) against the names
    /// visible here: a redeclaration in the same scope is an error, hiding an
    /// outer variable or a function is a warning.
    pub(super) fn check_new_name(&mut self, name: &str, location: &Location) {
        if let Some(other) = self.scopes.in_same_scope(name).cloned() {
            let message = format!(
                "There is already a variable called {} here. Choose another name.",
                quoted(name)
            );
            self.error_with_decl(
                codes::DUPLICATE_NAME,
                location.clone(),
                message,
                &other,
                "the other one",
            );
        } else if let Some(other) = self.scopes.lookup(name).cloned() {
            let message = format!(
                "{name} hides another variable called {name} from an enclosing block. This works, but it is easy \
                 to mix them up, so consider another name.",
                name = quoted(name)
            );
            self.warning_with_decl(
                codes::SHADOWING,
                location.clone(),
                message,
                &other,
                "the hidden variable",
            );
        }
        self.warn_if_function_name(name, location);
    }
}
