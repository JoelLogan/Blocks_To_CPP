//! Run sessions: the IDE's running programs (`docs/spec/07-toolchain-build-run.md`
//! §7.6.2–7.6.5, `docs/spec/02-architecture.md` §2.4.3, §2.5.3 and §2.6).
//!
//! [`RunSessions`] starts a built program in a pseudo-terminal (or, when none
//! can be created or the caller asks for it, with pipes) through
//! `b2c_process`, and runs it as a *session* until it ends:
//!
//! * **Threads.** Each run has three threads and holds no lock while waiting
//!   for the program: an output reader (blocking reads of the terminal, in
//!   chunks of up to 64 KiB, handed over through a bounded queue), a pump
//!   (the only sender on the run's two channels: it batches output through
//!   the [`Coalescer`], scans it for sanitizer reports, and sends the
//!   `started`, `skipped` and `exit` events in order) and an input writer
//!   (writes queued `run_input` data, which may block while the program
//!   does not read, so IPC calls never block).
//! * **Events.** `started` comes first, then output batches numbered from 1
//!   and any `skipped` events, then exactly one `exit` once the output has
//!   ended and its last batch was sent, with `afterSeq` naming the batches
//!   before it.
//! * **Limits.** At most [`MAX_CONCURRENT_RUNS`] programs run at once and one
//!   per project (starting one stops the project's previous run); input is
//!   at most [`MAX_RUN_INPUT_BYTES`] per call and rate-limited (see `rate`);
//!   terminal sizes are within [`RUN_COLS`] and [`RUN_ROWS`]; at most 64 input
//!   calls wait for the program to read them.
//! * **Logging.** A `run` tracing span per session carries the run ID,
//!   containment, terminal mode, duration and outcome. Program input and
//!   output are never logged; paths only at debug level.

use std::collections::{HashMap, VecDeque};
use std::ffi::OsString;
use std::fmt;
use std::io::{ErrorKind, Read, Write as _};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender, TrySendError};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, Weak};
use std::thread;
use std::time::{Duration, Instant};

use b2c_ipc::dto::{Containment, ExitStatus as IpcStatus, RunEvent, RunMode};
use b2c_ipc::limits::{
    MAX_CONCURRENT_RUNS, MAX_RUN_INPUT_BYTES, MAX_SAFE_INTEGER, RUN_COLS, RUN_ROWS, SCROLLBACK_LINES,
};
use b2c_ipc::{ByteSink, EventSink, InvalidReason, IpcError, RunId};
use b2c_process::{Command, ContainmentLevel, IoMode, ProcessError, PtyChild, PtySize, PtyWriter, Stdin};
use b2c_toolchain::sanitizer::Detector;

use super::coalesce::{Coalescer, Emit, FlowLimits, SystemClock};
use super::exit::{self, ExitReport};
use super::rate::InputLimiter;
use crate::cache::RunHold;

/// How much one read of the program's output takes at most.
const READ_CHUNK: usize = 64 * 1024;

/// How many read chunks may wait for the pump (so at most 1 MiB).
const OUTPUT_QUEUE: usize = 16;

/// How many `run_input` calls may wait for the program to read them.
const INPUT_QUEUE: usize = 64;

/// How many finished runs are remembered, so that late calls for them get
/// `notRunning` rather than `unknownRun`.
const MAX_FINISHED_RUNS: usize = 64;

/// How long [`RunSessions::stop_all`] waits for the programs to end.
const STOP_ALL_TIMEOUT: Duration = Duration::from_secs(3);

/// How often [`RunSessions::stop_all`] looks whether the programs have ended.
const STOP_ALL_POLL: Duration = Duration::from_millis(10);

