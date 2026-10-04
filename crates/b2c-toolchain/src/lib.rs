//! Finding, probing and invoking g++ (`docs/spec/07-toolchain-build-run.md`
//! §7.1–7.5; security rules in `docs/spec/08-security.md` §8.5).
//!
//! The pieces, in the order a build uses them:
//!
//! 1. [`discovery`]: find g++ candidates (§7.2), or check an explicit path.
//! 2. [`probe`]: ask a candidate what it can do (§7.3), giving a cacheable
//!    [`probe::Toolchain`] fingerprinted by [`fingerprint`].
//! 3. [`flags`]: validate the inputs that end up on the command line
//!    (project defines, library profiles, `pkg-config` output, machine-local
//!    extra flags and their denylist, §7.4.4–7.4.5).
//! 4. [`command`]: build the exact argv from the probed toolchain and the
//!    project's closed build options (§7.4), for a compile-only, link-only
//!    or single compile-and-link step.
//! 5. [`env`]: the compiler's allowlisted environment (§7.5.2).
//! 6. [`diagnostics`]: parse what g++ and the linker reported (§7.5.3).
//!
//! Every problem is reported as a [`b2c_ir::Diagnostic`] with a
//! `B2C-T1xxx` code from [`codes`].
//!
//! # Example: compile and run hello world
//!
//! ```no_run
//! use std::path::Path;
//! use b2c_ir::sast::CppStandard;
//! use b2c_model::{BuildSettings, Language};
//! use b2c_process::run_captured;
//! use b2c_toolchain::command::{BuildInputs, CommandPlan};
//! use b2c_toolchain::diagnostics::parse_output;
//! use b2c_toolchain::env::{HostEnv, compiler_env};
//! use b2c_toolchain::flags::ExtraFlags;
//! use b2c_toolchain::probe::{ProbeOptions, probe};
//!
//! let toolchain = probe(Path::new("/usr/bin/g++"), &ProbeOptions::default())?;
//! let settings = BuildSettings::default();
//! let extra = ExtraFlags::none();
//! let build = Path::new("/home/ada/.cache/blocks2cpp/builds/p/debug-0123abcd");
//! let language = Language { standard: CppStandard::Cpp20, gnu_extensions: false };
//! let inputs = BuildInputs::new(&toolchain, &settings.configurations.debug, language, build.join("gen"), &extra);
//! let plan = CommandPlan::new(&inputs).map_err(|d| std::io::Error::other(d.message))?;
//! let step = plan.compile_and_link(&build.join("gen/main.cpp"), &build.join("out/main"));
//!
//! let temp = tempfile::tempdir()?;
//! let env = compiler_env(toolchain.platform(), toolchain.bin_dir(), temp.path(), &HostEnv::from_process(&[]));
//! let command = step.process_command(&build.join("diag"), &env, None)?;
//! let captured = run_captured(&command)?;
//! let messages = step.read_diagnostics(&build.join("diag"), &captured.stderr);
//! println!("{} messages; exit {}", messages.messages.len(), captured.status);
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

// Every failure here is a user-facing `Diagnostic` (about 170 bytes), made
// once per failed step and shown to the user; boxing it would add noise
// without measurable benefit.
#![allow(clippy::result_large_err)]

pub mod codes;
pub mod command;
pub mod diagnostics;
pub mod discovery;
pub mod env;
pub mod fingerprint;
pub mod flags;
pub mod paths;
pub mod probe;
pub mod target;
