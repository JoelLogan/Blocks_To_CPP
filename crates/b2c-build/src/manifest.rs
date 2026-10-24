//! `build-manifest.json`: what a build folder's executable was built from
//! (`docs/spec/07-toolchain-build-run.md` §7.5.1).
//!
//! ```json
//! { "format": "blocks2cpp/build-manifest", "formatVersion": 1,
//!   "projectHash": "<64 hex>",
//!   "toolchain": { "path": "/usr/bin/g++-13", "size": 1, "modifiedNs": 2,
//!                  "sha256": "<64 hex>", "version": "13.3.0", "target": "x86_64-linux-gnu" },
//!   "ide": true,
//!   "steps": [{ "kind": "compileAndLink", "argv": ["/usr/bin/g++-13", "…"] }],
//!   "executable": { "name": "main", "size": 17040, "sha256": "<64 hex>" },
//!   "result": "success" }
//! ```
//!
//! The manifest is written atomically ([`b2c_store::write_atomic`], `0600`)
//! only after a successful build, and deleted before a build compiles and
//! when a build fails or is cancelled, so a manifest always describes the
//! executable next to it. A build is up to date when the manifest records
//! exactly the inputs of the new build (project hash, toolchain fingerprint,
//! IDE flag, every step's argv and the executable's name), no generated file
//! changed, and the executable still has the recorded size and SHA-256.
//! `run_start` checks the executable against it again before a program
//! starts ([`crate::session::BuildRecord::verify_executable`]).
//!
//! It replaces M1's `build-stamp`, which an M2 build deletes.
//!
//! The file lives in the user's private cache, but it is still read as
//! untrusted: through no link, at most [`MAX_MANIFEST_BYTES`], with unknown
//! keys refused and every field checked.

use std::fs;
use std::io::{self, Read as _};
use std::path::Path;

use b2c_toolchain::command::CompilerCommand;
use b2c_toolchain::probe::Toolchain;
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

/// The manifest's file name in a build folder.
pub(crate) const MANIFEST_FILE: &str = "build-manifest.json";

/// M1's build stamp, which the manifest replaces.
pub(crate) const LEGACY_STAMP_FILE: &str = "build-stamp";

/// The `format` value.
const FORMAT: &str = "blocks2cpp/build-manifest";

/// The `formatVersion` value.
const FORMAT_VERSION: u32 = 1;

/// The only `result` value: manifests are written for successful builds.
const RESULT_SUCCESS: &str = "success";

/// The largest manifest read: generous for a few steps of a few hundred
/// arguments each.
pub(crate) const MAX_MANIFEST_BYTES: u64 = 1024 * 1024;

/// What kind of compiler step a manifest entry is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum StepKind {
    /// Compiled and linked in one invocation.
    CompileAndLink,
    /// Compiled one translation unit to an object.
    Compile,
    /// Linked objects into the executable.
    Link,
}

/// One compiler step: its kind and its whole argv (the program first).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ManifestStep {
    pub(crate) kind: StepKind,
    pub(crate) argv: Vec<String>,
}

impl ManifestStep {
    /// The entry for `command`. Paths that are not Unicode are written
    /// lossily; they only need to compare equal for the same build folder.
    pub(crate) fn new(kind: StepKind, command: &CompilerCommand) -> Self {
        let mut argv = Vec::with_capacity(command.args.len() + 1);
        argv.push(command.program.to_string_lossy().into_owned());
        argv.extend(command.args.iter().map(|arg| arg.to_string_lossy().into_owned()));
        Self { kind, argv }
    }
}

/// The toolchain's fingerprint, as recorded.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ManifestToolchain {
    pub(crate) path: String,
    pub(crate) size: u64,
    pub(crate) modified_ns: u64,
    pub(crate) sha256: String,
    pub(crate) version: Option<String>,
    pub(crate) target: String,
}

impl ManifestToolchain {
    /// The fingerprint of `toolchain`.
    pub(crate) fn new(toolchain: &Toolchain) -> Self {
        Self {
            path: toolchain.path().to_string_lossy().into_owned(),
            size: toolchain.fingerprint.size,
            modified_ns: toolchain.fingerprint.modified_ns,
            sha256: toolchain.fingerprint.sha256.clone(),
            version: toolchain.version.map(|version| version.to_string()),
            target: toolchain.target.triple.clone(),
        }
    }
}

/// The size and SHA-256 of a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FileDigest {
    pub(crate) size: u64,
    pub(crate) sha256: [u8; 32],
}

