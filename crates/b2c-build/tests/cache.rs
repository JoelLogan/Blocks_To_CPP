//! Build cache eviction, pruning and clearing (`docs/spec/07-toolchain-build-run.md`
//! §7.5.1, `docs/spec/08-security.md` §8.6), on temporary cache roots with
//! fake build folders of known sizes and forged modification times.
//!
//! Headless on Linux; Windows CI runs the same tests, with junctions where
//! Linux uses symbolic links.

// Test helpers fail the test by panicking.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use b2c_build::cache::{
    CacheError, ClearReport, EvictionPolicy, EvictionReport, clear, evict_to_cap, evict_to_cap_keeping,
    hold_for_run, prune_and_evict, prune_and_evict_keeping, usage,
};
use proptest::prelude::*;
use tempfile::TempDir;

const DAY: Duration = Duration::from_hours(24);

/// A temporary cache root with helpers to plant fake build folders.
struct Cache {
    dir: TempDir,
    now: SystemTime,
}

impl Cache {
    fn new() -> Self {
        Self {
            dir: tempfile::tempdir().unwrap(),
            now: SystemTime::now(),
        }
    }

    fn root(&self) -> &Path {
        self.dir.path()
    }

    fn builds(&self) -> PathBuf {
        self.root().join("builds")
    }

    /// Plants `builds/<project>/<config>/` holding exactly `bytes` bytes (in
    /// `out/program`), plus an empty `gen/` folder and a `lock` file last
    /// touched `age` before `self.now`.
    fn entry(&self, project: &str, config: &str, bytes: usize, age: Duration) -> PathBuf {
        let entry = self.builds().join(project).join(config);
        fs::create_dir_all(entry.join("out")).unwrap();
        fs::create_dir_all(entry.join("gen")).unwrap();
        fs::write(entry.join("out").join("program"), vec![7u8; bytes]).unwrap();
        touch_lock(&entry, self.now - age);
        entry
    }

    fn policy(max_bytes: u64) -> EvictionPolicy {
        EvictionPolicy {
            max_bytes,
            max_age: 30 * DAY,
        }
    }
}

/// Sets the modification time of `entry`'s lock file (creating it).
fn touch_lock(entry: &Path, time: SystemTime) {
    let lock = File::options()
        .create(true)
        .truncate(false)
        .write(true)
        .open(entry.join("lock"))
        .unwrap();
    lock.set_modified(time).unwrap();
}

/// Holds `entry`'s lock exclusively, as a running build does.
fn lock_like_a_build(entry: &Path) -> File {
    let lock = File::options().write(true).open(entry.join("lock")).unwrap();
    lock.lock().unwrap();
    lock
}

fn modified(path: &Path) -> SystemTime {
    fs::symlink_metadata(path).unwrap().modified().unwrap()
}

fn exists(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok()
}

fn cache_error(error: &io::Error) -> Option<CacheError> {
    error.get_ref()?.downcast_ref::<CacheError>().copied()
}

/// A folder outside the cache that links must never lead to.
struct Outside {
    dir: TempDir,
}

impl Outside {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("sub")).unwrap();
        fs::write(dir.path().join("victim.txt"), vec![1u8; 2000]).unwrap();
        fs::write(dir.path().join("sub").join("inner.txt"), "keep me").unwrap();
        Self { dir }
    }

    fn path(&self) -> &Path {
        self.dir.path()
    }

    fn assert_untouched(&self) {
        assert_eq!(fs::read(self.path().join("victim.txt")).unwrap(), vec![1u8; 2000]);
        assert_eq!(
            fs::read_to_string(self.path().join("sub").join("inner.txt")).unwrap(),
            "keep me"
        );
    }
}

/// Plants a link to the folder `target` at `link`: a symbolic link on Unix,
/// a junction on Windows (which needs no privilege).
#[cfg(unix)]
fn link_dir(target: &Path, link: &Path) {
    std::os::unix::fs::symlink(target, link).unwrap();
}

