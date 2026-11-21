//! The lookup model, M3 subset (06 §6.14.2, §6.14.6; w3-modules.md §2.12,
//! package N2): how each reference to a namespace-scope entity is written,
//! the `using namespace` directives of each module's `.cpp`, and the
//! problems they cause.
//!
//! * **Directives.** A *use namespace … in this file* block nominates `std`
//!   or a top-level user namespace for its module's `.cpp`; *Textbook
//!   style* nominates `std` in every `.cpp` (code generation writes it only
//!   where a standard header is included, 06 §6.14.3 rule 2; the analyser
//!   counts it everywhere, which only ever adds a qualification). A block
//!   that another source already covers is `B2C-I0523`; a namespace that is
//!   gone `B2C-E0201`, a nested one or something else `B2C-E0207`, one the
//!   file cannot see `B2C-E0203`. Naming a namespace whose members only
//!   another module's header declares adds that header.
//! * **Lookup.** At a reference, C++ looks in the enclosing user namespaces
//!   from the innermost out, then in the global namespace together with
//!   the module's anonymous namespace and the nominated namespaces, and
//!   stops at the first scope that declares the name. A reference is
//!   written plainly only when that finds exactly the intended entity (for
//!   a function, exactly its overload set); otherwise `::name` for an
//!   entity of the global or the anonymous namespace and `geo::name` for a
//!   member of a user namespace (`::geo::name` when a namespace or type of
//!   an enclosing user namespace is called `geo` too, 06 §6.14.2: `geo` is
//!   contested). A user function is qualified also when a
//!   nominated namespace declares its name, and for argument-dependent
//!   lookup when the namespace of an argument's struct or enum (also inside
//!   a collection) other than its own declares a function of its name.
//!   `std` as an associated namespace qualifies nothing in M3 (w3-modules.md
//!   §7.1): a user function's parameters match its arguments exactly, so it
//!   wins over `std`'s templates, and the functions of `std` that are no
//!   templates and take text are `B2C-E0220`. What a shared item's header
//!   prints is looked up as a header sees it: with no directive, and with
//!   every module's shared definitions, since a `.cpp` may include any other
//!   header before it; what its `.cpp` prints too is written plainly only
//!   when it is plain in both. Blocks and functions are not scopes
//!   here: a local that hides a definition is still `B2C-E0205`. M3 has no
//!   classes, library packs or Raw C++, so their rules wait for M4.
//! * **Standard names** are written short in a `.cpp` that nominates `std`
//!   only when no declaration of the program is spelled like them (06
//!   §6.14.5): `Spellings::short_standard`.
//! * **`B2C-E0215`**: a struct or enum referred to in a module whose
//!   nominated namespaces declare a type or namespace of its name, and a
//!   top-level user namespace named like a namespace or type that a
//!   namespace nominated where it is named declares; only the names of the
//!   project's standard count.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use b2c_ir::diag::{DiagSource, Diagnostic, Location};
use b2c_ir::ids::{BlockId, ModuleId, SymbolId};
use b2c_ir::layout::Layout;
use b2c_ir::sast::{CppStandard, Expr, ExprKind, Item, ItemKind, Module, SymbolKind};
use b2c_ir::spelling::{DirectiveSource, Directives, Spelling, Spellings};
use b2c_ir::std_names::{STD_NAMES, StdNameKind, std_name};
use b2c_ir::types::Type;
use b2c_model::{Block, FieldValue};

use super::{Lowerer, field};
use crate::codes;
use crate::layout::{body_stmts, stmt_symbols, user_types};
use crate::messages::quoted;
use crate::order::item_symbol;

/// What a *use namespace … in this file* block names.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Nominee {
    /// `std`.
    Std,
    /// A top-level user namespace, by the symbol that stands for it.
    User(SymbolId),
}

/// A checked *use namespace … in this file* block.
#[derive(Debug, Clone)]
pub(super) struct UseBlock {
    block: BlockId,
    nominee: Nominee,
}

/// What a definition is, for lookup.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DefKind {
    Function,
    Type,
    Global,
}

/// A namespace-scope definition of the program.
#[derive(Debug, Clone, Copy)]
struct Def<'p> {
    sym: &'p SymbolId,
    name: &'p str,
    /// Its innermost namespace (the symbol that stands for it).
    ns: Option<&'p SymbolId>,
    module: usize,
    shared: bool,
    kind: DefKind,
}

impl Def<'_> {
    /// Whether `self` is `target` or, for a function, one of its overloads.
    fn same_entity(&self, target: &Def<'_>) -> bool {
        self.sym == target.sym
            || (self.kind == DefKind::Function
                && target.kind == DefKind::Function
                && self.ns == target.ns
                && self.name == target.name)
    }
}

/// A reference found in an item: the block that holds it, what it names,
/// for a call the arguments' types, and where it is printed.
struct Reference<'p> {
    block: &'p BlockId,
    target: &'p SymbolId,
    args: Vec<&'p Type>,
    printed: Printed,
}

