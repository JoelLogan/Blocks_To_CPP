//! The IDE init unit (`docs/spec/07-toolchain-build-run.md` §7.6.3).
//!
//! Programs that the app builds and runs get one extra translation unit,
//! `<build>/ide/b2c_ide_init.cpp`, compiled in the same g++ invocation as
//! `main.cpp`. Its static initialiser sets the Windows console's input and
//! output code pages to UTF-8, so `std::cout << "héllo ✓"` shows correctly,
//! and, when the app runs the program in a console window of its own
//! ([`crate::EXTERNAL_TERMINAL_VAR`]), waits for a key before that window
//! closes; on other systems it compiles to nothing. It never reads
//! `B2C_EVENTS`: the terminate handler and the event channel arrive in M5.
//!
//! The unit is never part of the generated project: it is not in the code
//! view, the source map, `b2c generate` or an export, and the command-line
//! tool never links it. Because the IDE flag is part of the build folder's
//! options hash, builds with and without it never share an executable. It
//! must compile without a single warning under every supported GCC (11–15),
//! standard and configuration, including `-Wall -Wextra -Wpedantic` and the
//! strict warning level with `-Werror`; a diagnostic in it is reported as a
//! bug in Blocks2Cpp.

use std::path::PathBuf;

use crate::build_dir::{BuildDir, BuildDirError, replace_if_changed};

/// The init unit's file name inside [`BuildDir::ide_dir`].
pub const INIT_UNIT_FILE: &str = "b2c_ide_init.cpp";

/// The init unit's source text. `<windows.h>` is not included: the two
/// functions it needs are declared directly, which keeps the unit small and
/// free of the header's macros and warnings.
pub const INIT_UNIT_SOURCE: &str = r#"// Blocks2Cpp IDE init unit (docs/spec/07-toolchain-build-run.md §7.6.3).
// It is linked only into programs that the IDE builds and runs. It is never
// shown in the code view and never exported.
#if defined(_WIN32)
#if defined(_WIN64)
#define B2C_IDE_WINAPI
#else
#define B2C_IDE_WINAPI __stdcall
#endif

#include <conio.h>
#include <cstdio>
#include <cstdlib>

// Declared here instead of including <windows.h>, which is large and would
// bring warnings and macros into this file.
extern "C" __declspec(dllimport) int B2C_IDE_WINAPI SetConsoleCP(unsigned int code_page);
extern "C" __declspec(dllimport) int B2C_IDE_WINAPI SetConsoleOutputCP(unsigned int code_page);

namespace {

// In a console window of its own (the IDE's external terminal), the window
// closes with the program: once the program ends, wait for a key so its
// output can be read.
void b2c_ide_wait_for_a_key() {
    std::fputs("\n[The program has ended. Press a key to close this window.]", stdout);
    std::fflush(stdout);
    static_cast<void>(_getch());
}

// Before main() runs, switches the console's input and output code pages to
// UTF-8, so text the program reads and prints shows correctly in the IDE. When
// the IDE runs the program in a window of its own (B2C_EXTERNAL_TERMINAL), it
// arranges the wait above and removes the variable, so programs this one
// starts never see it.
struct B2cIdeInit {
    B2cIdeInit() noexcept {
        static_cast<void>(SetConsoleCP(65001u));
        static_cast<void>(SetConsoleOutputCP(65001u));
        if (std::getenv("B2C_EXTERNAL_TERMINAL") != nullptr) {
            static_cast<void>(_putenv("B2C_EXTERNAL_TERMINAL="));
            static_cast<void>(std::atexit(b2c_ide_wait_for_a_key));
        }
    }
};

const B2cIdeInit b2c_ide_init_instance;

}  // namespace
#endif
"#;

/// Writes the init unit into the build folder's `ide/` folder (only when its
/// content differs). Returns its path and whether the file was written.
///
/// # Errors
/// When the file is a link or not a regular file, or on an I/O error.
pub(crate) fn write_init_unit(dir: &BuildDir) -> Result<(PathBuf, bool), BuildDirError> {
    let path = dir.ide_dir().join(INIT_UNIT_FILE);
    let changed = replace_if_changed(&path, INIT_UNIT_SOURCE.as_bytes(), false)?;
    Ok((path, changed))
}

#[cfg(test)]
mod tests {
    use b2c_ir::ids::ProjectId;

    use super::*;
    use crate::build_dir::IDE_INIT_STEM;

    /// Generated files may not take the unit's name (see `build_dir`).
    #[test]
    fn the_file_name_matches_the_reserved_stem() {
        assert_eq!(INIT_UNIT_FILE.strip_suffix(".cpp"), Some(IDE_INIT_STEM));
    }

    #[test]
    fn the_unit_is_written_once_and_kept() {
        let cache = tempfile::tempdir().unwrap();
        let dir = BuildDir::create(cache.path(), &ProjectId::new("prj_ide").unwrap(), "debug").unwrap();
        let (path, changed) = write_init_unit(&dir).unwrap();
        assert!(changed);
        assert_eq!(path, dir.ide_dir().join("b2c_ide_init.cpp"));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), INIT_UNIT_SOURCE);
        let (again, changed) = write_init_unit(&dir).unwrap();
        assert_eq!(again, path);
        assert!(!changed);
        std::fs::write(&path, "tampered").unwrap();
        assert!(write_init_unit(&dir).unwrap().1);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), INIT_UNIT_SOURCE);
    }

    /// Only Windows code, which never opens the event channel (M5): the one
    /// variable it reads is the external terminal's, and it includes only
    /// the C runtime headers its wait needs.
    #[test]
    fn the_unit_is_windows_only_and_never_reads_the_event_channel() {
        assert!(!INIT_UNIT_SOURCE.contains("B2C_EVENTS"));
        assert_eq!(INIT_UNIT_SOURCE.matches("getenv(").count(), 1);
        assert!(INIT_UNIT_SOURCE.contains(&format!("std::getenv(\"{}\")", crate::EXTERNAL_TERMINAL_VAR)));
        let includes: Vec<&str> = INIT_UNIT_SOURCE
            .lines()
            .filter(|line| line.contains("#include"))
            .collect();
        assert_eq!(
            includes,
            ["#include <conio.h>", "#include <cstdio>", "#include <cstdlib>"]
        );
        let code: Vec<&str> = INIT_UNIT_SOURCE
            .lines()
            .filter(|line| !line.trim().is_empty() && !line.trim_start().starts_with("//"))
            .collect();
        assert_eq!(code.first(), Some(&"#if defined(_WIN32)"));
        assert_eq!(code.last(), Some(&"#endif"));
        assert!(INIT_UNIT_SOURCE.contains("SetConsoleCP(65001u)"));
        assert!(INIT_UNIT_SOURCE.contains("SetConsoleOutputCP(65001u)"));
    }
}
