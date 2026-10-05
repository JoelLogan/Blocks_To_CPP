//! Unix sessions: a pseudo-terminal from `/dev/ptmx` (or pipes), with the
//! program leading a session of its own.
//!
//! The terminal is opened here (`posix_openpt`, `grantpt`, `unlockpt`, then
//! the peer with `TIOCGPTPEER`, or by name on kernels before 4.13), sized
//! with `TIOCSWINSZ`, and its slave side becomes the program's standard
//! input, output and error. In the child, between `fork` and `exec`, the
//! program calls `setsid` (a new session and process group, so `pgid ==
//! pid`) and in PTY mode `TIOCSCTTY` (the terminal becomes its controlling
//! terminal, so `0x03` is delivered as `SIGINT` to its foreground group).
//! That hook is the only `unsafe` code here: `pre_exec` itself is unsafe
//! because the closure runs in a forked copy of a multi-threaded process.
//!
//! This process keeps only the master side (or its pipe ends), non-blocking
//! and close-on-exec. Reads and writes wait in `poll`, so they can notice
//! that the program has ended even when a process that escaped the tree
//! keeps the terminal open. A write to a pipe whose reader is gone fails
//! with `EPIPE`; like every Rust program, this process ignores `SIGPIPE`
//! (the standard library's start-up code sets that).

use std::io::{self, Read};
use std::os::fd::OwnedFd;
use std::os::unix::process::CommandExt as _;
use std::process::{Child, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use rustix::event::{PollFd, PollFlags, Timespec, poll};
use rustix::io::Errno;
use rustix::pty::{OpenptFlags, grantpt, openpt, unlockpt};
use rustix::termios::{Winsize, tcsetwinsize};

use super::session::{Shared, Supervised};
use super::{ContainmentLevel, Io, PtySize};
use crate::command::Command;
use crate::error::ProcessError;
use crate::platform::{self, Placement, Tree};
use crate::status::ExitStatus;

/// What Unix sessions guarantee without a cgroup scope.
pub(super) const CONTAINMENT: ContainmentLevel = ContainmentLevel::ProcessGroupOnly;

/// After the program ended, how long the output reader waits for
/// end-of-file while no output arrives before it gives up (a process that
/// escaped the tree may hold the terminal open).
const DRAIN_QUIET: Duration = Duration::from_secs(2);
/// After the program ended, the longest the output reader keeps reading (an
/// escaped process could otherwise keep a session's output open forever).
const DRAIN_LIMIT: Duration = Duration::from_secs(10);

/// How long one `poll` waits before the reader or writer checks whether the
/// program has ended.
const TICK: Timespec = Timespec {
    tv_sec: 0,
    tv_nsec: 50_000_000,
};

/// A started session, before the supervisor takes over.
pub(super) struct Spawned {
    pub(super) program: Program,
    pub(super) tree: Tree,
    pub(super) pid: u32,
    pub(super) started: Instant,
    pub(super) output: OutputEnd,
    pub(super) input: InputEnd,
    pub(super) terminal: Terminal,
}

/// Starts `command` in a new session, connected as `io` says.
pub(super) fn spawn(command: &Command, io: Io) -> Result<Spawned, ProcessError> {
    let program = command.program().to_path_buf();
    let spawn_error = |source: io::Error| ProcessError::Spawn {
        program: program.clone(),
        source,
    };
    let mut std_command = command.to_std();
    let (output, input, master) = match io {
        Io::Pty(size) => {
            let (master, slave) = open_terminal(size).map_err(ProcessError::Pty)?;
            // Only this process's side: the slave is a separate open file.
            rustix::io::ioctl_fionbio(&master, true).map_err(|errno| ProcessError::Pty(errno.into()))?;
            let slave_out = slave.try_clone().map_err(ProcessError::Pty)?;
            let slave_err = slave.try_clone().map_err(ProcessError::Pty)?;
            std_command
                .stdin(Stdio::from(slave))
                .stdout(Stdio::from(slave_out))
                .stderr(Stdio::from(slave_err));
            new_session(&mut std_command, true);
            let master = Arc::new(master);
            (Arc::clone(&master), Arc::clone(&master), Some(master))
        }
        Io::Pipes => {
            let (output_read, output_write) = io::pipe().map_err(spawn_error)?;
            let (input_read, input_write) = io::pipe().map_err(spawn_error)?;
            let error_write = output_write.try_clone().map_err(spawn_error)?;
            // Only this process's ends: each end of a pipe is a separate open
            // file, so the program's ends stay blocking.
            rustix::io::ioctl_fionbio(&output_read, true)
                .and_then(|()| rustix::io::ioctl_fionbio(&input_write, true))
                .map_err(|errno| spawn_error(errno.into()))?;
            std_command
                .stdin(input_read)
                .stdout(output_write)
                .stderr(error_write);
            new_session(&mut std_command, false);
            (
                Arc::new(OwnedFd::from(output_read)),
                Arc::new(OwnedFd::from(input_write)),
                None,
            )
        }
    };
    let spawned = std_command.spawn();
    // The command holds this process's copies of the child's ends (the
    // terminal's slave side, the pipe ends). They must be closed now, or the
    // reader would never see end-of-file.
    drop(std_command);
    let mut child = spawned.map_err(spawn_error)?;
    let started = Instant::now();
    let pid = child.id();
    let tree = match platform::contain(&mut child, Placement::NewGroup, command.get_limits()) {
        Ok(tree) => tree,
        Err(source) => {
            abandon(&mut child);
            return Err(ProcessError::Containment { program, source });
        }
    };
    Ok(Spawned {
        program: Program { child },
        tree,
        pid,
        started,
        output: OutputEnd(output),
        input: InputEnd(input),
        terminal: Terminal { master },
    })
}

/// Kills and reaps a child that could not be put under control. It leads
/// its own process group, so killing the group takes anything it started.
fn abandon(child: &mut Child) {
    if let Ok(pid) = i32::try_from(child.id())
        && let Some(pid) = rustix::process::Pid::from_raw(pid)
    {
        // The child has not been reaped, so its process group ID is still
        // reserved.
        let _ = rustix::process::kill_process_group(pid, rustix::process::Signal::KILL);
    }
    let _ = child.kill();
    let _ = child.wait();
}

/// Opens a pseudo-terminal of `size`: the master side and the slave side,
/// both close-on-exec, neither becoming this process's controlling terminal.
fn open_terminal(size: PtySize) -> io::Result<(OwnedFd, OwnedFd)> {
    let master = openpt(OpenptFlags::RDWR | OpenptFlags::NOCTTY | cloexec())?;
    set_cloexec(&master)?;
    grantpt(&master)?;
    unlockpt(&master)?;
    let slave = open_slave(&master)?;
    set_cloexec(&slave)?;
    tcsetwinsize(&master, winsize(size))?;
    Ok((master, slave))
}

/// The systems that open terminals close-on-exec atomically.
#[cfg(any(
    target_os = "linux",
    target_os = "android",
    target_os = "freebsd",
    target_os = "netbsd"
))]
fn cloexec() -> OpenptFlags {
    OpenptFlags::CLOEXEC
}

