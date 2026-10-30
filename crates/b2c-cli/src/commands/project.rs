//! Commands that only need the pure pipeline: `check`, `generate`, `fmt` and
//! `migrate`.
//!
//! Block upgrades (`docs/spec/05-project-format.md` §5.7): `check` and
//! `generate` upgrade old blocks in memory only (the resolve stage does it);
//! `fmt` only rewrites the file in canonical form and never upgrades a block;
//! `migrate` applies format migrations and block upgrades and writes the
//! result.

use std::io::IsTerminal as _;
use std::path::Path;

use b2c_build::{BlockUpgrades, FrontendOptions, run_frontend};

use super::{err, fail, out};
use crate::project_file::{self, shown};
use crate::report::{self, BlockIndex, json_terminal_safe};
use crate::{Format, Status};

/// `b2c check`: report every problem; succeed when there are no errors.
pub(crate) fn check(path: &Path, format: Format) -> Status {
    let bytes = match project_file::read(path) {
        Ok(bytes) => bytes,
        Err(message) => {
            fail(&message);
            return Status::Usage;
        }
    };
    let frontend = run_frontend(&bytes, &FrontendOptions::default());
    let file = shown(path);
    match format {
        Format::Text => {
            out(&report::render_text(
                &file,
                &frontend.diagnostics,
                &BlockIndex::new(frontend.document.as_ref()),
            ));
            if !frontend.has_errors() {
                out(&format!("{file}: no errors\n"));
            }
        }
        Format::Json => out(&report::render_json(
            &path.display().to_string(),
            &frontend.diagnostics,
        )),
    }
    if frontend.has_errors() {
        Status::ProjectErrors
    } else {
        Status::Success
    }
}

/// `b2c generate`: write the generated C++ into a folder. Files already
/// there are replaced only when they have a generated file's name, and
/// nothing is ever deleted (unlike a build's own `gen/`, which holds exactly
/// the generated files).
pub(crate) fn generate(path: &Path, out_dir: &Path) -> Status {
    let bytes = match project_file::read(path) {
        Ok(bytes) => bytes,
        Err(message) => {
            fail(&message);
            return Status::Usage;
        }
    };
    let frontend = run_frontend(&bytes, &FrontendOptions::default());
    let file = shown(path);
    err(&report::render_text(
        &file,
        &frontend.diagnostics,
        &BlockIndex::new(frontend.document.as_ref()),
    ));
    let Some(generated) = frontend.generated.as_ref().filter(|_| !frontend.has_errors()) else {
        fail(&format!("{file} has errors; no C++ was generated"));
        return Status::ProjectErrors;
    };
    if let Err(error) = std::fs::create_dir_all(out_dir) {
        fail(&format!("cannot create {}: {error}", shown(out_dir)));
        return Status::Usage;
    }
    match b2c_build::write_generated_files(out_dir, generated) {
        Ok(_) => {
            for generated_file in &generated.files {
                out(&format!("{}\n", shown(&out_dir.join(&generated_file.path))));
            }
            Status::Success
        }
        Err(error) => {
            fail(&report::terminal_safe(&error.to_string()));
            Status::Usage
        }
    }
}

