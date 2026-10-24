//! Mark-of-the-Web: whether Windows recorded that a project file came from
//! the Internet (`docs/spec/08-security.md` §8.3 and §8.3.1; 10 §10.2).
//!
//! Browsers, mail clients and archive tools on Windows add an alternate data
//! stream named `Zone.Identifier` to the files they save, in INI form:
//!
//! ```text
//! [ZoneTransfer]
//! ZoneId=3
//! ReferrerUrl=https://example.com/
//! HostUrl=https://example.com/game.b2c
//! ```
//!
//! CONTRACT (milestone M2):
//! * [`mark_of_the_web`] reads at most [`MAX_ZONE_IDENTIFIER_BYTES`] (64 KiB)
//!   of `<path>:Zone.Identifier` and is `true` when [`zone_id`] finds a zone
//!   of 3 or higher (3 Internet, 4 Restricted sites). Zones 0–2 (this
//!   computer, the local intranet, trusted sites), a missing stream, an
//!   unreadable or oversized one, and any parse failure mean no mark.
//!   Elsewhere than on Windows it is always `false` and reads nothing.
//! * The mark never changes the trust state by itself (08 §8.3.1): the app
//!   adds it to the trust state it reports, and the native trust dialog
//!   shows a stronger warning for it.
//! * [`zone_id`] parses strictly: UTF-8 (with or without a byte order mark)
//!   or UTF-16LE with its byte order mark, no NUL characters, lines of
//!   `[section]`, `key=value`, blank lines and `;` or `#` comments only, no
//!   keys outside a section, and exactly one `ZoneId` in the `[ZoneTransfer]`
//!   section whose value is 1 to 10 ASCII digits. Section and key names are
//!   compared ignoring ASCII case, as Windows does. Any other content is a
//!   parse failure, never a guess.

use std::path::Path;

/// The most bytes of a `Zone.Identifier` stream that are read; a larger
/// stream is not parsed.
pub const MAX_ZONE_IDENTIFIER_BYTES: u64 = 64 * 1024;

/// The lowest zone that marks a file: 3, the Internet zone.
pub const INTERNET_ZONE: u32 = 3;

/// Whether the file at `path` carries the Mark-of-the-Web: its
/// `Zone.Identifier` stream names zone [`INTERNET_ZONE`] or higher. Always
/// `false` on other platforms than Windows, and whenever the stream is
/// missing, unreadable, larger than [`MAX_ZONE_IDENTIFIER_BYTES`] or not
/// parsed by [`zone_id`].
pub fn mark_of_the_web(path: &Path) -> bool {
    read_zone_identifier(path)
        .and_then(|stream| zone_id(&stream))
        .is_some_and(|zone| zone >= INTERNET_ZONE)
}