/// Plants a link to the folder `target` at `link`: a symbolic link on Unix,
/// a junction on Windows (which needs no privilege).
#[cfg(windows)]
fn link_dir(target: &Path, link: &Path) {
    let system_root = std::env::var_os("SystemRoot").expect("SystemRoot is set on Windows");
    let cmd = Path::new(&system_root).join("System32").join("cmd.exe");
    let mut command = b2c_process::Command::new(cmd, target).unwrap();
    command
        .arg("/d")
        .arg("/c")
        .arg("mklink")
        .arg("/J")
        .arg(link)
        .arg(target)
        .env("SystemRoot", &system_root)
        .timeout(Duration::from_mins(1));
    let captured = b2c_process::run_captured(&command).unwrap();
    assert!(
        captured.status.success(),
        "mklink /J failed: {}",
        String::from_utf8_lossy(&captured.stderr)
    );
    assert!(fs::symlink_metadata(link).unwrap().file_type().is_symlink());
}

#[test]
fn over_the_cap_the_oldest_entries_go_first() {
    let cache = Cache::new();
    let five_days = cache.entry("prj_one-00000001", "debug-0000000a", 1000, 5 * DAY);
    let four_days = cache.entry("prj_one-00000001", "release-0000000b", 1000, 4 * DAY);
    let three_days = cache.entry("prj_two-00000002", "debug-0000000c", 1000, 3 * DAY);
    let two_days = cache.entry("prj_two-00000002", "debug-0000000d", 1000, 2 * DAY);
    let one_day = cache.entry("prj_two-00000002", "release-0000000e", 1000, DAY);
    assert_eq!(usage(cache.root()).unwrap(), 5000);

    let report = evict_to_cap(cache.root(), &Cache::policy(2500)).unwrap();
    assert_eq!(
        report,
        EvictionReport {
            removed: 3,
            freed_bytes: 3000,
            skipped_locked: 0,
        }
    );
    for gone in [&five_days, &four_days, &three_days] {
        assert!(!exists(gone), "{}", gone.display());
    }
    for kept in [&two_days, &one_day] {
        assert_eq!(fs::read(kept.join("out").join("program")).unwrap().len(), 1000);
    }
    assert_eq!(usage(cache.root()).unwrap(), 2000);
    // The project folder left empty is gone; the other one stays.
    assert!(!exists(&cache.builds().join("prj_one-00000001")));
    assert!(exists(&cache.builds().join("prj_two-00000002")));

    // Under the cap: nothing more to do.
    let again = evict_to_cap(cache.root(), &Cache::policy(2500)).unwrap();
    assert_eq!(again, EvictionReport::default());
}

#[test]
fn prune_and_evict_also_stops_at_the_cap() {
    let cache = Cache::new();
    let old = cache.entry("prj_a-00000001", "debug-00000001", 3000, 3 * DAY);
    let mid = cache.entry("prj_a-00000001", "debug-00000002", 3000, 2 * DAY);
    let new = cache.entry("prj_b-00000002", "debug-00000003", 3000, DAY);
    let report = prune_and_evict(cache.root(), &Cache::policy(6000), cache.now).unwrap();
    assert_eq!(report.removed, 1);
    assert_eq!(report.freed_bytes, 3000);
    assert!(!exists(&old));
    assert!(exists(&mid) && exists(&new));
}

#[test]
fn an_entry_31_days_old_is_pruned() {
    let cache = Cache::new();
    let old = cache.entry("prj_a-00000001", "debug-00000001", 1500, 31 * DAY);
    let recent = cache.entry("prj_a-00000001", "release-00000002", 700, 29 * DAY);
    let policy = EvictionPolicy::default();

    // Size eviction alone never prunes by age.
    assert_eq!(
        evict_to_cap(cache.root(), &policy).unwrap(),
        EvictionReport::default()
    );
    assert!(exists(&old));

    let report = prune_and_evict(cache.root(), &policy, cache.now).unwrap();
    assert_eq!(
        report,
        EvictionReport {
            removed: 1,
            freed_bytes: 1500,
            skipped_locked: 0,
        }
    );
    assert!(!exists(&old));
    assert!(exists(&recent));
    assert_eq!(usage(cache.root()).unwrap(), 700);
}