#[cfg(any(
    target_os = "linux",
    target_os = "android",
    target_os = "freebsd",
    target_os = "netbsd"
))]
#[allow(clippy::unnecessary_wraps)] // same signature as the fallback below
fn set_cloexec(_fd: &OwnedFd) -> io::Result<()> {
    Ok(())
}

#[cfg(not(any(
    target_os = "linux",
    target_os = "android",
    target_os = "freebsd",
    target_os = "netbsd"
)))]
fn cloexec() -> OpenptFlags {
    OpenptFlags::empty()
}

/// Sets close-on-exec after the fact, on systems without an atomic flag. A
/// process spawned by another thread in between could inherit the
/// descriptor; that is the best these systems allow.
#[cfg(not(any(
    target_os = "linux",
    target_os = "android",
    target_os = "freebsd",
    target_os = "netbsd"
)))]
fn set_cloexec(fd: &OwnedFd) -> io::Result<()> {
    let flags = rustix::io::fcntl_getfd(fd)?;
    rustix::io::fcntl_setfd(fd, flags | rustix::io::FdFlags::CLOEXEC)?;
    Ok(())
}

/// Opens the slave side of `master`: through the master itself where the
/// kernel supports it (no path lookup, so nothing can be swapped in), by
/// name otherwise.
#[cfg(target_os = "linux")]
fn open_slave(master: &OwnedFd) -> io::Result<OwnedFd> {
    let flags = OpenptFlags::RDWR | OpenptFlags::NOCTTY | OpenptFlags::CLOEXEC;
    match rustix::pty::ioctl_tiocgptpeer(master, flags) {
        Ok(slave) => Ok(slave),
        // Kernels before 4.13 do not know TIOCGPTPEER.
        Err(Errno::INVAL | Errno::NOTTY | Errno::NOSYS) => open_slave_by_name(master),
        Err(errno) => Err(errno.into()),
    }
}

#[cfg(not(target_os = "linux"))]
fn open_slave(master: &OwnedFd) -> io::Result<OwnedFd> {
    open_slave_by_name(master)
}

fn open_slave_by_name(master: &OwnedFd) -> io::Result<OwnedFd> {
    use rustix::fs::{Mode, OFlags, open};
    let name = rustix::pty::ptsname(master, Vec::new())?;
    Ok(open(
        name.as_c_str(),
        OFlags::RDWR | OFlags::NOCTTY | OFlags::CLOEXEC,
        Mode::empty(),
    )?)
}

fn winsize(size: PtySize) -> Winsize {
    Winsize {
        ws_row: size.rows,
        ws_col: size.cols,
        ws_xpixel: 0,
        ws_ypixel: 0,
    }
}