/// Where a part of an item is printed, which decides the lookups that
/// spell its references.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Printed {
    /// In its module's `.cpp`, after the directives.
    Source,
    /// In its module's header only (a shared function's default values, a
    /// shared struct's fields, a shared `constexpr` global's value).
    Header,
    /// In both (a shared function's result and parameter types, a shared
    /// global's type): the spelling must be right in each.
    Both,
}

/// Where lookup at a reference happens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Reach {
    /// In module `.0`'s `.cpp`, which sees its own definitions and those
    /// other modules share in a header it includes.
    Source(usize),
    /// In a header, where no directive is. A `.cpp` may include it after
    /// any other module's header, so it may see every shared definition.
    Header,
}

/// The namespaces a module's `.cpp` nominates.
#[derive(Debug, Default)]
struct Nominated<'p> {
    std: bool,
    user: Vec<&'p SymbolId>,
}

/// The program as lookup sees it.
struct Model<'p> {
    defs: BTreeMap<&'p SymbolId, Def<'p>>,
    by_name: BTreeMap<(Option<&'p SymbolId>, &'p str), Vec<&'p SymbolId>>,
    /// The namespaces by their enclosing namespace and name (the symbols
    /// that stand for them).
    children: BTreeMap<(Option<&'p SymbolId>, &'p str), &'p SymbolId>,
    /// By namespace (the symbol that stands for it): the name of its
    /// outermost namespace, which a qualified name starts with.
    outermost: BTreeMap<&'p SymbolId, &'p str>,
    /// By module: the modules whose headers its `.cpp` includes, directly or
    /// through other headers (its own among them when it has one).
    reach: Vec<BTreeSet<usize>>,
    /// The project's standard, as a year (17, 20, 23, 26).
    year: u8,
}

