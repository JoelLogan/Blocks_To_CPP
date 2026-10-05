//! Windows containment: Job Objects.
//!
//! **Race handling.** The child is created with `CREATE_SUSPENDED`, so its
//! main thread has not run a single instruction when it is assigned to the
//! job; only then is that thread resumed. Every process it creates later is
//! in the job automatically, so nothing can escape between spawning and
//! assignment.
//!
//! * Captured runs ([`spawn_captured`]) are created with `CreateProcessW`
//!   (`platform/create.rs`), which returns the main thread's handle, and
//!   inherit exactly their three standard handles through
//!   `PROC_THREAD_ATTRIBUTE_HANDLE_LIST` (`docs/spec/08-security.md` §8.7).
//! * Interactive runs ([`crate::run_interactive`], the command line's
//!   `b2c run`, attached to the console) use the standard library's spawn
//!   with the console's handles; `std` does not expose the main thread
//!   handle, so [`contain`] finds the thread with a Toolhelp snapshot (a
//!   process created suspended has exactly one thread).
//!
//! The job has `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` (closing the last handle,
//! including when this process dies, kills everything in it) and
//! `JOB_OBJECT_LIMIT_DIE_ON_UNHANDLED_EXCEPTION` (a crashing program ends with
//! its exception code at once instead of waiting on a Windows Error Reporting
//! dialog), plus the optional memory and process limits. With a limit, the
//! job reports to an I/O completion port when a process in it went over it
//! (`JOB_OBJECT_MSG_JOB_MEMORY_LIMIT`, `JOB_OBJECT_MSG_ACTIVE_PROCESS_LIMIT`);
//! that is how a run is found out of memory or over its process cap, and
//! then stopped.

use std::fs::{File, OpenOptions};
use std::io;
use std::mem::{size_of, size_of_val};
use std::os::windows::io::{
    AsHandle as _, AsRawHandle as _, BorrowedHandle, FromRawHandle as _, OwnedHandle,
};
use std::os::windows::process::CommandExt as _;
use std::process::Child;
use std::ptr;
use std::sync::atomic::{AtomicBool, Ordering};

use windows_sys::Win32::Foundation::{HANDLE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::System::Console::{GetStdHandle, STD_INPUT_HANDLE};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First, Thread32Next,
};
use windows_sys::Win32::System::IO::{CreateIoCompletionPort, GetQueuedCompletionStatus, OVERLAPPED};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_ACTIVE_PROCESS,
    JOB_OBJECT_LIMIT_DIE_ON_UNHANDLED_EXCEPTION, JOB_OBJECT_LIMIT_JOB_MEMORY,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOBOBJECT_ASSOCIATE_COMPLETION_PORT,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectAssociateCompletionPortInformation,
    JobObjectExtendedLimitInformation, SetInformationJobObject, TerminateJobObject,
};
use windows_sys::Win32::System::Threading::{
    CREATE_NO_WINDOW, CREATE_SUSPENDED, INFINITE, OpenThread, PROC_THREAD_ATTRIBUTE_HANDLE_LIST,
    ResumeThread, STARTF_USESTDHANDLES, THREAD_SUSPEND_RESUME,
};

use super::Placement;
use super::create::{self, AttributeList, Created, Launch};
use crate::command::{Command, Stdin};
use crate::containment::{Breach, Enforcement, TreeSpec};
use crate::error::ProcessError;
use crate::status::ExitStatus;

/// Exit code given to processes the job terminates.
const TERMINATED_EXIT_CODE: u32 = 1;

/// Buffer size asked for each pipe of a captured run (a hint; Windows may
/// round it).
const PIPE_BUFFER: u32 = 64 * 1024;

/// The job messages that report a limit (`winnt.h`; windows-sys has them
/// only in its large `SystemServices` module).
const JOB_OBJECT_MSG_ACTIVE_PROCESS_LIMIT: u32 = 3;
const JOB_OBJECT_MSG_PROCESS_MEMORY_LIMIT: u32 = 9;
const JOB_OBJECT_MSG_JOB_MEMORY_LIMIT: u32 = 10;

/// At most this many queued job messages are read per look (the queue only
/// grows with process starts and exits, so this is never reached in
/// practice; it keeps every look bounded).
const MAX_MESSAGES: usize = 4096;

