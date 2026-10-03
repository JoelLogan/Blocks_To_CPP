//! Finding, probing and invoking g++ (`docs/spec/07-toolchain-build-run.md`
//! §7.1–7.5; security rules in `docs/spec/08-security.md` §8.5).
//!
//! CONTRACT (implemented in milestone M1):
//! * Discovery: search order and exclusions of §7.2 (absolute PATH entries
//!   only, never the project folder or current directory, only `g++`/`g++.exe`
//!   files, canonical paths, deduplicated, fingerprinted). Platform inputs
//!   (PATH, home, well-known roots) are passed in so discovery is testable.
//! * Probing (§7.3): version, target, integrity, clang masquerade, standards,
//!   library features, diagnostics format, sanitizers, hardening, static link;
//!   each probe with a timeout, run through `b2c-process`.
//! * Command construction (§7.4): argv built only from closed enums
//!   (`b2c_model::BuildConfiguration`, language standard) plus validated
//!   defines and library profiles; flags gated on probed capabilities.
//! * Environment allowlist (§7.5.2) and the machine-local extra-flags denylist
//!   (§7.4.5).
//! * Diagnostics parsing (§7.5.3): SARIF (GCC 13+), JSON (GCC 11–12) and plain
//!   text, with strict size limits; never panics on any input.