#[test]
fn a_locked_entry_is_kept() {
    let cache = Cache::new();
    let oldest = cache.entry("prj_a-00000001", "debug-00000001", 1000, 40 * DAY);
    let middle = cache.entry("prj_a-00000001", "debug-00000002", 1000, 10 * DAY);
    let newest = cache.entry("prj_b-00000002", "debug-00000003", 1000, DAY);
    let build = lock_like_a_build(&oldest);

    let report = prune_and_evict(cache.root(), &Cache::policy(1500), cache.now).unwrap();
    // The oldest is due twice over (age and size) but in use; the middle one
    // goes instead; the newest is never evicted for size.
    assert_eq!(
        report,
        EvictionReport {
            removed: 1,
            freed_bytes: 1000,
            skipped_locked: 1,
        }
    );
    assert_eq!(fs::read(oldest.join("out").join("program")).unwrap().len(), 1000);
    assert!(exists(&oldest.join("gen")));
    assert!(!exists(&middle));
    assert!(exists(&newest));

    drop(build);
    let report = prune_and_evict(cache.root(), &Cache::policy(1500), cache.now).unwrap();
    assert_eq!(report.removed, 1);
    assert!(!exists(&oldest));
    assert!(!exists(&cache.builds().join("prj_a-00000001")));
}

#[test]
fn the_most_recently_used_entry_is_never_evicted_for_size() {
    let cache = Cache::new();
    let older = cache.entry("prj_a-00000001", "debug-00000001", 4000, 2 * DAY);
    let just_built = cache.entry("prj_a-00000001", "debug-00000002", 9000, Duration::ZERO);
    let report = evict_to_cap(cache.root(), &Cache::policy(0)).unwrap();
    assert_eq!(report.removed, 1);
    assert!(!exists(&older));
    assert!(exists(&just_built.join("out").join("program")));
    // Still over the cap, but nothing more is evicted.
    assert_eq!(
        evict_to_cap(cache.root(), &Cache::policy(0)).unwrap(),
        EvictionReport::default()
    );
}

/// The eviction after a build names the build's folder to keep: even when
/// another entry was used after the build (another build, or a run), the
/// program that was just built survives. `keep` matches by canonical path,
/// and the kept entry still counts towards the size.
#[test]
fn the_entry_to_keep_survives_eviction_and_pruning() {
    let cache = Cache::new();
    let just_built = cache.entry("prj_a-00000001", "debug-00000001", 4000, Duration::from_secs(5));
    let older = cache.entry("prj_a-00000001", "debug-00000002", 1000, 2 * DAY);
    let used_since = cache.entry("prj_b-00000002", "debug-00000003", 1000, Duration::ZERO);
    // Spelled another way than the cache lists it.
    let spelled = just_built.join("out").join("..");
    let report = evict_to_cap_keeping(cache.root(), &Cache::policy(0), Some(&spelled)).unwrap();
    assert_eq!(report.removed, 1, "{report:?}");
    assert!(!exists(&older));
    assert!(exists(&just_built.join("out").join("program")));
    assert!(
        exists(&used_since.join("out").join("program")),
        "the newest is kept too"
    );
    // Without it, the build's folder would have gone.
    let report = evict_to_cap(cache.root(), &Cache::policy(0)).unwrap();
    assert_eq!(report.removed, 1);
    assert!(!exists(&just_built));

    // Pruning keeps it too, however old its lock looks; the rest goes.
    let cache = Cache::new();
    let kept = cache.entry("prj_a-00000001", "debug-00000001", 1000, 40 * DAY);
    let stale = cache.entry("prj_a-00000001", "debug-00000002", 1000, 40 * DAY);
    let report = prune_and_evict_keeping(cache.root(), &Cache::policy(0), cache.now, Some(&kept)).unwrap();
    assert_eq!(report.removed, 1, "{report:?}");
    assert!(!exists(&stale));
    assert!(exists(&kept.join("out").join("program")));
    // A path that is not an entry (or not there) keeps nothing extra.
    let report = prune_and_evict_keeping(
        cache.root(),
        &Cache::policy(0),
        cache.now,
        Some(&cache.root().join("missing")),
    )
    .unwrap();
    assert_eq!(report.removed, 1);
    assert!(!exists(&kept));
}