/// `b2c generate --export`: export the project as a C++ project into a
/// folder that is missing or empty, or with `replace`, over an earlier
/// export (only the files it wrote that nobody changed since; each file
/// kept is named on standard error).
pub(crate) fn export(path: &Path, out_dir: &Path, replace: bool) -> Status {
    let bytes = match project_file::read(path) {
        Ok(bytes) => bytes,
        Err(message) => {
            fail(&message);
            return Status::Usage;
        }
    };
    let file = shown(path);
    let export = match b2c_build::export::prepare(&bytes, 4, env!("CARGO_PKG_VERSION")) {
        Ok(export) => export,
        Err(
            b2c_build::export::ExportError::ProjectErrors(diagnostics)
            | b2c_build::export::ExportError::DefineNotExportable(diagnostics),
        ) => {
            let document = b2c_model::load(&bytes).ok();
            err(&report::render_text(
                &file,
                &diagnostics,
                &BlockIndex::new(document.as_ref()),
            ));
            fail(&format!("{file} has errors; nothing was exported"));
            return Status::ProjectErrors;
        }
        Err(error) => {
            fail(&report::terminal_safe(&error.to_string()));
            return Status::Usage;
        }
    };
    let mode = if replace {
        b2c_build::export::ExportMode::Replace
    } else {
        b2c_build::export::ExportMode::New
    };
    // `--out` is created if missing (export::write makes its last level).
    if let Some(parent) = out_dir.parent().filter(|parent| !parent.as_os_str().is_empty())
        && let Err(error) = std::fs::create_dir_all(parent)
    {
        fail(&format!("cannot create {}: {error}", shown(parent)));
        return Status::Usage;
    }
    match b2c_build::export::write(out_dir, &export, mode) {
        Ok(written) => {
            for name in &written.files {
                out(&format!("{}\n", shown(&out_dir.join(name))));
            }
            // A file of the earlier export that may hold the student's own
            // changes is kept, and named (w3-modules.md §2.8).
            for kept in &written.kept {
                fail(&report::terminal_safe(&kept.to_string()));
            }
            Status::Success
        }
        Err(b2c_build::export::ExportError::NotEmpty) => {
            fail(&format!(
                "{} is not empty; export into a new folder, or add --replace to replace an earlier export there",
                shown(out_dir)
            ));
            Status::Usage
        }
        Err(error) => {
            fail(&report::terminal_safe(&format!("{}: {error}", shown(out_dir))));
            Status::Usage
        }
    }
}

/// Loads a project for `fmt`/`migrate`, printing problems on failure.
fn load(path: &Path) -> Result<(Vec<u8>, b2c_model::Document), Status> {
    let bytes = project_file::read(path).map_err(|message| {
        fail(&message);
        Status::Usage
    })?;
    match b2c_model::load(&bytes) {
        Ok(document) => Ok((bytes, document)),
        Err(error) => {
            err(&report::render_text(
                &shown(path),
                &error.diagnostics,
                &BlockIndex::new(None),
            ));
            Err(Status::ProjectErrors)
        }
    }
}

/// Atomically replaces the project file with `contents`, keeping the file
/// as it was as `<name>.b2c.bak` (05 §5.7, §5.10).
fn save_with_backup(path: &Path, contents: &str) -> Status {
    let absolute = match std::path::absolute(path) {
        Ok(absolute) => absolute,
        Err(error) => {
            fail(&format!("cannot write {}: {error}", shown(path)));
            return Status::Usage;
        }
    };
    match b2c_build::save_project(&absolute, contents.as_bytes()) {
        Ok(()) => Status::Success,
        Err(error) => {
            fail(&report::terminal_safe(&error.to_string()));
            Status::Usage
        }
    }
}

/// Atomically replaces the project file with `contents`.
fn write_back(path: &Path, contents: &str) -> Status {
    match b2c_build::write_if_changed(path, contents.as_bytes()) {
        Ok(()) => Status::Success,
        Err(error) => {
            fail(&report::terminal_safe(&error.to_string()));
            Status::Usage
        }
    }
}

/// `b2c fmt`: rewrite the file in canonical form, or with `--check` report
/// whether it already is. Blocks are never upgraded: that is `b2c migrate`'s
/// job.
pub(crate) fn fmt(path: &Path, check_only: bool) -> Status {
    let (bytes, document) = match load(path) {
        Ok(loaded) => loaded,
        Err(status) => return status,
    };
    let canonical = b2c_model::to_canonical_json(&document);
    if canonical.as_bytes() == bytes.as_slice() {
        return Status::Success;
    }
    if check_only {
        out(&format!(
            "{} is not formatted; run `b2c fmt` on it\n",
            shown(path)
        ));
        return Status::ProjectErrors;
    }
    write_back(path, &canonical)
}

