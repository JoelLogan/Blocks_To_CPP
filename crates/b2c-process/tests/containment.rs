//! Containment on Linux (`docs/spec/07-toolchain-build-run.md` §7.5.2,
//! §7.6.2; `docs/spec/08-security.md` §8.14 item 3): the RSS watchdog, the
//! process cap, `RLIMIT_AS` never applied to programs, and cgroup v2 scopes.
//!
//! * **Fallback tests** force [`Containment::ProcessGroupOnly`], so they run
//!   everywhere (here, in the gcc containers, on every CI runner), headless.
//! * **cgroup tests** need a user systemd instance with cgroup v2
//!   ([`containment_level`] is [`ContainmentLevel::Cgroup`]). Elsewhere they
//!   are skipped with a logged reason, unless `B2C_REQUIRE_CGROUP` is set (as
//!   in CI's `test-cgroup` job), when they fail instead.
//!
//! The memory hog is this test binary itself, run again with
//! `B2C_TEST_HOG_MIB` set (see [`memory_hog_helper`]).
#![cfg(target_os = "linux")]
// Helper functions outside `#[test]`s fail the test by panicking.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::print_stderr)]

use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use b2c_process::{
    Captured, Command, Containment, ContainmentLevel, ExitStatus, Limits, PtyChild, Stdin,
    cleanup_stale_scopes, containment_level, run_captured, spawn_piped,
};

const MIB: u64 = 1024 * 1024;
/// Generous bound for things that should take milliseconds, for loaded CI
/// runners.
const PATIENCE: Duration = Duration::from_secs(20);
const SETSID: &str = "/usr/bin/setsid";

fn sh(script: &str, containment: Containment) -> Command {
    let mut command = Command::new("/bin/sh", std::env::temp_dir()).unwrap();
    command
        .args(["-c", script])
        .env("PATH", "/usr/bin:/bin")
        .containment(containment);
    command
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// This test binary, run as the memory hog: it touches `mib` MiB, one MiB
/// at a time, and then sleeps.
fn hog(mib: u64, containment: Containment) -> Command {
    let exe = std::env::current_exe().unwrap();
    let mut command = Command::new(&exe, std::env::temp_dir()).unwrap();
    command
        .args(["--exact", "memory_hog_helper", "--nocapture", "--test-threads=1"])
        .env("B2C_TEST_HOG_MIB", mib.to_string())
        .containment(containment);
    command
}

/// Not a test on its own: does nothing unless `B2C_TEST_HOG_MIB` is set, in
/// which case this process is the memory hog of the tests below.
#[test]
fn memory_hog_helper() {
    let Some(mib) = std::env::var("B2C_TEST_HOG_MIB")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
    else {
        return;
    };
    let mut blocks = Vec::with_capacity(mib);
    for _ in 0..mib {
        // Filled with a non-zero byte, so every page is really resident.
        blocks.push(vec![0x5a_u8; 1024 * 1024]);
        thread::sleep(Duration::from_millis(1));
    }
    println!("allocated {mib} MiB");
    thread::sleep(Duration::from_mins(1));
    assert_eq!(blocks.len(), mib);
}

/// Whether cgroup v2 scopes are available; logs why a test is skipped when
/// they are not, and fails when `B2C_REQUIRE_CGROUP` is set.
fn cgroups(test: &str) -> bool {
    let level = containment_level();
    if level == ContainmentLevel::Cgroup {
        return true;
    }
    assert!(
        std::env::var_os("B2C_REQUIRE_CGROUP").is_none(),
        "B2C_REQUIRE_CGROUP is set but the containment level is {level:?}: \
         no user systemd instance with cgroup v2 (see .github/workflows/ci.yml, test-cgroup)"
    );
    eprintln!(
        "skipped {test}: no cgroup v2 user scope here (containment level {level:?}); \
         set B2C_REQUIRE_CGROUP=1 to require it"
    );
    false
}

/// Whether a process is gone: it no longer exists, or it is a zombie
/// waiting for a (possibly non-reaping) init to collect it.
fn is_gone(pid: i32) -> bool {
    let until = Instant::now() + Duration::from_secs(5);
    loop {
        if is_gone_now(pid) {
            return true;
        }
        if Instant::now() > until {
            return false;
        }
        thread::sleep(Duration::from_millis(10));
    }
}

fn is_gone_now(pid: i32) -> bool {
    std::fs::read_to_string(format!("/proc/{pid}/stat")).map_or(true, |stat| {
        stat.rsplit_once(')')
            .and_then(|(_, after)| after.split_whitespace().next())
            .is_none_or(|state| state == "Z" || state == "X")
    })
}

fn kill_pid(pid: i32) {
    if let Some(pid) = rustix::process::Pid::from_raw(pid) {
        let _ = rustix::process::kill_process(pid, rustix::process::Signal::KILL);
    }
}

/// The first number after `label` in `text`.
fn number_after(text: &str, label: &str) -> i32 {
    let start = text
        .find(label)
        .unwrap_or_else(|| panic!("{label:?} not in {text:?}"))
        + label.len();
    text[start..]
        .chars()
        .take_while(char::is_ascii_digit)
        .collect::<String>()
        .parse()
        .unwrap()
}

/// Collects a session's output on a thread of its own.
struct Output(Arc<Mutex<Vec<u8>>>);

impl Output {
    fn start(child: &mut PtyChild) -> Self {
        let mut reader = child.take_reader().unwrap();
        let data = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&data);
        thread::spawn(move || {
            let mut buffer = [0_u8; 4096];
            while let Ok(read @ 1..) = reader.read(&mut buffer) {
                sink.lock().unwrap().extend_from_slice(&buffer[..read]);
            }
        });
        Self(data)
    }

    fn text(&self) -> String {
        text(&self.0.lock().unwrap())
    }

    fn wait_for(&self, needle: &str) -> String {
        let until = Instant::now() + PATIENCE;
        loop {
            let text = self.text();
            if text.contains(needle) {
                return text;
            }
            assert!(Instant::now() < until, "{needle:?} never appeared in {text:?}");
            thread::sleep(Duration::from_millis(10));
        }
    }
}