/// The contents of `<path>:Zone.Identifier`, if the stream exists and holds
/// at most [`MAX_ZONE_IDENTIFIER_BYTES`].
#[cfg(windows)]
fn read_zone_identifier(path: &Path) -> Option<Vec<u8>> {
    use std::io::Read as _;

    let mut stream_path = path.as_os_str().to_os_string();
    stream_path.push(":Zone.Identifier");
    let file = b2c_process::os::open_read_nonblocking(Path::new(&stream_path)).ok()?;
    let mut bytes = Vec::new();
    file.take(MAX_ZONE_IDENTIFIER_BYTES + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    u64::try_from(bytes.len())
        .is_ok_and(|len| len <= MAX_ZONE_IDENTIFIER_BYTES)
        .then_some(bytes)
}

/// Only Windows has `Zone.Identifier` streams.
#[cfg(not(windows))]
fn read_zone_identifier(_path: &Path) -> Option<Vec<u8>> {
    None
}

/// The `ZoneId` of a `Zone.Identifier` stream's contents, parsed strictly
/// (see the module documentation); `None` when there is none or the
/// contents are not valid.
pub fn zone_id(stream: &[u8]) -> Option<u32> {
    if !u64::try_from(stream.len()).is_ok_and(|len| len <= MAX_ZONE_IDENTIFIER_BYTES) {
        return None;
    }
    let text = decode(stream)?;
    if text.contains('\0') {
        return None;
    }
    let mut in_zone_transfer: Option<bool> = None;
    let mut zone = None;
    for line in text.split('\n') {
        let line = line.strip_suffix('\r').unwrap_or(line).trim_matches([' ', '\t']);
        if line.is_empty() || line.starts_with(';') || line.starts_with('#') {
            continue;
        }
        if let Some(name) = line.strip_prefix('[') {
            let name = name.strip_suffix(']')?;
            if name.is_empty() || name.contains(['[', ']']) {
                return None;
            }
            in_zone_transfer = Some(name.eq_ignore_ascii_case("ZoneTransfer"));
            continue;
        }
        let (key, value) = line.split_once('=')?;
        let key = key.trim_matches([' ', '\t']);
        if key.is_empty() {
            return None;
        }
        match in_zone_transfer {
            // A key before any section.
            None => return None,
            Some(true) if key.eq_ignore_ascii_case("ZoneId") => {
                if zone.is_some() {
                    // Two zones: ambiguous.
                    return None;
                }
                zone = Some(parse_zone(value.trim_matches([' ', '\t']))?);
            }
            Some(_) => {}
        }
    }
    zone
}

/// A zone number: 1 to 10 ASCII digits that fit in a `u32`.
fn parse_zone(value: &str) -> Option<u32> {
    if value.is_empty() || value.len() > 10 || !value.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    value.parse().ok()
}

/// The stream as text: UTF-16LE after its byte order mark, otherwise UTF-8
/// with an optional byte order mark. Invalid encodings are `None`.
fn decode(stream: &[u8]) -> Option<String> {
    if let Some(utf16) = stream.strip_prefix(&[0xFF, 0xFE]) {
        if utf16.len() % 2 != 0 {
            return None;
        }
        let units: Vec<u16> = utf16
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect();
        return String::from_utf16(&units).ok();
    }
    let utf8 = stream.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(stream);
    std::str::from_utf8(utf8).ok().map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utf16(text: &str) -> Vec<u8> {
        let mut bytes = vec![0xFF, 0xFE];
        for unit in text.encode_utf16() {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }
        bytes
    }

    #[test]
    fn zones_are_read_from_the_zone_transfer_section() {
        assert_eq!(zone_id(b"[ZoneTransfer]\r\nZoneId=3"), Some(3));
        assert_eq!(zone_id(b"[ZoneTransfer]\r\nZoneId=3\r\n"), Some(3));
        assert_eq!(zone_id(b"[ZoneTransfer]\nZoneId=2\n"), Some(2));
        assert_eq!(zone_id(b"[ZoneTransfer]\r\nZoneId=0"), Some(0));
        assert_eq!(zone_id(b"[ZoneTransfer]\r\nZoneId=4"), Some(4));
        let browser = b"[ZoneTransfer]\r\nZoneId=3\r\nReferrerUrl=https://example.com/a?b=c\r\nHostUrl=https://example.com/game.b2c\r\n";
        assert_eq!(zone_id(browser), Some(3));
        // Case, spacing, comments, blank lines and other sections.
        let relaxed = b"; written by a tool\r\n\r\n[zonetransfer]\r\n  zoneid = 4 \r\n# note\r\n[Other]\r\nZoneId=1\r\n";
        assert_eq!(zone_id(relaxed), Some(4));
        // Encodings.
        assert_eq!(zone_id(b"\xEF\xBB\xBF[ZoneTransfer]\r\nZoneId=3"), Some(3));
        assert_eq!(zone_id(&utf16("[ZoneTransfer]\r\nZoneId=3\r\n")), Some(3));
    }

    #[test]
    fn anything_unclear_is_no_zone() {
        for stream in [
            &b""[..],
            b"[ZoneTransfer]\r\n",
            b"ZoneId=3",
            b"[Other]\r\nZoneId=3",
            b"[ZoneTransfer]\r\nZoneId=",
            b"[ZoneTransfer]\r\nZoneId=3a",
            b"[ZoneTransfer]\r\nZoneId=-3",
            b"[ZoneTransfer]\r\nZoneId=+3",
            b"[ZoneTransfer]\r\nZoneId=0x3",
            b"[ZoneTransfer]\r\nZoneId=3 3",
            b"[ZoneTransfer]\r\nZoneId=99999999999",
            b"[ZoneTransfer]\r\nZoneId=4294967296",
            b"[ZoneTransfer]\r\nZoneId=3\r\nZoneId=3",
            b"[ZoneTransfer]\r\nZoneId=3\r\n[ZoneTransfer]\r\nZoneId=1",
            b"[ZoneTransfer]\r\nZoneId=3\r\njunk",
            b"[ZoneTransfer]\r\n=3\r\nZoneId=3",
            b"[ZoneTransfer\r\nZoneId=3",
            b"[]\r\nZoneId=3",
            b"[Zone[Transfer]]\r\nZoneId=3",
            b"[ZoneTransfer]\r\nZoneId=3\0",
            b"[ZoneTransfer]\r\nZoneId=\xff3",
            b"\xFF\xFE[\0Z\0",
            b"\xFF\xFE\0",
        ] {
            assert_eq!(zone_id(stream), None, "{}", String::from_utf8_lossy(stream));
        }
        assert_eq!(zone_id(&utf16("[ZoneTransfer]\r\nZoneId=3\u{0}")), None);
        // Unpaired surrogate.
        assert_eq!(zone_id(&[0xFF, 0xFE, 0x00, 0xD8]), None);
    }

    #[test]
    fn large_streams_are_not_parsed() {
        let mut stream = b"[ZoneTransfer]\r\nZoneId=3\r\n".to_vec();
        stream.resize(usize::try_from(MAX_ZONE_IDENTIFIER_BYTES).unwrap(), b'\n');
        assert_eq!(zone_id(&stream), Some(3));
        stream.push(b'\n');
        assert_eq!(zone_id(&stream), None);
    }

    /// Only Windows has the stream; elsewhere nothing is read, even a file
    /// whose name looks like one.
    #[cfg(not(windows))]
    #[test]
    fn other_platforms_have_no_mark() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("game.b2c");
        std::fs::write(&file, b"{}").unwrap();
        std::fs::write(
            dir.path().join("game.b2c:Zone.Identifier"),
            b"[ZoneTransfer]\r\nZoneId=3",
        )
        .unwrap();
        assert!(!mark_of_the_web(&file));
        assert!(!mark_of_the_web(&dir.path().join("missing.b2c")));
    }
}
