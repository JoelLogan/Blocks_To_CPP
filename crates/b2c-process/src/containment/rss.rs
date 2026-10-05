//! The RSS watchdog's measurement: the resident memory of a set of
//! processes, read from `/proc/<pid>/statm` (Linux).
//!
//! `statm`'s second field is the resident set size (`VmRSS`) in pages. The
//! sum over a tree counts pages shared between its processes (libraries,
//! shared memory) once per process, so it can only overestimate the tree's
//! real use; for a runaway compiler or program, whose memory is its own heap,
//! the difference is small.

use std::fs::File;
use std::io::Read as _;

/// Longest `statm` line read (seven decimal numbers).
const MAX_STATM: u64 = 512;

/// The resident set size in pages from one `/proc/<pid>/statm` line
/// (`size resident shared text lib data dt`).
pub(crate) fn parse_statm(line: &str) -> Option<u64> {
    let mut fields = line.split_ascii_whitespace();
    let _size: u64 = fields.next()?.parse().ok()?;
    fields.next()?.parse().ok()
}

/// The page size in bytes.
fn page_size() -> u64 {
    u64::try_from(rustix::param::page_size()).unwrap_or(4096)
}

/// The resident memory of process `pid` in bytes; 0 when it cannot be read
/// (the process has just exited).
fn resident_bytes(pid: i32, page: u64) -> u64 {
    let Ok(file) = File::open(format!("/proc/{pid}/statm")) else {
        return 0;
    };
    let mut line = String::new();
    if file.take(MAX_STATM).read_to_string(&mut line).is_err() {
        return 0;
    }
    parse_statm(&line).map_or(0, |pages| pages.saturating_mul(page))
}

/// The resident memory of all of `pids` together, in bytes.
pub(crate) fn total(pids: &[i32]) -> u64 {
    let page = page_size();
    pids.iter()
        .fold(0_u64, |sum, &pid| sum.saturating_add(resident_bytes(pid, page)))
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    #[test]
    fn statm_lines_are_parsed() {
        assert_eq!(parse_statm("1029 262 225 4 0 104 0\n"), Some(262));
        assert_eq!(parse_statm("1 2"), Some(2));
        assert_eq!(parse_statm(""), None);
        assert_eq!(parse_statm("1029"), None);
        assert_eq!(parse_statm("x 262"), None);
        assert_eq!(parse_statm("1029 -262"), None);
        assert_eq!(parse_statm("1029 99999999999999999999999"), None);
    }

    #[test]
    fn this_process_uses_some_memory() {
        let me = i32::try_from(std::process::id()).unwrap();
        assert!(total(&[me]) > 0);
        // A process that does not exist counts as nothing.
        assert_eq!(total(&[i32::MAX]), 0);
        assert!(total(&[me, i32::MAX]) > 0);
        assert_eq!(total(&[]), 0);
    }

    #[test]
    fn the_sum_grows_with_touched_memory() {
        let me = i32::try_from(std::process::id()).unwrap();
        let before = total(&[me]);
        // 64 MiB, every page written so it is resident.
        let block = vec![1_u8; 64 * 1024 * 1024];
        let after = total(&[me]);
        assert!(after >= before + 32 * 1024 * 1024, "{before} -> {after}");
        drop(block);
    }

    proptest! {
        #[test]
        fn parse_never_panics(line in ".{0,100}") {
            let _ = parse_statm(&line);
        }

        #[test]
        fn the_second_number_is_the_resident_size(size in any::<u64>(), resident in any::<u64>(), rest in "( [0-9]{1,5}){0,5}") {
            prop_assert_eq!(parse_statm(&format!("{size} {resident}{rest}\n")), Some(resident));
        }
    }
}
