//! Creating Windows processes with `CreateProcessW`: suspended, from an
//! absolute `.exe` path, with exactly the command's environment and an
//! attribute list that names everything the child may inherit
//! (`PROC_THREAD_ATTRIBUTE_HANDLE_LIST`) or its pseudo console. Shared by
//! the sessions of `src/pty/windows.rs` and the captured runs of
//! `platform/windows.rs`.
//!
//! The process is created with `CREATE_SUSPENDED`, `EXTENDED_STARTUPINFO_PRESENT`
//! and `CREATE_UNICODE_ENVIRONMENT`, from an absolute `.exe` path (never a
//! search), a command line quoted with the C runtime rules
//! (`super::cmdline`) and exactly the command's environment. The caller
//! assigns it to its Job Object and only then resumes it ([`resume`]).
//!
//! **Handle lists.** A handle in a `PROC_THREAD_ATTRIBUTE_HANDLE_LIST` must
//! be inheritable, so the child's ends are made inheritable just before
//! creation and closed right after it. A process that another thread
//! creates in that moment *without* a handle list (the standard library's
//! spawn, used only by [`crate::run_interactive`], which the app never
//! calls) could inherit them; that would only delay end-of-file on these
//! pipes until it exits.

use std::ffi::{OsStr, c_void};
use std::io;
use std::mem::size_of;
use std::os::windows::ffi::OsStrExt as _;
use std::os::windows::io::{AsRawHandle as _, FromRawHandle as _, OwnedHandle};
use std::ptr;

use windows_sys::Win32::Foundation::{
    DUPLICATE_SAME_ACCESS, DuplicateHandle, HANDLE, HANDLE_FLAG_INHERIT, SetHandleInformation, WAIT_OBJECT_0,
    WAIT_TIMEOUT,
};
use windows_sys::Win32::System::Pipes::CreatePipe;
use windows_sys::Win32::System::Threading::{
    CREATE_SUSPENDED, CREATE_UNICODE_ENVIRONMENT, CreateProcessW, DeleteProcThreadAttributeList,
    EXTENDED_STARTUPINFO_PRESENT, GetCurrentProcess, INFINITE, InitializeProcThreadAttributeList,
    LPPROC_THREAD_ATTRIBUTE_LIST, PROCESS_CREATION_FLAGS, PROCESS_INFORMATION, ResumeThread, STARTUPINFOEXW,
    STARTUPINFOW, TerminateProcess, UpdateProcThreadAttribute, WaitForSingleObject,
};

use super::cmdline;
use crate::command::Command;
use crate::error::ProcessError;

/// Exit code given to a process terminated before it ran.
const DISCARDED_EXIT_CODE: u32 = 1;

/// The parts of `CreateProcessW` that do not depend on how the child is
/// connected, each NUL-terminated (the environment block ends with two
/// NULs).
pub(crate) struct Launch {
    application: Vec<u16>,
    command_line: Vec<u16>,
    environment: Vec<u16>,
    directory: Vec<u16>,
}

/// A created, suspended process.
pub(crate) struct Created {
    /// The process handle (all access rights).
    pub(crate) process: OwnedHandle,
    /// The main thread's handle, for [`resume`].
    pub(crate) thread: OwnedHandle,
    /// The process ID.
    pub(crate) pid: u32,
}