/// Makes the child lead a new session (and so a new process group) before
/// it runs the program; with `controlling_terminal`, its standard input (the
/// terminal's slave side) becomes its controlling terminal.
fn new_session(command: &mut std::process::Command, controlling_terminal: bool) {
    let setup = move || -> io::Result<()> {
        rustix::process::setsid().map_err(os_error)?;
        if controlling_terminal {
            rustix::process::ioctl_tiocsctty(rustix::stdio::stdin()).map_err(os_error)?;
        }
        Ok(())
    };
    // SAFETY: the closure runs in the child between `fork` and `exec`, where
    // only async-signal-safe operations are allowed. It makes two system
    // calls (`setsid`, and `ioctl(0, TIOCSCTTY, 0)`) through rustix, which
    // issues them directly without locks or allocation; it captures only a
    // `bool`, touches no shared state, and builds its error with
    // `io::Error::from_raw_os_error`, which does not allocate. Standard
    // input is already the terminal's slave side at that point (std
    // redirects the standard descriptors before running the closure), and
    // the child is not a process group leader (no `process_group` is set),
    // so `setsid` succeeds.
    unsafe {
        command.pre_exec(setup);
    }
}

/// An error number as an `io::Error`, without allocating (safe after
/// `fork`).
fn os_error(errno: Errno) -> io::Error {
    io::Error::from_raw_os_error(errno.raw_os_error())
}

/// The program, owned by the supervisor thread.
pub(super) struct Program {
    child: Child,
}

impl Supervised for Program {
    fn has_exited(&mut self) -> io::Result<bool> {
        platform::has_exited(&mut self.child)
    }

    fn reap(&mut self) -> io::Result<ExitStatus> {
        self.child.wait().map(ExitStatus::from_std)
    }

    fn finish(self) {}
}

/// The terminal side of a session, for resizing.
pub(super) struct Terminal {
    master: Option<Arc<OwnedFd>>,
}

impl Terminal {
    /// Sets the terminal size; the kernel sends `SIGWINCH` to the program's
    /// foreground process group. Pipe sessions have no terminal.
    pub(super) fn resize(&self, size: PtySize) -> io::Result<()> {
        match &self.master {
            Some(master) => Ok(tcsetwinsize(&**master, winsize(size))?),
            None => Ok(()),
        }
    }
}

/// The end this process reads the program's output from.
pub(super) struct OutputEnd(Arc<OwnedFd>);

impl OutputEnd {
    pub(super) fn into_reader(self, shared: &Arc<Shared>) -> Box<dyn Read + Send> {
        Box::new(Reader {
            fd: self.0,
            shared: Arc::clone(shared),
        })
    }
}

/// The end this process writes the program's input to.
pub(super) struct InputEnd(Arc<OwnedFd>);

impl InputEnd {
    pub(super) fn into_writer(self, shared: &Arc<Shared>) -> Writer {
        Writer {
            fd: self.0,
            shared: Arc::clone(shared),
        }
    }
}

/// Reads the program's output until end-of-file: the terminal reports
/// `EIO` once no process has the slave side open any more (a pipe reads 0
/// bytes once no writer is left). After the program ended, a quiet output
/// for [`DRAIN_QUIET`], or [`DRAIN_LIMIT`] in all, also count as
/// end-of-file.
struct Reader {
    fd: Arc<OwnedFd>,
    shared: Arc<Shared>,
}

impl Read for Reader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        loop {
            let since_end = self.shared.since_end();
            if since_end.is_some_and(|since| since >= DRAIN_LIMIT) {
                return Ok(0);
            }
            let mut fds = [PollFd::new(&*self.fd, PollFlags::IN)];
            match poll(&mut fds, Some(&TICK)) {
                Ok(0) => {
                    if since_end.is_some_and(|since| since >= DRAIN_QUIET) {
                        return Ok(0);
                    }
                    continue;
                }
                Ok(_) | Err(Errno::INTR) => {}
                Err(errno) => return Err(errno.into()),
            }
            match rustix::io::read(&*self.fd, &mut *buf) {
                Ok(read) => return Ok(read),
                // The terminal has no slave side left: everything is read.
                Err(Errno::IO) => return Ok(0),
                Err(Errno::AGAIN | Errno::INTR) => {}
                Err(errno) => return Err(errno.into()),
            }
        }
    }
}

/// Writes the program's input (see [`super::PtyWriter`]).
pub(super) struct Writer {
    fd: Arc<OwnedFd>,
    shared: Arc<Shared>,
}

impl Writer {
    pub(super) fn write(&self, buf: &[u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        loop {
            if self.shared.has_ended() {
                return Err(io::ErrorKind::BrokenPipe.into());
            }
            match rustix::io::write(&*self.fd, buf) {
                Ok(written) => return Ok(written),
                Err(Errno::AGAIN) => {
                    let mut fds = [PollFd::new(&*self.fd, PollFlags::OUT)];
                    match poll(&mut fds, Some(&TICK)) {
                        Ok(_) | Err(Errno::INTR) => {}
                        Err(errno) => return Err(errno.into()),
                    }
                }
                Err(Errno::INTR) => {}
                // No reader left: the program and its tree have closed the
                // terminal or the pipe.
                Err(Errno::IO | Errno::PIPE) => return Err(io::ErrorKind::BrokenPipe.into()),
                Err(errno) => return Err(errno.into()),
            }
        }
    }
}