/// The executable, as recorded.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ManifestExecutable {
    /// Its file name in `out/`.
    pub(crate) name: String,
    pub(crate) size: u64,
    pub(crate) sha256: String,
}

impl ManifestExecutable {
    /// Whether `digest` is the recorded size and hash.
    pub(crate) fn matches(&self, digest: &FileDigest) -> bool {
        self.size == digest.size && self.sha256 == b2c_model::hex(&digest.sha256)
    }
}

/// A build manifest (see the module documentation).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct BuildManifest {
    pub(crate) format: String,
    pub(crate) format_version: u32,
    pub(crate) project_hash: String,
    pub(crate) toolchain: ManifestToolchain,
    pub(crate) ide: bool,
    pub(crate) steps: Vec<ManifestStep>,
    pub(crate) executable: ManifestExecutable,
    pub(crate) result: String,
}

/// Everything a manifest records about a build's inputs, before the
/// executable exists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ManifestInputs {
    pub(crate) project_hash: [u8; 32],
    pub(crate) toolchain: ManifestToolchain,
    pub(crate) ide: bool,
    pub(crate) steps: Vec<ManifestStep>,
    pub(crate) executable_name: String,
}

impl ManifestInputs {
    /// The manifest of a successful build of these inputs.
    pub(crate) fn finish(&self, executable: &FileDigest) -> BuildManifest {
        BuildManifest {
            format: FORMAT.to_owned(),
            format_version: FORMAT_VERSION,
            project_hash: b2c_model::hex(&self.project_hash),
            toolchain: self.toolchain.clone(),
            ide: self.ide,
            steps: self.steps.clone(),
            executable: ManifestExecutable {
                name: self.executable_name.clone(),
                size: executable.size,
                sha256: b2c_model::hex(&executable.sha256),
            },
            result: RESULT_SUCCESS.to_owned(),
        }
    }
}

impl BuildManifest {
    /// Whether this manifest records exactly `inputs`.
    pub(crate) fn records(&self, inputs: &ManifestInputs) -> bool {
        self.project_hash == b2c_model::hex(&inputs.project_hash)
            && self.toolchain == inputs.toolchain
            && self.ide == inputs.ide
            && self.steps == inputs.steps
            && self.executable.name == inputs.executable_name
    }

    /// Whether the fields that are not free text are well-formed.
    fn is_valid(&self) -> bool {
        self.format == FORMAT
            && self.format_version == FORMAT_VERSION
            && self.result == RESULT_SUCCESS
            && is_sha256_hex(&self.project_hash)
            && is_sha256_hex(&self.toolchain.sha256)
            && is_sha256_hex(&self.executable.sha256)
            && !self.steps.is_empty()
            && is_file_name(&self.executable.name)
    }
}

/// Whether `text` is 64 lower-case hex digits.
fn is_sha256_hex(text: &str) -> bool {
    text.len() == 64
        && text
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// Whether `name` is a plain file name (no separators, not `.` or `..`).
fn is_file_name(name: &str) -> bool {
    !name.is_empty() && name != "." && name != ".." && !name.contains(['/', '\\', ':', '\0'])
}

/// Reads the manifest in `build_dir`. `None` when there is none, or when it
/// is a link, too large, malformed or not a successful build's.
pub(crate) fn read(build_dir: &Path) -> Option<BuildManifest> {
    let path = build_dir.join(MANIFEST_FILE);
    let metadata = fs::symlink_metadata(&path).ok()?;
    if !metadata.is_file() {
        return None;
    }
    let bytes = b2c_store::read_bounded(&path, MAX_MANIFEST_BYTES).ok()?;
    let manifest: BuildManifest = serde_json::from_slice(&bytes).ok()?;
    manifest.is_valid().then_some(manifest)
}

/// Writes `manifest` into `build_dir` atomically (owner-only).
///
/// # Errors
/// When the file cannot be written (see [`b2c_store::write_atomic`]).
pub(crate) fn write(build_dir: &Path, manifest: &BuildManifest) -> Result<(), b2c_store::StoreError> {
    let mut json = serde_json::to_vec_pretty(manifest)
        .map_err(|_| b2c_store::StoreError::Invalid("the build manifest could not be serialised"))?;
    json.push(b'\n');
    b2c_store::write_atomic(&build_dir.join(MANIFEST_FILE), &json, b2c_store::Backup::None)
}

/// Deletes the manifest (and M1's build stamp) in `build_dir`, so the folder
/// no longer looks up to date.
///
/// # Errors
/// When a file exists but cannot be deleted.
pub(crate) fn remove(build_dir: &Path) -> io::Result<()> {
    for name in [MANIFEST_FILE, LEGACY_STAMP_FILE] {
        match fs::remove_file(build_dir.join(name)) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error),
            _ => {}
        }
    }
    Ok(())
}

