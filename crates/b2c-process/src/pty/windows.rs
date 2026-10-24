//! Windows sessions: a pseudo console (`ConPTY`) or pipes, a program created
//! suspended with `CreateProcessW`, and a Job Object assigned before it runs.
//!
//! **Creation.** The program is created suspended by `platform/create.rs`
//! (an absolute `.exe` path, a command line quoted with the C runtime rules,
//! exactly the command's environment). It is assigned to a new Job Object
//! (`KILL_ON_JOB_CLOSE | DIE_ON_UNHANDLED_EXCEPTION`, see
//! `platform/windows.rs`) and only then resumed, so nothing it starts can
//! escape the job.
//!
//! **Handles.** In PTY mode the only attribute is
//! `PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE`, and `bInheritHandles` is FALSE:
//! the program inherits nothing and gets its standard handles from the
//! pseudo console. The standard handles in `STARTUPINFOW` are set to null
//! with `STARTF_USESTDHANDLES`, so redirected standard handles of this
//! process (a log file, a test harness's pipe) are never passed on. In pipe
//! mode `PROC_THREAD_ATTRIBUTE_HANDLE_LIST` names the program's pipe ends
//! (standard input; standard output and error share one), which the list
//! requires to be inheritable; they are made inheritable just before
//! creation and closed right after it (see `platform/create.rs` for the one
//! remaining window).
//!
//! **Output.** The pseudo console keeps its own copy of the output pipe, so
//! the reader sees end-of-file only when it is closed. That happens on the
//! supervisor thread after the program's exit has been recorded:
//! `ClosePseudoConsole` flushes the last output (on older Windows 10 it
//! waits until the output pipe has been read, which is why the caller
//! reads output on a thread of its own), then the reader reaches
//! end-of-file.

use std::fs::File;
use std::io::{self, Read, Write as _};
use std::mem::{size_of, size_of_val};
use std::os::windows::io::{AsHandle as _, AsRawHandle as _, OwnedHandle};
use std::ptr;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Instant;

use windows_sys::Win32::Foundation::{HANDLE, S_OK};
use windows_sys::Win32::System::Console::{
    COORD, ClosePseudoConsole, CreatePseudoConsole, HPCON, ResizePseudoConsole,
};
use windows_sys::Win32::System::Threading::{
    CREATE_NO_WINDOW, INFINITE, PROC_THREAD_ATTRIBUTE_HANDLE_LIST, PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE,
    STARTF_USESTDHANDLES,
};

use super::session::{Shared, Supervised};
use super::{Io, PtySize};
use crate::command::Command;
use crate::containment::{self, ContainmentLevel, Kind};
use crate::error::ProcessError;
use crate::platform::create::{self, AttributeList, Created, Launch};
use crate::platform::{self, Tree};
use crate::status::ExitStatus;

/// Buffer size asked for each pipe (a hint; Windows may round it).
const PIPE_BUFFER: u32 = 64 * 1024;

/// A started session, before the supervisor takes over.
pub(super) struct Spawned {
    pub(super) program: Program,
    pub(super) tree: Tree,
    pub(super) pid: u32,
    pub(super) started: Instant,
    pub(super) output: OutputEnd,
    pub(super) input: InputEnd,
    pub(super) terminal: Terminal,
    pub(super) level: ContainmentLevel,
}

/// A pseudo console shared by the session (to resize it) and the
/// supervisor (to close it after the program's exit).
type SharedConsole = Arc<Mutex<Option<PseudoConsole>>>;

/// Starts `command` suspended, connected as `io` says, contains it in a new
/// Job Object and resumes it.
pub(super) fn spawn(command: &Command, io: Io) -> Result<Spawned, ProcessError> {
    let program = command.program().to_path_buf();
    let plan = containment::plan(command, Kind::Run)?;
    let mut launch = Launch::new(&plan.command)?;
    let tree = platform::job_tree(&plan.tree.enforcement).map_err(|source| ProcessError::Containment {
        program: program.clone(),
        source,
    })?;
    let connected = match io {
        Io::Pty(size) => in_console(&mut launch, size),
        Io::Pipes => with_pipes(&mut launch),
    };
    let Connected {
        created,
        output,
        input,
        console,
    } = connected.map_err(|failure| match failure {
        Failure::Pty(source) => ProcessError::Pty(source),
        Failure::Spawn(source) => ProcessError::Spawn {
            program: program.clone(),
            source,
        },
    })?;
    let Created { process, thread, pid } = created;
    if let Err(source) = tree
        .assign(process.as_handle())
        .and_then(|()| create::resume(&thread))
    {
        create::discard(&process);
        // The output end goes first, so closing the console cannot wait for
        // output nobody reads.
        drop(output);
        drop(console);
        return Err(ProcessError::Containment { program, source });
    }
    let started = Instant::now();
    drop(thread);
    Ok(Spawned {
        program: Program {
            process,
            console: console.clone(),
        },
        tree,
        pid,
        started,
        output: OutputEnd(File::from(output)),
        input: InputEnd(File::from(input)),
        terminal: Terminal { console },
        level: plan.level,
    })
}