/// `b2c migrate`: upgrade the file to the current format version and upgrade
/// its blocks (05 §5.7), printing the result or (with `--in-place`)
/// rewriting the file and keeping the previous version as `<name>.b2c.bak`.
/// A file that is already current is left alone. When a block cannot be
/// upgraded, its problems are printed and nothing is written. A file whose
/// blocks were upgraded names this app and the current catalog in
/// `generator`, as the app's save does, but never an older catalog than it
/// named ([`stamp_generator`]).
pub(crate) fn migrate(path: &Path, in_place: bool) -> Status {
    migrate_with(path, in_place, &BlockUpgrades::SHIPPED)
}

/// [`migrate`] with the given block upgrade tables.
fn migrate_with(path: &Path, in_place: bool, upgrades: &BlockUpgrades<'_>) -> Status {
    let (bytes, mut document) = match load(path) {
        Ok(loaded) => loaded,
        Err(status) => return status,
    };
    let upgrade = b2c_build::upgrade_document_with(&mut document, b2c_build::core_catalog(), upgrades);
    if !upgrade.diagnostics.is_empty() {
        let file = shown(path);
        err(&report::render_text(
            &file,
            &upgrade.diagnostics,
            &BlockIndex::new(Some(&document)),
        ));
        fail(&format!(
            "{file} has blocks that cannot be upgraded; nothing was written"
        ));
        return Status::ProjectErrors;
    }
    if upgrade.changed() {
        stamp_generator(&mut document);
    }
    let canonical = b2c_model::to_canonical_json(&document);
    if in_place {
        if canonical.as_bytes() == bytes.as_slice() {
            return Status::Success;
        }
        save_with_backup(path, &canonical)
    } else {
        // Project text may hold control and invisible characters. Escaped,
        // the JSON is the same but cannot drive the terminal; piped output
        // stays byte for byte canonical.
        if std::io::stdout().is_terminal() {
            out(&json_terminal_safe(&canonical));
        } else {
            out(&canonical);
        }
        Status::Success
    }
}

/// Names this app in `generator.app` and the current catalog in
/// `generator.catalog`, as the app's save does (05 §5.3), for a document
/// whose blocks were upgraded: the upgraded blocks have the current form, so
/// an older app that opens the file reports a newer field or option of
/// theirs as made by a newer catalog (`B2C-E0602`), not as unknown
/// (ADR-0015). A newer catalog the file names is kept, so the blocks a newer
/// catalog made stay `B2C-E0602` here too.
fn stamp_generator(document: &mut b2c_model::Document) {
    let generator = &mut document.generator;
    generator.app = String::from(env!("CARGO_PKG_VERSION"));
    if !b2c_model::is_newer_version(&generator.catalog, b2c_build::CATALOG_VERSION) {
        generator.catalog = String::from(b2c_build::CATALOG_VERSION);
    }
}

#[cfg(test)]
mod tests {
    use b2c_build::BlockReplacement;
    use b2c_model::Block;

    use super::*;