/// A script that starts a sleeper which double-forks and leaves the session
/// (`setsid`), waits until it has, prints `escaped <pid>`, then runs `rest`.
fn double_fork_then(rest: &str) -> String {
    format!(
        "( {SETSID} /bin/sleep 600 </dev/null >/dev/null 2>&1 & p=$!; \
           while [ \"$(cut -d' ' -f6 /proc/$p/stat)\" != \"$p\" ]; do sleep 0.01; done; \
           echo \"escaped $p\" ); {rest}"
    )
}

// ---------------------------------------------------------------------------
// Fallback: process group, RLIMIT_AS for the compiler, RSS watchdog
// ---------------------------------------------------------------------------

#[test]
fn the_rss_watchdog_stops_a_memory_hog() {
    let mut command = hog(512, Containment::ProcessGroupOnly);
    command.limits(Limits {
        rss_limit: Some(256 * MIB),
        timeout: Some(Duration::from_mins(1)),
        ..Limits::default()
    });
    let start = Instant::now();
    let result = run_captured(&command).unwrap();
    assert!(result.out_of_memory, "{result:?}");
    assert!(!result.timed_out && !result.cancelled && !result.too_many_processes);
    assert_eq!(result.status, ExitStatus::Signaled(9));
    assert!(!text(&result.stdout).contains("allocated"), "{result:?}");
    assert!(start.elapsed() < Duration::from_secs(30), "{:?}", start.elapsed());
}

#[test]
fn the_rss_watchdog_adds_up_the_whole_group() {
    // Two processes of 160 MiB: each is under the 256 MiB cap, both
    // together are not.
    let exe = std::env::current_exe().unwrap();
    let mut command = sh(
        "\"$1\" --exact memory_hog_helper --nocapture & \"$1\" --exact memory_hog_helper --nocapture; wait",
        Containment::ProcessGroupOnly,
    );
    command
        .arg("sh")
        .arg(&exe)
        .env("B2C_TEST_HOG_MIB", "160")
        .limits(Limits {
            rss_limit: Some(256 * MIB),
            timeout: Some(Duration::from_mins(1)),
            ..Limits::default()
        });
    let result = run_captured(&command).unwrap();
    assert!(result.out_of_memory, "{result:?}");
    assert!(!result.timed_out);
}

