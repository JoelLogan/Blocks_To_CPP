//! The standard-name table is fresh for the local g++ (w3-modules.md §2.12,
//! §5.2 N1): `tools/gen-std-names.py --check` probes the g++ on `PATH` and
//! fails when the table lacks a name it declares in `std`. A g++ newer than
//! the table's newest GCC counts as an incomplete table (§1.8) and is
//! checked by CI's GCC 15 step instead.

use std::path::Path;

use b2c_ir::std_names::NEWEST_GCC_MAJOR;

use crate::gxx::{available, command, major_version};

#[test]
fn the_table_has_every_name_the_local_gxx_declares() {
    if !available() {
        return;
    }
    let major = major_version();
    if major > NEWEST_GCC_MAJOR {
        eprintln!(
            "note: g++ {major} is newer than the standard-name table (GCC {NEWEST_GCC_MAJOR}); \
             CI's GCC 15 step checks it"
        );
        return;
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let output = command("python3")
        .current_dir(&root)
        .args(["tools/gen-std-names.py", "--check", "--compiler", "g++"])
        .output()
        .expect("python3 runs tools/gen-std-names.py");
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
