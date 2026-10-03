//! Loading, validating and saving `.b2c` project files
//! (`docs/spec/05-project-format.md`).
//!
//! * [`document`]: the typed Block Document Model.
//! * [`load`]: parse untrusted bytes with every limit and rule of §5.6.
//! * [`to_canonical_json`]: deterministic serialisation (§5.2).
//! * [`content_hash`]: SHA-256 of the semantic content (§5.11).
//! * [`limits`]: the validation limits.
//!
//! Pure functions only: no I/O, compiles for `wasm32-unknown-unknown`.

pub mod document;
pub mod limits;
mod load;
mod save;

pub use document::*;
pub use load::{LoadError, load};
pub use save::{content_hash, to_canonical_json};
