//! Building and running through the backend (`docs/spec/02-architecture.md`
//! §2.4.2–§2.4.3 and §2.5.3, `docs/spec/07-toolchain-build-run.md` §7.5–§7.6,
//! `docs/spec/08-security.md` §8.3 and §8.7): Restricted Mode refuses before
//! anything touches the disk or starts a process; builds and runs report in
//! order with exactly one `finished` and one `exit`; `run_start` checks its
//! preconditions again; a new build cancels the running one; closing a
//! project kills its program; and the backend generates exactly the files
//! the editor's preview shows.
//!
//! The tests that compile need g++ on `PATH`; without one they return early,
//! unless `B2C_REQUIRE_GXX` is set (as in CI). The fake compilers are shell
//! scripts, so those tests run on Unix only.

// Test code: helpers fail the test by panicking.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::too_many_lines
)]

mod common;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use b2c_app::TrustChoice;
use b2c_ipc::dto::{
    BuildEvent, BuildOutcome, ExitStatus, ProjectCloseRequest, ProjectNewRequest, RunEvent, RunOptions,
    RunStartRequest, SettingsPatch, Template, TrustGrantRequest, TrustRevokeRequest,
};
use b2c_ipc::{BuildId, Handle, IpcError, decode};
use common::{
    Compiler, TestApp, TestProber, edit, example_text, examples, gxx, run_sinks, wait_exit, wait_finished,
};
use serde_json::json;

/// The outcome of the one `finished` event, which must be the last event.
fn outcome(events: &[BuildEvent]) -> BuildOutcome {
    let finished = events
        .iter()
        .filter(|event| matches!(event, BuildEvent::Finished { .. }))
        .count();
    assert_eq!(finished, 1, "{events:#?}");
    match events.last() {
        Some(BuildEvent::Finished { outcome, .. }) => *outcome,
        other => panic!("the last event is not finished: {other:?}"),
    }
}

/// Builds `document` for `handle` and waits for the outcome.
fn build(app: &TestApp, handle: &Handle, document: &str) -> (BuildId, BuildOutcome, Vec<BuildEvent>) {
    let (id, sink) = app.build(handle, document);
    let events = wait_finished(&sink);
    let result = outcome(&events);
    (id, result, events)
}

/// A started run: its ID and its two channels.
type Started = (
    b2c_ipc::RunId,
    std::sync::Arc<b2c_ipc::sink::testing::RecordingBytes>,
    std::sync::Arc<b2c_ipc::sink::testing::RecordingSink<RunEvent>>,
);

/// Starts the program of `build_id`.
fn run(app: &TestApp, build_id: &BuildId) -> Result<Started, IpcError> {
    let (output, events) = run_sinks();
    let response = app.backend.run_start(
        RunStartRequest {
            build_id: build_id.clone(),
            run_options: RunOptions { cols: 80, rows: 24 },
        },
        output.clone(),
        events.clone(),
    )?;
    Ok((response.run_id, output, events))
}

/// The status of the one `exit` event, which must be the last event, after
/// `started` came first.
fn exit_status(events: &[RunEvent]) -> ExitStatus {
    assert!(
        matches!(events.first(), Some(RunEvent::Started { .. })),
        "{events:#?}"
    );
    let exits = events
        .iter()
        .filter(|event| matches!(event, RunEvent::Exit { .. }))
        .count();
    assert_eq!(exits, 1, "{events:#?}");
    match events.last() {
        Some(RunEvent::Exit { status, .. }) => *status,
        other => panic!("the last event is not exit: {other:?}"),
    }
}

/// Grants trust to `handle` through the (scripted) dialog.
fn trust(app: &TestApp, handle: &Handle) {
    app.dialogs.will_trust(TrustChoice::TrustProject);
    app.backend
        .trust_grant(TrustGrantRequest {
            handle: handle.clone(),
        })
        .unwrap();
}