impl<'p> Model<'p> {
    /// The definitions called `name` in namespace `ns` that lookup at
    /// `reach` may see: in module `m`'s `.cpp`, its own and those other
    /// modules share in a header it includes; in a header, every shared one.
    fn visible(&self, reach: Reach, ns: Option<&'p SymbolId>, name: &str) -> Vec<Def<'p>> {
        self.by_name
            .get(&(ns, name))
            .into_iter()
            .flatten()
            .filter_map(|sym| self.defs.get(sym))
            .filter(|def| match reach {
                Reach::Source(m) => def.module == m || (def.shared && self.reach[m].contains(&def.module)),
                Reach::Header => def.shared,
            })
            .copied()
            .collect()
    }

    /// Whether `std` declares `name` in the project's standard.
    fn std_has(&self, name: &str) -> Option<StdNameKind> {
        std_name(name)
            .filter(|entry| entry.since <= self.year)
            .map(|entry| entry.kind)
    }

    /// Whether plain lookup of `target`'s name at a place in namespace
    /// `ctx`, seeing `reach`, with the namespaces `nominated`, finds exactly
    /// it (for a function, exactly its overload set). When it finds nothing
    /// (a header the file lacks, reported elsewhere), a definition of the
    /// global namespace keeps its plain spelling.
    fn plain_finds(
        &self,
        reach: Reach,
        ctx: &[&'p SymbolId],
        nominated: &Nominated<'p>,
        target: &Def<'p>,
    ) -> bool {
        let name = target.name;
        let mut scopes: Vec<Option<&'p SymbolId>> = ctx.iter().rev().map(|ns| Some(*ns)).collect();
        scopes.push(None);
        for scope in scopes {
            let mut found = self.visible(reach, scope, name);
            let mut other = self.children.contains_key(&(scope, name));
            if scope.is_none() {
                other |= nominated.std && self.std_has(name).is_some();
                for user in &nominated.user {
                    found.extend(self.visible(reach, Some(*user), name));
                    other |= self.children.contains_key(&(Some(*user), name));
                }
            }
            if found.is_empty() && !other {
                continue;
            }
            return !other && found.iter().all(|def| def.same_entity(target));
        }
        target.ns.is_none()
    }

    /// Whether a nominated namespace declares `target`'s name as something
    /// other than `target` (06 §6.14.2: a user function whose name is a
    /// nominated name is always qualified).
    fn nominated_name(&self, reach: Reach, nominated: &Nominated<'p>, target: &Def<'p>) -> bool {
        (nominated.std && self.std_has(target.name).is_some())
            || nominated.user.iter().any(|user| {
                self.children.contains_key(&(Some(*user), target.name))
                    || self
                        .visible(reach, Some(*user), target.name)
                        .iter()
                        .any(|def| !def.same_entity(target))
            })
    }

    /// Whether the outermost namespace of `target`, which a qualified
    /// reference to it starts with, is contested at a place in the
    /// namespaces `ctx` (06 §6.14.2): one of those declares a namespace or
    /// type of its name, which lookup of the name before `::` finds first.
    fn contested(&self, ctx: &[&'p SymbolId], target: &Def<'p>) -> bool {
        let Some(first) = target.ns.and_then(|ns| self.outermost.get(ns)) else {
            return false;
        };
        ctx.iter().any(|scope| {
            self.children.contains_key(&(Some(*scope), *first))
                || self
                    .by_name
                    .get(&(Some(*scope), *first))
                    .into_iter()
                    .flatten()
                    .filter_map(|sym| self.defs.get(sym))
                    .any(|def| def.kind == DefKind::Type)
        })
    }

    /// The user namespaces associated with a value of type `ty` (06
    /// §6.14.6): those of its structs and enums, also inside collections
    /// (`None` for the global namespace).
    fn associated(&self, ty: &'p Type, out: &mut BTreeSet<Option<&'p SymbolId>>) {
        for sym in user_types(ty) {
            if let Some(def) = self.defs.get(sym) {
                out.insert(def.ns);
            }
        }
    }

    /// Whether argument-dependent lookup of a call to `target` with
    /// arguments of `args` could find another function: a user namespace
    /// associated with an argument, other than the function's own,
    /// declares one of its name.
    fn adl_finds_another(&self, target: &Def<'p>, args: &[&'p Type]) -> bool {
        let mut associated = BTreeSet::new();
        for ty in args {
            self.associated(ty, &mut associated);
        }
        associated.into_iter().any(|ns| {
            ns != target.ns
                && self
                    .by_name
                    .get(&(ns, target.name))
                    .into_iter()
                    .flatten()
                    .filter_map(|sym| self.defs.get(sym))
                    .any(|def| def.kind == DefKind::Function)
        })
    }
}

/// The year of a standard, as the standard-name table counts it.
fn year(standard: CppStandard) -> u8 {
    match standard {
        CppStandard::Cpp17 => 17,
        CppStandard::Cpp20 => 20,
        CppStandard::Cpp23 => 23,
        CppStandard::Cpp26 => 26,
    }
}

/// The references an expression tree makes: calls, globals read, each
/// enum value's enum, each struct value's struct, and the user types of its
/// values.
fn expr_references<'p>(root: &'p Expr, printed: Printed, out: &mut Vec<Reference<'p>>) {
    let mut stack = vec![root];
    while let Some(expr) = stack.pop() {
        let block = &expr.origin.block;
        match &expr.kind {
            ExprKind::Call { function, args } => out.push(Reference {
                block,
                target: function,
                args: args.iter().map(|arg| &arg.ty).collect(),
                printed,
            }),
            ExprKind::Var(sym) | ExprKind::Enumerator(sym) | ExprKind::New { ty: sym, .. } => {
                out.push(Reference {
                    block,
                    target: sym,
                    args: Vec::new(),
                    printed,
                });
            }
            _ => {}
        }
        for id in user_types(&expr.ty) {
            out.push(Reference {
                block,
                target: id,
                args: Vec::new(),
                printed,
            });
        }
        stack.extend(crate::flow::children(expr));
    }
}

/// The references an item makes, each with where it is printed, and the
/// blocks it holds. A shared item's header prints its declaration part (a
/// function's result type and default values, a struct's fields, a
/// global's type, and a `constexpr` global's starting value); its `.cpp`
/// prints the rest, and a function's result type and a global's type again.
fn item_references<'p>(item: &'p Item, blocks: &mut BTreeSet<&'p BlockId>) -> Vec<Reference<'p>> {
    let (head, value) = if item.shared {
        (Printed::Both, Printed::Header)
    } else {
        (Printed::Source, Printed::Source)
    };
    let mut out = Vec::new();
    let mut exprs: Vec<(&'p Expr, Printed)> = Vec::new();
    let mut bodies = Vec::new();
    match &item.kind {
        ItemKind::Main(main) => bodies.push(&main.body),
        ItemKind::Function(def) => {
            for id in user_types(&def.ret) {
                out.push(Reference {
                    block: &item.origin.block,
                    target: id,
                    args: Vec::new(),
                    printed: head,
                });
            }
            let defaults = def.params.iter().filter_map(|param| param.default.as_ref());
            exprs.extend(defaults.map(|expr| (expr, value)));
            bodies.push(&def.body);
        }
        ItemKind::Struct(def) => exprs.extend(def.defaults.values().map(|expr| (expr, value))),
        // A shared global that is not `constexpr` gets its starting value
        // in its `.cpp`, after the directives.
        ItemKind::Global(def) => {
            let printed = if def.constexpr { value } else { Printed::Source };
            exprs.extend(def.init.iter().map(|expr| (expr, printed)));
        }
        ItemKind::Enum(_) | ItemKind::UseHeader { .. } => {}
    }
    blocks.insert(&item.origin.block);
    for (expr, printed) in exprs {
        expr_references(expr, printed, &mut out);
    }
    for body in bodies {
        let mut body_exprs = Vec::new();
        for stmt in body_stmts(body) {
            blocks.insert(&stmt.origin.block);
            for target in stmt_symbols(stmt) {
                out.push(Reference {
                    block: &stmt.origin.block,
                    target,
                    args: Vec::new(),
                    printed: Printed::Source,
                });
            }
            body_exprs.extend(crate::nesting::parts(stmt).0);
        }
        for expr in body_exprs {
            expr_references(expr, Printed::Source, &mut out);
        }
    }
    out
}

impl Lowerer<'_> {
    /// Checks a *use namespace … in this file* block and records it for its
    /// module (w3-modules.md §1.4, 06 §6.14.3 rule 6).
    pub(super) fn use_namespace(&mut self, block: &Block) {
        let location = self.loc(&block.id, field("NS"));
        let value = match block.fields.get("NS") {
            Some(FieldValue::Text(value)) if !value.is_empty() => value.as_str(),
            _ => {
                self.incomplete(location, "This block needs a namespace to use.");
                return;
            }
        };
        let nominee = if value == "::std" {
            Nominee::Std
        } else {
            match self.user_nominee(value, &location) {
                Some(standing) => Nominee::User(standing),
                None => return,
            }
        };
        self.use_blocks
            .entry(self.module_index)
            .or_default()
            .push(UseBlock {
                block: block.id.clone(),
                nominee,
            });
    }

    /// The top-level user namespace a *use namespace* block's value names,
    /// by the symbol that stands for it, or `None` after reporting why it
    /// cannot be used here.
    fn user_nominee(&mut self, value: &str, location: &Location) -> Option<SymbolId> {
        let gone =
            "This block uses a namespace that no longer exists. Choose another one, or remove the block.";
        let Ok(sym) = SymbolId::new(value) else {
            self.diags.error(codes::NOT_DECLARED, location.clone(), gone);
            return None;
        };
        let Some(symbol) = self.symbols.get(&sym) else {
            self.diags.error(codes::NOT_DECLARED, location.clone(), gone);
            return None;
        };
        match &symbol.kind {
            SymbolKind::Namespace { parent: None } => {}
            SymbolKind::Namespace { parent: Some(_) } => {
                let path = self.symbols.namespace_path(Some(&sym));
                let outer = path
                    .first()
                    .and_then(|id| self.symbols.get(id))
                    .map(|symbol| quoted(symbol.name.as_str()))
                    .unwrap_or_default();
                let message = format!(
                    "{} is inside the namespace {outer}, and only a namespace that is not inside another one can \
                     be used in a file. Use {outer} instead.",
                    quoted(symbol.name.as_str())
                );
                self.error_with_decl(
                    codes::WRONG_KIND,
                    location.clone(),
                    message,
                    &sym,
                    "the namespace",
                );
                return None;
            }
            _ => {
                let message = format!(
                    "{} is not a namespace. Choose a namespace to use.",
                    quoted(symbol.name.as_str())
                );
                self.error_with_decl(
                    codes::WRONG_KIND,
                    location.clone(),
                    message,
                    &sym,
                    "what it names",
                );
                return None;
            }
        }
        let standing = self.namespaces.standing.get(&sym).cloned().unwrap_or(sym);
        let opened_here = self
            .namespaces
            .blocks
            .get(&standing)
            .is_some_and(|blocks| blocks.iter().any(|(module, _)| *module == self.module_index));
        let shared_somewhere = self.shared_blocks.iter().any(|block| {
            self.namespaces
                .member_of
                .get(block)
                .is_some_and(|ns| self.symbols.namespace_path(Some(ns)).contains(&&standing))
        });
        if !opened_here && !shared_somewhere {
            let name = self
                .symbols
                .get(&standing)
                .map(|symbol| quoted(symbol.name.as_str()))
                .unwrap_or_default();
            let message = format!(
                "{name} can't be used in this module: its definitions are in other modules, which share none of \
                 them. Share one of them, or use {name} in a module that has it."
            );
            self.error_with_decl(
                codes::OUT_OF_SCOPE,
                location.clone(),
                message,
                &standing,
                "the namespace",
            );
            return None;
        }
        Some(standing)
    }

    /// Fills the program's spellings and directives once the layout is
    /// known, reports `B2C-I0523` and `B2C-E0215`, and adds to `layout` the
    /// headers that directives need.
    pub(super) fn spell(&mut self, modules: &[Module], layout: &mut Layout) -> Spellings {
        let uses = std::mem::take(&mut self.use_blocks);
        let (spellings, reports) = self.spell_with(modules, layout, &uses);
        for diagnostic in reports {
            self.diags.push(diagnostic);
        }
        spellings
    }

    /// [`Self::spell`], with the problems it finds returned.
    fn spell_with<'p>(
        &'p self,
        modules: &'p [Module],
        layout: &mut Layout,
        uses: &BTreeMap<usize, Vec<UseBlock>>,
    ) -> (Spellings, Vec<Diagnostic>) {
        let mut spellings = Spellings::default();
        let mut reports = Vec::new();
        let mut model = self.model(modules, layout);
        let textbook = self.document.project.options.using_namespace_std;
        if self.namespaces.blocks.is_empty() && uses.is_empty() && !textbook {
            // The global namespace is the only scope: only argument-dependent
            // lookup may qualify a call.
            self.spell_references(&model, modules, &[], &mut spellings);
            return (spellings, reports);
        }
        let mut directives: Vec<Directives> = Vec::with_capacity(modules.len());
        for m in 0..modules.len() {
            let blocks = uses.get(&m).cloned().unwrap_or_default();
            directives.push(self.directives(&model, layout, m, blocks, textbook, &mut reports));
        }
        // A directive may have added a header.
        model.reach = reach(layout);
        let mut nominated = Vec::with_capacity(modules.len());
        for (module, found) in modules.iter().zip(directives) {
            let namespaces: Vec<&SymbolId> = found
                .user
                .iter()
                .filter_map(|(sym, _)| self.symbols.symbols.get_key_value(sym).map(|(key, _)| key))
                .collect();
            nominated.push(Nominated {
                std: found.std.is_some(),
                user: namespaces,
            });
            if !found.is_empty() {
                spellings.directives.insert(module.id.clone(), found);
            }
        }
        let referred = self.spell_references(&model, modules, &nominated, &mut spellings);
        self.nominated_type_names(&model, &nominated, &referred, &mut reports);
        for (module, nominated) in modules.iter().zip(&nominated) {
            if nominated.std {
                spellings
                    .short_standard
                    .insert(module.id.clone(), self.short_standard(model.year));
            }
        }
        (spellings, reports)
    }

