//! Random opaque IDs (02 §2.5 "Opaque handles"): a short prefix such as
//! `rc_` followed by 32 lower-case hexadecimal digits, from 128 bits of the
//! operating system's cryptographically secure random number generator.

use std::io;

use crate::error::StoreError;

/// How many random bytes an ID carries (128 bits).
const RANDOM_BYTES: usize = 16;

/// A fresh ID: `prefix` followed by 32 lower-case hexadecimal digits from
/// 128 bits of OS randomness (`getrandom`).
///
/// # Errors
/// [`StoreError::Random`] when the operating system's random number
/// generator fails (never silently weaker randomness).
pub fn random_hex_id(prefix: &str) -> Result<String, StoreError> {
    let mut bytes = [0_u8; RANDOM_BYTES];
    getrandom::fill(&mut bytes).map_err(|error| StoreError::Random {
        source: error.raw_os_error().map_or_else(
            || io::Error::other(error.to_string()),
            io::Error::from_raw_os_error,
        ),
    })?;
    let mut id = String::with_capacity(prefix.len() + 2 * RANDOM_BYTES);
    id.push_str(prefix);
    id.push_str(&hex_lower(&bytes));
    Ok(id)
}

/// Whether `id` is `prefix` followed by exactly 32 lower-case hexadecimal
/// digits, the form [`random_hex_id`] produces.
pub fn is_hex_id(id: &str, prefix: &str) -> bool {
    id.strip_prefix(prefix)
        .is_some_and(|digits| is_lower_hex(digits, 2 * RANDOM_BYTES))
}

/// Whether `text` is exactly `len` lower-case hexadecimal digits.
pub(crate) fn is_lower_hex(text: &str, len: usize) -> bool {
    text.len() == len
        && text
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// `bytes` as lower-case hexadecimal.
pub(crate) fn hex_lower(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut text = String::with_capacity(2 * bytes.len());
    for &byte in bytes {
        text.push(char::from(DIGITS[usize::from(byte >> 4)]));
        text.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    text
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn ids_have_the_fixed_form_and_differ() {
        let ids: HashSet<String> = (0..100).map(|_| random_hex_id("rc_").unwrap()).collect();
        assert_eq!(ids.len(), 100);
        for id in &ids {
            assert_eq!(id.len(), 35);
            assert!(is_hex_id(id, "rc_"), "{id}");
            assert!(!is_hex_id(id, "sn_"));
        }
    }

    #[test]
    fn malformed_ids_are_recognised() {
        let good = format!("rc_{}", "0123456789abcdef".repeat(2));
        assert!(is_hex_id(&good, "rc_"));
        for bad in [
            String::new(),
            "rc_".to_owned(),
            format!("rc_{}", "0".repeat(31)),
            format!("rc_{}", "0".repeat(33)),
            format!("rc_{}", "A".repeat(32)),
            format!("rc_{}", "g".repeat(32)),
            format!("RC_{}", "0".repeat(32)),
            format!("rc_{}é", "0".repeat(31)),
        ] {
            assert!(!is_hex_id(&bad, "rc_"), "{bad}");
        }
    }

    #[test]
    fn hex_is_lower_case() {
        assert_eq!(hex_lower(&[0x00, 0x0f, 0xa5, 0xff]), "000fa5ff");
        assert_eq!(hex_lower(&[]), "");
    }
}
