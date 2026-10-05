//! Windows sessions: a pseudo console (`ConPTY`) or pipes, a program created
//! suspended with `CreateProcessW`, and a Job Object assigned before it runs.
//!
//! **Creation.** The program is started with `STARTUPINFOEXW` and an
//! attribute list, `CREATE_SUSPENDED`, `EXTENDED_STARTUPINFO_PRESENT` and
//! `CREATE_UNICODE_ENVIRONMENT`, from an absolute `.exe` path (never a
//! search), a command line quoted with the C runtime rules
//! (`super::cmdline`) and exactly the command's environment. It is assigned
//! to a new Job Object (`KILL_ON_JOB_CLOSE | DIE_ON_UNHANDLED_EXCEPTION`,
//! see `platform/windows.rs`) and only then resumed, so nothing it starts
//! can escape the job.
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
//! creation and closed right after it. A process that another thread
//! creates in that moment *without* a handle list (the standard library's
//! spawn, still used for captured runs) could inherit them; it would only
//! delay this session's end-of-file until it exits.
//!
//! **Output.** The pseudo console keeps its own copy of the output pipe, so
//! the reader sees end-of-file only when it is closed. That happens on the
//! supervisor thread after the program's exit has been recorded:
//! `ClosePseudoConsole` flushes the last output (on older Windows 10 it
//! waits until the output pipe has been read, which is why the caller
//! reads output on a thread of its own), then the reader reaches
//! end-of-file.

use std::ffi::{OsStr, c_void};
use std::fs::File;
use std::io::{self, Read, Write as _};
use std::mem::{size_of, size_of_val};
use std::os::windows::ffi::OsStrExt as _;
use std::os::windows::io::{AsHandle as _, AsRawHandle as _, FromRawHandle as _, OwnedHandle};
use std::ptr;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Instant;

use windows_sys::Win32::Foundation::{
    HANDLE, HANDLE_FLAG_INHERIT, S_OK, SetHandleInformation, WAIT_OBJECT_0, WAIT_TIMEOUT,
};
use windows_sys::Win32::System::Console::{
    COORD, ClosePseudoConsole, CreatePseudoConsole, HPCON, ResizePseudoConsole,
};
use windows_sys::Win32::System::Pipes::CreatePipe;
use windows_sys::Win32::System::Threading::{
    CREATE_NO_WINDOW, CREATE_SUSPENDED, CREATE_UNICODE_ENVIRONMENT, CreateProcessW,
    DeleteProcThreadAttributeList, EXTENDED_STARTUPINFO_PRESENT, GetExitCodeProcess, INFINITE,
    InitializeProcThreadAttributeList, LPPROC_THREAD_ATTRIBUTE_LIST, PROC_THREAD_ATTRIBUTE_HANDLE_LIST,
    PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE, PROCESS_CREATION_FLAGS, PROCESS_INFORMATION, ResumeThread,
    STARTF_USESTDHANDLES, STARTUPINFOEXW, STARTUPINFOW, TerminateProcess, UpdateProcThreadAttribute,
    WaitForSingleObject,
};

use super::cmdline;
use super::session::{Shared, Supervised};
use super::{ContainmentLevel, Io, PtySize};
use crate::command::Command;
use crate::error::ProcessError;
use crate::platform::{self, Tree};
use crate::status::ExitStatus;

/// What Windows sessions guarantee.
pub(super) const CONTAINMENT: ContainmentLevel = ContainmentLevel::JobObject;

/// Buffer size asked for each pipe (a hint; Windows may round it).
const PIPE_BUFFER: u32 = 64 * 1024;

/// Exit code given to a program terminated before it ran.
const DISCARDED_EXIT_CODE: u32 = 1;

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

/// A pseudo console shared by the session (to resize it) and the
/// supervisor (to close it after the program's exit).
type SharedConsole = Arc<Mutex<Option<PseudoConsole>>>;