    /// The program's definitions and what each `.cpp` includes.
    fn model<'p>(&'p self, modules: &'p [Module], layout: &Layout) -> Model<'p> {
        let mut defs = BTreeMap::new();
        let mut by_name: BTreeMap<(Option<&SymbolId>, &str), Vec<&SymbolId>> = BTreeMap::new();
        for (m, module) in modules.iter().enumerate() {
            for item in &module.items {
                let Some(sym) = item_symbol(item) else {
                    continue;
                };
                let Some(symbol) = self.symbols.get(sym) else {
                    continue;
                };
                let kind = match item.kind {
                    ItemKind::Function(_) => DefKind::Function,
                    ItemKind::Struct(_) | ItemKind::Enum(_) => DefKind::Type,
                    _ => DefKind::Global,
                };
                let def = Def {
                    sym,
                    name: symbol.name.as_str(),
                    ns: item.namespace.as_ref(),
                    module: m,
                    shared: item.shared,
                    kind,
                };
                defs.insert(sym, def);
                by_name.entry((def.ns, def.name)).or_default().push(sym);
            }
        }
        let mut children = BTreeMap::new();
        let mut outermost = BTreeMap::new();
        for standing in self.namespaces.blocks.keys() {
            if let Some(symbol) = self.symbols.get(standing)
                && let SymbolKind::Namespace { parent } = &symbol.kind
            {
                children.insert((parent.as_ref(), symbol.name.as_str()), standing);
            }
            if let Some(first) = self
                .symbols
                .namespace_path(Some(standing))
                .first()
                .and_then(|id| self.symbols.get(id))
            {
                outermost.insert(standing, first.name.as_str());
            }
        }
        Model {
            defs,
            by_name,
            children,
            outermost,
            reach: reach(layout),
            year: year(self.document.project.language.standard),
        }
    }

