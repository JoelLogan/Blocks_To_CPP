//! Reading the Linux process table from `/proc`, for finding a child's
//! descendants and the members of a process group.

use std::collections::{HashMap, HashSet};
use std::fs;

/// The fields of `/proc/<pid>/stat` this crate needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ProcStat {
    pub(crate) pid: i32,
    pub(crate) ppid: i32,
    pub(crate) pgrp: i32,
}

/// Parses one `/proc/<pid>/stat` line: `pid (comm) state ppid pgrp …`. The
/// command name can contain spaces and parentheses, so the fields after it
/// are found from the *last* `)`.
pub(crate) fn parse_stat(line: &str) -> Option<ProcStat> {
    let open = line.find(" (")?;
    let pid = line.get(..open)?.trim().parse().ok()?;
    let close = line.rfind(')')?;
    let mut rest = line.get(close + 1..)?.split_ascii_whitespace();
    let _state = rest.next()?;
    let parent = rest.next()?.parse().ok()?;
    let group = rest.next()?.parse().ok()?;
    Some(ProcStat {
        pid,
        ppid: parent,
        pgrp: group,
    })
}

/// Every process currently visible in `/proc`. Processes that exit while the
/// table is read are skipped.
pub(crate) fn snapshot() -> Vec<ProcStat> {
    let Ok(entries) = fs::read_dir("/proc") else {
        return Vec::new();
    };
    entries
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .file_name()
                .to_str()
                .is_some_and(|name| name.bytes().all(|b| b.is_ascii_digit()))
        })
        .filter_map(|entry| fs::read_to_string(entry.path().join("stat")).ok())
        .filter_map(|line| parse_stat(&line))
        .collect()
}

/// The PIDs of every descendant of `root` in `table` (not `root` itself), in
/// breadth-first order.
pub(crate) fn descendants_in(root: i32, table: &[ProcStat]) -> Vec<i32> {
    let mut children: HashMap<i32, Vec<i32>> = HashMap::new();
    for entry in table {
        children.entry(entry.ppid).or_default().push(entry.pid);
    }
    let mut seen = HashSet::from([root]);
    let mut queue = vec![root];
    let mut found = Vec::new();
    while let Some(pid) = queue.pop() {
        for &child in children.get(&pid).map(Vec::as_slice).unwrap_or_default() {
            if seen.insert(child) {
                found.push(child);
                queue.push(child);
            }
        }
    }
    found
}

/// The PIDs of every live descendant of `root`.
pub(crate) fn descendants(root: i32) -> Vec<i32> {
    descendants_in(root, &snapshot())
}

/// The PIDs of the processes in process group `pgrp`.
pub(crate) fn group_members(pgrp: i32) -> Vec<i32> {
    snapshot()
        .iter()
        .filter(|entry| entry.pgrp == pgrp)
        .map(|entry| entry.pid)
        .collect()
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    #[test]
    fn parses_stat_lines() {
        assert_eq!(
            parse_stat("1234 (sleep) S 1200 1234 1200 34816 1234 4194304 98 0"),
            Some(ProcStat {
                pid: 1234,
                ppid: 1200,
                pgrp: 1234
            })
        );
        // Spaces and parentheses in the command name.
        assert_eq!(
            parse_stat("77 (a) b (c)) R 5 6 7"),
            Some(ProcStat {
                pid: 77,
                ppid: 5,
                pgrp: 6
            })
        );
        assert_eq!(parse_stat(""), None);
        assert_eq!(parse_stat("12 (x) S"), None);
        assert_eq!(parse_stat("x (y) S 1 2"), None);
    }

    #[test]
    fn finds_descendants_transitively() {
        let table = [
            ProcStat {
                pid: 10,
                ppid: 1,
                pgrp: 10,
            },
            ProcStat {
                pid: 11,
                ppid: 10,
                pgrp: 10,
            },
            ProcStat {
                pid: 12,
                ppid: 11,
                pgrp: 10,
            },
            ProcStat {
                pid: 13,
                ppid: 1,
                pgrp: 13,
            },
            ProcStat {
                pid: 14,
                ppid: 12,
                pgrp: 14,
            },
        ];
        let mut found = descendants_in(10, &table);
        found.sort_unstable();
        assert_eq!(found, [11, 12, 14]);
        assert!(descendants_in(13, &table).is_empty());
    }

    #[test]
    fn the_test_process_is_in_the_table() {
        let me = i32::try_from(std::process::id()).unwrap();
        assert!(snapshot().iter().any(|entry| entry.pid == me));
        let group = rustix::process::getpgrp().as_raw_pid();
        assert!(group_members(group).contains(&me));
    }

    proptest! {
        #[test]
        fn parse_never_panics(line in ".{0,200}") {
            let _ = parse_stat(&line);
        }

        #[test]
        fn cycles_terminate(edges in proptest::collection::vec((0_i32..20, 0_i32..20), 0..60)) {
            let table: Vec<ProcStat> = edges
                .iter()
                .map(|&(pid, ppid)| ProcStat { pid, ppid, pgrp: 0 })
                .collect();
            let found = descendants_in(0, &table);
            prop_assert!(found.len() <= 20);
        }
    }
}