/// The size and SHA-256 of the regular file at `path` (never through a
/// link), read in blocks.
///
/// # Errors
/// When the file is missing, a link or not a regular file, or cannot be
/// read.
pub(crate) fn digest_file(path: &Path) -> io::Result<FileDigest> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_file() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "not a regular file"));
    }
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 256 * 1024];
    let mut size: u64 = 0;
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        size = size.saturating_add(u64::try_from(read).unwrap_or(u64::MAX));
        hasher.update(buffer.get(..read).unwrap_or_default());
    }
    Ok(FileDigest {
        size,
        sha256: hasher.finalize().into(),
    })
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn inputs() -> ManifestInputs {
        ManifestInputs {
            project_hash: [0xab; 32],
            toolchain: ManifestToolchain {
                path: String::from("/usr/bin/g++-13"),
                size: 10,
                modified_ns: 20,
                sha256: "c".repeat(64),
                version: Some(String::from("13.3.0")),
                target: String::from("x86_64-linux-gnu"),
            },
            ide: true,
            steps: vec![ManifestStep {
                kind: StepKind::CompileAndLink,
                argv: vec![String::from("/usr/bin/g++-13"), String::from("main.cpp")],
            }],
            executable_name: String::from("main"),
        }
    }

    fn digest() -> FileDigest {
        FileDigest {
            size: 3,
            sha256: [0x01; 32],
        }
    }

    #[test]
    fn manifests_round_trip_in_the_documented_shape() {
        let folder = tempfile::tempdir().unwrap();
        let manifest = inputs().finish(&digest());
        write(folder.path(), &manifest).unwrap();
        assert_eq!(read(folder.path()), Some(manifest.clone()));
        let json: serde_json::Value =
            serde_json::from_slice(&fs::read(folder.path().join(MANIFEST_FILE)).unwrap()).unwrap();
        assert_eq!(json["format"], "blocks2cpp/build-manifest");
        assert_eq!(json["formatVersion"], 1);
        assert_eq!(json["projectHash"], "ab".repeat(32));
        assert_eq!(json["toolchain"]["modifiedNs"], 20);
        assert_eq!(json["ide"], true);
        assert_eq!(json["steps"][0]["kind"], "compileAndLink");
        assert_eq!(json["executable"]["name"], "main");
        assert_eq!(json["executable"]["size"], 3);
        assert_eq!(json["executable"]["sha256"], "01".repeat(32));
        assert_eq!(json["result"], "success");
        assert!(manifest.records(&inputs()));
        assert!(manifest.executable.matches(&digest()));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let mode = fs::metadata(folder.path().join(MANIFEST_FILE))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }

    #[test]
    fn every_recorded_input_counts() {
        let manifest = inputs().finish(&digest());
        let mut changed = inputs();
        changed.project_hash[0] = 0;
        assert!(!manifest.records(&changed));
        let mut changed = inputs();
        changed.toolchain.modified_ns += 1;
        assert!(!manifest.records(&changed));
        let mut changed = inputs();
        changed.ide = false;
        assert!(!manifest.records(&changed));
        let mut changed = inputs();
        changed.steps[0].argv.push(String::from("-O2"));
        assert!(!manifest.records(&changed));
        let mut changed = inputs();
        changed.steps[0].kind = StepKind::Compile;
        assert!(!manifest.records(&changed));
        let mut changed = inputs();
        changed.executable_name = String::from("other");
        assert!(!manifest.records(&changed));

        assert!(!manifest.executable.matches(&FileDigest { size: 4, ..digest() }));
        assert!(!manifest.executable.matches(&FileDigest {
            sha256: [0x02; 32],
            ..digest()
        }));
    }

    #[test]
    fn bad_manifests_are_not_read() {
        let folder = tempfile::tempdir().unwrap();
        let good = serde_json::to_value(inputs().finish(&digest())).unwrap();
        let cases: Vec<(&str, serde_json::Value)> = vec![
            ("format", serde_json::json!("other")),
            ("formatVersion", serde_json::json!(2)),
            ("result", serde_json::json!("failure")),
            ("projectHash", serde_json::json!("AB".repeat(32))),
            ("projectHash", serde_json::json!("ab")),
            ("steps", serde_json::json!([])),
        ];
        for (key, value) in cases {
            let mut bad = good.clone();
            bad[key] = value;
            fs::write(folder.path().join(MANIFEST_FILE), bad.to_string()).unwrap();
            assert_eq!(read(folder.path()), None, "{key}");
        }
        for name in ["", ".", "..", "a/b", "a\\b", "c:x"] {
            let mut bad = good.clone();
            bad["executable"]["name"] = serde_json::json!(name);
            fs::write(folder.path().join(MANIFEST_FILE), bad.to_string()).unwrap();
            assert_eq!(read(folder.path()), None, "{name}");
        }
        let mut bad = good.clone();
        bad["extra"] = serde_json::json!(1);
        fs::write(folder.path().join(MANIFEST_FILE), bad.to_string()).unwrap();
        assert_eq!(read(folder.path()), None);
        let mut bad = good.clone();
        bad["executable"]["sha256"] = serde_json::json!("0".repeat(63));
        fs::write(folder.path().join(MANIFEST_FILE), bad.to_string()).unwrap();
        assert_eq!(read(folder.path()), None);
        let mut bad = good.clone();
        bad["toolchain"]["sha256"] = serde_json::json!("g".repeat(64));
        fs::write(folder.path().join(MANIFEST_FILE), bad.to_string()).unwrap();
        assert_eq!(read(folder.path()), None);
        // Too large: refused without parsing.
        let padding = " ".repeat(usize::try_from(MAX_MANIFEST_BYTES).unwrap());
        fs::write(folder.path().join(MANIFEST_FILE), format!("{good}{padding}")).unwrap();
        assert_eq!(read(folder.path()), None);
        // A folder in its place.
        fs::remove_file(folder.path().join(MANIFEST_FILE)).unwrap();
        fs::create_dir(folder.path().join(MANIFEST_FILE)).unwrap();
        assert_eq!(read(folder.path()), None);
    }

    #[cfg(unix)]
    #[test]
    fn a_linked_manifest_is_never_read() {
        let folder = tempfile::tempdir().unwrap();
        let elsewhere = tempfile::tempdir().unwrap();
        write(elsewhere.path(), &inputs().finish(&digest())).unwrap();
        std::os::unix::fs::symlink(
            elsewhere.path().join(MANIFEST_FILE),
            folder.path().join(MANIFEST_FILE),
        )
        .unwrap();
        assert_eq!(read(folder.path()), None);
    }

    #[test]
    fn remove_deletes_the_manifest_and_the_old_stamp() {
        let folder = tempfile::tempdir().unwrap();
        remove(folder.path()).unwrap();
        write(folder.path(), &inputs().finish(&digest())).unwrap();
        fs::write(folder.path().join(LEGACY_STAMP_FILE), "stamp").unwrap();
        remove(folder.path()).unwrap();
        assert!(!folder.path().join(MANIFEST_FILE).exists());
        assert!(!folder.path().join(LEGACY_STAMP_FILE).exists());
        // Something that cannot be deleted as a file is an error.
        fs::create_dir(folder.path().join(MANIFEST_FILE)).unwrap();
        assert!(remove(folder.path()).is_err());
    }

    #[test]
    fn files_are_digested_in_full_and_never_through_links() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("program");
        let contents: Vec<u8> = (0..600_000_u32).map(|i| u8::try_from(i % 251).unwrap()).collect();
        fs::write(&path, &contents).unwrap();
        let digest = digest_file(&path).unwrap();
        assert_eq!(digest.size, 600_000);
        let expected: [u8; 32] = Sha256::digest(&contents).into();
        assert_eq!(digest.sha256, expected);
        assert!(digest_file(&folder.path().join("missing")).is_err());
        assert!(digest_file(folder.path()).is_err());
        #[cfg(unix)]
        {
            let link = folder.path().join("link");
            std::os::unix::fs::symlink(&path, &link).unwrap();
            assert_eq!(
                digest_file(&link).unwrap_err().kind(),
                io::ErrorKind::InvalidInput
            );
        }
    }

    #[test]
    fn steps_record_the_program_and_every_argument() {
        let command = CompilerCommand {
            program: PathBuf::from("/usr/bin/g++"),
            args: vec!["-c".into(), "main.cpp".into()],
            format: b2c_toolchain::probe::DiagnosticsFormat::Plain,
            sarif_files: Vec::new(),
        };
        let step = ManifestStep::new(StepKind::Compile, &command);
        assert_eq!(step.argv, ["/usr/bin/g++", "-c", "main.cpp"]);
        assert_eq!(step.kind, StepKind::Compile);
    }
}
