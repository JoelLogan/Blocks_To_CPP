//! Fuzzes canonical saving: whatever the loader accepts must save to text
//! that loads back to the same document, saves to the same bytes again, and
//! hashes the same.

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(document) = b2c_model::load(data) else {
        return;
    };
    let saved = b2c_model::to_canonical_json(&document);
    let reloaded = match b2c_model::load(saved.as_bytes()) {
        Ok(reloaded) => reloaded,
        Err(error) => panic!("the canonical form does not load: {:?}", error.diagnostics),
    };
    assert_eq!(b2c_model::to_canonical_json(&reloaded), saved);
    assert_eq!(
        b2c_model::content_hash(&reloaded),
        b2c_model::content_hash(&document)
    );
});