    /// The directives of module `m`'s `.cpp`: `B2C-I0523` for each block
    /// another source covers, and the header a user namespace needs.
    fn directives(
        &self,
        model: &Model<'_>,
        layout: &mut Layout,
        m: usize,
        mut blocks: Vec<UseBlock>,
        textbook: bool,
        reports: &mut Vec<Diagnostic>,
    ) -> Directives {
        let mut found = Directives {
            std: textbook.then_some(DirectiveSource::Setting),
            ..Directives::default()
        };
        blocks.sort_by(|a, b| a.block.cmp(&b.block));
        let mut user: BTreeMap<SymbolId, BlockId> = BTreeMap::new();
        for UseBlock { block, nominee } in blocks {
            let source = match &nominee {
                Nominee::Std => {
                    let source = found.std.clone();
                    if source.is_none() {
                        found.std = Some(DirectiveSource::Block(block.clone()));
                    }
                    source
                }
                Nominee::User(standing) => {
                    let source = user.get(standing).cloned().map(DirectiveSource::Block);
                    if source.is_none() {
                        user.insert(standing.clone(), block.clone());
                    }
                    source
                }
            };
            if let Some(source) = source {
                reports.push(self.in_use(m, &block, &nominee, &source));
            }
        }
        let mut named: Vec<(String, SymbolId, BlockId)> = user
            .into_iter()
            .map(|(standing, block)| {
                let name = self
                    .symbols
                    .get(&standing)
                    .map(|symbol| symbol.name.as_str().to_owned())
                    .unwrap_or_default();
                (name, standing, block)
            })
            .collect();
        named.sort();
        for (_, standing, _) in &named {
            // The modules whose headers declare the namespace: those that
            // share one of its members, at any depth.
            let declaring: BTreeSet<usize> = model
                .defs
                .values()
                .filter(|def| def.shared && self.symbols.namespace_path(def.ns).contains(&standing))
                .map(|def| def.module)
                .collect();
            let reached = &model.reach[m];
            let opened_here = self
                .namespaces
                .blocks
                .get(standing)
                .is_some_and(|blocks| blocks.iter().any(|(module, _)| *module == m));
            let mut declared = declaring.iter().any(|n| reached.contains(n));
            if !declared
                && !opened_here
                && let Some(first) = declaring.first()
            {
                add_include(layout, m, *first);
                declared = true;
            }
            if !declared {
                found.declare_empty.push(standing.clone());
            }
        }
        found.user = named
            .into_iter()
            .map(|(_, standing, block)| (standing, block))
            .collect();
        found
    }