#[test]
fn links_inside_the_cache_are_never_followed() {
    let cache = Cache::new();
    let outside = Outside::new();
    let entry = cache.entry("prj_a-00000001", "debug-00000001", 1000, 3 * DAY);
    let newest = cache.entry("prj_a-00000001", "debug-00000002", 10, DAY);
    // A link inside an entry's output folder.
    link_dir(outside.path(), &entry.join("out").join("escape"));
    #[cfg(unix)]
    std::os::unix::fs::symlink(
        outside.path().join("victim.txt"),
        entry.join("gen").join("file-link"),
    )
    .unwrap();
    // A link where an entry would be, and one where a project folder would be.
    let entry_link = cache.builds().join("prj_a-00000001").join("debug-00000003");
    link_dir(outside.path(), &entry_link);
    let project_link = cache.builds().join("prj_linked-00000003");
    link_dir(outside.path(), &project_link);

    // Links are not measured.
    assert_eq!(usage(cache.root()).unwrap(), 1010);

    let report = evict_to_cap(cache.root(), &Cache::policy(0)).unwrap();
    assert_eq!(
        report,
        EvictionReport {
            removed: 1,
            freed_bytes: 1000,
            skipped_locked: 0,
        }
    );
    assert!(!exists(&entry));
    outside.assert_untouched();
    // Eviction leaves links that are not entries alone.
    assert!(exists(&entry_link) && exists(&project_link));

    let report = clear(cache.root()).unwrap();
    assert_eq!(
        report,
        ClearReport {
            freed_bytes: 10,
            skipped_in_use: 0,
        }
    );
    assert!(!exists(&newest));
    // Clearing removes the links themselves, never what they point to.
    assert!(!exists(&entry_link));
    assert!(!exists(&project_link));
    outside.assert_untouched();
    assert_eq!(fs::read_dir(cache.builds()).unwrap().count(), 0);
}

#[test]
fn a_builds_folder_or_cache_root_that_is_a_link_is_refused() {
    let cache = Cache::new();
    let outside = Outside::new();
    let fake_entry = outside.path().join("prj_a-00000001").join("debug-00000001");
    fs::create_dir_all(&fake_entry).unwrap();
    fs::write(fake_entry.join("lock"), "").unwrap();
    link_dir(outside.path(), &cache.builds());

    let check = |result: io::Result<()>| {
        let error = result.unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::NotADirectory);
        assert_eq!(
            cache_error(&error),
            Some(CacheError::NotAFolder("build cache folder"))
        );
        assert!(!error.to_string().contains(&*outside.path().to_string_lossy()));
    };
    check(usage(cache.root()).map(drop));
    check(clear(cache.root()).map(drop));
    check(evict_to_cap(cache.root(), &Cache::policy(0)).map(drop));
    check(prune_and_evict(cache.root(), &Cache::policy(0), cache.now + 365 * DAY).map(drop));
    outside.assert_untouched();
    assert!(exists(&fake_entry.join("lock")));

    let parent = Cache::new();
    let linked_root = parent.root().join("cache");
    link_dir(cache.root(), &linked_root);
    let error = clear(&linked_root).unwrap_err();
    assert_eq!(cache_error(&error), Some(CacheError::NotAFolder("cache folder")));
    outside.assert_untouched();
}

#[test]
fn clear_keeps_the_entry_in_use_and_reports_what_it_freed() {
    let cache = Cache::new();
    let first = cache.entry("prj_a-00000001", "debug-00000001", 1000, DAY);
    let busy = cache.entry("prj_a-00000001", "release-00000002", 2000, 2 * DAY);
    let other = cache.entry("prj_b-00000002", "debug-00000003", 3000, 3 * DAY);
    let build = lock_like_a_build(&busy);

    let report = clear(cache.root()).unwrap();
    assert_eq!(
        report,
        ClearReport {
            freed_bytes: 4000,
            skipped_in_use: 1,
        }
    );
    assert!(!exists(&first));
    assert!(!exists(&other));
    assert!(!exists(&cache.builds().join("prj_b-00000002")));
    assert_eq!(fs::read(busy.join("out").join("program")).unwrap().len(), 2000);
    assert!(exists(&busy.join("lock")));
    assert_eq!(usage(cache.root()).unwrap(), 2000);

    drop(build);
    let report = clear(cache.root()).unwrap();
    assert_eq!(
        report,
        ClearReport {
            freed_bytes: 2000,
            skipped_in_use: 0,
        }
    );
    assert!(exists(&cache.builds()));
    assert_eq!(fs::read_dir(cache.builds()).unwrap().count(), 0);
}

