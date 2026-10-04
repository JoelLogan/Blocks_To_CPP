//! Commands that only need the pure pipeline: `check`, `generate`, `fmt` and
//! `migrate`.

use std::io::IsTerminal as _;
use std::path::Path;

use b2c_build::{FrontendOptions, run_frontend};

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

/// `b2c generate`: write the generated C++ into a folder.
pub(crate) fn generate(path: &Path, out_dir: &Path, export: bool) -> Status {
    let bytes = match project_file::read(path) {
        Ok(bytes) => bytes,
        Err(message) => {
            fail(&message);
            return Status::Usage;
        }
    };
    let options = FrontendOptions {
        do_not_edit_banner: !export,
        ..FrontendOptions::default()
    };
    let frontend = run_frontend(&bytes, &options);
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
/// whether it already is.
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

/// `b2c migrate`: upgrade the file to the current format version, printing
/// the result or (with `--in-place`) rewriting the file.
pub(crate) fn migrate(path: &Path, in_place: bool) -> Status {
    let (_, document) = match load(path) {
        Ok(loaded) => loaded,
        Err(status) => return status,
    };
    let canonical = b2c_model::to_canonical_json(&document);
    if in_place {
        write_back(path, &canonical)
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