/// What to run, as the backend prepared it from a successful build. Every
/// path comes from the backend's own records, never from the webview.
pub struct RunSpec {
    /// The project the run belongs to: one run per project at a time.
    pub project_key: String,
    /// The program (absolute; on Windows an `.exe`).
    pub executable: PathBuf,
    /// Its arguments, passed without a shell.
    pub args: Vec<OsString>,
    /// Its working directory (absolute): the project folder or its sandbox.
    pub working_dir: PathBuf,
    /// Its complete environment (see [`super::run_environment`]).
    pub env: Vec<(OsString, OsString)>,
    /// The terminal size: columns within [`RUN_COLS`], rows within
    /// [`RUN_ROWS`].
    pub size: PtySize,
    /// The console's scrollback (`console.scrollbackLines`): how many lines
    /// are kept for a console that falls behind. Clamped to
    /// [`SCROLLBACK_LINES`].
    pub scrollback_lines: u32,
    /// Whether the IDE helper unit is linked into the program (reported in
    /// the `started` event).
    pub ide_helpers: bool,
    /// Whether to use a pseudo-terminal (falling back to pipes when none can
    /// be created) rather than pipes.
    pub prefer_pty: bool,
    /// The program's claim on its build folder ([`crate::cache::hold_for_run`]),
    /// kept until the program has ended so eviction and *Clear build cache*
    /// leave the folder alone.
    pub hold: Option<RunHold>,
}

impl fmt::Debug for RunSpec {
    /// Arguments and environment values are not shown: they can hold
    /// secrets, and a `RunSpec` may end up in a log.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RunSpec")
            .field("project_key", &self.project_key)
            .field("args", &self.args.len())
            .field("env", &self.env.len())
            .field("size", &self.size)
            .field("scrollback_lines", &self.scrollback_lines)
            .field("ide_helpers", &self.ide_helpers)
            .field("prefer_pty", &self.prefer_pty)
            .field("hold", &self.hold.is_some())
            .finish_non_exhaustive()
    }
}

/// The running (and recently finished) programs of the app.
///
/// Every method is safe to call from any thread and returns at once, except
/// [`RunSessions::stop_all`]. Dropping the sessions kills every program still
/// running.
pub struct RunSessions {
    shared: Arc<Shared>,
}

impl fmt::Debug for RunSessions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let runs = self.shared.lock();
        f.debug_struct("RunSessions")
            .field("runs", &runs.by_id.len())
            .field("finished", &runs.finished.len())
            .finish()
    }
}

impl Default for RunSessions {
    fn default() -> Self {
        Self::new()
    }
}