/// Why connecting the program failed.
enum Failure {
    /// The pseudo console (or its pipes) could not be created.
    Pty(io::Error),
    /// Creating the pipes or the process failed.
    Spawn(io::Error),
}

/// A created, still suspended program and this process's ends of its I/O.
struct Connected {
    created: Created,
    output: OwnedHandle,
    input: OwnedHandle,
    console: Option<SharedConsole>,
}

/// Creates the program attached to a new pseudo console of `size`.
fn in_console(launch: &mut Launch, size: PtySize) -> Result<Connected, Failure> {
    let (console_input, input) = create::pipe(PIPE_BUFFER).map_err(Failure::Pty)?;
    let (output, console_output) = create::pipe(PIPE_BUFFER).map_err(Failure::Pty)?;
    let console = PseudoConsole::create(size, &console_input, &console_output).map_err(Failure::Pty)?;
    // The pseudo console holds its own duplicates of its ends.
    drop(console_input);
    drop(console_output);
    let created = AttributeList::new().and_then(|mut attributes| {
        // SAFETY: for this attribute the value is the pseudo console
        // handle itself (not a pointer to it), as the documentation of
        // `UpdateProcThreadAttribute` specifies; `console` keeps it open
        // until after the process has been created.
        unsafe {
            attributes.set(
                PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE,
                ptr::without_provenance(console.0.cast_unsigned()),
                size_of::<HPCON>(),
            )?;
        }
        let mut startup = create::startup_info(&mut attributes);
        // Null standard handles: the pseudo console supplies them.
        startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
        launch.create(&startup, false, 0)
    });
    match created {
        Ok(created) => Ok(Connected {
            created,
            output,
            input,
            console: Some(Arc::new(Mutex::new(Some(console)))),
        }),
        Err(error) => {
            // Close the output end first, so closing the console cannot wait
            // for output nobody reads.
            drop(output);
            drop(console);
            Err(Failure::Spawn(error))
        }
    }
}

/// Creates the program with pipes: standard input from one pipe, standard
/// output and error into another, and no other handle inherited.
fn with_pipes(launch: &mut Launch) -> Result<Connected, Failure> {
    let (child_input, input) = create::pipe(PIPE_BUFFER).map_err(Failure::Spawn)?;
    let (output, child_output) = create::pipe(PIPE_BUFFER).map_err(Failure::Spawn)?;
    let handles: [HANDLE; 2] = [child_input.as_raw_handle(), child_output.as_raw_handle()];
    let created = create::set_inheritable(&child_input)
        .and_then(|()| create::set_inheritable(&child_output))
        .and_then(|()| AttributeList::new())
        .and_then(|mut attributes| {
            // SAFETY: `handles` is an array of two valid handles (the program's
            // ends, open until after this closure) that lives, unchanged,
            // until after the process has been created; the size is the
            // array's exact size in bytes.
            unsafe {
                attributes.set(
                    PROC_THREAD_ATTRIBUTE_HANDLE_LIST,
                    handles.as_ptr().cast(),
                    size_of_val(&handles),
                )?;
            }
            let mut startup = create::startup_info(&mut attributes);
            startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
            startup.StartupInfo.hStdInput = child_input.as_raw_handle();
            startup.StartupInfo.hStdOutput = child_output.as_raw_handle();
            startup.StartupInfo.hStdError = child_output.as_raw_handle();
            // A console program started from the app would otherwise open a
            // console window.
            launch.create(&startup, true, CREATE_NO_WINDOW)
        });
    // This process's copies of the program's ends (inheritable until now)
    // are closed at once: only the program's copies remain, so the reader
    // sees end-of-file when the program's tree has ended.
    drop(child_input);
    drop(child_output);
    Ok(Connected {
        created: created.map_err(Failure::Spawn)?,
        output,
        input,
        console: None,
    })
}

/// A pseudo console; dropping it closes it (`ClosePseudoConsole`).
pub(super) struct PseudoConsole(HPCON);