/// Starts `command` suspended, connected as `io` says, contains it in a new
/// Job Object and resumes it.
pub(super) fn spawn(command: &Command, io: Io) -> Result<Spawned, ProcessError> {
    let program = command.program().to_path_buf();
    let invalid = |reason: &'static str| ProcessError::InvalidCommand {
        program: program.clone(),
        reason,
    };
    let program_wide = wide(command.program().as_os_str());
    let args: Vec<Vec<u16>> = command.get_args().iter().map(|arg| wide(arg)).collect();
    let mut launch = Launch {
        application: cmdline::nul_terminated(&program_wide, "the program path contains a NUL character")
            .map_err(invalid)?,
        command_line: cmdline::command_line(&program_wide, &args).map_err(invalid)?,
        environment: cmdline::environment_block(
            command
                .get_envs()
                .map(|(name, value)| (wide(name), wide(value)))
                .collect(),
        )
        .map_err(invalid)?,
        directory: cmdline::nul_terminated(
            &wide(command.working_dir().as_os_str()),
            "the working folder contains a NUL character",
        )
        .map_err(invalid)?,
    };
    let tree = platform::job_tree(command.get_limits()).map_err(|source| ProcessError::Containment {
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
    if let Err(source) = tree.assign(process.as_handle()).and_then(|()| resume(&thread)) {
        discard(&process);
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
    let (console_input, input) = pipe(PIPE_BUFFER).map_err(Failure::Pty)?;
    let (output, console_output) = pipe(PIPE_BUFFER).map_err(Failure::Pty)?;
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
        let mut startup = startup_info(&mut attributes);
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
    let (child_input, input) = pipe(PIPE_BUFFER).map_err(Failure::Spawn)?;
    let (output, child_output) = pipe(PIPE_BUFFER).map_err(Failure::Spawn)?;
    let handles: [HANDLE; 2] = [child_input.as_raw_handle(), child_output.as_raw_handle()];
    let created = set_inheritable(&child_input)
        .and_then(|()| set_inheritable(&child_output))
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
            let mut startup = startup_info(&mut attributes);
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

/// The parts of `CreateProcessW` that do not depend on the I/O mode, each
/// NUL-terminated (the environment block ends with two NULs).
struct Launch {
    application: Vec<u16>,
    command_line: Vec<u16>,
    environment: Vec<u16>,
    directory: Vec<u16>,
}

/// A created, suspended process.
struct Created {
    process: OwnedHandle,
    thread: OwnedHandle,
    pid: u32,
}

impl Launch {
    /// Creates the process, suspended.
    fn create(
        &mut self,
        startup: &STARTUPINFOEXW,
        inherit_handles: bool,
        flags: PROCESS_CREATION_FLAGS,
    ) -> io::Result<Created> {
        let flags = flags | CREATE_SUSPENDED | EXTENDED_STARTUPINFO_PRESENT | CREATE_UNICODE_ENVIRONMENT;
        let mut info = PROCESS_INFORMATION::default();
        // SAFETY: every pointer is valid for the duration of the call: the
        // NUL-terminated application path and working directory, the
        // mutable NUL-terminated command line (CreateProcessW may write to
        // it), the environment block in the CREATE_UNICODE_ENVIRONMENT
        // format, a STARTUPINFOEXW whose `cb` covers the extended structure
        // (EXTENDED_STARTUPINFO_PRESENT) and whose attribute list, with the
        // values it points to, is alive, and `info`, valid for writes. Null
        // security attributes are allowed.
        let ok = unsafe {
            CreateProcessW(
                self.application.as_ptr(),
                self.command_line.as_mut_ptr(),
                ptr::null(),
                ptr::null(),
                i32::from(inherit_handles),
                flags,
                self.environment.as_ptr().cast(),
                self.directory.as_ptr(),
                ptr::from_ref(startup).cast::<STARTUPINFOW>(),
                &raw mut info,
            )
        };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: CreateProcessW succeeded, so both handles are valid, open
        // and owned by this process; `OwnedHandle` closes each once.
        let (process, thread) = unsafe {
            (
                OwnedHandle::from_raw_handle(info.hProcess),
                OwnedHandle::from_raw_handle(info.hThread),
            )
        };
        Ok(Created {
            process,
            thread,
            pid: info.dwProcessId,
        })
    }
}

/// A `STARTUPINFOEXW` that uses `attributes`.
fn startup_info(attributes: &mut AttributeList) -> STARTUPINFOEXW {
    let mut startup = STARTUPINFOEXW::default();
    startup.StartupInfo.cb = u32::try_from(size_of::<STARTUPINFOEXW>()).unwrap_or(u32::MAX);
    startup.lpAttributeList = attributes.as_mut_ptr();
    startup
}

/// A `PROC_THREAD_ATTRIBUTE_LIST` with room for one attribute.
struct AttributeList {
    /// Storage for the opaque list, in pointer-sized words so it is aligned
    /// for the pointers it holds.
    buffer: Vec<usize>,
}

impl AttributeList {
    fn new() -> io::Result<Self> {
        let mut size = 0_usize;
        // SAFETY: with a null list the call only reports, through `size` (a
        // valid pointer), how many bytes a list of one attribute needs; it
        // fails with ERROR_INSUFFICIENT_BUFFER by design.
        unsafe {
            InitializeProcThreadAttributeList(ptr::null_mut(), 1, 0, &raw mut size);
        }
        if size == 0 {
            return Err(io::Error::last_os_error());
        }
        let mut buffer = vec![0_usize; size.div_ceil(size_of::<usize>())];
        // SAFETY: `buffer` provides at least `size` writable bytes, aligned
        // for pointers, and outlives the list (it is deleted in `drop`).
        let ok =
            unsafe { InitializeProcThreadAttributeList(buffer.as_mut_ptr().cast(), 1, 0, &raw mut size) };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Self { buffer })
    }

    fn as_mut_ptr(&mut self) -> LPPROC_THREAD_ATTRIBUTE_LIST {
        self.buffer.as_mut_ptr().cast()
    }

    /// Sets `attribute` to `value` of `size` bytes.
    ///
    /// # Safety
    /// `value` must be what `attribute` expects (a pointer to `size` bytes,
    /// or for `PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE` the handle itself), and
    /// what it refers to must stay valid and unchanged until the process has
    /// been created with this list.
    unsafe fn set(&mut self, attribute: u32, value: *const c_void, size: usize) -> io::Result<()> {
        let attribute = usize::try_from(attribute).map_err(|_| io::Error::other("attribute out of range"))?;
        // SAFETY: the list was initialised for one attribute by `new`; the
        // caller guarantees `value` and `size`; the optional out-parameters
        // are null.
        let ok = unsafe {
            UpdateProcThreadAttribute(
                self.as_mut_ptr(),
                0,
                attribute,
                value,
                size,
                ptr::null_mut(),
                ptr::null(),
            )
        };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
}

impl Drop for AttributeList {
    fn drop(&mut self) {
        // SAFETY: the list was initialised by `new` and is deleted exactly
        // once, here; its storage is still alive.
        unsafe {
            DeleteProcThreadAttributeList(self.as_mut_ptr());
        }
    }
}

/// An anonymous pipe: (read end, write end), neither inheritable.
fn pipe(buffer: u32) -> io::Result<(OwnedHandle, OwnedHandle)> {
    let mut read: HANDLE = ptr::null_mut();
    let mut write: HANDLE = ptr::null_mut();
    // SAFETY: both out-pointers are valid for writes; null security
    // attributes make both ends non-inheritable.
    let ok = unsafe { CreatePipe(&raw mut read, &raw mut write, ptr::null(), buffer) };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: CreatePipe succeeded, so both are valid, open handles that
    // nothing else owns; `OwnedHandle` closes each once.
    Ok(unsafe {
        (
            OwnedHandle::from_raw_handle(read),
            OwnedHandle::from_raw_handle(write),
        )
    })
}

/// Makes `handle` inheritable (only handles in a list are then passed on).
fn set_inheritable(handle: &OwnedHandle) -> io::Result<()> {
    // SAFETY: `handle` is a valid handle owned by this process.
    let ok =
        unsafe { SetHandleInformation(handle.as_raw_handle(), HANDLE_FLAG_INHERIT, HANDLE_FLAG_INHERIT) };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// Resumes the main thread of a process created suspended.
fn resume(thread: &OwnedHandle) -> io::Result<()> {
    // SAFETY: `thread` is the valid main-thread handle from CreateProcessW,
    // with all access rights.
    if unsafe { ResumeThread(thread.as_raw_handle()) } == u32::MAX {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// Terminates a process that never ran (it is still suspended) and waits
/// for it to go.
fn discard(process: &OwnedHandle) {
    // SAFETY: `process` is a valid process handle with all access rights.
    unsafe {
        TerminateProcess(process.as_raw_handle(), DISCARDED_EXIT_CODE);
    }
    let _ = wait(process, INFINITE);
}

/// Waits up to `timeout` milliseconds for `process` to exit; whether it has.
fn wait(process: &OwnedHandle, timeout: u32) -> io::Result<bool> {
    // SAFETY: `process` is a valid process handle with SYNCHRONIZE access.
    match unsafe { WaitForSingleObject(process.as_raw_handle(), timeout) } {
        WAIT_OBJECT_0 => Ok(true),
        WAIT_TIMEOUT => Ok(false),
        _ => Err(io::Error::last_os_error()),
    }
}

/// The UTF-16 form of an OS string, without a terminator.
fn wide(text: &OsStr) -> Vec<u16> {
    text.encode_wide().collect()
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
        wait(&self.process, 0)
    }

    fn reap(&mut self) -> io::Result<ExitStatus> {
        wait(&self.process, INFINITE)?;
        let mut code = 0_u32;
        // SAFETY: `self.process` is a valid process handle with
        // PROCESS_QUERY_INFORMATION access and `code` is valid for writes.
        if unsafe { GetExitCodeProcess(self.process.as_raw_handle(), &raw mut code) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(ExitStatus::from_windows_code(code))
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
