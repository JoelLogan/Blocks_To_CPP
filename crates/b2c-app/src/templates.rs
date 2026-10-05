//! The bundled project templates of `project_new`
//! (`docs/spec/04-user-interface.md` §4.10, `docs/spec/02-architecture.md`
//! §2.5.2).
//!
//! A template is one of a closed set ([`Template`]), embedded in the binary;
//! nothing is ever read from a path. M2 has two:
//!
//! * `empty` (`templates/empty.b2c`): one module `main` with an empty
//!   `when program starts`;
//! * `helloWorld` (`templates/hello_world.b2c`): a copy of
//!   `examples/hello_world.b2c`.
//!
//! A new project gets a fresh project ID (`prj_` + 96 random bits), the
//! default C++ standard from the settings, and the app's and catalog's
//! versions as its `generator`.

use b2c_ipc::IpcError;
use b2c_ipc::dto::Template;
use b2c_ir::sast::CppStandard;
use b2c_model::{Document, Generator};

/// The bytes of the `empty` template.
const EMPTY: &[u8] = include_bytes!("../templates/empty.b2c");

/// The bytes of the `helloWorld` template.
const HELLO_WORLD: &[u8] = include_bytes!("../templates/hello_world.b2c");

/// The embedded bytes of `template`.
pub(crate) fn template_bytes(template: Template) -> &'static [u8] {
    match template {
        Template::Empty => EMPTY,
        Template::HelloWorld => HELLO_WORLD,
    }
}

/// A new project from `template`: a fresh project ID, the C++ `standard`
/// and the generator versions.
///
/// # Errors
/// [`IpcError::Internal`] when the OS random number generator fails, or a
/// template does not load (a test rules that out).
pub(crate) fn instantiate(
    template: Template,
    standard: CppStandard,
    app_version: &str,
) -> Result<Document, IpcError> {
    let mut document = b2c_model::load(template_bytes(template)).map_err(|error| {
        tracing::error!(
            template = template.as_str(),
            problems = error.diagnostics.len(),
            "a bundled template does not load"
        );
        IpcError::Internal
    })?;
    document.project.id = b2c_ipc::random_project_id()?;
    document.project.language.standard = standard;
    document.generator = Generator {
        app: app_version.to_owned(),
        catalog: b2c_build::CATALOG_VERSION.to_owned(),
    };
    Ok(document)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_template_loads_and_gets_its_own_identity() {
        for template in Template::ALL.iter().copied() {
            let first = instantiate(template, CppStandard::Cpp23, "9.8.7").unwrap();
            let second = instantiate(template, CppStandard::Cpp23, "9.8.7").unwrap();
            assert_ne!(first.project.id, second.project.id);
            assert!(first.project.id.as_str().starts_with("prj_"));
            assert_eq!(first.project.language.standard, CppStandard::Cpp23);
            assert_eq!(first.generator.app, "9.8.7");
            assert_eq!(first.generator.catalog, b2c_build::CATALOG_VERSION);
            assert_eq!(first.modules.len(), 1);
            assert_eq!(first.modules[0].name, "main");
            // The canonical text loads back to the same document.
            let text = b2c_model::to_canonical_json(&first);
            assert_eq!(b2c_model::load(text.as_bytes()).unwrap(), first);
        }
    }

    #[test]
    fn hello_world_is_the_example() {
        let example = include_bytes!("../../../examples/hello_world.b2c");
        assert_eq!(template_bytes(Template::HelloWorld), example);
    }

    #[test]
    fn the_empty_template_has_an_empty_program() {
        let document = b2c_model::load(template_bytes(Template::Empty)).unwrap();
        let blocks = &document.modules[0].workspace.blocks;
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].block_type, "program.main");
        assert!(blocks[0].statements.get("BODY").is_none_or(Vec::is_empty));
    }
}
