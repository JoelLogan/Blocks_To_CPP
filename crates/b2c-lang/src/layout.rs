//! The program's files (ADR-0019; `docs/plans/m3-contracts/modules.md` §1,
//! §3, §4; `w3-modules.md` §1.7, §2.4): which files each module gets, what
//! they include and in which order their declarations are printed. The
//! analyser computes the [`Layout`] and reports its problems; code
//! generation only prints it.
//!
//! [`compute_layout`] starts from milestone M2's flat layout (one `.cpp` per
//! module, named by its [`FileStem`], or `module<N>` for a name the stem
//! refuses), orders each module's declarations with the declaration graph
//! (`crate::order`), and then asks three hooks what goes into headers:
//!
//! * [`shared_items`]: the items whose declarations go into their module's
//!   header. Package F turns functions on ([`FUNCTIONS_IN_HEADERS`]),
//!   package T structs, enums, globals and constants
//!   ([`TYPES_AND_GLOBALS_IN_HEADERS`]).
//! * [`header_parts`]: the order of those declarations in the header
//!   (`header_order`), the declaration graph's; T's program-wide graph
//!   replaces it.
//! * [`includes`]: the project headers each file includes and the types its
//!   header forward-declares. F and T add the headers whose shared
//!   definitions a module uses, and T the forward declarations.
//!
//! Until those packages switch them on, nothing goes into a header, so every
//! program gets M2's layout and the C++ of a single-module project does not
//! change. The layout also reports `B2C-E0221` for a module whose name starts
//! with `b2c` (modules.md §2), in every project.

use b2c_ir::diag::{Location, Part};
use b2c_ir::layout::{Layout, ModuleFiles};
use b2c_ir::sast::{Item, ItemKind, Module, SymbolTable};
use b2c_ir::text::{FileStem, StemError};

use crate::codes;
use crate::messages::Diags;

/// Whether a shared function's declaration goes into its module's header
/// (package F, w3-modules.md §1.2).
const FUNCTIONS_IN_HEADERS: bool = false;

/// Whether a shared struct, enum, global or constant goes into its module's
/// header (package T, w3-modules.md §1.3).
const TYPES_AND_GLOBALS_IN_HEADERS: bool = false;

/// The message of `B2C-E0221` (modules.md §2).
const RESERVED_MODULE_MESSAGE: &str =
    "Module names that start with `b2c` are kept for files that Blocks2Cpp writes. Rename the module.";

/// The layout of a lowered program, with its problems: the declaration
/// graph's cycles (`B2C-E0420`, `B2C-E0421`) and module names that start
/// with `b2c` (`B2C-E0221`).
pub(crate) fn compute_layout(modules: &[Module], symbols: &SymbolTable, diags: &mut Diags) -> Layout {
    check_module_names(modules, diags);
    let mut layout = Layout::flat(modules, symbols);
    crate::order::order_declarations(modules, symbols, &mut layout, diags);
    for (module, files) in modules.iter().zip(&mut layout.modules) {
        let shared = shared_items(module);
        files.header_order = header_parts(files, &shared);
        files.header = !files.header_order.is_empty();
    }
    includes(&mut layout);
    layout
}

/// Reports each module whose name starts with `b2c` (`B2C-E0221`; the
/// loader's pattern allows no capital letter): the loader accepts it, so that the project opens and the
/// module can be renamed, but it never becomes a file name. Its file is
/// `module<N>.cpp` until then ([`Layout::flat`]).
fn check_module_names(modules: &[Module], diags: &mut Diags) {
    for module in modules {
        if FileStem::new(&module.name) == Err(StemError::Reserved) {
            let location = Location {
                module: Some(module.id.clone()),
                block: None,
                part: Part::Whole,
            };
            diags.error(codes::RESERVED_MODULE_NAME, location, RESERVED_MODULE_MESSAGE);
        }
    }
}

/// The items of a module whose declarations go into its header, by index
/// into `Module::items`: the shared ones (`Item::shared`, which the analyser
/// sets only in a project of several modules) of a kind headers hold yet.
pub(crate) fn shared_items(module: &Module) -> Vec<usize> {
    module
        .items
        .iter()
        .enumerate()
        .filter(|(_, item)| goes_into_header(item))
        .map(|(index, _)| index)
        .collect()
}

/// Whether an item's declaration goes into its module's header.
fn goes_into_header(item: &Item) -> bool {
    item.shared
        && match &item.kind {
            ItemKind::Function(_) => FUNCTIONS_IN_HEADERS,
            ItemKind::Struct(_) | ItemKind::Enum(_) | ItemKind::Global(_) => TYPES_AND_GLOBALS_IN_HEADERS,
            // `main` and *use header* are never shared.
            ItemKind::Main(_) | ItemKind::UseHeader { .. } => false,
        }
}