#[test]
fn restricted_projects_are_never_built() {
    let app = TestApp::with(Compiler::spy(), TestProber::new());
    app.wait_discovery();
    let path = app.write("hello.b2c", example_text("hello_world").as_bytes());
    let opened = app.open(&path);
    let (output, events) = run_sinks();
    assert_eq!(
        app.backend
            .build_start(
                b2c_ipc::dto::BuildStartRequest {
                    handle: opened.handle.clone(),
                    document: opened.document.clone(),
                    config: b2c_ipc::dto::BuildConfig::Debug,
                },
                std::sync::Arc::new(b2c_ipc::sink::testing::RecordingSink::new()),
            )
            .unwrap_err(),
        IpcError::Restricted
    );
    assert!(!app.has_builds(), "a build folder was created");
    assert!(!app.spawned(), "a compiler ran");
    assert!(output.batches().is_empty() && events.is_empty());

    // After the user trusts it in the dialog, it builds.
    trust(&app, &opened.handle);
    let (_, result, events) = build(&app, &opened.handle, &opened.document);
    assert!(app.spawned(), "the compiler did not run");
    assert!(app.has_builds());
    if gxx().is_some() {
        assert_eq!(result, BuildOutcome::Built, "{events:#?}");
    }
}

#[test]
fn a_new_project_builds_and_runs_in_its_sandbox() {
    if gxx().is_none() {
        return;
    }
    let app = TestApp::with(Compiler::spy(), TestProber::new());
    let created = app
        .backend
        .project_new(ProjectNewRequest {
            template: Template::HelloWorld,
        })
        .unwrap();
    let (build_id, result, events) = build(&app, &created.handle, &created.document);
    assert_eq!(result, BuildOutcome::Built, "{events:#?}");
    assert!(matches!(events.first(), Some(BuildEvent::Progress { .. })));
    let Some(BuildEvent::Finished { project_hash, .. }) = events.last() else {
        panic!("{events:#?}");
    };
    let document = b2c_model::load(created.document.as_bytes()).unwrap();
    assert_eq!(
        project_hash.as_deref(),
        Some(b2c_model::hex(&b2c_model::content_hash(&document)).as_str())
    );

    let (_, output, run_events) = run(&app, &build_id).unwrap();
    let all = wait_exit(&run_events);
    assert_eq!(exit_status(&all), ExitStatus::Exited { code: 0 });
    let text = String::from_utf8_lossy(&output.concat()).into_owned();
    assert!(text.contains("Hello, world!"), "{text:?}");
    // A project that was never saved runs in its sandbox folder.
    let sandbox = app.dirs.sandbox();
    let folders: Vec<_> = std::fs::read_dir(&sandbox).unwrap().collect();
    assert_eq!(folders.len(), 1);
    // A second build of the same content is up to date.
    let (_, again, _) = build(&app, &created.handle, &created.document);
    assert_eq!(again, BuildOutcome::UpToDate);

    // The empty template builds and runs too.
    let empty = app
        .backend
        .project_new(ProjectNewRequest {
            template: Template::Empty,
        })
        .unwrap();
    let (build_id, result, events) = build(&app, &empty.handle, &empty.document);
    assert_eq!(result, BuildOutcome::Built, "{events:#?}");
    let (_, output, run_events) = run(&app, &build_id).unwrap();
    assert_eq!(
        exit_status(&wait_exit(&run_events)),
        ExitStatus::Exited { code: 0 }
    );
    // Nothing visible: a Windows pseudo console still paints the screen (clear,
    // cursor, title) for a program that prints nothing.
    let shown = visible_text(&output.concat());
    assert!(shown.trim().is_empty(), "{shown:?}");
}

/// `bytes` without terminal escape sequences (CSI `ESC [ … final`, OSC
/// `ESC ] … BEL` or `ESC ] … ESC \`, and two-byte `ESC x`) and without
/// other control characters except line breaks.
fn visible_text(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    let mut shown = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\u{1b}' {
            if !c.is_control() || c == '\n' {
                shown.push(c);
            }
            continue;
        }
        match chars.next() {
            Some('[') => {
                for c in chars.by_ref() {
                    if ('@'..='~').contains(&c) {
                        break;
                    }
                }
            }
            Some(']') => {
                while let Some(c) = chars.next() {
                    if c == '\u{7}' || (c == '\u{1b}' && chars.next_if_eq(&'\\').is_some()) {
                        break;
                    }
                }
            }
            _ => {}
        }
    }
    shown
}