#[test]
fn a_hog_under_its_cap_is_not_out_of_memory() {
    let mut command = hog(32, Containment::ProcessGroupOnly);
    command.limits(Limits {
        rss_limit: Some(256 * MIB),
        timeout: Some(Duration::from_millis(1500)),
        ..Limits::default()
    });
    let result = run_captured(&command).unwrap();
    assert!(result.timed_out, "{result:?}");
    assert!(!result.out_of_memory);
    assert!(text(&result.stdout).contains("allocated 32 MiB"));
    // And an ordinary run never is.
    let plain = run_captured(&sh("exit 3", Containment::Auto)).unwrap();
    assert_eq!(plain.status, ExitStatus::Exited(3));
    assert!(!plain.out_of_memory && !plain.too_many_processes);
}

#[test]
fn a_run_cap_never_becomes_an_address_space_limit() {
    for containment in [Containment::ProcessGroupOnly, Containment::Auto] {
        let mut command = sh("ulimit -v", containment);
        command.limits(Limits {
            rss_limit: Some(256 * MIB),
            ..Limits::default()
        });
        let result = run_captured(&command).unwrap();
        assert_eq!(text(&result.stdout).trim(), "unlimited", "{containment:?}");
    }
}

#[test]
fn a_session_over_its_rss_limit_is_out_of_memory() {
    let mut command = hog(512, Containment::ProcessGroupOnly);
    command.limits(Limits {
        rss_limit: Some(256 * MIB),
        timeout: Some(Duration::from_mins(1)),
        ..Limits::default()
    });
    let mut child = spawn_piped(&command).unwrap();
    assert_eq!(child.containment(), ContainmentLevel::ProcessGroupOnly);
    let _output = Output::start(&mut child);
    let exit = child.wait().unwrap();
    assert!(exit.out_of_memory, "{exit:?}");
    assert!(!exit.stopped && !exit.timed_out && !exit.too_many_processes);
    assert_eq!(exit.status, ExitStatus::Signaled(9));
}

/// A run that keeps starting sleepers, far more than `cap`.
fn fork_bomb(cap: u32, containment: Containment) -> (Captured, Duration) {
    let mut command = sh(
        "i=0; while [ $i -lt 300 ]; do sleep 60 & i=$((i+1)); done; wait",
        containment,
    );
    command.limits(Limits {
        processes: Some(cap),
        timeout: Some(Duration::from_mins(1)),
        ..Limits::default()
    });
    let start = Instant::now();
    let result = run_captured(&command).unwrap();
    (result, start.elapsed())
}

#[test]
fn a_fork_bomb_stops_at_the_process_cap() {
    let (result, elapsed) = fork_bomb(32, Containment::ProcessGroupOnly);
    assert!(result.too_many_processes, "{result:?}");
    assert!(!result.timed_out && !result.out_of_memory);
    assert!(elapsed < Duration::from_secs(30), "{elapsed:?}");
}

#[test]
fn without_a_scope_a_double_forked_sleeper_survives_stop() {
    if !Path::new(SETSID).exists() {
        eprintln!("skipped: {SETSID} is not installed");
        return;
    }
    let mut child = spawn_piped(&sh(
        &double_fork_then("exec sleep 600"),
        Containment::ProcessGroupOnly,
    ))
    .unwrap();
    assert_eq!(child.containment(), ContainmentLevel::ProcessGroupOnly);
    let output = Output::start(&mut child);
    let escaped = number_after(&output.wait_for("escaped "), "escaped ");
    child.stop();
    let exit = child.wait().unwrap();
    assert!(exit.stopped);
    // Documented (08 §8.14 item 3): only a cgroup scope can reach it.
    let survived = !is_gone_now(escaped);
    kill_pid(escaped);
    assert!(survived, "the escaped sleeper should have outlived Stop");
}

