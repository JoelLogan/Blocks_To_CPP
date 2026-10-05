//! What the toolchain setup page needs to know about this computer
//! (`docs/spec/04-user-interface.md` §4.6): on Linux, the distribution from
//! `os-release(5)`, so the page can suggest the right install command
//! (`apt`, `dnf` or `pacman`).
//!
//! The file is read with a size bound ([`MAX_OS_RELEASE_BYTES`]) and parsed
//! strictly: [`parse_os_release`] accepts only `KEY=value` lines, comments and
//! blank lines, with the quoting and escapes the format allows, and keeps
//! nothing but `ID` and `ID_LIKE`, whose values must be lower-case words of
//! `[a-z0-9._-]`. Anything else makes the whole file count as unreadable, and
//! the setup page then shows every install command. Nothing in the file is
//! ever run or used as a path.

use std::fs::File;
use std::io::{self, Read as _};
use std::path::Path;

/// The largest `os-release` file read (64 KiB). Real files are under 1 KiB.
pub const MAX_OS_RELEASE_BYTES: u64 = 64 * 1024;

/// Where `os-release` is looked for, in order: `/etc/os-release`, and only
/// when that does not exist, `/usr/lib/os-release` (as `os-release(5)` says).
pub const OS_RELEASE_PATHS: [&str; 2] = ["/etc/os-release", "/usr/lib/os-release"];

/// The longest distribution ID accepted.
const MAX_ID_BYTES: usize = 64;
/// The most `ID_LIKE` entries accepted.
const MAX_ID_LIKE: usize = 16;
/// The longest variable name accepted.
const MAX_KEY_BYTES: usize = 128;

/// The distribution, from `os-release`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OsRelease {
    /// `ID`, for example `ubuntu`; `linux` when the file has no `ID`, as
    /// `os-release(5)` says.
    pub id: String,
    /// `ID_LIKE`, split at spaces, for example `["debian"]`; empty when the
    /// file has no `ID_LIKE`.
    pub id_like: Vec<String>,
}

/// Parses the contents of an `os-release` file.
///
/// The format is a strict subset of shell variable assignments:
///
/// * the file is UTF-8 and at most [`MAX_OS_RELEASE_BYTES`] long;
/// * each line is blank, a comment (`#` first), or `KEY=value`, with spaces
///   and tabs around it ignored; `KEY` is `[A-Za-z_][A-Za-z0-9_]*`;
/// * a value is unquoted (letters, digits and `._-:/+,=@%~`, possibly
///   empty), or wholly in double quotes (where `\"`, `\\`, `` \` `` and `\$`
///   are the only escapes), or wholly in single quotes (no escapes);
/// * `ID` and `ID_LIKE` appear at most once each. `ID` is 1 to 64 characters
///   of `[a-z0-9._-]`; `ID_LIKE` is up to 16 such words separated by spaces.
///
/// Any other line makes the file malformed: the result is then `None`.
///
/// ```
/// use b2c_toolchain::host::parse_os_release;
///
/// let release = parse_os_release(b"NAME=\"Ubuntu\"\nID=ubuntu\nID_LIKE=debian\n").unwrap();
/// assert_eq!(release.id, "ubuntu");
/// assert_eq!(release.id_like, ["debian"]);
/// assert!(parse_os_release(b"ID=\"ubuntu\n").is_none()); // unterminated quote
/// ```
pub fn parse_os_release(bytes: &[u8]) -> Option<OsRelease> {
    if u64::try_from(bytes.len()).map_or(true, |len| len > MAX_OS_RELEASE_BYTES) {
        return None;
    }
    let text = std::str::from_utf8(bytes).ok()?;
    let mut id = None;
    let mut id_like = None;
    for line in text.split('\n') {
        let line = line.trim_matches([' ', '\t']);
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (key, raw) = line.split_once('=')?;
        if !is_key(key) {
            return None;
        }
        let value = unquote(raw)?;
        let slot = match key {
            "ID" => &mut id,
            "ID_LIKE" => &mut id_like,
            _ => continue,
        };
        // A second assignment would make the result depend on which one
        // wins; a file with one is not trusted at all.
        if slot.replace(value).is_some() {
            return None;
        }
    }
    let id = match id {
        None => String::from("linux"),
        Some(value) if is_id(&value) => value,
        Some(_) => return None,
    };
    let id_like: Vec<String> = id_like
        .as_deref()
        .unwrap_or_default()
        .split(' ')
        .filter(|word| !word.is_empty())
        .map(str::to_owned)
        .collect();
    if id_like.len() > MAX_ID_LIKE || !id_like.iter().all(|word| is_id(word)) {
        return None;
    }
    Some(OsRelease { id, id_like })
}

