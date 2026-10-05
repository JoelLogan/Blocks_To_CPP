//! RFC 3339 timestamps in UTC (`savedAt`, `lastOpenedAt`, `grantedAt`), with
//! no time-zone database and no extra dependency.
//!
//! The writer always produces `YYYY-MM-DDTHH:MM:SS.mmmZ` (millisecond
//! precision, the form JavaScript's `Date.prototype.toISOString` uses). The
//! parser accepts any RFC 3339 `date-time` with a `Z` or numeric offset and
//! up to nine fractional digits.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Seconds in a day.
const DAY: i64 = 86_400;
/// Milliseconds from the epoch to 0000-01-01T00:00:00Z.
const MIN_MILLIS: i128 = -62_167_219_200_000;
/// Milliseconds from the epoch to 9999-12-31T23:59:59.999Z.
const MAX_MILLIS: i128 = 253_402_300_799_999;

/// `time` as an RFC 3339 UTC timestamp with milliseconds, such as
/// `2026-10-05T14:03:27.512Z`. Sub-millisecond parts are dropped (rounded
/// towards the past); times outside years 0000–9999 are clamped.
pub fn rfc3339_utc(time: SystemTime) -> String {
    let nanos: i128 = match time.duration_since(UNIX_EPOCH) {
        Ok(after) => i128::try_from(after.as_nanos()).unwrap_or(i128::MAX),
        Err(before) => i128::try_from(before.duration().as_nanos()).map_or(i128::MIN, |nanos| -nanos),
    };
    let millis = nanos.div_euclid(1_000_000).clamp(MIN_MILLIS, MAX_MILLIS);
    // In range after the clamp: |seconds| < 2^38.
    let seconds = i64::try_from(millis.div_euclid(1000)).unwrap_or(0);
    let milli = millis.rem_euclid(1000);
    let (year, month, day) = civil_from_days(seconds.div_euclid(DAY));
    let second_of_day = seconds.rem_euclid(DAY);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{milli:03}Z",
        second_of_day / 3600,
        second_of_day % 3600 / 60,
        second_of_day % 60
    )
}

/// Parses an RFC 3339 `date-time` (`2026-10-05T14:03:27Z`,
/// `2026-10-05T16:03:27.5+02:00`, ...), converting any offset to UTC.
///
/// Strict: the separators must be exactly `-`, `T` (or `t`), `:` and `.`;
/// every field must have its fixed number of digits and be in range
/// (including the day of the month); leap seconds (`:60`) and years outside
/// 0000–9999 are refused. Returns `None` for anything else.
pub fn parse_rfc3339_utc(text: &str) -> Option<SystemTime> {
    let bytes = text.as_bytes();
    if bytes.len() > 64 {
        return None;
    }
    let mut cursor = Cursor { bytes, at: 0 };
    let year = cursor.digits(4)?;
    cursor.expect(b"-")?;
    let month = cursor.digits(2)?;
    cursor.expect(b"-")?;
    let day = cursor.digits(2)?;
    cursor.expect(b"Tt")?;
    let hour = cursor.digits(2)?;
    cursor.expect(b":")?;
    let minute = cursor.digits(2)?;
    cursor.expect(b":")?;
    let second = cursor.digits(2)?;
    let mut nanos: i64 = 0;
    if cursor.peek() == Some(b'.') {
        cursor.at += 1;
        let start = cursor.at;
        while cursor.peek().is_some_and(|b| b.is_ascii_digit()) {
            cursor.at += 1;
        }
        let fraction = &bytes[start..cursor.at];
        if fraction.is_empty() || fraction.len() > 9 {
            return None;
        }
        for position in 0..9 {
            let digit = fraction.get(position).map_or(0, |b| i64::from(b - b'0'));
            nanos = nanos * 10 + digit;
        }
    }
    let offset_seconds = match cursor.next()? {
        b'Z' | b'z' => 0,
        sign @ (b'+' | b'-') => {
            let hours = cursor.digits(2)?;
            cursor.expect(b":")?;
            let minutes = cursor.digits(2)?;
            if hours > 23 || minutes > 59 {
                return None;
            }
            let offset = hours * 3600 + minutes * 60;
            if sign == b'+' { offset } else { -offset }
        }
        _ => return None,
    };
    if cursor.at != bytes.len()
        || !(1..=12).contains(&month)
        || day < 1
        || day > days_in_month(year, month)
        || hour > 23
        || minute > 59
        || second > 59
    {
        return None;
    }
    let seconds =
        days_from_civil(year, month, day) * DAY + hour * 3600 + minute * 60 + second - offset_seconds;
    let whole = Duration::from_secs(seconds.unsigned_abs());
    let fraction = Duration::from_nanos(u64::try_from(nanos).ok()?);
    if seconds >= 0 {
        UNIX_EPOCH.checked_add(whole)?.checked_add(fraction)
    } else {
        UNIX_EPOCH.checked_sub(whole)?.checked_add(fraction)
    }
}