/// The path of a g++ on `PATH`; panics when there is none but
/// `B2C_REQUIRE_GXX` is set.
fn gxx() -> Option<PathBuf> {
    let found = std::env::var_os("PATH").and_then(|path| {
        std::env::split_paths(&path)
            .map(|dir| dir.join("g++"))
            .find(|candidate| candidate.is_file())
    });
    assert!(
        found.is_some() || std::env::var_os("B2C_REQUIRE_GXX").is_none(),
        "B2C_REQUIRE_GXX is set but g++ was not found"
    );
    found
}

#[test]
fn address_sanitizer_programs_run_under_an_rss_cap() {
    let Some(gxx) = gxx() else {
        eprintln!("skipped: no g++ on PATH");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("asan.cpp"),
        "#include <cstdio>\nint main() { std::puts(\"asan ok\"); return 0; }\n",
    )
    .unwrap();
    let mut compile = Command::new(&gxx, dir.path()).unwrap();
    compile
        .args(["-fsanitize=address", "-g", "asan.cpp", "-o", "asan"])
        .env(
            "PATH",
            format!("{}:/usr/bin:/bin", gxx.parent().unwrap().display()),
        )
        .env("LC_ALL", "C")
        .limits(Limits {
            timeout: Some(Duration::from_mins(2)),
            ..Limits::default()
        });
    if let Some(home) = std::env::var_os("HOME") {
        compile.env("HOME", home);
    }
    let compiled = run_captured(&compile).unwrap();
    if !compiled.status.success() {
        eprintln!(
            "skipped: g++ cannot build AddressSanitizer programs here: {}",
            text(&compiled.stderr)
        );
        return;
    }
    let program = |limits: Limits| {
        let mut run = Command::new(dir.path().join("asan"), dir.path()).unwrap();
        run.env("ASAN_OPTIONS", "detect_leaks=0")
            .stdin(Stdin::Null)
            .containment(Containment::ProcessGroupOnly)
            .limits(Limits {
                timeout: Some(Duration::from_mins(1)),
                ..limits
            });
        run_captured(&run).unwrap()
    };
    // Older sanitizer runtimes crash now and then under the large address
    // space randomisation of recent kernels, whatever the limits; a few
    // attempts tell that apart from a limit that always breaks them.
    let first_success = |limits: Limits| {
        let mut last = None;
        for _ in 0..3 {
            let run = program(limits.clone());
            if run.status.success() {
                return Ok(run);
            }
            last = Some(run);
        }
        Err(last.unwrap())
    };
    if let Err(free) = first_success(Limits::default()) {
        eprintln!(
            "skipped: AddressSanitizer programs do not run here at all: {}",
            text(&free.stderr)
        );
        return;
    }
    let capped = first_success(Limits {
        rss_limit: Some(256 * MIB),
        ..Limits::default()
    })
    .unwrap_or_else(|run| panic!("an RSS cap broke the AddressSanitizer program: {run:?}"));
    assert_eq!(text(&capped.stdout), "asan ok\n");
    assert!(!capped.out_of_memory);
    // Why the run cap must not be an address-space limit: AddressSanitizer
    // reserves terabytes of address space for its shadow memory.
    let address_space = program(Limits {
        memory: Some(256 * MIB),
        ..Limits::default()
    });
    assert!(!address_space.status.success(), "{address_space:?}");
}

// ---------------------------------------------------------------------------
// cgroup v2 scopes (the test-cgroup job)
// ---------------------------------------------------------------------------