#[test]
fn visible_text_drops_terminal_sequences() {
    let painted = "\u{1b}[?9001h\u{1b}[?25l\u{1b}[2J\u{1b}[m\u{1b}[H\u{1b}]0;C:\\a.exe\u{7}\u{1b}[?25h";
    assert_eq!(visible_text(painted.as_bytes()), "");
    assert_eq!(
        visible_text(b"\x1b[1mhi\x1b[0m\r\nthere\x1b]0;t\x1b\\"),
        "hi\nthere"
    );
}

#[test]
fn run_start_checks_its_preconditions() {
    if gxx().is_none() {
        return;
    }
    let app = TestApp::with(Compiler::spy(), TestProber::new());
    let created = app
        .backend
        .project_new(ProjectNewRequest {
            template: Template::HelloWorld,
        })
        .unwrap();
    let handle = created.handle.clone();

    // A build with analyser errors produced no program.
    let broken = edit(&created.document, |project| {
        project["modules"][0]["workspace"]["blocks"][0]["statements"]["BODY"][0]["inputs"]["ITEM0"]["expr"] =
            json!([{"ref": "s_missing"}]);
    });
    let (broken_build, result, _) = build(&app, &handle, &broken);
    assert_eq!(result, BuildOutcome::ProjectErrors);
    assert_eq!(
        run(&app, &broken_build).unwrap_err(),
        IpcError::BuildNotSuccessful
    );

    // A build of an older document is stale.
    let (old_build, result, _) = build(&app, &handle, &created.document);
    assert_eq!(result, BuildOutcome::Built);
    let renamed = edit(&created.document, |project| {
        project["project"]["name"] = json!("Renamed");
    });
    let (new_build, result, _) = build(&app, &handle, &renamed);
    assert_eq!(result, BuildOutcome::Built);
    assert_eq!(run(&app, &old_build).unwrap_err(), IpcError::StaleBuild);

    // A program changed after its build is refused.
    let executable = find_executable(&app.dirs.builds()).unwrap();
    let original = std::fs::read(&executable).unwrap();
    let mut tampered = original.clone();
    tampered.extend_from_slice(b"tampered");
    std::fs::write(&executable, &tampered).unwrap();
    assert_eq!(run(&app, &new_build).unwrap_err(), IpcError::StaleBuild);
    std::fs::write(&executable, &original).unwrap();
    // So is a build whose manifest was changed.
    let manifest = executable
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("build-manifest.json");
    let recorded = std::fs::read(&manifest).unwrap();
    let mut changed: serde_json::Value = serde_json::from_slice(&recorded).unwrap();
    changed["projectHash"] = json!("0".repeat(64));
    std::fs::write(&manifest, serde_json::to_vec(&changed).unwrap()).unwrap();
    assert_eq!(run(&app, &new_build).unwrap_err(), IpcError::StaleBuild);
    std::fs::remove_file(&manifest).unwrap();
    assert_eq!(run(&app, &new_build).unwrap_err(), IpcError::StaleBuild);
    std::fs::write(&manifest, &recorded).unwrap();
    let (_, _, events) = run(&app, &new_build).unwrap();
    assert_eq!(exit_status(&wait_exit(&events)), ExitStatus::Exited { code: 0 });

    // Not trusted any more.
    app.backend
        .trust_revoke(TrustRevokeRequest {
            handle: handle.clone(),
        })
        .unwrap();
    assert_eq!(run(&app, &new_build).unwrap_err(), IpcError::Restricted);

    // A closed project's builds are unknown.
    app.backend.project_close(ProjectCloseRequest { handle }).unwrap();
    assert_eq!(run(&app, &new_build).unwrap_err(), IpcError::UnknownBuild);
}

/// The one program in the build cache (most recently modified).
fn find_executable(builds: &Path) -> Option<PathBuf> {
    let mut found: Vec<(std::time::SystemTime, PathBuf)> = Vec::new();
    let mut stack = vec![builds.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).ok()?.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path
                .parent()
                .and_then(Path::file_name)
                .is_some_and(|name| name == "out")
                && path.extension().is_none_or(|ext| ext == "exe")
            {
                found.push((entry.metadata().ok()?.modified().ok()?, path));
            }
        }
    }
    found.sort();
    found.pop().map(|(_, path)| path)
}

