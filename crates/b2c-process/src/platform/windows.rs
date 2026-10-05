//! Windows containment: Job Objects.
//!
//! **Race handling.** The child is created with `CREATE_SUSPENDED`, so its
//! main thread has not run a single instruction when it is assigned to the
//! job; only then is that thread resumed. Every process it creates later is
//! in the job automatically, so nothing can escape between spawning and
//! assignment. `std` does not expose the main thread handle, so the thread is
//! found with a Toolhelp snapshot: a process created suspended has exactly
//! one thread.
//!
//! The job has `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` (closing the last handle,
//! including when this process dies, kills everything in it) and
//! `JOB_OBJECT_LIMIT_DIE_ON_UNHANDLED_EXCEPTION` (a crashing program ends with
//! its exception code at once instead of waiting on a Windows Error Reporting
//! dialog), plus the optional memory and process limits.

use std::io;
use std::mem::size_of;
use std::os::windows::io::{
    AsHandle as _, AsRawHandle as _, BorrowedHandle, FromRawHandle as _, OwnedHandle,
};
use std::os::windows::process::CommandExt as _;
use std::process::Child;
use std::ptr;

use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First, Thread32Next,
};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_ACTIVE_PROCESS,
    JOB_OBJECT_LIMIT_DIE_ON_UNHANDLED_EXCEPTION, JOB_OBJECT_LIMIT_JOB_MEMORY,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JobObjectExtendedLimitInformation, SetInformationJobObject, TerminateJobObject,
};
use windows_sys::Win32::System::Threading::{
    CREATE_NO_WINDOW, CREATE_SUSPENDED, OpenThread, ResumeThread, THREAD_SUSPEND_RESUME,
};

use super::Placement;
use crate::command::Limits;

/// Exit code given to processes the job terminates.
const TERMINATED_EXIT_CODE: u32 = 1;

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

/// The child's process tree: its Job Object.
#[derive(Debug)]
pub(crate) struct Tree {
    job: Job,
}

/// Creates the job, assigns the suspended child to it and resumes it. On
/// failure the child is terminated before it ran any code.
pub(crate) fn contain(child: &mut Child, _placement: Placement, limits: &Limits) -> io::Result<Tree> {
    let result = job_tree(limits).and_then(|tree| {
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

/// A new, empty Job Object with the limits of the module documentation, for
/// a process that is created suspended and assigned with [`Tree::assign`]
/// before it is resumed (`src/pty/windows.rs`).
pub(crate) fn job_tree(limits: &Limits) -> io::Result<Tree> {
    Ok(Tree {
        job: create_job(limits)?,
    })
}

fn create_job(limits: &Limits) -> io::Result<Job> {
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
    if let Some(bytes) = limits.memory {
        flags |= JOB_OBJECT_LIMIT_JOB_MEMORY;
        info.JobMemoryLimit = usize::try_from(bytes).unwrap_or(usize::MAX);
    }
    if let Some(count) = limits.processes {
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

/// Resumes the only thread of a process created suspended.
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

    /// The job enforces the process limit itself.
    #[allow(clippy::unused_self)] // same signature as the Unix version
    pub(crate) fn process_count(&self) -> Option<usize> {
        None
    }
}