/// Reads this computer's distribution: `/etc/os-release`, or
/// `/usr/lib/os-release` when the first does not exist
/// ([`OS_RELEASE_PATHS`]). `None` on Windows, and when the file is missing,
/// unreadable, larger than [`MAX_OS_RELEASE_BYTES`] or malformed
/// ([`parse_os_release`]).
pub fn read_os_release() -> Option<OsRelease> {
    if cfg!(windows) {
        return None;
    }
    let paths = OS_RELEASE_PATHS.map(Path::new);
    read_os_release_in(&paths)
}

/// [`read_os_release`] with the candidate files given by the caller (for
/// tests): the first file that exists is read and parsed; later ones are
/// tried only while the earlier ones do not exist.
pub fn read_os_release_in(paths: &[&Path]) -> Option<OsRelease> {
    for path in paths {
        match read_limited(path, MAX_OS_RELEASE_BYTES) {
            Ok(bytes) => return parse_os_release(&bytes),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(_) => return None,
        }
    }
    None
}

/// Reads a regular file of at most `limit` bytes, reading no more than
/// `limit + 1` bytes whatever its size. The file is opened without blocking
/// (a FIFO in its place cannot hang the caller) and must be a regular file.
fn read_limited(path: &Path, limit: u64) -> io::Result<Vec<u8>> {
    let file: File = b2c_process::os::open_read_nonblocking(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "not a regular file"));
    }
    if metadata.len() > limit {
        return Err(io::ErrorKind::FileTooLarge.into());
    }
    let mut bytes = Vec::new();
    file.take(limit.saturating_add(1)).read_to_end(&mut bytes)?;
    if u64::try_from(bytes.len()).map_or(true, |len| len > limit) {
        return Err(io::ErrorKind::FileTooLarge.into());
    }
    Ok(bytes)
}

/// Whether `key` is a variable name: `[A-Za-z_][A-Za-z0-9_]*`, at most
/// [`MAX_KEY_BYTES`] long.
fn is_key(key: &str) -> bool {
    let mut bytes = key.bytes();
    key.len() <= MAX_KEY_BYTES
        && bytes
            .next()
            .is_some_and(|first| first.is_ascii_alphabetic() || first == b'_')
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

/// Whether `word` is a distribution ID: 1 to 64 characters of
/// `[a-z0-9._-]`.
fn is_id(word: &str) -> bool {
    !word.is_empty()
        && word.len() <= MAX_ID_BYTES
        && word.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'_' | b'-')
        })
}