#[test]
fn scopes_are_named_after_this_process() {
    if !cgroups("scopes_are_named_after_this_process") {
        return;
    }
    let me = std::process::id();
    let check = |cgroup: &str, word: &str| {
        let line = cgroup
            .lines()
            .find_map(|line| line.strip_prefix("0::"))
            .unwrap_or_else(|| panic!("no cgroup v2 line in {cgroup:?}"));
        let unit = line.rsplit('/').next().unwrap();
        let prefix = format!("b2c-{word}-{me}-");
        let hex = unit
            .strip_prefix(&prefix)
            .and_then(|rest| rest.strip_suffix(".scope"))
            .unwrap_or_else(|| panic!("{unit:?} is not a {prefix}… scope"));
        assert!(
            hex.len() == 16 && hex.bytes().all(|b| b.is_ascii_hexdigit()),
            "{unit:?}"
        );
    };
    let captured = run_captured(&sh("cat /proc/self/cgroup", Containment::Auto)).unwrap();
    assert!(captured.status.success(), "{captured:?}");
    check(&text(&captured.stdout), "build");

    let mut child = spawn_piped(&sh("cat /proc/self/cgroup", Containment::Auto)).unwrap();
    assert_eq!(child.containment(), ContainmentLevel::Cgroup);
    let output = Output::start(&mut child);
    assert!(child.wait().unwrap().status.success());
    check(&output.wait_for(".scope\n"), "run");
}

#[test]
fn scoped_runs_get_only_the_bus_variables_in_addition() {
    if !cgroups("scoped_runs_get_only_the_bus_variables_in_addition") {
        return;
    }
    let mut command = Command::new("/usr/bin/env", std::env::temp_dir()).unwrap();
    command.env("ONLY_THIS", "yes");
    let result = run_captured(&command).unwrap();
    assert!(result.status.success(), "{result:?}");
    let names: Vec<String> = text(&result.stdout)
        .lines()
        .filter_map(|line| line.split_once('=').map(|(name, _)| name.to_owned()))
        .collect();
    assert!(names.contains(&"ONLY_THIS".to_owned()));
    assert!(names.contains(&"XDG_RUNTIME_DIR".to_owned()));
    for name in &names {
        assert!(
            [
                "ONLY_THIS",
                "XDG_RUNTIME_DIR",
                "DBUS_SESSION_BUS_ADDRESS",
                "INVOCATION_ID"
            ]
            .contains(&name.as_str()),
            "unexpected variable {name} in {names:?}"
        );
    }
}

#[test]
fn a_scope_takes_double_forked_children_when_the_run_ends() {
    if !cgroups("a_scope_takes_double_forked_children_when_the_run_ends") || !Path::new(SETSID).exists() {
        return;
    }
    let start = Instant::now();
    let result = run_captured(&sh(&double_fork_then("exit 0"), Containment::Auto)).unwrap();
    assert!(result.status.success(), "{result:?}");
    let escaped = number_after(&text(&result.stdout), "escaped ");
    let gone = is_gone(escaped);
    kill_pid(escaped);
    assert!(gone, "the escaped sleeper {escaped} outlived its run");
    assert!(start.elapsed() < PATIENCE, "{:?}", start.elapsed());
}

#[test]
fn stop_takes_double_forked_children_of_a_scoped_session() {
    if !cgroups("stop_takes_double_forked_children_of_a_scoped_session") || !Path::new(SETSID).exists() {
        return;
    }
    let mut child = spawn_piped(&sh(&double_fork_then("exec sleep 600"), Containment::Auto)).unwrap();
    assert_eq!(child.containment(), ContainmentLevel::Cgroup);
    let output = Output::start(&mut child);
    let escaped = number_after(&output.wait_for("escaped "), "escaped ");
    child.stop();
    let exit = child.wait().unwrap();
    assert!(exit.stopped);
    let gone = is_gone(escaped);
    kill_pid(escaped);
    assert!(gone, "the escaped sleeper {escaped} outlived Stop");
}

