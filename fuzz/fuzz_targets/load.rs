//! Fuzzes the project loader with arbitrary bytes: it must never panic,
//! overflow the stack or allocate without bound, and every rejection must
//! carry at least one diagnostic.

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Err(error) = b2c_model::load(data) {
        assert!(!error.diagnostics.is_empty());
    }
});