    /// `B2C-I0523` on a block whose namespace `source` already nominates.
    fn in_use(&self, m: usize, block: &BlockId, nominee: &Nominee, source: &DirectiveSource) -> Diagnostic {
        let name = match nominee {
            Nominee::Std => String::from("`std`"),
            Nominee::User(standing) => self
                .symbols
                .get(standing)
                .map(|symbol| quoted(symbol.name.as_str()))
                .unwrap_or_default(),
        };
        let module = self.document.modules[m].id.clone();
        let location = Location {
            module: Some(module.clone()),
            block: Some(block.clone()),
            part: field("NS"),
        };
        let mut diagnostic = match source {
            DirectiveSource::Setting => Diagnostic::info(
                codes::NAMESPACE_IN_USE,
                DiagSource::Analyser,
                location,
                format!(
                    "{name} is already in use in every file: Textbook style is on in the project settings. This \
                     block changes nothing."
                ),
            ),
            DirectiveSource::Block(_) => Diagnostic::info(
                codes::NAMESPACE_IN_USE,
                DiagSource::Analyser,
                location,
                format!(
                    "{name} is already in use in this file through another 'use namespace' block. This block \
                     changes nothing."
                ),
            ),
        };
        if let DirectiveSource::Block(first) = source {
            diagnostic = diagnostic.with_related(
                Location {
                    module: Some(module),
                    block: Some(first.clone()),
                    part: field("NS"),
                },
                "the other block",
            );
        }
        diagnostic
    }

