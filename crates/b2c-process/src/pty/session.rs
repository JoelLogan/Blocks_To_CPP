//! Supervising a session's program on a background thread: exit detection,
//! whole-tree cleanup, stop with a grace period, cancellation, timeout and
//! the process watchdog. Platform-neutral; the platform modules supply the
//! [`Supervised`] program and the [`Tree`].
//!
//! # Process ID safety (Unix)
//!
//! Signals go to the program's process group, whose ID is the program's
//! process ID. That ID stays reserved while the program is a zombie, so every
//! signal is sent while holding the state lock *and* before the exit has been
//! recorded; the supervisor reaps the program only under the same lock, after
//! killing what is left of the group. A signal can therefore never reach an
//! unrelated process that reused the ID.

use std::io;
use std::sync::mpsc;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, OnceLock, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use crate::cancel::CancelToken;
use crate::platform::Tree;
use crate::status::ExitStatus;

use super::PtyExit;

/// First and longest pause between checks on a running program. The
/// supervisor also wakes at once when [`Shared::stop`] or [`Shared::kill`] is
/// called.
const FIRST_POLL: Duration = Duration::from_millis(1);
const MAX_POLL: Duration = Duration::from_millis(25);
/// Shortest pause, so a deadline that has just passed cannot spin the loop.
const MIN_PAUSE: Duration = Duration::from_micros(100);
/// How often the process-count watchdog looks at the tree.
const WATCHDOG_INTERVAL: Duration = Duration::from_millis(100);

/// The program of a session, as the supervisor thread owns it.
pub(crate) trait Supervised: Send + 'static {
    /// Whether the program has exited, *without* reaping it (Unix), so its
    /// process ID stays reserved.
    fn has_exited(&mut self) -> io::Result<bool>;

    /// Waits for the program to exit (it already has, or its tree was just
    /// killed) and collects its status.
    fn reap(&mut self) -> io::Result<ExitStatus>;

    /// Runs after the exit has been recorded, without the state lock.
    /// Windows closes the pseudo console here, which can block until its
    /// last output has been read.
    fn finish(self);
}

/// Why the session stopped the program itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StopReason {
    /// [`Shared::stop`], [`Shared::kill`], dropping the session, or the
    /// command's cancellation token.
    Stopped,
    /// The command's wall-clock timeout.
    TimedOut,
    /// The process-count watchdog.
    TooManyProcesses,
}

/// A failure to watch the program, kept in a copyable form so every
/// [`Shared::wait`] can report it.
#[derive(Debug, Clone, Copy)]
struct Failure {
    kind: io::ErrorKind,
    code: Option<i32>,
}

impl Failure {
    fn new(error: &io::Error) -> Self {
        Self {
            kind: error.kind(),
            code: error.raw_os_error(),
        }
    }

    fn to_error(self) -> io::Error {
        self.code
            .map_or_else(|| io::Error::from(self.kind), io::Error::from_raw_os_error)
    }
}

/// What the supervisor and the session's owner share under the lock.
#[derive(Debug, Default)]
struct State {
    /// Set once, when the program has been reaped (or can no longer be
    /// watched).
    exit: Option<Result<PtyExit, Failure>>,
    /// The first reason the session stopped the program.
    reason: Option<StopReason>,
    /// Whether the polite stop request (`SIGTERM`) has been sent.
    stopping: bool,
    /// When the polite request turns into a forced kill; `None` when no
    /// kill is due (not stopping, already killed, or a grace period that
    /// ends beyond what an [`Instant`] can represent).
    kill_at: Option<Instant>,
}

/// How the supervisor enforces the command's limits.
#[derive(Debug, Clone)]
pub(crate) struct Supervision {
    /// Wall-clock limit.
    pub(crate) timeout: Option<Duration>,
    /// Maximum processes in the tree (counted where the platform can).
    pub(crate) processes: Option<u32>,
    /// The command's cancellation token: cancelling it stops the program
    /// like [`Shared::stop`].
    pub(crate) cancel: Option<CancelToken>,
}