#[test]
fn a_memory_hog_over_its_scope_limit_is_out_of_memory() {
    if !cgroups("a_memory_hog_over_its_scope_limit_is_out_of_memory") {
        return;
    }
    for containment_kind in ["captured", "session"] {
        let mut command = hog(512, Containment::Auto);
        command.limits(Limits {
            rss_limit: Some(256 * MIB),
            timeout: Some(Duration::from_mins(1)),
            ..Limits::default()
        });
        let (out_of_memory, timed_out, status) = if containment_kind == "captured" {
            let result = run_captured(&command).unwrap();
            (result.out_of_memory, result.timed_out, result.status)
        } else {
            let mut child = spawn_piped(&command).unwrap();
            let _output = Output::start(&mut child);
            let exit = child.wait().unwrap();
            (exit.out_of_memory, exit.timed_out, exit.status)
        };
        assert!(out_of_memory, "{containment_kind}: {status:?}");
        assert!(!timed_out, "{containment_kind}");
        assert!(!status.success(), "{containment_kind}");
    }
}

#[test]
fn a_fork_bomb_stops_at_the_scope_task_limit() {
    if !cgroups("a_fork_bomb_stops_at_the_scope_task_limit") {
        return;
    }
    let (result, elapsed) = fork_bomb(32, Containment::Auto);
    assert!(result.too_many_processes, "{result:?}");
    assert!(!result.timed_out);
    assert!(elapsed < Duration::from_secs(30), "{elapsed:?}");
}

/// The `systemd-run` this system has.
fn systemd_run() -> PathBuf {
    ["/usr/bin/systemd-run", "/bin/systemd-run"]
        .into_iter()
        .map(PathBuf::from)
        .find(|path| path.is_file())
        .expect("detection found systemd-run")
}

#[test]
fn stale_scopes_of_dead_owners_are_killed() {
    if !cgroups("stale_scopes_of_dead_owners_are_killed") || !Path::new(SETSID).exists() {
        return;
    }
    // A process ID that no longer runs: a session's program, reaped.
    let gone = spawn_piped(&sh("exit 0", Containment::ProcessGroupOnly)).unwrap();
    let dead_owner = gone.pid();
    gone.wait().unwrap();
    drop(gone);
    assert!(is_gone(i32::try_from(dead_owner).unwrap()));

    // A scope named as if that process had started it, holding a sleeper
    // that left the scope's first process behind.
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let unit = format!("b2c-run-{dead_owner}-{:016x}", nanos & u128::from(u64::MAX));
    let mut orphan = Command::new(systemd_run(), "/").unwrap();
    orphan
        .args([
            "--user",
            "--scope",
            "--quiet",
            "--collect",
            &format!("--unit={unit}"),
            "--",
        ])
        .args([
            "/bin/sh",
            "-c",
            &format!("{SETSID} /bin/sleep 600 </dev/null >/dev/null 2>&1 & echo \"escaped $!\""),
        ])
        .env("PATH", "/usr/bin:/bin")
        .containment(Containment::ProcessGroupOnly)
        .limits(Limits {
            timeout: Some(PATIENCE),
            ..Limits::default()
        });
    for name in ["XDG_RUNTIME_DIR", "DBUS_SESSION_BUS_ADDRESS"] {
        if let Some(value) = std::env::var_os(name) {
            orphan.env(name, value);
        }
    }
    let started = run_captured(&orphan).unwrap();
    assert!(started.status.success(), "{started:?}");
    let escaped = number_after(&text(&started.stdout), "escaped ");
    thread::sleep(Duration::from_millis(200));
    let alive_before = !is_gone_now(escaped);

    let killed = cleanup_stale_scopes();
    let gone_after = is_gone(escaped);
    kill_pid(escaped);
    assert!(
        alive_before,
        "the orphaned sleeper {escaped} should run until the cleanup"
    );
    assert!(killed >= 1, "no stale scope was killed");
    assert!(gone_after, "the orphaned sleeper {escaped} survived the cleanup");
}

#[test]
fn live_scopes_are_left_alone() {
    if !cgroups("live_scopes_are_left_alone") {
        return;
    }
    let mut child = spawn_piped(&sh("echo ready; exec sleep 600", Containment::Auto)).unwrap();
    let output = Output::start(&mut child);
    output.wait_for("ready");
    cleanup_stale_scopes();
    assert!(
        child.try_wait().unwrap().is_none(),
        "this process's own scope was killed"
    );
    child.kill();
    assert!(child.wait().unwrap().stopped);
}