#[test]
fn sandbox_and_machine_files_survive_clear_and_eviction() {
    let cache = Cache::new();
    let sandbox = cache.root().join("sandbox").join("prj_a-00000001");
    fs::create_dir_all(&sandbox).unwrap();
    fs::write(sandbox.join("scores.txt"), "42").unwrap();
    for file in ["toolchains.json", "trust.json"] {
        fs::write(cache.root().join(file), "{}").unwrap();
    }
    fs::create_dir(cache.root().join("recovery")).unwrap();
    fs::write(cache.root().join("recovery").join("snapshot.b2c"), "{}").unwrap();
    cache.entry("prj_a-00000001", "debug-00000001", 100, 50 * DAY);
    cache.entry("prj_a-00000001", "debug-00000002", 100, DAY);

    let report = prune_and_evict(cache.root(), &Cache::policy(0), cache.now).unwrap();
    assert_eq!(report.removed, 1);
    let report = clear(cache.root()).unwrap();
    assert_eq!(report.freed_bytes, 100);

    assert_eq!(fs::read_to_string(sandbox.join("scores.txt")).unwrap(), "42");
    for file in ["toolchains.json", "trust.json"] {
        assert_eq!(fs::read_to_string(cache.root().join(file)).unwrap(), "{}");
    }
    assert!(exists(&cache.root().join("recovery").join("snapshot.b2c")));
    // The sandbox does not count towards the build cache.
    assert_eq!(usage(cache.root()).unwrap(), 0);
}

#[test]
fn stray_files_count_and_are_removed_only_by_clear() {
    let cache = Cache::new();
    fs::create_dir_all(cache.builds()).unwrap();
    fs::write(cache.builds().join("notes.txt"), vec![0u8; 100]).unwrap();
    let project = cache.builds().join("prj_a-00000001");
    fs::create_dir(&project).unwrap();
    fs::write(project.join("junk"), vec![0u8; 50]).unwrap();
    cache.entry("prj_a-00000001", "debug-00000001", 1000, DAY);
    assert_eq!(usage(cache.root()).unwrap(), 1150);

    // Not entries: eviction leaves them (and the newest entry) alone.
    assert_eq!(
        evict_to_cap(cache.root(), &Cache::policy(0)).unwrap(),
        EvictionReport::default()
    );
    let report = clear(cache.root()).unwrap();
    assert_eq!(
        report,
        ClearReport {
            freed_bytes: 1150,
            skipped_in_use: 0,
        }
    );
    assert_eq!(fs::read_dir(cache.builds()).unwrap().count(), 0);
}

#[test]
fn entries_without_a_lock_file_are_handled() {
    let cache = Cache::new();
    let aged = cache.entry("prj_a-00000001", "debug-00000001", 300, 10 * DAY);
    // No lock: recency is the folder's own modification time (just now).
    let unlocked = cache.builds().join("prj_a-00000001").join("debug-00000002");
    fs::create_dir_all(unlocked.join("out")).unwrap();
    fs::write(unlocked.join("out").join("program"), vec![0u8; 200]).unwrap();

    let report = evict_to_cap(cache.root(), &Cache::policy(0)).unwrap();
    assert_eq!(report.removed, 1);
    assert!(!exists(&aged));
    assert!(exists(&unlocked));

    let report = clear(cache.root()).unwrap();
    assert_eq!(report.freed_bytes, 200);
    assert!(!exists(&unlocked));
}