    /// `b2c generate` writes the generated files into a folder the user
    /// chose and never deletes anything there: not a `.cpp` with another
    /// module's name, not a file of another kind (w3-modules.md §2.7).
    #[test]
    fn generate_never_deletes_files_in_its_folder() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("hello.b2c");
        std::fs::write(&project, include_bytes!("../../../../examples/hello_world.b2c")).unwrap();
        let out_dir = dir.path().join("out");
        std::fs::create_dir(&out_dir).unwrap();
        for name in ["utils.cpp", "utils.hpp", "notes.txt"] {
            std::fs::write(out_dir.join(name), "// mine").unwrap();
        }
        assert_eq!(generate(&project, &out_dir), Status::Success);
        assert!(out_dir.join("main.cpp").is_file());
        for name in ["utils.cpp", "utils.hpp", "notes.txt"] {
            assert_eq!(
                std::fs::read_to_string(out_dir.join(name)).unwrap(),
                "// mine",
                "{name}"
            );
        }
    }

    /// The test-only retired type `test.say`, replaced by `io.print`.
    fn say_to_print(block: &mut Block) -> Result<(), String> {
        let what = block.inputs.remove("WHAT").ok_or("the block has no WHAT input")?;
        block.inputs.insert(String::from("ITEM0"), what);
        Ok(())
    }

    const TEST_UPGRADES: BlockUpgrades<'static> = BlockUpgrades {
        migrations: &[],
        replacements: &[BlockReplacement {
            from: "test.say",
            version: 1,
            to: "io.print",
            to_version: 1,
            apply: say_to_print,
        }],
        retired: &["test.say"],
    };

    fn old_project() -> String {
        serde_json::json!({
            "format": "blocks2cpp/project", "formatVersion": 1,
            "generator": {"app": "0.1.0", "catalog": "1.0.0"},
            "project": {"id": "prj_old", "name": "Old", "language": {"standard": "c++20"}},
            "modules": [{"id": "mod_main", "name": "main", "workspace": {"blocks": [
                {"id": "b_main", "type": "program.main", "v": 1,
                 "statements": {"BODY": [
                    {"id": "b_say", "type": "test.say", "v": 1,
                     "inputs": {"WHAT": {"expr": [{"str": "hi"}]}}}
                 ]}}
            ]}}]
        })
        .to_string()
    }

    #[test]
    fn migrate_upgrades_blocks_and_keeps_a_backup() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("old.b2c");
        let backup = dir.path().join("old.b2c.bak");
        std::fs::write(&path, old_project()).unwrap();
        assert_eq!(migrate_with(&path, true, &TEST_UPGRADES), Status::Success);
        let migrated = std::fs::read_to_string(&path).unwrap();
        let document = b2c_model::load(migrated.as_bytes()).unwrap();
        let say = &document.modules[0].workspace.blocks[0].statements["BODY"][0];
        assert_eq!((say.id.as_str(), say.block_type.as_str()), ("b_say", "io.print"));
        assert!(say.inputs.contains_key("ITEM0"));
        assert_eq!(migrated, b2c_model::to_canonical_json(&document));
        assert_eq!(std::fs::read_to_string(&backup).unwrap(), old_project());
        // The upgraded blocks have the current form, so the file names the
        // current catalog and this app, as the app's save writes them.
        assert_eq!(document.generator.catalog, b2c_build::CATALOG_VERSION);
        assert_eq!(document.generator.app, env!("CARGO_PKG_VERSION"));

        // With the shipped tables `test.say` is only an unknown type (check
        // reports it): migrate writes the canonical form and keeps the block.
        std::fs::write(&path, old_project()).unwrap();
        assert_eq!(migrate(&path, true), Status::Success);
        let kept = b2c_model::load(&std::fs::read(&path).unwrap()).unwrap();
        let say = &kept.modules[0].workspace.blocks[0].statements["BODY"][0];
        assert_eq!(say.block_type, "test.say");
        // Nothing was upgraded, so `generator` stays as the file had it.
        assert_eq!(kept.generator.catalog, "1.0.0");
    }

    /// A file from a newer catalog keeps naming it after an upgrade, so the
    /// blocks that catalog made stay `B2C-E0602`.
    #[test]
    fn migrate_never_names_an_older_catalog() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("newer.b2c");
        let newer = old_project().replace("\"catalog\":\"1.0.0\"", "\"catalog\":\"99.0.0\"");
        assert!(newer.contains("99.0.0"));
        std::fs::write(&path, &newer).unwrap();
        assert_eq!(migrate_with(&path, true, &TEST_UPGRADES), Status::Success);
        let document = b2c_model::load(&std::fs::read(&path).unwrap()).unwrap();
        let say = &document.modules[0].workspace.blocks[0].statements["BODY"][0];
        assert_eq!(say.block_type, "io.print");
        assert_eq!(document.generator.catalog, "99.0.0");
        assert_eq!(document.generator.app, env!("CARGO_PKG_VERSION"));
    }
}