/// The state of one session, shared by the supervisor thread, the session
/// owner, and the output reader and input writer.
#[derive(Debug)]
pub(crate) struct Shared {
    tree: Tree,
    state: Mutex<State>,
    changed: Condvar,
    /// When the program's exit was detected (set once, read without the
    /// lock by the reader and the writer).
    ended: OnceLock<Instant>,
    started: Instant,
    grace: Duration,
}

impl Shared {
    /// The state of a program started at `started`, whose tree is `tree`.
    /// `grace` is the time between the polite stop request and the kill;
    /// one too long to represent (such as [`Duration::MAX`]) means the kill
    /// never comes.
    pub(crate) fn new(tree: Tree, started: Instant, grace: Duration) -> Arc<Self> {
        Arc::new(Self {
            tree,
            state: Mutex::new(State::default()),
            changed: Condvar::new(),
            ended: OnceLock::new(),
            started,
            grace,
        })
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        // Every update under the lock is a plain field write, so a panic
        // while it was held cannot leave the state inconsistent.
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Whether the program has ended (its exit has been detected).
    pub(crate) fn has_ended(&self) -> bool {
        self.ended.get().is_some()
    }

    /// How long ago the program ended, if it has.
    #[cfg(unix)] // the Windows reader needs no drain deadline
    pub(crate) fn since_end(&self) -> Option<Duration> {
        self.ended.get().map(Instant::elapsed)
    }

    /// Asks the tree to stop: `SIGTERM` to the process group now and
    /// `SIGKILL` after the grace period (Windows: the job is terminated at
    /// once). Does nothing once the program has ended.
    pub(crate) fn stop(&self) {
        let mut state = self.lock();
        if state.exit.is_none() {
            self.begin_stop(&mut state, StopReason::Stopped, Instant::now());
            self.changed.notify_all();
        }
    }

    /// Kills the tree at once. Does nothing once the program has ended.
    pub(crate) fn kill(&self) {
        let mut state = self.lock();
        if state.exit.is_none() {
            state.reason.get_or_insert(StopReason::Stopped);
            state.stopping = true;
            state.kill_at = None;
            self.tree.kill();
            self.changed.notify_all();
        }
    }

    /// Waits until the program has ended and returns how.
    pub(crate) fn wait(&self) -> io::Result<PtyExit> {
        let mut state = self.lock();
        loop {
            if let Some(exit) = state.exit {
                return exit.map_err(Failure::to_error);
            }
            state = self.changed.wait(state).unwrap_or_else(PoisonError::into_inner);
        }
    }

    /// How the program ended, or `None` while it runs.
    pub(crate) fn try_wait(&self) -> io::Result<Option<PtyExit>> {
        self.lock()
            .exit
            .map(|exit| exit.map_err(Failure::to_error))
            .transpose()
    }

    /// Records `reason` (unless an earlier one exists) and sends the polite
    /// stop request, once. It runs on the caller's thread for
    /// [`Shared::stop`] and on the supervisor thread for the token and the
    /// timeout, so it must not panic: a grace period that ends beyond what
    /// an [`Instant`] can represent means no forced kill, as an overflowing
    /// timeout means no timeout.
    fn begin_stop(&self, state: &mut State, reason: StopReason, now: Instant) {
        state.reason.get_or_insert(reason);
        if state.stopping {
            return;
        }
        state.stopping = true;
        if self.grace.is_zero() {
            self.tree.kill();
        } else {
            self.tree.stop();
            state.kill_at = now.checked_add(self.grace);
        }
    }
}

/// Starts the supervisor thread for `program`. If the thread cannot be
/// started, the tree is killed and the program reaped before the error is
/// returned, so nothing is left running.
pub(crate) fn start<P: Supervised>(
    shared: &Arc<Shared>,
    program: P,
    supervision: Supervision,
) -> io::Result<()> {
    // The program is handed over only once the thread exists, so a failure
    // to start it leaves the program here to be cleaned up.
    let (sender, receiver) = mpsc::sync_channel::<P>(1);
    let worker = Arc::clone(shared);
    let spawned = thread::Builder::new()
        .name(String::from("b2c-process-session"))
        .spawn(move || {
            if let Ok(program) = receiver.recv() {
                supervise(&worker, program, &supervision);
            }
        });
    let failure = match spawned {
        Ok(_) => match sender.send(program) {
            Ok(()) => return Ok(()),
            Err(mpsc::SendError(program)) => (program, io::Error::other("the session thread stopped")),
        },
        Err(error) => (program, error),
    };
    let (mut program, error) = failure;
    shared.kill();
    let status = program.reap();
    let _ = shared.ended.set(Instant::now());
    {
        let mut state = shared.lock();
        state.exit = Some(
            status
                .map(|status| exit_record(shared, &state, status, Instant::now()))
                .map_err(|error| Failure::new(&error)),
        );
    }
    shared.changed.notify_all();
    program.finish();
    Err(error)
}

/// The exit record for a program that ended at `ended` with `status`.
fn exit_record(shared: &Shared, state: &State, status: ExitStatus, ended: Instant) -> PtyExit {
    PtyExit {
        status,
        stopped: state.reason == Some(StopReason::Stopped),
        duration: ended.saturating_duration_since(shared.started),
        timed_out: state.reason == Some(StopReason::TimedOut),
        too_many_processes: state.reason == Some(StopReason::TooManyProcesses),
    }
}

/// The supervisor loop: waits for the program while enforcing stop requests
/// and limits, then cleans up the tree, reaps the program and records the
/// exit.
fn supervise<P: Supervised>(shared: &Shared, mut program: P, supervision: &Supervision) {
    let deadline = supervision
        .timeout
        .and_then(|timeout| shared.started.checked_add(timeout));
    let mut poll = FIRST_POLL;
    let mut next_watchdog = shared.started + WATCHDOG_INTERVAL;
    let mut state = shared.lock();
    loop {
        match program.has_exited() {
            Ok(true) => {
                let ended = Instant::now();
                // Anything the program left running goes too, while its
                // process ID is still reserved.
                shared.tree.after_exit();
                let record = program
                    .reap()
                    .map(|status| exit_record(shared, &state, status, ended))
                    .map_err(|error| Failure::new(&error));
                let _ = shared.ended.set(ended);
                state.exit = Some(record);
                break;
            }
            Ok(false) => {}
            Err(error) => {
                // The program can no longer be watched (for example another
                // part of this process reaped it). Its process ID may be in
                // use by now, so the tree is not signalled.
                let _ = shared.ended.set(Instant::now());
                state.exit = Some(Err(Failure::new(&error)));
                break;
            }
        }
        let now = Instant::now();
        if state.reason.is_none() {
            if supervision.cancel.as_ref().is_some_and(CancelToken::is_cancelled) {
                shared.begin_stop(&mut state, StopReason::Stopped, now);
            } else if deadline.is_some_and(|deadline| now >= deadline) {
                shared.begin_stop(&mut state, StopReason::TimedOut, now);
            } else if let Some(max) = supervision.processes
                && now >= next_watchdog
            {
                next_watchdog = now + WATCHDOG_INTERVAL;
                let max = usize::try_from(max).unwrap_or(usize::MAX);
                if shared.tree.process_count().is_some_and(|count| count > max) {
                    state.reason = Some(StopReason::TooManyProcesses);
                    state.stopping = true;
                    shared.tree.kill();
                }
            }
        }
        if state.kill_at.is_some_and(|at| now >= at) {
            state.kill_at = None;
            shared.tree.kill();
        }
        let mut pause = poll;
        if let Some(at) = state.kill_at {
            pause = pause.min(at.saturating_duration_since(now));
        }
        if state.reason.is_none()
            && let Some(deadline) = deadline
        {
            pause = pause.min(deadline.saturating_duration_since(now));
        }
        state = shared
            .changed
            .wait_timeout(state, pause.max(MIN_PAUSE))
            .map_or_else(|poisoned| poisoned.into_inner().0, |(guard, _)| guard);
        poll = (poll * 2).min(MAX_POLL);
    }
    drop(state);
    shared.changed.notify_all();
    program.finish();
}