/// A position in the text being parsed.
struct Cursor<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl Cursor<'_> {
    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.at).copied()
    }

    fn next(&mut self) -> Option<u8> {
        let byte = self.peek()?;
        self.at += 1;
        Some(byte)
    }

    /// One byte that must be one of `allowed`.
    fn expect(&mut self, allowed: &[u8]) -> Option<()> {
        let byte = self.next()?;
        allowed.contains(&byte).then_some(())
    }

    /// Exactly `count` ASCII digits as a number.
    fn digits(&mut self, count: usize) -> Option<i64> {
        let mut value = 0;
        for _ in 0..count {
            let byte = self.next()?;
            if !byte.is_ascii_digit() {
                return None;
            }
            value = value * 10 + i64::from(byte - b'0');
        }
        Some(value)
    }
}

/// Whether `year` is a Gregorian leap year.
fn is_leap(year: i64) -> bool {
    year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)
}

/// The number of days in `month` (1–12) of `year`.
fn days_in_month(year: i64, month: i64) -> i64 {
    match month {
        2 if is_leap(year) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// Days since 1970-01-01 of a proleptic Gregorian date (Howard Hinnant's
/// `days_from_civil`).
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year.rem_euclid(400);
    let month_index = if month > 2 { month - 3 } else { month + 9 };
    let day_of_year = (153 * month_index + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

/// The proleptic Gregorian date of a day count since 1970-01-01 (Howard
/// Hinnant's `civil_from_days`).
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let days = days + 719_468;
    let era = days.div_euclid(146_097);
    let day_of_era = days.rem_euclid(146_097);
    let year_of_era = (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    fn at(seconds: i64, nanos: u32) -> SystemTime {
        let base = if seconds >= 0 {
            UNIX_EPOCH + Duration::from_secs(seconds.unsigned_abs())
        } else {
            UNIX_EPOCH - Duration::from_secs(seconds.unsigned_abs())
        };
        base + Duration::from_nanos(u64::from(nanos))
    }

    #[test]
    fn formats_known_instants() {
        assert_eq!(rfc3339_utc(UNIX_EPOCH), "1970-01-01T00:00:00.000Z");
        assert_eq!(
            rfc3339_utc(at(1_791_209_007, 512_999_999)),
            "2026-10-05T14:03:27.512Z"
        );
        assert_eq!(rfc3339_utc(at(951_782_400, 0)), "2000-02-29T00:00:00.000Z");
        assert_eq!(rfc3339_utc(at(-1, 999_000_000)), "1969-12-31T23:59:59.999Z");
        assert_eq!(rfc3339_utc(at(-86_400, 0)), "1969-12-31T00:00:00.000Z");
    }

    #[test]
    fn clamps_to_four_digit_years() {
        assert_eq!(rfc3339_utc(at(300_000_000_000, 0)), "9999-12-31T23:59:59.999Z");
        assert_eq!(rfc3339_utc(at(-70_000_000_000, 0)), "0000-01-01T00:00:00.000Z");
    }

    #[test]
    fn parses_offsets_and_fractions() {
        let expected = at(1_791_209_007, 0);
        for text in [
            "2026-10-05T14:03:27Z",
            "2026-10-05t14:03:27z",
            "2026-10-05T16:03:27+02:00",
            "2026-10-05T09:33:27-04:30",
            "2026-10-05T14:03:27.000000000Z",
        ] {
            assert_eq!(parse_rfc3339_utc(text), Some(expected), "{text}");
        }
        assert_eq!(
            parse_rfc3339_utc("2026-10-05T14:03:27.5Z"),
            Some(at(1_791_209_007, 500_000_000))
        );
        assert_eq!(
            parse_rfc3339_utc("2026-10-05T14:03:27.123456789Z"),
            Some(at(1_791_209_007, 123_456_789))
        );
        assert_eq!(
            parse_rfc3339_utc("1969-12-31T23:59:59.250Z"),
            Some(at(-1, 250_000_000))
        );
        assert_eq!(
            parse_rfc3339_utc("0000-01-01T00:00:00Z"),
            Some(at(-62_167_219_200, 0))
        );
        assert_eq!(
            parse_rfc3339_utc("2024-02-29T00:00:00Z"),
            Some(at(1_709_164_800, 0))
        );
    }

    #[test]
    fn refuses_anything_else() {
        for text in [
            "",
            "2026-10-05",
            "2026-10-05T14:03Z",
            "2026-10-05T14:03:27",
            "2026-10-05 14:03:27Z",
            "2026-10-05T14:03:27.Z",
            "2026-10-05T14:03:27.1234567890Z",
            "2026-10-05T14:03:27+0200",
            "2026-10-05T14:03:27+24:00",
            "2026-10-05T14:03:27+02:60",
            "2026-13-05T14:03:27Z",
            "2026-00-05T14:03:27Z",
            "2026-10-00T14:03:27Z",
            "2026-10-32T14:03:27Z",
            "2026-02-29T00:00:00Z",
            "1900-02-29T00:00:00Z",
            "2026-10-05T24:00:00Z",
            "2026-10-05T14:60:00Z",
            "2026-10-05T23:59:60Z",
            "+2026-10-05T14:03:27Z",
            "20261-10-05T14:03:27Z",
            "2026-10-05T14:03:27Zjunk",
            "２０２６-10-05T14:03:27Z",
            "2026-1O-05T14:03:27Z",
        ] {
            assert_eq!(parse_rfc3339_utc(text), None, "{text:?}");
        }
        assert_eq!(
            parse_rfc3339_utc(&format!("2026-10-05T14:03:27.{}Z", "0".repeat(60))),
            None
        );
    }

    proptest! {
        /// Every millisecond instant in years 0000–9999 survives a round trip.
        #[test]
        fn round_trips(millis in MIN_MILLIS..=MAX_MILLIS) {
            let millis = i64::try_from(millis).unwrap();
            let time = at(millis.div_euclid(1000), u32::try_from(millis.rem_euclid(1000)).unwrap() * 1_000_000);
            let text = rfc3339_utc(time);
            prop_assert_eq!(text.len(), 24);
            prop_assert_eq!(parse_rfc3339_utc(&text), Some(time));
        }

        /// The calendar conversions are inverse to each other.
        #[test]
        fn calendar_round_trips(days in -800_000_i64..3_000_000) {
            let (year, month, day) = civil_from_days(days);
            prop_assert!((1..=12).contains(&month));
            prop_assert!(day >= 1 && day <= days_in_month(year, month));
            prop_assert_eq!(days_from_civil(year, month, day), days);
        }

        /// The parser never panics.
        #[test]
        fn parsing_never_panics(text in "\\PC{0,40}") {
            let _ = parse_rfc3339_utc(&text);
        }
    }
}