#[cfg(unix)]
#[test]
fn a_new_build_cancels_the_running_one() {
    if gxx().is_none() {
        return;
    }
    let app = TestApp::with(Compiler::Slow(3), TestProber::new());
    let created = app
        .backend
        .project_new(ProjectNewRequest {
            template: Template::HelloWorld,
        })
        .unwrap();
    let (_, first) = app.build(&created.handle, &created.document);
    // Wait until the first build is compiling.
    common::wait_events(&first, |events| {
        events
            .iter()
            .any(|event| matches!(event, BuildEvent::Progress { stage, .. } if stage.as_str() == "compile"))
    });
    let (_, second, events) = build(&app, &created.handle, &created.document);
    assert_eq!(outcome(&wait_finished(&first)), BuildOutcome::Cancelled);
    assert_eq!(second, BuildOutcome::Built, "{events:#?}");
}

#[cfg(target_os = "linux")]
#[test]
fn closing_a_project_kills_its_program() {
    use b2c_ipc::dto::{RunInputRequest, RunStopRequest};
    use b2c_ipc::encode_base64;

    if gxx().is_none() {
        return;
    }
    let app = TestApp::with(Compiler::spy(), TestProber::new());
    let path = app.write("game.b2c", example_text("guessing_game").as_bytes());
    let opened = app.open(&path);
    trust(&app, &opened.handle);
    let (build_id, result, events) = build(&app, &opened.handle, &opened.document);
    assert_eq!(result, BuildOutcome::Built, "{events:#?}");
    let executable = find_executable(&app.dirs.builds()).unwrap();

    // The guessing game waits for input.
    let (run_id, output, events) = run(&app, &build_id).unwrap();
    wait_output(&output);
    assert!(!processes_running(&executable).is_empty());
    // Input reaches it.
    let request =
        decode::<RunInputRequest>(json!({"runId": run_id.as_str(), "data": encode_base64(b"50\n")})).unwrap();
    app.backend.run_input(request).unwrap();

    // A build of the same project stops it first.
    let (_, _, _) = build(&app, &opened.handle, &opened.document);
    assert_eq!(exit_status(&wait_exit(&events)), ExitStatus::Stopped);
    assert_eq!(
        app.backend
            .run_stop(RunStopRequest {
                run_id: run_id.clone()
            })
            .unwrap_err(),
        IpcError::NotRunning
    );

    // Running again, then closing the project, kills the program's tree.
    let (_, output, events) = run(&app, &build_id).unwrap();
    wait_output(&output);
    app.backend
        .project_close(ProjectCloseRequest {
            handle: opened.handle,
        })
        .unwrap();
    assert_eq!(exit_status(&wait_exit(&events)), ExitStatus::Stopped);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !processes_running(&executable).is_empty() {
        assert!(
            std::time::Instant::now() < deadline,
            "the program survived its project"
        );
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

/// 02 §2.6: closing a project stops its program, also one whose `run_start`
/// was still being prepared (checked, hashed, analysed) when the close came.
/// Either the close finds the new program, or `run_start` finds the project
/// gone, stops what it started and answers `unknownBuild`; nothing is left
/// running, so the next build of the same content is not kept waiting for a
/// program's hold on its build folder.
#[cfg(target_os = "linux")]
#[test]
fn a_run_racing_a_close_never_outlives_its_project() {
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    if gxx().is_none() {
        return;
    }
    let app = TestApp::with(Compiler::spy(), TestProber::new());
    let game = example_text("guessing_game");
    let mut executable = None;
    for attempt in 0..20_u64 {
        let created = app
            .backend
            .project_new(ProjectNewRequest {
                template: Template::Empty,
            })
            .unwrap();
        let (build_id, result, events) = build(&app, &created.handle, &game);
        assert!(
            matches!(result, BuildOutcome::Built | BuildOutcome::UpToDate),
            "{events:#?}"
        );
        let program = executable
            .get_or_insert_with(|| find_executable(&app.dirs.builds()).unwrap())
            .clone();
        let backend = Arc::clone(&app.backend);
        let (output, run_events) = run_sinks();
        let runner = {
            let run_events = run_events.clone();
            std::thread::spawn(move || {
                backend.run_start(
                    RunStartRequest {
                        build_id,
                        run_options: RunOptions { cols: 80, rows: 24 },
                    },
                    output,
                    run_events,
                )
            })
        };
        // Closes at different points of the run's preparation.
        std::thread::sleep(Duration::from_micros(attempt * 1_500));
        app.backend
            .project_close(ProjectCloseRequest {
                handle: created.handle,
            })
            .unwrap();
        let started = runner.join().unwrap();
        assert!(
            matches!(started, Ok(_) | Err(IpcError::UnknownBuild)),
            "attempt {attempt}: run_start gave {started:?}"
        );
        let deadline = Instant::now() + Duration::from_secs(10);
        while !processes_running(&program).is_empty() {
            assert!(
                Instant::now() < deadline,
                "attempt {attempt}: the program of a closed project is still running ({started:?})"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
        if started.is_ok() {
            assert_eq!(exit_status(&wait_exit(&run_events)), ExitStatus::Stopped);
        }
    }
}

/// Waits until the program has printed something.
#[cfg(target_os = "linux")]
fn wait_output(output: &b2c_ipc::sink::testing::RecordingBytes) {
    let deadline = std::time::Instant::now() + common::TIMEOUT;
    while output.concat().is_empty() {
        assert!(
            std::time::Instant::now() < deadline,
            "the program printed nothing"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

/// The processes running `executable` (Linux: `/proc/<pid>/exe`).
#[cfg(target_os = "linux")]
fn processes_running(executable: &Path) -> Vec<u32> {
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|entry| {
            let pid: u32 = entry.file_name().to_str()?.parse().ok()?;
            let exe = std::fs::read_link(entry.path().join("exe")).ok()?;
            let exe = exe.to_string_lossy().trim_end_matches(" (deleted)").to_owned();
            (Path::new(&exe) == executable).then_some(pid)
        })
        .collect()
}

#[test]
fn the_build_generates_what_the_preview_shows() {
    if gxx().is_none() {
        return;
    }
    let app = TestApp::with(Compiler::spy(), TestProber::new());
    for (indent, width) in [
        (b2c_core_wasm::options::IndentWidth::Two, 2),
        (b2c_core_wasm::options::IndentWidth::Four, 4),
    ] {
        let patch = decode::<SettingsPatch>(json!({"codeStyle": {"indentWidth": width}})).unwrap();
        app.backend.settings_update(patch).unwrap();
        for (name, bytes) in examples() {
            let created = app
                .backend
                .project_new(ProjectNewRequest {
                    template: Template::Empty,
                })
                .unwrap();
            let text = String::from_utf8(bytes).unwrap();
            app.backend.build_cache_clear().unwrap();
            let (_, result, events) = build(&app, &created.handle, &text);
            assert_eq!(result, BuildOutcome::Built, "{name}: {events:#?}");
            let generated = generated_files(&app.dirs.builds());
            let preview = b2c_core_wasm::preview_document(
                &text,
                &b2c_core_wasm::options::PreviewOptions { indent_width: indent },
            );
            assert!(preview.buildable, "{name}");
            let previewed: BTreeMap<String, String> = preview
                .files
                .iter()
                .map(|file| (file.path.clone(), file.contents.clone()))
                .collect();
            assert_eq!(generated, previewed, "{name} at indent {width}");
            app.backend
                .project_close(ProjectCloseRequest {
                    handle: created.handle,
                })
                .unwrap();
        }
    }
}

/// Every file of the one `gen/` folder in the build cache, by name.
fn generated_files(builds: &Path) -> BTreeMap<String, String> {
    let mut gen_dirs = Vec::new();
    let mut stack = vec![builds.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                if path.file_name().is_some_and(|name| name == "gen") {
                    gen_dirs.push(path);
                } else {
                    stack.push(path);
                }
            }
        }
    }
    assert_eq!(gen_dirs.len(), 1, "{gen_dirs:?}");
    std::fs::read_dir(&gen_dirs[0])
        .unwrap()
        .flatten()
        .map(|entry| {
            (
                entry.file_name().to_string_lossy().into_owned(),
                std::fs::read_to_string(entry.path()).unwrap(),
            )
        })
        .collect()
}