/// The session table, shared with the pump threads (which hold it weakly).
#[derive(Debug, Default)]
struct Shared {
    runs: Mutex<Runs>,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, Runs> {
        // Updates under the lock are plain inserts and removals, so a panic
        // while it was held cannot leave the table inconsistent.
        self.runs.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// The runs by ID, and the finished ones in the order they ended.
#[derive(Debug, Default)]
struct Runs {
    by_id: HashMap<RunId, Arc<Run>>,
    finished: VecDeque<RunId>,
}

/// One run session.
struct Run {
    project_key: String,
    child: PtyChild,
    /// Input waiting for the writer thread; `None` once the run has ended.
    input: Mutex<Option<SyncSender<Vec<u8>>>>,
    limiter: Mutex<InputLimiter>,
    /// The number of the last output batch sent (set before it is sent).
    sent: AtomicU64,
    /// The highest batch the console has acknowledged.
    acked: AtomicU64,
    /// Whether the `exit` event has been sent.
    done: AtomicBool,
    hold: Mutex<Option<RunHold>>,
}

impl fmt::Debug for Run {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Run")
            .field("child", &self.child)
            .field("sent", &self.sent)
            .field("acked", &self.acked)
            .field("done", &self.done)
            .finish_non_exhaustive()
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Run {
    /// Whether the program is still running (its tree may still be
    /// stopping).
    fn is_live(&self) -> bool {
        !self.done.load(Ordering::Acquire) && matches!(self.child.try_wait(), Ok(None))
    }

    /// Releases what the run held once its `exit` event has gone.
    fn end(&self) {
        self.done.store(true, Ordering::Release);
        lock(&self.input).take();
        lock(&self.hold).take();
    }
}

impl RunSessions {
    /// No runs yet.
    pub fn new() -> Self {
        Self {
            shared: Arc::new(Shared::default()),
        }
    }

    /// Starts the program of `spec` and returns its new run ID at once. The
    /// program's output goes to `output` in numbered batches and its events
    /// to `events` (`started` first, `exit` last), both from the session's
    /// own thread.
    ///
    /// A run of the same project that is still going is stopped first (like
    /// [`RunSessions::stop`]); it does not count towards the limit.
    ///
    /// # Errors
    /// * [`IpcError::InvalidRequest`] (`outOfRange` at `runOptions.cols` or
    ///   `runOptions.rows`) for a terminal size outside the limits;
    /// * [`IpcError::TooManySessions`] when [`MAX_CONCURRENT_RUNS`] other
    ///   programs are running;
    /// * [`IpcError::Io`] when the program cannot be started (for example its
    ///   file is gone), [`IpcError::Internal`] for anything else (the details
    ///   are logged at debug level).
    pub fn start(
        &self,
        spec: RunSpec,
        output: Arc<dyn ByteSink>,
        events: Arc<dyn EventSink<RunEvent>>,
    ) -> Result<RunId, IpcError> {
        check_size(spec.size.cols, spec.size.rows, "runOptions.")?;
        let command = command(&spec)?;
        let mut runs = self.shared.lock();
        let (others, same_project): (Vec<Arc<Run>>, Vec<Arc<Run>>) = runs
            .by_id
            .values()
            .filter(|run| run.is_live())
            .cloned()
            .partition(|run| run.project_key != spec.project_key);
        // The project's current run is being replaced, so it does not count;
        // runs of the project that are already stopping do, which bounds the
        // processes a burst of starts can leave behind.
        if others.len() + same_project.len().saturating_sub(1) >= MAX_CONCURRENT_RUNS {
            return Err(IpcError::TooManySessions);
        }
        let id = RunId::random()?;
        // One run per project: the one this replaces is stopped first.
        for old in same_project {
            old.child.stop();
        }

        let mut child = spawn(&command, spec.size, spec.prefer_pty)?;
        let Some(reader) = child.take_reader() else {
            // Impossible for a new child; dropping it kills the program.
            tracing::warn!("a new program had no output reader");
            return Err(IpcError::Internal);
        };
        let writer = child.writer();
        let containment = ipc_containment(child.containment());
        let mode = match child.mode() {
            IoMode::Pty => RunMode::Pty,
            IoMode::Pipes => RunMode::Pipes,
        };
        let (input_tx, input_rx) = mpsc::sync_channel(INPUT_QUEUE);
        let run = Arc::new(Run {
            project_key: spec.project_key,
            child,
            input: Mutex::new(Some(input_tx)),
            limiter: Mutex::new(InputLimiter::new()),
            sent: AtomicU64::new(0),
            acked: AtomicU64::new(0),
            done: AtomicBool::new(false),
            hold: Mutex::new(spec.hold),
        });
        let span = tracing::info_span!(
            "run",
            run_id = %id,
            containment = containment.as_str(),
            mode = mode.as_str(),
            ide_helpers = spec.ide_helpers,
            elapsed_ms = tracing::field::Empty,
            outcome = tracing::field::Empty,
        );
        let pump = Pump {
            run: Arc::clone(&run),
            sessions: Arc::downgrade(&self.shared),
            id: id.clone(),
            output,
            events,
            started: RunEvent::Started {
                containment,
                mode,
                ide_helpers: spec.ide_helpers,
            },
            limits: FlowLimits::for_scrollback(
                spec.scrollback_lines
                    .clamp(*SCROLLBACK_LINES.start(), *SCROLLBACK_LINES.end()),
            ),
            span,
        };
        // The writer and reader first: if a later thread cannot start, the
        // program is killed and they end by themselves, and no event has
        // been sent (the pump sends them all).
        let (chunks_tx, chunks_rx) = mpsc::sync_channel(OUTPUT_QUEUE);
        let spawned = spawn_thread("b2c-run-input", move || write_input(writer, &input_rx))
            .and_then(|()| {
                let run = Arc::clone(&run);
                spawn_thread("b2c-run-output", move || read_output(reader, &chunks_tx, &run))
            })
            .and_then(|()| spawn_thread("b2c-run-events", move || pump.run(&chunks_rx)));
        if let Err(error) = spawned {
            tracing::warn!(%error, "cannot start the threads of a run");
            run.child.kill();
            return Err(IpcError::Internal);
        }
        tracing::debug!(run_id = %id, "program started");
        runs.by_id.insert(id.clone(), run);
        Ok(id)
    }

    /// Sends `bytes` to the program's input (keystrokes in a terminal,
    /// standard input with pipes). Returns once they are queued; a writer
    /// thread delivers them as the program reads.
    ///
    /// # Errors
    /// [`IpcError::PayloadTooLarge`] above [`MAX_RUN_INPUT_BYTES`],
    /// [`IpcError::UnknownRun`], [`IpcError::NotRunning`] once the program
    /// has ended, and [`IpcError::RateLimited`] above 200 calls or 1 MiB a
    /// second, or while 64 calls still wait for the program to read them.
    pub fn input(&self, id: &RunId, bytes: &[u8]) -> Result<(), IpcError> {
        if bytes.len() > MAX_RUN_INPUT_BYTES {
            return Err(IpcError::too_large(MAX_RUN_INPUT_BYTES));
        }
        let run = self.get(id)?;
        if !run.is_live() {
            return Err(IpcError::NotRunning);
        }
        if !lock(&run.limiter).admit(bytes.len(), Instant::now()) {
            return Err(IpcError::RateLimited);
        }
        if bytes.is_empty() {
            return Ok(());
        }
        let Some(queue) = lock(&run.input).clone() else {
            return Err(IpcError::NotRunning);
        };
        match queue.try_send(bytes.to_vec()) {
            Ok(()) => Ok(()),
            Err(TrySendError::Full(_)) => Err(IpcError::RateLimited),
            Err(TrySendError::Disconnected(_)) => Err(IpcError::NotRunning),
        }
    }

    /// Resizes the program's terminal (a no-op with pipes).
    ///
    /// # Errors
    /// [`IpcError::InvalidRequest`] (`outOfRange` at `cols` or `rows`) outside
    /// [`RUN_COLS`] and [`RUN_ROWS`], [`IpcError::UnknownRun`],
    /// [`IpcError::NotRunning`] once the program has ended, and
    /// [`IpcError::Io`] when the system refuses the size (the program keeps
    /// the old one).
    pub fn resize(&self, id: &RunId, cols: u16, rows: u16) -> Result<(), IpcError> {
        check_size(cols, rows, "")?;
        let run = self.get(id)?;
        if !run.is_live() {
            return Err(IpcError::NotRunning);
        }
        run.child.resize(PtySize { cols, rows }).map_err(|error| {
            tracing::debug!(%error, "cannot resize a program's terminal");
            match error {
                ProcessError::Resize(source) => IpcError::io(&source),
                _ => IpcError::Internal,
            }
        })
    }

    /// Stops the program's whole process tree: `SIGTERM` to its process
    /// group and `SIGKILL` 2 s later (Windows: the Job Object is terminated
    /// at once). Returns at once; the `exit` event follows with the status
    /// `stopped`.
    ///
    /// # Errors
    /// [`IpcError::UnknownRun`], and [`IpcError::NotRunning`] once the program
    /// has ended.
    pub fn stop(&self, id: &RunId) -> Result<(), IpcError> {
        let run = self.get(id)?;
        if !run.is_live() {
            return Err(IpcError::NotRunning);
        }
        run.child.stop();
        Ok(())
    }

    /// Records that the console has written every output batch up to `seq`
    /// (flow control). Acknowledgements may arrive late or out of order and
    /// after the program has ended; only the highest counts.
    ///
    /// # Errors
    /// [`IpcError::InvalidRequest`] (`outOfRange` at `seq`) for a batch that
    /// was never sent, and [`IpcError::UnknownRun`].
    pub fn ack(&self, id: &RunId, seq: u64) -> Result<(), IpcError> {
        if seq > MAX_SAFE_INTEGER {
            return Err(IpcError::invalid(InvalidReason::OutOfRange, Some("seq")));
        }
        let run = self.get(id)?;
        if seq > run.sent.load(Ordering::Acquire) {
            return Err(IpcError::invalid(InvalidReason::OutOfRange, Some("seq")));
        }
        run.acked.fetch_max(seq, Ordering::AcqRel);
        Ok(())
    }

    /// Stops the project's running program, if any (see
    /// [`RunSessions::stop`]); for example before a build, because Windows
    /// cannot replace a running program's file. Returns at once.
    pub fn stop_project(&self, key: &str) {
        for run in self.live_runs(Some(key)) {
            run.child.stop();
        }
    }

    /// Stops every program and waits until their process trees have ended,
    /// for at most 3 s; anything still running then is killed (shutdown).
    pub fn stop_all(&self) {
        let runs = self.live_runs(None);
        for run in &runs {
            run.child.stop();
        }
        let deadline = Instant::now() + STOP_ALL_TIMEOUT;
        while runs.iter().any(|run| matches!(run.child.try_wait(), Ok(None))) {
            if Instant::now() >= deadline {
                for run in &runs {
                    run.child.kill();
                }
                tracing::warn!("programs were still running 3 s after they were stopped; killed");
                return;
            }
            thread::sleep(STOP_ALL_POLL);
        }
    }

    /// Whether the project has a program running (or still stopping).
    pub fn is_running(&self, key: &str) -> bool {
        !self.live_runs(Some(key)).is_empty()
    }

    fn get(&self, id: &RunId) -> Result<Arc<Run>, IpcError> {
        self.shared
            .lock()
            .by_id
            .get(id)
            .cloned()
            .ok_or(IpcError::UnknownRun)
    }

    fn live_runs(&self, key: Option<&str>) -> Vec<Arc<Run>> {
        self.shared
            .lock()
            .by_id
            .values()
            .filter(|run| key.is_none_or(|key| run.project_key == key) && run.is_live())
            .cloned()
            .collect()
    }
}

impl Drop for RunSessions {
    fn drop(&mut self) {
        for run in self.shared.lock().by_id.values() {
            run.child.kill();
        }
    }
}

/// Checks a terminal size against the IPC limits.
fn check_size(cols: u16, rows: u16, prefix: &str) -> Result<(), IpcError> {
    if !RUN_COLS.contains(&cols) {
        return Err(IpcError::invalid(
            InvalidReason::OutOfRange,
            Some(&format!("{prefix}cols")),
        ));
    }
    if !RUN_ROWS.contains(&rows) {
        return Err(IpcError::invalid(
            InvalidReason::OutOfRange,
            Some(&format!("{prefix}rows")),
        ));
    }
    Ok(())
}

/// The command for a run: no shell, the given environment only, input
/// through the session's writer, no time limit, and the default 2 s between
/// the polite stop and the kill.
fn command(spec: &RunSpec) -> Result<Command, IpcError> {
    let mut command = Command::new(&spec.executable, &spec.working_dir).map_err(|error| {
        tracing::debug!(%error, "cannot run the program");
        IpcError::Internal
    })?;
    command
        .args(spec.args.iter().cloned())
        .envs(spec.env.iter().cloned())
        .stdin(Stdin::Null);
    Ok(command)
}

/// Starts the program in a terminal (or with pipes when no terminal can be
/// created, or when `prefer_pty` is false).
fn spawn(command: &Command, size: PtySize, prefer_pty: bool) -> Result<PtyChild, IpcError> {
    let started = if prefer_pty {
        match b2c_process::spawn_pty(command, size) {
            Err(ProcessError::Pty(error)) => {
                tracing::info!(%error, "no terminal could be created; running the program with pipes");
                b2c_process::spawn_piped(command)
            }
            other => other,
        }
    } else {
        b2c_process::spawn_piped(command)
    };
    started.map_err(|error| {
        tracing::debug!(%error, "cannot start the program");
        match error {
            ProcessError::Spawn { source, .. } => IpcError::io(&source),
            _ => IpcError::Internal,
        }
    })
}

fn ipc_containment(level: ContainmentLevel) -> Containment {
    match level {
        ContainmentLevel::JobObject => Containment::JobObject,
        ContainmentLevel::Cgroup => Containment::Cgroup,
        ContainmentLevel::ProcessGroupOnly => Containment::ProcessGroupOnly,
    }
}

fn spawn_thread(name: &str, body: impl FnOnce() + Send + 'static) -> std::io::Result<()> {
    thread::Builder::new()
        .name(name.to_owned())
        .spawn(body)
        .map(|_| ())
}

/// The input writer thread: writes queued input until the queue closes or
/// the program has ended.
fn write_input(mut writer: PtyWriter, queue: &Receiver<Vec<u8>>) {
    for bytes in queue {
        if let Err(error) = writer.write_all(&bytes) {
            if error.kind() != ErrorKind::BrokenPipe {
                tracing::debug!(%error, "cannot write a program's input");
            }
            return;
        }
    }
}

/// The output reader thread: hands each chunk of output to the pump until
/// end-of-file. If reading fails, or the pump is gone, the program is killed,
/// since nothing would read its output any more.
fn read_output(mut reader: Box<dyn Read + Send>, chunks: &SyncSender<Vec<u8>>, run: &Run) {
    let mut buffer = vec![0; READ_CHUNK];
    loop {
        match reader.read(&mut buffer) {
            Ok(0) => return,
            Ok(read) => {
                if chunks.send(buffer[..read].to_vec()).is_err() {
                    run.child.kill();
                    return;
                }
            }
            Err(error) if error.kind() == ErrorKind::Interrupted => {}
            Err(error) => {
                tracing::warn!(%error, "cannot read a program's output; stopping it");
                run.child.kill();
                return;
            }
        }
    }
}

/// The pump thread's state: the only sender on the run's channels.
struct Pump {
    run: Arc<Run>,
    sessions: Weak<Shared>,
    id: RunId,
    output: Arc<dyn ByteSink>,
    events: Arc<dyn EventSink<RunEvent>>,
    started: RunEvent,
    limits: FlowLimits,
    span: tracing::Span,
}

impl Pump {
    fn run(self, chunks: &Receiver<Vec<u8>>) {
        let span = self.span.clone();
        let _entered = span.enter();
        self.events.send(self.started.clone());
        let mut coalescer = Coalescer::new(SystemClock, self.limits);
        let mut detector = Detector::new();
        let mut emits = Vec::new();
        let mut output_open = true;
        loop {
            coalescer.acknowledge(self.run.acked.load(Ordering::Acquire));
            coalescer.poll(&mut emits);
            self.deliver(&mut emits, &mut output_open);
            let next = match coalescer.wake_at() {
                None => chunks.recv().map_err(|_| RecvTimeoutError::Disconnected),
                Some(at) => chunks.recv_timeout(at.saturating_duration_since(Instant::now())),
            };
            match next {
                Ok(chunk) => {
                    detector.feed(&chunk);
                    if output_open {
                        coalescer.push(&chunk);
                    }
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
        }
        // The output has ended: the rest goes now, then the exit event.
        coalescer.acknowledge(self.run.acked.load(Ordering::Acquire));
        coalescer.finish(&mut emits);
        self.deliver(&mut emits, &mut output_open);

        let (report, elapsed_ms) = match self.run.child.wait() {
            Ok(exit) => (
                exit::decode(&exit, detector.report().as_ref()),
                millis(exit.duration),
            ),
            Err(error) => {
                // The error names the program's path: debug level only.
                tracing::warn!("lost track of a program");
                tracing::debug!(%error, "lost track of a program");
                (lost_track(), 0)
            }
        };
        span.record("elapsed_ms", elapsed_ms);
        span.record("outcome", outcome(report.status));
        tracing::info!(
            skipped_lines = coalescer.total_dropped_lines(),
            batches = coalescer.seq(),
            "program ended"
        );
        self.events.send(RunEvent::Exit {
            after_seq: coalescer.seq(),
            elapsed_ms,
            status: report.status,
            crash: report.crash,
            sanitizer: report.sanitizer,
            message: report.message,
        });
        self.run.end();
        if let Some(sessions) = self.sessions.upgrade() {
            let mut runs = sessions.lock();
            runs.finished.push_back(self.id);
            while runs.finished.len() > MAX_FINISHED_RUNS {
                if let Some(oldest) = runs.finished.pop_front() {
                    runs.by_id.remove(&oldest);
                }
            }
        }
    }

    /// Sends what the coalescer emitted. Once the output channel is gone,
    /// output is no longer sent (events still are).
    fn deliver(&self, emits: &mut Vec<Emit>, output_open: &mut bool) {
        for emit in emits.drain(..) {
            match emit {
                Emit::Batch { seq, bytes } => {
                    // Recorded before the send, so the acknowledgement of this
                    // batch can never arrive first.
                    self.run.sent.store(seq, Ordering::Release);
                    if *output_open && !self.output.send(bytes) {
                        *output_open = false;
                        tracing::debug!("the output channel closed; discarding the program's output");
                    }
                }
                Emit::Skipped { lines, after_seq, .. } => {
                    self.events.send(RunEvent::Skipped { lines, after_seq });
                }
            }
        }
    }
}

fn millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

/// The exit of a program the session could no longer watch.
fn lost_track() -> ExitReport {
    ExitReport {
        status: IpcStatus::Exited { code: -1 },
        crash: Some(exit::ipc_crash(b2c_process::Crash::Other)),
        sanitizer: None,
        message: String::from("Ended: Blocks2Cpp lost track of the program"),
    }
}

/// The outcome recorded in the `run` span.
fn outcome(status: IpcStatus) -> &'static str {
    match status {
        IpcStatus::Exited { code: 0 } => "finished",
        IpcStatus::Exited { .. } => "exitCode",
        IpcStatus::Signaled { .. } | IpcStatus::Exception { .. } => "crashed",
        IpcStatus::Stopped => "stopped",
    }
}

#[cfg(test)]
mod tests {
    use b2c_ipc::sink::testing::{RecordingBytes, RecordingSink};

    use super::*;

    fn spec(size: PtySize) -> RunSpec {
        RunSpec {
            project_key: String::from("p"),
            executable: PathBuf::from(if cfg!(windows) {
                r"C:\nowhere\x.exe"
            } else {
                "/nowhere/x"
            }),
            args: vec![OsString::from("--secret-arg")],
            working_dir: std::env::temp_dir(),
            env: vec![(OsString::from("TOKEN"), OsString::from("secret-value"))],
            size,
            scrollback_lines: 10_000,
            ide_helpers: true,
            prefer_pty: true,
            hold: None,
        }
    }

    #[test]
    fn sizes_are_checked_before_anything_starts() {
        let sessions = RunSessions::new();
        let sinks = || -> (Arc<dyn ByteSink>, Arc<dyn EventSink<RunEvent>>) {
            (Arc::new(RecordingBytes::new()), Arc::new(RecordingSink::new()))
        };
        for (cols, rows, field) in [
            (1, 24, "runOptions.cols"),
            (1001, 24, "runOptions.cols"),
            (80, 0, "runOptions.rows"),
            (80, 1001, "runOptions.rows"),
        ] {
            let (output, events) = sinks();
            assert_eq!(
                sessions
                    .start(spec(PtySize { cols, rows }), output, events)
                    .unwrap_err(),
                IpcError::invalid(InvalidReason::OutOfRange, Some(field))
            );
        }
        let id = RunId::example();
        assert_eq!(
            sessions.resize(&id, 2, 0).unwrap_err(),
            IpcError::invalid(InvalidReason::OutOfRange, Some("rows"))
        );
        assert_eq!(
            sessions.resize(&id, 1000, 1001).unwrap_err(),
            IpcError::invalid(InvalidReason::OutOfRange, Some("rows"))
        );
        assert_eq!(sessions.resize(&id, 2, 1).unwrap_err(), IpcError::UnknownRun);
    }

    #[test]
    fn a_missing_program_is_an_io_error_and_leaves_nothing_behind() {
        let sessions = RunSessions::new();
        let events = Arc::new(RecordingSink::new());
        let error = sessions
            .start(
                spec(PtySize::default()),
                Arc::new(RecordingBytes::new()),
                events.clone(),
            )
            .unwrap_err();
        assert!(
            matches!(error, IpcError::Io { .. } | IpcError::Internal),
            "{error:?}"
        );
        assert!(events.is_empty(), "no event for a run that never started");
        assert!(!sessions.is_running("p"));
        assert_eq!(sessions.shared.lock().by_id.len(), 0);
    }

    #[test]
    fn unknown_runs_and_bad_input_are_refused() {
        let sessions = RunSessions::new();
        let id = RunId::example();
        assert_eq!(sessions.input(&id, b"x").unwrap_err(), IpcError::UnknownRun);
        assert_eq!(
            sessions
                .input(&id, &vec![0; MAX_RUN_INPUT_BYTES + 1])
                .unwrap_err(),
            IpcError::too_large(MAX_RUN_INPUT_BYTES)
        );
        assert_eq!(sessions.stop(&id).unwrap_err(), IpcError::UnknownRun);
        assert_eq!(sessions.ack(&id, 0).unwrap_err(), IpcError::UnknownRun);
        assert_eq!(
            sessions.ack(&id, MAX_SAFE_INTEGER + 1).unwrap_err(),
            IpcError::invalid(InvalidReason::OutOfRange, Some("seq"))
        );
        sessions.stop_project("p");
        sessions.stop_all();
        assert!(!sessions.is_running("p"));
    }

    #[test]
    fn debug_output_hides_arguments_and_environment() {
        let text = format!("{:?}", spec(PtySize::default()));
        assert!(!text.contains("secret"), "{text}");
        assert!(text.contains("project_key"));
        assert!(format!("{:?}", RunSessions::new()).contains("runs: 0"));
    }

    #[test]
    fn outcomes_and_the_lost_track_exit() {
        assert_eq!(outcome(IpcStatus::Exited { code: 0 }), "finished");
        assert_eq!(outcome(IpcStatus::Exited { code: 2 }), "exitCode");
        assert_eq!(outcome(IpcStatus::Signaled { signal: 11 }), "crashed");
        assert_eq!(
            outcome(IpcStatus::Exception {
                ntstatus: 0xC000_0005
            }),
            "crashed"
        );
        assert_eq!(outcome(IpcStatus::Stopped), "stopped");
        let lost = lost_track();
        assert_eq!(lost.status, IpcStatus::Exited { code: -1 });
        assert_eq!(millis(Duration::MAX), u64::MAX);
        assert_eq!(ipc_containment(ContainmentLevel::Cgroup), Containment::Cgroup);
        assert_eq!(
            ipc_containment(ContainmentLevel::JobObject),
            Containment::JobObject
        );
    }
}