/// The value of an assignment (see [`parse_os_release`] for the rules), or
/// `None` when it breaks them.
fn unquote(raw: &str) -> Option<String> {
    if let Some(rest) = raw.strip_prefix('"') {
        let mut value = String::new();
        let mut chars = rest.chars();
        loop {
            match chars.next()? {
                '"' => break,
                '\\' => match chars.next()? {
                    escaped @ ('"' | '\\' | '`' | '$') => value.push(escaped),
                    _ => return None,
                },
                // Unescaped, these would expand or end the string in a shell.
                '`' | '$' => return None,
                other => value.push(other),
            }
        }
        return chars.as_str().is_empty().then_some(value);
    }
    if let Some(rest) = raw.strip_prefix('\'') {
        let (value, after) = rest.split_once('\'')?;
        return after.is_empty().then(|| value.to_owned());
    }
    raw.bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || b"._-:/+,=@%~".contains(&byte))
        .then(|| raw.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> Option<OsRelease> {
        parse_os_release(text.as_bytes())
    }

    fn release(id: &str, like: &[&str]) -> OsRelease {
        OsRelease {
            id: id.to_owned(),
            id_like: like.iter().map(|word| (*word).to_owned()).collect(),
        }
    }

    #[test]
    fn values_may_be_quoted_in_every_allowed_way() {
        assert_eq!(
            parse("ID=ubuntu\nID_LIKE=debian"),
            Some(release("ubuntu", &["debian"]))
        );
        assert_eq!(
            parse("ID=\"rocky\"\nID_LIKE=\"rhel centos fedora\"\n"),
            Some(release("rocky", &["rhel", "centos", "fedora"]))
        );
        assert_eq!(
            parse("ID='opensuse-tumbleweed'\nID_LIKE='opensuse suse'"),
            Some(release("opensuse-tumbleweed", &["opensuse", "suse"]))
        );
        // Escapes inside double quotes, in keys that are not kept.
        assert_eq!(
            parse("NAME=\"A \\\"quoted\\\" \\\\ \\$ \\` name\"\nID=x_y.z-1"),
            Some(release("x_y.z-1", &[]))
        );
        // Surrounding blanks, comments, empty values and repeated spaces.
        assert_eq!(
            parse(
                "# comment\n\n  ID=arch \t\n\tID_LIKE=\"  a   b \"\nVERSION_CODENAME=\nSUPPORT_END=2025-05-13\n"
            ),
            Some(release("arch", &["a", "b"]))
        );
    }

    #[test]
    fn a_missing_id_means_linux() {
        assert_eq!(parse("NAME=Linux\n"), Some(release("linux", &[])));
        assert_eq!(parse(""), Some(release("linux", &[])));
        assert_eq!(parse("ID_LIKE=debian"), Some(release("linux", &["debian"])));
    }

    #[test]
    fn malformed_lines_reject_the_whole_file() {
        for text in [
            "ID=\"ubuntu",    // unterminated double quote
            "ID='ubuntu",     // unterminated single quote
            "ID=\"ubuntu\"x", // text after the closing quote
            "ID='ubuntu'x",
            "ID=ubu ntu",               // unquoted space
            "ID=$(reboot)",             // shell expansion
            "NAME=\"$(reboot)\"\nID=x", // expansion inside double quotes
            "NAME=\"`id`\"\nID=x",
            "NAME=\"\\n\"\nID=x", // an escape the format does not have
            "NAME=a\\ b\nID=x",   // backslash outside quotes
            "ID",                 // no '='
            "1D=ubuntu",          // bad key
            "I-D=ubuntu",
            "export ID=ubuntu",
            "ID=ubuntu\r\n",        // CR is not a blank here
            "ID=ubuntu\nID=debian", // a second assignment
            "ID_LIKE=a\nID_LIKE=b",
        ] {
            assert_eq!(parse(text), None, "{text:?}");
        }
        assert_eq!(parse_os_release(b"ID=ubuntu\n\xff\n"), None, "not UTF-8");
    }

    #[test]
    fn ids_must_be_lower_case_words() {
        for text in [
            "ID=Ubuntu",
            "ID=\"\"",
            "ID=\"ubuntu linux\"",
            "ID=ubuntu/24",
            "ID_LIKE=\"debian Ubuntu\"",
            "ID_LIKE=\"debian\tubuntu\"",
        ] {
            assert_eq!(parse(text), None, "{text:?}");
        }
        let longest = "a".repeat(MAX_ID_BYTES);
        assert_eq!(parse(&format!("ID={longest}")), Some(release(&longest, &[])));
        assert_eq!(parse(&format!("ID={longest}a")), None);
        let many: Vec<String> = (0..MAX_ID_LIKE).map(|n| format!("d{n}")).collect();
        let joined = many.join(" ");
        assert_eq!(
            parse(&format!("ID=x\nID_LIKE=\"{joined}\"")).map(|r| r.id_like),
            Some(many)
        );
        assert_eq!(parse(&format!("ID=x\nID_LIKE=\"{joined} more\"")), None);
    }

    #[test]
    fn keys_have_a_bounded_length() {
        let key = "K".repeat(MAX_KEY_BYTES);
        assert_eq!(parse(&format!("{key}=1\nID=x")), Some(release("x", &[])));
        assert_eq!(parse(&format!("{key}K=1\nID=x")), None);
    }

    #[test]
    fn oversized_input_is_refused() {
        let limit = usize::try_from(MAX_OS_RELEASE_BYTES).unwrap();
        let mut text = String::from("ID=ubuntu\n");
        text.push_str(&"#".repeat(limit - text.len()));
        assert_eq!(text.len(), limit);
        assert_eq!(parse(&text), Some(release("ubuntu", &[])));
        text.push('#');
        assert_eq!(parse(&text), None);
    }
}