/// Applies creation flags before spawning.
pub(crate) fn configure(command: &mut std::process::Command, _placement: Placement, captured: bool) {
    let mut flags = CREATE_SUSPENDED;
    if captured {
        // Captured runs (the compiler, probes) never need a console window;
        // without this flag a GUI parent would flash one.
        flags |= CREATE_NO_WINDOW;
    }
    command.creation_flags(flags);
}

/// An owned Job Object handle; closing it kills whatever is still inside.
/// (`OwnedHandle` closes it exactly once and makes the job `Send + Sync`, so
/// a session's supervisor thread can share it.)
#[derive(Debug)]
struct Job(OwnedHandle);

/// An I/O completion port that receives the job's messages.
#[derive(Debug)]
struct Port(OwnedHandle);

/// The child's process tree: its Job Object.
#[derive(Debug)]
pub(crate) struct Tree {
    job: Job,
    /// Present when the job has a limit.
    port: Option<Port>,
    /// Set once the job reported its memory limit.
    out_of_memory: AtomicBool,
    /// Set once the job reported its process limit.
    process_limit_hit: AtomicBool,
}

/// The limits a look at the job's messages found reported.
#[derive(Debug, Clone, Copy, Default)]
struct Reported {
    memory: bool,
    processes: bool,
}

/// A started child, owned by its supervisor.
#[derive(Debug)]
pub(crate) enum Process {
    /// Started by the standard library (interactive runs).
    Std(Child),
    /// Started by [`spawn_captured`].
    Created(OwnedHandle),
}

impl Process {
    /// Wraps a child started by the standard library.
    pub(crate) fn from_std(child: Child) -> Self {
        Self::Std(child)
    }

    /// Whether the child has exited.
    pub(crate) fn has_exited(&mut self) -> io::Result<bool> {
        match self {
            Self::Std(child) => has_exited(child),
            Self::Created(process) => create::wait(process, 0),
        }
    }

    /// Waits for the child and collects its status.
    pub(crate) fn wait(&mut self) -> io::Result<ExitStatus> {
        match self {
            Self::Std(child) => child.wait().map(ExitStatus::from_std),
            Self::Created(process) => {
                create::wait(process, INFINITE)?;
                create::exit_status(process)
            }
        }
    }

    /// Terminates the child itself (not its tree) and waits for it, after a
    /// setup failure.
    pub(crate) fn discard(&mut self) {
        match self {
            Self::Std(child) => {
                let _ = child.kill();
                let _ = child.wait();
            }
            Self::Created(process) => create::discard(process),
        }
    }
}

/// Creates the job, assigns the suspended child to it and resumes it. On
/// failure the child is terminated before it ran any code.
#[allow(clippy::needless_pass_by_value)] // the Unix version keeps the spec's scope
pub(crate) fn contain(child: &mut Child, _placement: Placement, spec: TreeSpec) -> io::Result<Tree> {
    let result = job_tree(&spec.enforcement).and_then(|tree| {
        tree.assign(child.as_handle())?;
        resume_main_thread(child.id())?;
        Ok(tree)
    });
    if result.is_err() {
        let _ = child.kill();
        let _ = child.wait();
    }
    result
}

/// A new, empty Job Object with the limits of `enforcement`, for a process
/// that is created suspended and assigned with [`Tree::assign`] before it is
/// resumed.
pub(crate) fn job_tree(enforcement: &Enforcement) -> io::Result<Tree> {
    let job = create_job(enforcement)?;
    let port = if enforcement.job_memory.is_some() || enforcement.job_processes.is_some() {
        Some(Port::for_job(&job)?)
    } else {
        None
    };
    Ok(Tree {
        job,
        port,
        out_of_memory: AtomicBool::new(false),
        process_limit_hit: AtomicBool::new(false),
    })
}