/// The header's declarations in printing order (`header_order`): the shared
/// items in the order the declaration graph gives the module's `.cpp`.
pub(crate) fn header_parts(files: &ModuleFiles, shared: &[usize]) -> Vec<usize> {
    files
        .source_order
        .iter()
        .copied()
        .filter(|index| shared.contains(index))
        .collect()
}

/// The project headers each file includes. A `.cpp` includes its own
/// header, before its definitions (w3-modules.md §1.7: without it a `const`
/// constant's definition has internal linkage and does not link).
pub(crate) fn includes(layout: &mut Layout) {
    for files in &mut layout.modules {
        files.source_includes = if files.header {
            vec![files.module.clone()]
        } else {
            Vec::new()
        };
    }
}

#[cfg(test)]
mod tests {
    use b2c_ir::headers::ItemHeaders;
    use b2c_ir::ids::{BlockId, ModuleId, SymbolId};
    use b2c_ir::sast::{Block, EnumDef, FunctionDef, MainDef, Origin};
    use b2c_ir::types::Type;

    use super::*;

    fn item(kind: ItemKind, shared: bool) -> Item {
        Item {
            kind,
            origin: Origin::whole(ModuleId::new("m").expect("id"), BlockId::new("b").expect("id")),
            comment: None,
            headers: ItemHeaders::default(),
            shared,
            namespace: None,
        }
    }

    fn function(shared: bool) -> Item {
        item(
            ItemKind::Function(FunctionDef {
                symbol: SymbolId::new("f").expect("id"),
                params: Vec::new(),
                ret: Type::Void,
                body: Block::default(),
            }),
            shared,
        )
    }

    fn module(name: &str, items: Vec<Item>) -> Module {
        Module {
            id: ModuleId::new(&format!("mod_{name}")).expect("id"),
            name: name.to_owned(),
            items,
        }
    }

    #[test]
    fn nothing_goes_into_a_header_until_its_package_says_so() {
        let main = item(
            ItemKind::Main(MainDef {
                body: Block::default(),
                args: None,
            }),
            true,
        );
        let colour = item(
            ItemKind::Enum(EnumDef {
                symbol: SymbolId::new("e").expect("id"),
            }),
            true,
        );
        let module = module("utils", vec![main, function(true), function(false), colour]);
        assert_eq!(shared_items(&module), Vec::<usize>::new());
        let mut diags = Diags::default();
        let layout = compute_layout(&[module], &SymbolTable::default(), &mut diags);
        assert!(diags.list.is_empty());
        let files = &layout.modules[0];
        assert!(!files.header);
        assert!(files.header_order.is_empty() && files.source_includes.is_empty());
    }

    #[test]
    fn a_header_lists_the_shared_items_in_graph_order_and_its_cpp_includes_it() {
        let mut layout = Layout::flat(&[module("utils", Vec::new())], &SymbolTable::default());
        let files = &mut layout.modules[0];
        files.source_order = vec![3, 0, 2, 1];
        files.header_order = header_parts(files, &[1, 3]);
        assert_eq!(files.header_order, [3, 1]);
        files.header = true;
        includes(&mut layout);
        assert_eq!(
            layout.modules[0].source_includes,
            [ModuleId::new("mod_utils").expect("id")]
        );
        layout.modules[0].header = false;
        includes(&mut layout);
        assert!(layout.modules[0].source_includes.is_empty());
    }

    #[test]
    fn module_names_starting_with_b2c_are_reported_and_numbered() {
        let modules = [
            module("main", Vec::new()),
            module("b2c_tools", Vec::new()),
            module("b2cx", Vec::new()),
            module("xb2c", Vec::new()),
        ];
        let mut diags = Diags::default();
        let layout = compute_layout(&modules, &SymbolTable::default(), &mut diags);
        let reported: Vec<(&str, Option<&str>, &str)> = diags
            .list
            .iter()
            .map(|d| {
                (
                    d.code.0.as_str(),
                    d.primary.module.as_ref().map(ModuleId::as_str),
                    d.message.as_str(),
                )
            })
            .collect();
        assert_eq!(
            reported,
            [
                (
                    codes::RESERVED_MODULE_NAME,
                    Some("mod_b2c_tools"),
                    RESERVED_MODULE_MESSAGE
                ),
                (
                    codes::RESERVED_MODULE_NAME,
                    Some("mod_b2cx"),
                    RESERVED_MODULE_MESSAGE
                ),
            ]
        );
        assert!(diags.list.iter().all(|d| d.primary.block.is_none()));
        let stems: Vec<&str> = layout.modules.iter().map(|files| files.stem.as_str()).collect();
        assert_eq!(stems, ["main", "module2", "module3", "xb2c"]);
    }
}