#[test]
fn a_lock_that_is_a_link_keeps_its_entry_and_its_target() {
    let cache = Cache::new();
    let outside = Outside::new();
    let entry = cache.builds().join("prj_a-00000001").join("debug-00000001");
    fs::create_dir_all(entry.join("out")).unwrap();
    fs::write(entry.join("out").join("program"), vec![0u8; 100]).unwrap();
    link_dir(&outside.path().join("sub"), &entry.join("lock"));

    let report = clear(cache.root()).unwrap();
    assert_eq!(
        report,
        ClearReport {
            freed_bytes: 0,
            skipped_in_use: 1,
        }
    );
    assert!(exists(&entry.join("out").join("program")));
    outside.assert_untouched();
    let error = hold_for_run(&entry).unwrap_err();
    assert_eq!(cache_error(&error), Some(CacheError::LockNotAFile));
}

#[test]
fn a_run_hold_marks_the_entry_used_and_protects_it() {
    let cache = Cache::new();
    let entry = cache.entry("prj_a-00000001", "debug-00000001", 500, 45 * DAY);
    let before = SystemTime::now();
    let hold = hold_for_run(&entry).unwrap();
    // Touched: no longer 45 days old.
    let touched = modified(&entry.join("lock"));
    assert!(
        touched >= before - Duration::from_secs(2),
        "{touched:?} < {before:?}"
    );
    // Shared: a second run of the same program may hold it too.
    let second = hold_for_run(&entry).unwrap();
    // A build cannot take the lock meanwhile.
    let build = File::options().write(true).open(entry.join("lock")).unwrap();
    assert!(build.try_lock().is_err());

    let report = prune_and_evict(cache.root(), &Cache::policy(0), cache.now + 60 * DAY).unwrap();
    assert_eq!(report.skipped_locked, 1);
    assert_eq!(clear(cache.root()).unwrap().skipped_in_use, 1);
    assert_eq!(fs::read(entry.join("out").join("program")).unwrap().len(), 500);

    drop(hold);
    drop(second);
    drop(build);
    let report = clear(cache.root()).unwrap();
    assert_eq!(
        report,
        ClearReport {
            freed_bytes: 500,
            skipped_in_use: 0,
        }
    );
    assert!(!exists(&entry));
}

#[test]
fn a_run_hold_waits_for_nothing_and_checks_its_input() {
    let cache = Cache::new();
    let entry = cache.entry("prj_a-00000001", "debug-00000001", 10, DAY);
    let build = lock_like_a_build(&entry);
    let error = hold_for_run(&entry).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::WouldBlock);
    assert_eq!(cache_error(&error), Some(CacheError::InUse));
    drop(build);

    let error = hold_for_run(Path::new("builds/prj/debug")).unwrap_err();
    assert_eq!(cache_error(&error), Some(CacheError::RelativePath));
    let error = hold_for_run(&entry.join("out").join("program")).unwrap_err();
    assert_eq!(cache_error(&error), Some(CacheError::NotAFolder("build folder")));
    let missing = hold_for_run(&entry.join("missing")).unwrap_err();
    assert_eq!(missing.kind(), io::ErrorKind::NotFound);

    // A folder without a lock file gets one.
    let fresh = cache.builds().join("prj_a-00000001").join("debug-00000002");
    fs::create_dir_all(&fresh).unwrap();
    let hold = hold_for_run(&fresh).unwrap();
    assert!(fs::symlink_metadata(fresh.join("lock")).unwrap().is_file());
    drop(hold);
}

#[test]
fn an_empty_missing_or_relative_cache() {
    let cache = Cache::new();
    let missing = cache.root().join("missing");
    assert_eq!(usage(&missing).unwrap(), 0);
    assert_eq!(clear(&missing).unwrap(), ClearReport::default());
    assert_eq!(
        prune_and_evict(&missing, &Cache::policy(0), cache.now).unwrap(),
        EvictionReport::default()
    );
    // A cache root without builds/.
    assert_eq!(usage(cache.root()).unwrap(), 0);
    assert_eq!(clear(cache.root()).unwrap(), ClearReport::default());
    assert_eq!(
        evict_to_cap(cache.root(), &Cache::policy(0)).unwrap(),
        EvictionReport::default()
    );
    let error = clear(Path::new("relative/cache")).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
    assert_eq!(cache_error(&error), Some(CacheError::RelativePath));
    // builds/ that is a file is refused.
    fs::write(cache.builds(), "").unwrap();
    let error = usage(cache.root()).unwrap_err();
    assert_eq!(
        cache_error(&error),
        Some(CacheError::NotAFolder("build cache folder"))
    );
}