impl PseudoConsole {
    /// Creates a pseudo console of `size` that reads keystrokes from `input`
    /// and writes its output to `output` (it duplicates both handles).
    fn create(size: PtySize, input: &OwnedHandle, output: &OwnedHandle) -> io::Result<Self> {
        let mut console: HPCON = 0;
        // SAFETY: `input` and `output` are valid pipe ends, which the call
        // duplicates; `console` is valid for writes. Flags 0: no cursor
        // inheritance, so no terminal query has to be answered.
        let result = unsafe {
            CreatePseudoConsole(
                coord(size),
                input.as_raw_handle(),
                output.as_raw_handle(),
                0,
                &raw mut console,
            )
        };
        if result != S_OK {
            return Err(hresult_error(result));
        }
        Ok(Self(console))
    }

    fn resize(&self, size: PtySize) -> io::Result<()> {
        // SAFETY: `self.0` is an open pseudo console (it is closed only when
        // this value is dropped).
        let result = unsafe { ResizePseudoConsole(self.0, coord(size)) };
        if result != S_OK {
            return Err(hresult_error(result));
        }
        Ok(())
    }
}

impl Drop for PseudoConsole {
    fn drop(&mut self) {
        // SAFETY: `self.0` is an open pseudo console owned by this value and
        // closed exactly once, here.
        unsafe {
            ClosePseudoConsole(self.0);
        }
    }
}

/// A console size (both sides were validated to fit an `i16`).
fn coord(size: PtySize) -> COORD {
    COORD {
        X: i16::try_from(size.cols).unwrap_or(i16::MAX),
        Y: i16::try_from(size.rows).unwrap_or(i16::MAX),
    }
}

/// An `HRESULT` as an `io::Error`: Win32 errors keep their code.
fn hresult_error(result: i32) -> io::Error {
    let bits = result.cast_unsigned();
    if bits & 0xFFFF_0000 == 0x8007_0000 {
        io::Error::from_raw_os_error(i32::try_from(bits & 0xFFFF).unwrap_or(0))
    } else {
        io::Error::other(format!("the system returned HRESULT 0x{bits:08X}"))
    }
}

/// The program, owned by the supervisor thread.
pub(super) struct Program {
    process: OwnedHandle,
    console: Option<SharedConsole>,
}

impl Supervised for Program {
    fn has_exited(&mut self) -> io::Result<bool> {
        create::wait(&self.process, 0)
    }

    fn reap(&mut self) -> io::Result<ExitStatus> {
        create::wait(&self.process, INFINITE)?;
        create::exit_status(&self.process)
    }

    fn finish(self) {
        if let Some(console) = self.console {
            let console = console.lock().unwrap_or_else(PoisonError::into_inner).take();
            // Closing flushes the last output; the reader then sees
            // end-of-file. Done outside the lock, as it may wait for the
            // reader.
            drop(console);
        }
    }
}

/// The terminal side of a session, for resizing.
pub(super) struct Terminal {
    console: Option<SharedConsole>,
}

impl Terminal {
    /// Resizes the pseudo console; the program gets a buffer-size event.
    /// Pipe sessions have none, and after the program's exit it is closed.
    pub(super) fn resize(&self, size: PtySize) -> io::Result<()> {
        if let Some(console) = &self.console
            && let Some(console) = console.lock().unwrap_or_else(PoisonError::into_inner).as_ref()
        {
            console.resize(size)?;
        }
        Ok(())
    }
}

/// The end this process reads the program's output from.
pub(super) struct OutputEnd(File);

impl OutputEnd {
    /// Reads until the pipe's last writer (the pseudo console, or the
    /// program's tree in pipe mode) has closed it; std reports a broken pipe
    /// as end-of-file.
    pub(super) fn into_reader(self, _shared: &Arc<Shared>) -> Box<dyn Read + Send> {
        Box::new(self.0)
    }
}

/// The end this process writes the program's input to.
pub(super) struct InputEnd(File);

impl InputEnd {
    pub(super) fn into_writer(self, shared: &Arc<Shared>) -> Writer {
        Writer {
            file: self.0,
            shared: Arc::clone(shared),
        }
    }
}

/// Writes the program's input (see [`super::PtyWriter`]).
pub(super) struct Writer {
    file: File,
    shared: Arc<Shared>,
}

impl Writer {
    pub(super) fn write(&self, buf: &[u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        if self.shared.has_ended() {
            return Err(io::ErrorKind::BrokenPipe.into());
        }
        // A closed reader (the program's tree or the pseudo console has
        // gone) is reported by std as `BrokenPipe`.
        (&self.file).write(buf)
    }
}