    /// Records the spelling of every reference that is not written plainly,
    /// and returns, by module, the definitions each module's code refers to
    /// (its own definitions among them).
    fn spell_references<'p>(
        &'p self,
        model: &Model<'p>,
        modules: &'p [Module],
        nominated: &[Nominated<'p>],
        spellings: &mut Spellings,
    ) -> Vec<BTreeSet<&'p SymbolId>> {
        let none = Nominated::default();
        let mut referred = vec![BTreeSet::new(); modules.len()];
        let mut place: BTreeMap<&BlockId, (usize, Option<&SymbolId>)> = BTreeMap::new();
        // The blocks of the shared items, whose parameters', fields' and
        // globals' types their headers print.
        let mut header_blocks: BTreeSet<&BlockId> = BTreeSet::new();
        let mut record = |block: &BlockId, target: &Def<'p>, spelling: Spelling| {
            if spelling != Spelling::Plain {
                spellings
                    .references
                    .insert((block.clone(), target.sym.clone()), spelling);
            }
        };
        for (m, module) in modules.iter().enumerate() {
            let nominated = nominated.get(m).unwrap_or(&none);
            for item in &module.items {
                if let Some(sym) = item_symbol(item) {
                    referred[m].insert(sym);
                }
                let mut blocks = BTreeSet::new();
                let references = item_references(item, &mut blocks);
                for block in blocks {
                    place.insert(block, (m, item.namespace.as_ref()));
                }
                if item.shared {
                    header_blocks.insert(&item.origin.block);
                }
                let ctx = self.symbols.namespace_path(item.namespace.as_ref());
                for reference in references {
                    let Some(target) = model.defs.get(reference.target) else {
                        continue;
                    };
                    referred[m].insert(target.sym);
                    let at = (m, ctx.as_slice(), nominated);
                    let spelling = spelling_where(model, reference.printed, at, target, &reference.args);
                    record(reference.block, target, spelling);
                }
            }
        }
        // The types of locals, parameters, fields and globals, written where
        // they are declared.
        for symbol in self.symbols.symbols.values() {
            let local = matches!(
                symbol.kind,
                SymbolKind::Variable { .. }
                    | SymbolKind::Parameter { .. }
                    | SymbolKind::LoopVariable
                    | SymbolKind::Field { .. }
                    | SymbolKind::Global { .. }
            );
            let Some(&(m, ns)) = place.get(&symbol.origin.block).filter(|_| local) else {
                continue;
            };
            let ctx = self.symbols.namespace_path(ns);
            // A shared struct's fields are printed in its header only; a
            // shared function's parameters and a shared global in both.
            let printed = match symbol.kind {
                _ if !header_blocks.contains(&symbol.origin.block) => Printed::Source,
                SymbolKind::Field { .. } => Printed::Header,
                _ => Printed::Both,
            };
            let at = (m, ctx.as_slice(), nominated.get(m).unwrap_or(&none));
            for id in user_types(&symbol.ty) {
                let Some(target) = model.defs.get(id) else {
                    continue;
                };
                referred[m].insert(target.sym);
                let spelling = spelling_where(model, printed, at, target, &[]);
                record(&symbol.origin.block, target, spelling);
            }
        }
        referred
    }

    /// `B2C-E0215` (06 §6.14.7): each struct or enum referred to in a
    /// module whose nominated namespaces declare a type or namespace of its
    /// name, and each top-level user namespace named in such a module (by a
    /// qualified reference or a directive) whose name they declare so.
    fn nominated_type_names<'p>(
        &'p self,
        model: &Model<'p>,
        nominated: &[Nominated<'p>],
        referred: &[BTreeSet<&'p SymbolId>],
        reports: &mut Vec<Diagnostic>,
    ) {
        let mut types: BTreeMap<&SymbolId, String> = BTreeMap::new();
        let mut namespaces: BTreeMap<&SymbolId, String> = BTreeMap::new();
        for (m, nominated) in nominated.iter().enumerate() {
            if !nominated.std && nominated.user.is_empty() {
                continue;
            }
            let clash = |name: &str, own: Option<&SymbolId>| -> Option<String> {
                if nominated.std && model.std_has(name).is_some_and(StdNameKind::is_type_or_namespace) {
                    return Some(String::from("the standard library (`std`)"));
                }
                nominated.user.iter().find_map(|user| {
                    let declares = model.children.contains_key(&(Some(*user), name))
                        || model
                            .visible(Reach::Source(m), Some(*user), name)
                            .iter()
                            .any(|def| def.kind == DefKind::Type);
                    (declares && own != Some(*user)).then(|| {
                        let name = self
                            .symbols
                            .get(user)
                            .map(|s| s.name.as_str())
                            .unwrap_or_default();
                        format!("the namespace {}", quoted(name))
                    })
                })
            };
            for sym in &referred[m] {
                let Some(def) = model.defs.get(sym).filter(|def| def.kind == DefKind::Type) else {
                    continue;
                };
                if let Some(by) = clash(def.name, def.ns) {
                    types.entry(def.sym).or_insert(by);
                }
            }
            // The top-level namespaces this module names: those it opens,
            // nominates, or holds a definition of that it refers to.
            let mut named: BTreeSet<&SymbolId> = nominated.user.iter().copied().collect();
            for (standing, blocks) in &self.namespaces.blocks {
                if blocks.iter().any(|(module, _)| *module == m) {
                    named.insert(standing);
                }
            }
            for sym in &referred[m] {
                if let Some(first) = model
                    .defs
                    .get(sym)
                    .and_then(|def| self.symbols.namespace_path(def.ns).first().copied())
                {
                    named.insert(first);
                }
            }
            for standing in named {
                let Some(symbol) = self.symbols.get(standing) else {
                    continue;
                };
                if !matches!(symbol.kind, SymbolKind::Namespace { parent: None }) {
                    continue;
                }
                if let Some(by) = clash(symbol.name.as_str(), Some(standing)) {
                    namespaces.entry(standing).or_insert(by);
                }
            }
        }
        self.report_nominated(model, types, namespaces, reports);
    }

    /// `B2C-E0215` on each type and namespace `nominated_type_names` found,
    /// with the namespace that declares its name.
    fn report_nominated(
        &self,
        model: &Model<'_>,
        types: BTreeMap<&SymbolId, String>,
        namespaces: BTreeMap<&SymbolId, String>,
        reports: &mut Vec<Diagnostic>,
    ) {
        for (sym, by) in types {
            let Some(def) = model.defs.get(sym) else {
                continue;
            };
            let Some(decl) = self.decls.get(sym) else {
                continue;
            };
            let what = match self.symbols.get(sym).map(|symbol| &symbol.kind) {
                Some(SymbolKind::Enum { .. }) => "enum",
                _ => "struct",
            };
            let message = format!(
                "{name} is also the name of a type in {by}, which 'use namespace' or Textbook style makes \
                 visible where this {what} is used, so C++ would not know which one {name} means. Rename the \
                 {what}, for example to {capital}.",
                name = quoted(def.name),
                capital = quoted(&capitalised(def.name)),
            );
            reports.push(Diagnostic::error(
                codes::NOMINATED_NAME,
                DiagSource::Analyser,
                decl.location.clone(),
                message,
            ));
        }
        for (standing, by) in namespaces {
            let Some(symbol) = self.symbols.get(standing) else {
                continue;
            };
            let name = symbol.name.as_str();
            let message = format!(
                "{} is also the name of a namespace or type in {by}, which 'use namespace' or Textbook style \
                 makes visible where this namespace is named, so C++ would not know which one it means. Rename \
                 the namespace, for example to {}.",
                quoted(name),
                quoted(&capitalised(name))
            );
            for (module, block) in self.namespaces.blocks.get(standing).into_iter().flatten() {
                let location = Location {
                    module: self.document.modules.get(*module).map(|m| m.id.clone()),
                    block: Some(block.clone()),
                    part: field("NAME"),
                };
                reports.push(Diagnostic::error(
                    codes::NOMINATED_NAME,
                    DiagSource::Analyser,
                    location,
                    message.clone(),
                ));
            }
        }
    }

    /// The standard names a `.cpp` that nominates `std` may write short (06
    /// §6.14.5): those of the project's standard that no declaration of
    /// the program is spelled like, except `move` and `forward`.
    fn short_standard(&self, year: u8) -> BTreeSet<String> {
        let taken: BTreeSet<&str> = self
            .symbols
            .symbols
            .values()
            .map(|symbol| symbol.name.as_str())
            .collect();
        STD_NAMES
            .iter()
            .filter(|entry| entry.since <= year)
            .filter(|entry| !matches!(entry.name, "move" | "forward"))
            .filter(|entry| !taken.contains(entry.name))
            .map(|entry| entry.name.to_owned())
            .collect()
    }
}

