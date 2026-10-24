//! Validation limits (spec §5.6). Checked before any other processing, for
//! files, clipboard pastes and IPC payloads alike.

/// Maximum file or payload size in bytes (32 MiB).
pub const MAX_FILE_BYTES: usize = 32 * 1024 * 1024;
/// Maximum JSON nesting depth.
pub const MAX_JSON_DEPTH: usize = 128;
/// Maximum number of blocks in a project.
pub const MAX_BLOCKS: usize = 100_000;
/// Maximum number of modules.
pub const MAX_MODULES: usize = 256;
/// Maximum tokens in one expression slot.
pub const MAX_EXPR_TOKENS: usize = 512;
/// Maximum expression parse depth (enforced by the expression parser).
pub const MAX_EXPR_DEPTH: usize = 64;
/// Maximum length of an ordinary string field, in bytes.
pub const MAX_STRING_BYTES: usize = 64 * 1024;
/// Maximum length of Raw C++ text, in bytes.
pub const MAX_RAW_CPP_BYTES: usize = 256 * 1024;
/// Maximum identifier length (enforced again by `b2c_ir::text::Ident`).
pub const MAX_IDENT_LEN: usize = 64;
/// Maximum length, in characters, of a qualified name such as `geo::area`
/// in a clipboard payload's `refs` (spec §5.12). A name of
/// [`MAX_IDENT_LEN`] characters inside 14 namespaces of that length fits.
pub const MAX_QUALIFIED_NAME_LEN: usize = 1024;
/// Maximum number of parts in a variadic (`⊕`) block.
pub const MAX_VARIADIC_PARTS: usize = 64;
/// Maximum absolute canvas coordinate.
pub const MAX_COORDINATE: i32 = 10_000_000;
/// Largest magnitude of a preprocessor define's `int` value and of every
/// number in free-form data (`extra`, `x-ext`): 2^53 − 1, JavaScript's
/// `Number.MAX_SAFE_INTEGER`. The editor holds the document as JavaScript
/// numbers, which keep every whole number up to this size exactly; a larger
/// one would change on its way through the editor, and with it the program
/// or the security hash.
pub const MAX_SAFE_INTEGER: i64 = 9_007_199_254_740_991;
