//! Fuzzes the resolve stage with every document the loader accepts: it must
//! never panic, and resolving its own output must change nothing more.

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(document) = b2c_model::load(data) {
        let catalog = b2c_catalog::core_catalog();
        let (completed, _) = b2c_catalog::resolve(&document, catalog);
        let (again, _) = b2c_catalog::resolve(&completed, catalog);
        assert_eq!(again, completed);
    }
});