/// How a reference to `target` printed where `printed` says, from a place
/// in the namespaces `ctx` of module `m` whose `.cpp` nominates
/// `nominated`, is written: plainly only when it is plain wherever it is
/// printed.
fn spelling_where<'p>(
    model: &Model<'p>,
    printed: Printed,
    (m, ctx, nominated): (usize, &[&'p SymbolId], &Nominated<'p>),
    target: &Def<'p>,
    args: &[&'p Type],
) -> Spelling {
    let none = Nominated::default();
    let header = || spelling(model, Reach::Header, ctx, &none, target, args);
    let source = || spelling(model, Reach::Source(m), ctx, nominated, target, args);
    match printed {
        Printed::Source => source(),
        Printed::Header => header(),
        Printed::Both => match header() {
            Spelling::Plain => source(),
            other => other,
        },
    }
}

/// How a reference to `target` from a place in the namespaces `ctx`,
/// seeing `reach`, with the namespaces `nominated`, is written.
fn spelling<'p>(
    model: &Model<'p>,
    reach: Reach,
    ctx: &[&'p SymbolId],
    nominated: &Nominated<'p>,
    target: &Def<'p>,
    args: &[&'p Type],
) -> Spelling {
    let mut plain = model.plain_finds(reach, ctx, nominated, target);
    if plain && target.kind == DefKind::Function {
        plain = !model.nominated_name(reach, nominated, target) && !model.adl_finds_another(target, args);
    }
    match (plain, target.ns) {
        (true, _) => Spelling::Plain,
        (false, None) => Spelling::Global,
        (false, Some(_)) if model.contested(ctx, target) => Spelling::GlobalQualified,
        (false, Some(_)) => Spelling::Qualified,
    }
}

/// By module: the modules whose headers its `.cpp` includes, directly or
/// through other headers.
fn reach(layout: &Layout) -> Vec<BTreeSet<usize>> {
    let index: BTreeMap<&ModuleId, usize> = layout
        .modules
        .iter()
        .enumerate()
        .map(|(n, files)| (&files.module, n))
        .collect();
    layout
        .modules
        .iter()
        .map(|files| {
            let mut seen: BTreeSet<usize> = BTreeSet::new();
            let mut queue: VecDeque<usize> = files
                .source_includes
                .iter()
                .filter_map(|id| index.get(id).copied())
                .collect();
            while let Some(n) = queue.pop_front() {
                if !seen.insert(n) {
                    continue;
                }
                if let Some(next) = layout.modules.get(n) {
                    queue.extend(
                        next.header_includes
                            .iter()
                            .filter_map(|id| index.get(id).copied()),
                    );
                }
            }
            seen
        })
        .collect()
}

/// Adds module `n`'s header to module `m`'s `.cpp`, which keeps its
/// includes sorted by file name.
fn add_include(layout: &mut Layout, m: usize, n: usize) {
    let Some(id) = layout.modules.get(n).map(|files| files.module.clone()) else {
        return;
    };
    let stems: BTreeMap<ModuleId, _> = layout
        .modules
        .iter()
        .map(|files| (files.module.clone(), files.stem.clone()))
        .collect();
    if let Some(files) = layout.modules.get_mut(m)
        && !files.source_includes.contains(&id)
    {
        files.source_includes.push(id);
        files
            .source_includes
            .sort_by(|a, b| stems.get(a).cmp(&stems.get(b)));
    }
}

/// `name` with a capital first letter, as `B2C-E0215`'s advice.
fn capitalised(name: &str) -> String {
    let mut chars = name.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_ascii_uppercase().to_string() + chars.as_str()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_capitalised_for_the_advice() {
        assert_eq!(capitalised("list"), "List");
        assert_eq!(capitalised("Pair"), "Pair");
        assert_eq!(capitalised(""), "");
    }

    #[test]
    fn standards_count_as_years() {
        assert_eq!(year(CppStandard::Cpp17), 17);
        assert_eq!(year(CppStandard::Cpp26), 26);
    }
}