impl Launch {
    /// The program, command line, environment block and working directory of
    /// `command`.
    ///
    /// # Errors
    /// [`ProcessError::InvalidCommand`] when a part contains NUL, or the
    /// command line or environment block is too long.
    pub(crate) fn new(command: &Command) -> Result<Self, ProcessError> {
        let invalid = |reason: &'static str| ProcessError::InvalidCommand {
            program: command.program().to_path_buf(),
            reason,
        };
        let program = wide(command.program().as_os_str());
        let args: Vec<Vec<u16>> = command.get_args().iter().map(|arg| wide(arg)).collect();
        Ok(Self {
            application: cmdline::nul_terminated(&program, "the program path contains a NUL character")
                .map_err(invalid)?,
            command_line: cmdline::command_line(&program, &args).map_err(invalid)?,
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
        })
    }

    /// Creates the process, suspended, with `startup` (whose attribute list
    /// must stay alive until this returns), inheriting handles only when
    /// `inherit_handles` (and then only those in the list), with `flags` in
    /// addition to the creation flags of the module documentation.
    pub(crate) fn create(
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
pub(crate) fn startup_info(attributes: &mut AttributeList) -> STARTUPINFOEXW {
    let mut startup = STARTUPINFOEXW::default();
    startup.StartupInfo.cb = u32::try_from(size_of::<STARTUPINFOEXW>()).unwrap_or(u32::MAX);
    startup.lpAttributeList = attributes.as_mut_ptr();
    startup
}

/// A `PROC_THREAD_ATTRIBUTE_LIST` with room for one attribute.
pub(crate) struct AttributeList {
    /// Storage for the opaque list, in pointer-sized words so it is aligned
    /// for the pointers it holds.
    buffer: Vec<usize>,
}

impl AttributeList {
    /// An empty list for one attribute.
    pub(crate) fn new() -> io::Result<Self> {
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
    pub(crate) unsafe fn set(&mut self, attribute: u32, value: *const c_void, size: usize) -> io::Result<()> {
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
pub(crate) fn pipe(buffer: u32) -> io::Result<(OwnedHandle, OwnedHandle)> {
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
pub(crate) fn set_inheritable(handle: &OwnedHandle) -> io::Result<()> {
    // SAFETY: `handle` is a valid handle owned by this process.
    let ok =
        unsafe { SetHandleInformation(handle.as_raw_handle(), HANDLE_FLAG_INHERIT, HANDLE_FLAG_INHERIT) };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// A new handle of this process to the same object as `handle` (which this
/// process does not own, such as a standard handle), not inheritable.
pub(crate) fn duplicate(handle: HANDLE) -> io::Result<OwnedHandle> {
    let mut copy: HANDLE = ptr::null_mut();
    // SAFETY: `GetCurrentProcess` returns a pseudo handle that needs no
    // closing; `handle` is an open handle of this process (the caller
    // checked it is neither null nor INVALID_HANDLE_VALUE); `copy` is valid
    // for writes. DUPLICATE_SAME_ACCESS ignores the access argument.
    let ok = unsafe {
        DuplicateHandle(
            GetCurrentProcess(),
            handle,
            GetCurrentProcess(),
            &raw mut copy,
            0,
            0,
            DUPLICATE_SAME_ACCESS,
        )
    };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: DuplicateHandle succeeded, so `copy` is a valid handle owned
    // by this process; `OwnedHandle` closes it once.
    Ok(unsafe { OwnedHandle::from_raw_handle(copy) })
}

/// Resumes the main thread of a process created suspended.
pub(crate) fn resume(thread: &OwnedHandle) -> io::Result<()> {
    // SAFETY: `thread` is the valid main-thread handle from CreateProcessW,
    // with all access rights.
    if unsafe { ResumeThread(thread.as_raw_handle()) } == u32::MAX {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// Terminates a process that never ran (it is still suspended) and waits
/// for it to go.
pub(crate) fn discard(process: &OwnedHandle) {
    terminate(process);
    let _ = wait(process, INFINITE);
}

/// Terminates `process` (it may already have exited).
pub(crate) fn terminate(process: &OwnedHandle) {
    // SAFETY: `process` is a valid process handle with all access rights.
    unsafe {
        TerminateProcess(process.as_raw_handle(), DISCARDED_EXIT_CODE);
    }
}

/// Waits up to `timeout` milliseconds for `process` to exit; whether it has.
pub(crate) fn wait(process: &OwnedHandle, timeout: u32) -> io::Result<bool> {
    // SAFETY: `process` is a valid process handle with SYNCHRONIZE access.
    match unsafe { WaitForSingleObject(process.as_raw_handle(), timeout) } {
        WAIT_OBJECT_0 => Ok(true),
        WAIT_TIMEOUT => Ok(false),
        _ => Err(io::Error::last_os_error()),
    }
}

/// The UTF-16 form of an OS string, without a terminator.
pub(crate) fn wide(text: &OsStr) -> Vec<u16> {
    text.encode_wide().collect()
}

/// The exit status of `process`, which has exited.
pub(crate) fn exit_status(process: &OwnedHandle) -> io::Result<crate::status::ExitStatus> {
    let mut code = 0_u32;
    // SAFETY: `process` is a valid process handle with
    // PROCESS_QUERY_INFORMATION access and `code` is valid for writes.
    if unsafe {
        windows_sys::Win32::System::Threading::GetExitCodeProcess(process.as_raw_handle(), &raw mut code)
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(crate::status::ExitStatus::from_windows_code(code))
}