#[test]
fn concurrent_clears_delete_everything_once() {
    let cache = Cache::new();
    let mut total = 0u64;
    for project in 0..4 {
        for config in 0..5 {
            let bytes = 100 * (project + 1) + config;
            cache.entry(
                &format!("prj_{project}-0000000{project}"),
                &format!("debug-0000000{config}"),
                bytes,
                DAY,
            );
            total += u64::try_from(bytes).unwrap();
        }
    }
    let reports: Vec<ClearReport> = std::thread::scope(|scope| {
        let workers: Vec<_> = (0..3)
            .map(|_| scope.spawn(|| clear(cache.root()).unwrap()))
            .collect();
        workers.into_iter().map(|worker| worker.join().unwrap()).collect()
    });
    let freed: u64 = reports.iter().map(|report| report.freed_bytes).sum();
    assert_eq!(freed, total);
    assert_eq!(fs::read_dir(cache.builds()).unwrap().count(), 0);
}

/// Ages in whole minutes; at least 30 days (43 200 minutes) is expired.
const EXPIRED_MINUTES: u64 = 30 * 24 * 60;

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]

    /// Whatever the sizes, ages and cap: expired entries are pruned; then the
    /// least recently used entries go, and only until the cache fits (or only
    /// the newest entry is left); the report matches what happened.
    #[test]
    fn eviction_keeps_the_most_recent_entries_that_fit(
        ages in prop::collection::btree_map(0u64..2 * EXPIRED_MINUTES, 0usize..3000, 1..8),
        cap in 0u64..12_000,
    ) {
        let cache = Cache::new();
        // Oldest first.
        let mut entries: Vec<(u64, u64, PathBuf)> = ages
            .iter()
            .rev()
            .enumerate()
            .map(|(index, (&minutes, &bytes))| {
                let project = format!("prj_{}-0000000{}", index % 2, index % 2);
                let config = format!("debug-0000000{index}");
                let path = cache.entry(&project, &config, bytes, Duration::from_secs(minutes * 60));
                (minutes, u64::try_from(bytes).unwrap(), path)
            })
            .collect();
        entries.sort_by_key(|(minutes, _, _)| std::cmp::Reverse(*minutes));
        let before = usage(cache.root()).unwrap();

        let report = prune_and_evict(cache.root(), &Cache::policy(cap), cache.now).unwrap();
        let after = usage(cache.root()).unwrap();
        prop_assert_eq!(report.freed_bytes, before - after);
        prop_assert_eq!(report.skipped_locked, 0);

        let kept: BTreeMap<u64, u64> = entries
            .iter()
            .filter(|(_, _, path)| exists(path))
            .map(|(minutes, bytes, _)| (*minutes, *bytes))
            .collect();
        prop_assert_eq!(report.removed, entries.len() - kept.len());
        prop_assert!(kept.keys().all(|&minutes| minutes < EXPIRED_MINUTES));
        let evicted: Vec<(u64, u64)> = entries
            .iter()
            .filter(|(minutes, _, path)| *minutes < EXPIRED_MINUTES && !exists(path))
            .map(|(minutes, bytes, _)| (*minutes, *bytes))
            .collect();
        // Least recently used first: everything evicted is older than
        // everything kept.
        if let (Some(youngest_evicted), Some(oldest_kept)) =
            (evicted.iter().map(|(minutes, _)| *minutes).min(), kept.keys().max())
        {
            prop_assert!(youngest_evicted > *oldest_kept);
        }
        // Over the cap only when nothing but the newest entry is left.
        prop_assert!(after <= cap || kept.len() <= 1);
        // No more than needed: without the last eviction it was over the cap.
        if let Some((_, bytes)) = evicted.iter().min_by_key(|(minutes, _)| *minutes) {
            prop_assert!(after + bytes > cap);
        }
    }
}