fn create_job(enforcement: &Enforcement) -> io::Result<Job> {
    // SAFETY: both pointer arguments may be null (default security, unnamed
    // job). The returned handle is checked before use.
    let handle = unsafe { CreateJobObjectW(ptr::null(), ptr::null()) };
    if handle.is_null() {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: `handle` is a valid, open job handle that nothing else owns;
    // from here on `OwnedHandle` closes it exactly once.
    let job = Job(unsafe { OwnedHandle::from_raw_handle(handle) });

    let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    let mut flags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE | JOB_OBJECT_LIMIT_DIE_ON_UNHANDLED_EXCEPTION;
    if let Some(bytes) = enforcement.job_memory {
        flags |= JOB_OBJECT_LIMIT_JOB_MEMORY;
        info.JobMemoryLimit = usize::try_from(bytes).unwrap_or(usize::MAX);
    }
    if let Some(count) = enforcement.job_processes {
        flags |= JOB_OBJECT_LIMIT_ACTIVE_PROCESS;
        info.BasicLimitInformation.ActiveProcessLimit = count.max(1);
    }
    info.BasicLimitInformation.LimitFlags = flags;
    let size = u32::try_from(size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>())
        .map_err(|_| io::Error::other("job limit structure too large"))?;
    // SAFETY: `job.0` is a valid job handle; `info` is a properly initialised
    // JOBOBJECT_EXTENDED_LIMIT_INFORMATION that outlives the call, and `size`
    // is its exact size, as the information class requires.
    let ok = unsafe {
        SetInformationJobObject(
            job.0.as_raw_handle(),
            JobObjectExtendedLimitInformation,
            (&raw const info).cast(),
            size,
        )
    };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(job)
}

impl Port {
    /// A new completion port that receives the messages of `job`.
    fn for_job(job: &Job) -> io::Result<Self> {
        // SAFETY: with INVALID_HANDLE_VALUE and no existing port the call
        // creates a new port not associated with any file; the key is
        // ignored; one concurrent thread. The result is checked for null.
        let handle = unsafe { CreateIoCompletionPort(INVALID_HANDLE_VALUE, ptr::null_mut(), 0, 1) };
        if handle.is_null() {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: `handle` is a valid, open port handle that nothing else
        // owns; `OwnedHandle` closes it once.
        let port = Self(unsafe { OwnedHandle::from_raw_handle(handle) });
        let info = JOBOBJECT_ASSOCIATE_COMPLETION_PORT {
            CompletionKey: ptr::null_mut(),
            CompletionPort: port.0.as_raw_handle(),
        };
        let size = u32::try_from(size_of::<JOBOBJECT_ASSOCIATE_COMPLETION_PORT>())
            .map_err(|_| io::Error::other("job port structure too large"))?;
        // SAFETY: `job.0` is a valid job handle; `info` is a properly
        // initialised JOBOBJECT_ASSOCIATE_COMPLETION_PORT naming the open
        // port (which `port` keeps open as long as the tree exists) and
        // outlives the call; `size` is its exact size.
        let ok = unsafe {
            SetInformationJobObject(
                job.0.as_raw_handle(),
                JobObjectAssociateCompletionPortInformation,
                (&raw const info).cast(),
                size,
            )
        };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(port)
    }

    /// Reads every message queued so far, without waiting; which limits
    /// they reported.
    fn reported(&self) -> Reported {
        let mut reported = Reported::default();
        for _ in 0..MAX_MESSAGES {
            let mut message = 0_u32;
            let mut key = 0_usize;
            let mut overlapped: *mut OVERLAPPED = ptr::null_mut();
            // SAFETY: `self.0` is a valid completion port handle; the three
            // out-pointers are valid for writes; a zero timeout returns at
            // once. For job messages the "overlapped" value is a process ID,
            // never dereferenced.
            let ok = unsafe {
                GetQueuedCompletionStatus(
                    self.0.as_raw_handle(),
                    &raw mut message,
                    &raw mut key,
                    &raw mut overlapped,
                    0,
                )
            };
            if ok == 0 {
                // Nothing queued (WAIT_TIMEOUT).
                break;
            }
            match message {
                JOB_OBJECT_MSG_JOB_MEMORY_LIMIT | JOB_OBJECT_MSG_PROCESS_MEMORY_LIMIT => {
                    reported.memory = true;
                }
                JOB_OBJECT_MSG_ACTIVE_PROCESS_LIMIT => {
                    reported.processes = true;
                }
                _ => {}
            }
        }
        reported
    }
}

/// Resumes the only thread of a process created suspended by the standard
/// library.
fn resume_main_thread(pid: u32) -> io::Result<()> {
    // SAFETY: plain call; the result is checked against INVALID_HANDLE_VALUE.
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: `snapshot` is a valid, open handle that nothing else owns.
    let snapshot = unsafe { OwnedHandle::from_raw_handle(snapshot) };
    let entry_size = u32::try_from(size_of::<THREADENTRY32>()).unwrap_or(u32::MAX);
    let mut entry = THREADENTRY32 {
        dwSize: entry_size,
        ..THREADENTRY32::default()
    };
    let mut resumed = 0_u32;
    // SAFETY: `snapshot` is a valid snapshot handle and `entry` is a
    // THREADENTRY32 with `dwSize` set, as Thread32First requires.
    let mut more = unsafe { Thread32First(snapshot.as_raw_handle(), &raw mut entry) } != 0;
    while more {
        if entry.th32OwnerProcessID == pid {
            // SAFETY: plain call with a thread ID from the snapshot; the
            // result is checked for null.
            let thread = unsafe { OpenThread(THREAD_SUSPEND_RESUME, 0, entry.th32ThreadID) };
            if thread.is_null() {
                return Err(io::Error::last_os_error());
            }
            // SAFETY: `thread` is a valid, open handle that nothing else owns.
            let thread = unsafe { OwnedHandle::from_raw_handle(thread) };
            // SAFETY: `thread` is a valid handle with THREAD_SUSPEND_RESUME
            // access.
            if unsafe { ResumeThread(thread.as_raw_handle()) } == u32::MAX {
                return Err(io::Error::last_os_error());
            }
            resumed += 1;
        }
        entry.dwSize = entry_size;
        // SAFETY: as for Thread32First.
        more = unsafe { Thread32Next(snapshot.as_raw_handle(), &raw mut entry) } != 0;
    }
    if resumed == 0 {
        return Err(io::Error::other("the new process has no thread to resume"));
    }
    Ok(())
}

/// Whether the child has exited. Windows keeps the process handle (and so
/// the job membership) valid after exit, so this may use `try_wait`.
pub(crate) fn has_exited(child: &mut Child) -> io::Result<bool> {
    Ok(child.try_wait()?.is_some())
}

/// A captured run started by [`spawn_captured`]: the child, its job, and
/// this process's ends of its standard handles.
pub(crate) struct CapturedSpawn {
    /// The child, running.
    pub(crate) process: Process,
    /// Its job.
    pub(crate) tree: Tree,
    /// The write end of its input pipe, for [`Stdin::Bytes`].
    pub(crate) stdin: Option<File>,
    /// The read end of its output pipe.
    pub(crate) stdout: File,
    /// The read end of its error pipe.
    pub(crate) stderr: File,
}

/// Starts `command` for [`crate::run_captured`]: created suspended with
/// `CreateProcessW` and `CREATE_NO_WINDOW`, inheriting exactly its standard
/// input (the null device, a pipe, a file, or a copy of this process's
/// input) and two output pipes through `PROC_THREAD_ATTRIBUTE_HANDLE_LIST`,
/// assigned to a new Job Object, then resumed.
///
/// # Errors
/// [`ProcessError::StdinFile`] when the input file cannot be opened,
/// [`ProcessError::InvalidCommand`], [`ProcessError::Spawn`] and
/// [`ProcessError::Containment`].
pub(crate) fn spawn_captured(command: &Command, spec: &TreeSpec) -> Result<CapturedSpawn, ProcessError> {
    let program = command.program().to_path_buf();
    let spawn_error = |source: io::Error| ProcessError::Spawn {
        program: program.clone(),
        source,
    };
    let mut launch = Launch::new(command)?;
    let (child_input, input) = match command.get_stdin() {
        Stdin::Null => (null_device().map_err(spawn_error)?, None),
        Stdin::Bytes(_) => {
            let (read, write) = create::pipe(PIPE_BUFFER).map_err(spawn_error)?;
            (read, Some(File::from(write)))
        }
        Stdin::File(path) => match File::open(path) {
            Ok(file) => (OwnedHandle::from(file), None),
            Err(source) => {
                return Err(ProcessError::StdinFile {
                    path: path.clone(),
                    source,
                });
            }
        },
        Stdin::Inherit => (inherited_input().map_err(spawn_error)?, None),
    };
    let (output, child_output) = create::pipe(PIPE_BUFFER).map_err(spawn_error)?;
    let (error, child_error) = create::pipe(PIPE_BUFFER).map_err(spawn_error)?;
    let tree = job_tree(&spec.enforcement).map_err(|source| ProcessError::Containment {
        program: program.clone(),
        source,
    })?;
    let handles: [HANDLE; 3] = [
        child_input.as_raw_handle(),
        child_output.as_raw_handle(),
        child_error.as_raw_handle(),
    ];
    let created = create::set_inheritable(&child_input)
        .and_then(|()| create::set_inheritable(&child_output))
        .and_then(|()| create::set_inheritable(&child_error))
        .and_then(|()| AttributeList::new())
        .and_then(|mut attributes| {
            // SAFETY: `handles` is an array of three distinct, valid handles
            // (the child's ends, open until after this closure) that lives,
            // unchanged, until after the process has been created; the size
            // is the array's exact size in bytes.
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
            startup.StartupInfo.hStdError = child_error.as_raw_handle();
            launch.create(&startup, true, CREATE_NO_WINDOW)
        });
    // This process's copies of the child's ends (inheritable until now) are
    // closed at once: only the child's copies remain, so the readers see
    // end-of-file when the child's tree has ended.
    drop(child_input);
    drop(child_output);
    drop(child_error);
    let Created { process, thread, .. } = created.map_err(spawn_error)?;
    if let Err(source) = tree
        .assign(process.as_handle())
        .and_then(|()| create::resume(&thread))
    {
        create::discard(&process);
        return Err(ProcessError::Containment { program, source });
    }
    Ok(CapturedSpawn {
        process: Process::Created(process),
        tree,
        stdin: input,
        stdout: File::from(output),
        stderr: File::from(error),
    })
}

/// The null device, for a child that reads nothing.
fn null_device() -> io::Result<OwnedHandle> {
    Ok(OwnedHandle::from(OpenOptions::new().read(true).open("NUL")?))
}

/// A copy of this process's standard input, or the null device when it has
/// none.
fn inherited_input() -> io::Result<OwnedHandle> {
    // SAFETY: plain call; the result is checked below and only duplicated,
    // never closed here (this process does not own it).
    let handle = unsafe { GetStdHandle(STD_INPUT_HANDLE) };
    if handle.is_null() || handle == INVALID_HANDLE_VALUE {
        return null_device();
    }
    create::duplicate(handle)
}

impl Tree {
    /// Puts `process` (created suspended) into the job. Every process it
    /// creates after it is resumed is in the job too.
    pub(crate) fn assign(&self, process: BorrowedHandle<'_>) -> io::Result<()> {
        // SAFETY: `self.job.0` is a valid job handle and `process` is a valid
        // process handle, borrowed for the duration of the call.
        let ok = unsafe { AssignProcessToJobObject(self.job.0.as_raw_handle(), process.as_raw_handle()) };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    /// Windows has no polite stop for console programs in a job; this
    /// terminates at once.
    pub(crate) fn stop(&self) {
        self.kill();
    }

    /// Terminates every process in the job.
    pub(crate) fn kill(&self) {
        // SAFETY: `self.job.0` is a valid job handle.
        unsafe {
            TerminateJobObject(self.job.0.as_raw_handle(), TERMINATED_EXIT_CODE);
        }
    }

    /// Terminates anything the child left running.
    pub(crate) fn after_exit(&self) {
        self.kill();
    }

    /// Whether [`Tree::watchdog`] has anything to check: the job's limit
    /// notifications (the job enforces the limits themselves).
    pub(crate) fn watches(&self) -> bool {
        self.port.is_some()
    }

    /// Whether the job has reported a limit since the last look.
    pub(crate) fn watchdog(&self) -> Option<Breach> {
        let reported = self.note_reports();
        if reported.memory {
            Some(Breach::Memory)
        } else if reported.processes {
            Some(Breach::Processes)
        } else {
            None
        }
    }

    /// Whether the job reported its process limit at any time.
    pub(crate) fn process_limit_hit(&self) -> bool {
        self.note_reports();
        self.process_limit_hit.load(Ordering::Relaxed)
    }

    /// Whether the job reported its memory limit at any time.
    pub(crate) fn out_of_memory(&self, _failed: bool) -> bool {
        self.note_reports();
        self.out_of_memory.load(Ordering::Relaxed)
    }

    /// Reads the queued messages and keeps what they reported.
    fn note_reports(&self) -> Reported {
        let reported = self.port.as_ref().map(Port::reported).unwrap_or_default();
        if reported.memory {
            self.out_of_memory.store(true, Ordering::Relaxed);
        }
        if reported.processes {
            self.process_limit_hit.store(true, Ordering::Relaxed);
        }
        reported
    }
}
