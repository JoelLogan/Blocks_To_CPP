//! How a process ended, decoded for people (spec §7.6.4).

use std::fmt;

/// How a process ended.
///
/// ```
/// use b2c_process::{Crash, ExitStatus};
///
/// // A Windows access violation, as `GetExitCodeProcess` reports it.
/// let status = ExitStatus::from_windows_code(0xC000_0005);
/// assert_eq!(status, ExitStatus::Exception(0xC000_0005));
/// assert_eq!(status.crash(), Some(Crash::MemoryAccess));
/// assert!(status.describe().starts_with("Crashed: the program tried to use memory"));
///
/// assert_eq!(ExitStatus::Exited(0).describe(), "Finished (exit code 0)");
/// assert_eq!(ExitStatus::Signaled(11).shell_code(), 139);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ExitStatus {
    /// The program ended itself with this exit code (`return` from `main`,
    /// `exit`). On Windows the 32-bit code is shown as `i32`.
    Exited(i32),
    /// Unix: a signal stopped the program (signal number, e.g. 11 for
    /// `SIGSEGV`).
    Signaled(i32),
    /// Windows: an unhandled exception ended the program. The value is the
    /// `NTSTATUS` code (`0xC0000000..=0xDFFFFFFF`), e.g. `0xC0000005` for an
    /// access violation.
    Exception(u32),
}

/// What kind of crash an [`ExitStatus`] shows, for friendly messages and
/// runtime diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Crash {
    /// Memory it does not own: `SIGSEGV`, `SIGBUS`, `0xC0000005`.
    MemoryAccess,
    /// Stack overflow: `0xC00000FD` (on Linux a stack overflow is a
    /// `SIGSEGV` and cannot be told apart from the exit status alone).
    StackOverflow,
    /// Integer division by zero: `SIGFPE`, `0xC0000094`.
    DivisionByZero,
    /// The program stopped itself (uncaught exception, failed assertion,
    /// `std::abort`): `SIGABRT`, `0xC0000409`, or exit code 3 on Windows.
    Aborted,
    /// An illegal instruction, which in debug builds is usually a safety
    /// check (`-fsanitize-undefined-trap-on-error`, `__builtin_trap`):
    /// `SIGILL`, `SIGTRAP`, `0xC000001D`.
    Trap,
    /// Ctrl+C: `SIGINT`, `0xC000013A`.
    Interrupted,
    /// Asked to stop: `SIGTERM`, `SIGHUP`.
    Terminated,
    /// Killed without warning: `SIGKILL` (often the system running out of
    /// memory).
    Killed,
    /// Out of memory: `0xC0000017`.
    OutOfMemory,
    /// Corrupted its own heap: `0xC0000374`.
    HeapCorruption,
    /// Windows: a DLL it needs is missing or does not match
    /// (`0xC0000135`, `0xC0000139`, `0xC0000138`).
    MissingDll,
    /// Wrote to a pipe nobody reads: `SIGPIPE`.
    BrokenPipe,
    /// Hit a CPU-time or file-size limit: `SIGXCPU`, `SIGXFSZ`.
    ResourceLimit,
    /// Any other signal or exception.
    Other,
}

/// Unix signal numbers. They are the same on every Linux architecture
/// Blocks2Cpp supports (`x86_64`, `aarch64`); only `SIGBUS` differs on the BSDs.
mod sig {
    pub(super) const HUP: i32 = 1;
    pub(super) const INT: i32 = 2;
    pub(super) const QUIT: i32 = 3;
    pub(super) const ILL: i32 = 4;
    pub(super) const TRAP: i32 = 5;
    pub(super) const ABRT: i32 = 6;
    #[cfg(not(any(target_os = "macos", target_os = "freebsd")))]
    pub(super) const BUS: i32 = 7;
    #[cfg(any(target_os = "macos", target_os = "freebsd"))]
    pub(super) const BUS: i32 = 10;
    pub(super) const FPE: i32 = 8;
    pub(super) const KILL: i32 = 9;
    pub(super) const SEGV: i32 = 11;
    pub(super) const PIPE: i32 = 13;
    pub(super) const ALRM: i32 = 14;
    pub(super) const TERM: i32 = 15;
    pub(super) const XCPU: i32 = 24;
    pub(super) const XFSZ: i32 = 25;
}

/// Windows `NTSTATUS` codes the decoder recognises.
mod nt {
    pub(super) const ACCESS_VIOLATION: u32 = 0xC000_0005;
    pub(super) const IN_PAGE_ERROR: u32 = 0xC000_0006;
    pub(super) const NO_MEMORY: u32 = 0xC000_0017;
    pub(super) const ILLEGAL_INSTRUCTION: u32 = 0xC000_001D;
    pub(super) const PRIVILEGED_INSTRUCTION: u32 = 0xC000_0096;
    pub(super) const INTEGER_DIVIDE_BY_ZERO: u32 = 0xC000_0094;
    pub(super) const STACK_OVERFLOW: u32 = 0xC000_00FD;
    pub(super) const DLL_NOT_FOUND: u32 = 0xC000_0135;
    pub(super) const ORDINAL_NOT_FOUND: u32 = 0xC000_0138;
    pub(super) const ENTRYPOINT_NOT_FOUND: u32 = 0xC000_0139;
    pub(super) const CONTROL_C_EXIT: u32 = 0xC000_013A;
    pub(super) const HEAP_CORRUPTION: u32 = 0xC000_0374;
    pub(super) const STACK_BUFFER_OVERRUN: u32 = 0xC000_0409;
}

/// The exit code MinGW's `abort()` uses on Windows.
const WINDOWS_ABORT_EXIT_CODE: i32 = 3;

impl ExitStatus {
    /// Decodes a standard library exit status.
    pub fn from_std(status: std::process::ExitStatus) -> Self {
        #[cfg(unix)]
        {
            use std::os::unix::process::ExitStatusExt as _;
            if let Some(code) = status.code() {
                return Self::Exited(code);
            }
            if let Some(signal) = status.signal() {
                return Self::Signaled(signal);
            }
            // Stopped or continued statuses never come out of `wait`.
            Self::Exited(status.into_raw())
        }
        #[cfg(windows)]
        {
            // On Windows `code()` is always `Some`: the 32-bit exit code.
            Self::from_windows_code(status.code().map_or(0, i32::cast_unsigned))
        }
        #[cfg(not(any(unix, windows)))]
        {
            Self::Exited(status.code().unwrap_or(-1))
        }
    }

    /// Decodes a Windows exit code (`GetExitCodeProcess`). Codes in the
    /// `NTSTATUS` error range without the customer bit
    /// (`0xC0000000..=0xDFFFFFFF`) are exceptions; everything else, including
    /// negative codes a program returned itself such as `-1`, is an exit code.
    pub fn from_windows_code(code: u32) -> Self {
        if (0xC000_0000..=0xDFFF_FFFF).contains(&code) {
            Self::Exception(code)
        } else {
            Self::Exited(code.cast_signed())
        }
    }

    /// Whether the program finished with exit code 0.
    pub fn success(self) -> bool {
        self == Self::Exited(0)
    }

    /// The exit code, if the program ended itself.
    pub fn code(self) -> Option<i32> {
        match self {
            Self::Exited(code) => Some(code),
            Self::Signaled(_) | Self::Exception(_) => None,
        }
    }

    /// The code a shell would report: the exit code itself, `128 + N` for
    /// signal `N` (so `139` for `SIGSEGV`), and the `NTSTATUS` value (as a
    /// 32-bit exit code) for Windows exceptions.
    pub fn shell_code(self) -> i32 {
        match self {
            Self::Exited(code) => code,
            Self::Signaled(signal) => signal.saturating_add(128),
            Self::Exception(code) => code.cast_signed(),
        }
    }

    /// The kind of crash, or `None` for a normal exit.
    pub fn crash(self) -> Option<Crash> {
        self.crash_for(cfg!(windows))
    }

    fn crash_for(self, windows: bool) -> Option<Crash> {
        match self {
            Self::Exited(WINDOWS_ABORT_EXIT_CODE) if windows => Some(Crash::Aborted),
            Self::Exited(_) => None,
            Self::Signaled(signal) => Some(crash_for_signal(signal)),
            Self::Exception(code) => Some(crash_for_ntstatus(code)),
        }
    }

    /// A friendly, one-paragraph description in plain English (spec §7.6.4),
    /// for example *"Crashed: integer division by zero (SIGFPE)."*.
    pub fn describe(self) -> String {
        self.describe_for(cfg!(windows))
    }

    fn describe_for(self, windows: bool) -> String {
        let technical = match self {
            Self::Exited(code) => {
                if self.crash_for(windows).is_none() {
                    return if code == 0 {
                        String::from("Finished (exit code 0)")
                    } else {
                        format!("Finished with exit code {code}")
                    };
                }
                format!("exit code {code}")
            }
            Self::Signaled(signal) => {
                signal_name(signal).map_or_else(|| format!("signal {signal}"), String::from)
            }
            Self::Exception(code) => format!("exception 0x{code:08X}"),
        };
        let crash = self.crash_for(windows).unwrap_or(Crash::Other);
        format!("{} ({technical}).", crash.summary())
    }
}

impl fmt::Display for ExitStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.describe())
    }
}

impl Crash {
    /// A friendly sentence describing the crash, without the technical name.
    pub fn summary(self) -> &'static str {
        match self {
            Self::MemoryAccess => {
                "Crashed: the program tried to use memory it doesn't own (segmentation fault / access \
                 violation). Common causes are an index out of range with fast unchecked access, a null \
                 pointer, or a dangling reference"
            }
            Self::StackOverflow => "Crashed: stack overflow, probably infinite recursion",
            Self::DivisionByZero => "Crashed: integer division by zero",
            Self::Aborted => "Stopped itself: an uncaught error or failed check",
            Self::Trap => {
                "Crashed: a safety check stopped the program (illegal instruction). In a debug build this \
                 usually means undefined behaviour, such as a number that got too big for its type"
            }
            Self::Interrupted => "Stopped by Ctrl+C",
            Self::Terminated => "Stopped: the program was asked to stop",
            Self::Killed => {
                "Killed: the program was stopped without warning, often because the computer ran out of memory"
            }
            Self::OutOfMemory => "Crashed: the program ran out of memory",
            Self::HeapCorruption => {
                "Crashed: the program damaged its own memory (heap corruption), for example by writing past \
                 the end of an array"
            }
            Self::MissingDll => {
                "Could not start: a DLL the program needs is missing or the wrong version. Building with \
                 static linking avoids this"
            }
            Self::BrokenPipe => "Stopped: the program wrote output that nothing was reading",
            Self::ResourceLimit => "Stopped: the program hit a CPU time or file size limit",
            Self::Other => "Crashed",
        }
    }
}

fn crash_for_signal(signal: i32) -> Crash {
    match signal {
        sig::SEGV | sig::BUS => Crash::MemoryAccess,
        sig::FPE => Crash::DivisionByZero,
        sig::ABRT => Crash::Aborted,
        sig::ILL | sig::TRAP => Crash::Trap,
        sig::INT => Crash::Interrupted,
        sig::TERM | sig::HUP | sig::QUIT => Crash::Terminated,
        sig::KILL => Crash::Killed,
        sig::PIPE => Crash::BrokenPipe,
        sig::XCPU | sig::XFSZ => Crash::ResourceLimit,
        _ => Crash::Other,
    }
}

fn crash_for_ntstatus(code: u32) -> Crash {
    match code {
        nt::ACCESS_VIOLATION | nt::IN_PAGE_ERROR => Crash::MemoryAccess,
        nt::STACK_OVERFLOW => Crash::StackOverflow,
        nt::INTEGER_DIVIDE_BY_ZERO => Crash::DivisionByZero,
        nt::STACK_BUFFER_OVERRUN => Crash::Aborted,
        nt::ILLEGAL_INSTRUCTION | nt::PRIVILEGED_INSTRUCTION => Crash::Trap,
        nt::CONTROL_C_EXIT => Crash::Interrupted,
        nt::NO_MEMORY => Crash::OutOfMemory,
        nt::HEAP_CORRUPTION => Crash::HeapCorruption,
        nt::DLL_NOT_FOUND | nt::ENTRYPOINT_NOT_FOUND | nt::ORDINAL_NOT_FOUND => Crash::MissingDll,
        _ => Crash::Other,
    }
}

/// The conventional name of a Unix signal.
fn signal_name(signal: i32) -> Option<&'static str> {
    Some(match signal {
        sig::HUP => "SIGHUP",
        sig::INT => "SIGINT",
        sig::QUIT => "SIGQUIT",
        sig::ILL => "SIGILL",
        sig::TRAP => "SIGTRAP",
        sig::ABRT => "SIGABRT",
        sig::BUS => "SIGBUS",
        sig::FPE => "SIGFPE",
        sig::KILL => "SIGKILL",
        sig::SEGV => "SIGSEGV",
        sig::PIPE => "SIGPIPE",
        sig::ALRM => "SIGALRM",
        sig::TERM => "SIGTERM",
        sig::XCPU => "SIGXCPU",
        sig::XFSZ => "SIGXFSZ",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_exits() {
        assert!(ExitStatus::Exited(0).success());
        assert!(!ExitStatus::Exited(1).success());
        assert_eq!(
            ExitStatus::Exited(0).describe_for(false),
            "Finished (exit code 0)"
        );
        assert_eq!(
            ExitStatus::Exited(42).describe_for(false),
            "Finished with exit code 42"
        );
        assert_eq!(
            ExitStatus::Exited(3).describe_for(false),
            "Finished with exit code 3"
        );
        assert_eq!(ExitStatus::Exited(3).crash_for(false), None);
        assert_eq!(ExitStatus::Exited(42).shell_code(), 42);
        assert_eq!(ExitStatus::Exited(42).code(), Some(42));
    }

    #[test]
    fn windows_abort_exit_code() {
        assert_eq!(ExitStatus::Exited(3).crash_for(true), Some(Crash::Aborted));
        assert_eq!(
            ExitStatus::Exited(3).describe_for(true),
            "Stopped itself: an uncaught error or failed check (exit code 3)."
        );
    }

    #[test]
    fn signals_decode() {
        let cases = [
            (11, Crash::MemoryAccess, "SIGSEGV", 139),
            (8, Crash::DivisionByZero, "SIGFPE", 136),
            (6, Crash::Aborted, "SIGABRT", 134),
            (4, Crash::Trap, "SIGILL", 132),
            (2, Crash::Interrupted, "SIGINT", 130),
            (9, Crash::Killed, "SIGKILL", 137),
            (15, Crash::Terminated, "SIGTERM", 143),
            (13, Crash::BrokenPipe, "SIGPIPE", 141),
        ];
        for (signal, crash, name, shell) in cases {
            let status = ExitStatus::Signaled(signal);
            assert_eq!(status.crash(), Some(crash), "{signal}");
            assert_eq!(status.shell_code(), shell);
            assert_eq!(status.code(), None);
            let text = status.describe_for(false);
            assert!(text.ends_with(&format!("({name}).")), "{text}");
            assert!(text.starts_with(crash.summary()), "{text}");
        }
        assert_eq!(
            ExitStatus::Signaled(64).describe_for(false),
            "Crashed (signal 64)."
        );
    }

    #[test]
    fn ntstatus_decodes() {
        let cases = [
            (0xC000_0005, Crash::MemoryAccess),
            (0xC000_00FD, Crash::StackOverflow),
            (0xC000_0094, Crash::DivisionByZero),
            (0xC000_0409, Crash::Aborted),
            (0xC000_013A, Crash::Interrupted),
            (0xC000_001D, Crash::Trap),
            (0xC000_0135, Crash::MissingDll),
            (0xC000_0139, Crash::MissingDll),
            (0xC000_0374, Crash::HeapCorruption),
            (0xC000_0017, Crash::OutOfMemory),
            (0xC000_0001, Crash::Other),
        ];
        for (code, crash) in cases {
            let status = ExitStatus::from_windows_code(code);
            assert_eq!(status, ExitStatus::Exception(code));
            assert_eq!(status.crash_for(true), Some(crash), "{code:#x}");
            assert_eq!(status.shell_code().cast_unsigned(), code);
            assert!(status.describe_for(true).contains(&format!("0x{code:08X}")));
        }
        assert_eq!(
            ExitStatus::from_windows_code(0xC000_0005).describe_for(true),
            "Crashed: the program tried to use memory it doesn't own (segmentation fault / access \
             violation). Common causes are an index out of range with fast unchecked access, a null \
             pointer, or a dangling reference (exception 0xC0000005)."
        );
        assert_eq!(
            ExitStatus::from_windows_code(0xC000_00FD).describe_for(true),
            "Crashed: stack overflow, probably infinite recursion (exception 0xC00000FD)."
        );
    }

    /// Every row of the exit-decoding table in
    /// `docs/spec/07-toolchain-build-run.md` §7.6.4 that comes from the exit
    /// status alone (the sanitizer row and *Stopped* are decided by the
    /// caller), as a fixture status on the platform it comes from.
    #[test]
    fn spec_exit_decoding_table() {
        const MEMORY: &str = "Crashed: the program tried to use memory it doesn't own (segmentation fault / \
                              access violation). Common causes are an index out of range with fast unchecked \
                              access, a null pointer, or a dangling reference";
        const ABORTED: &str = "Stopped itself: an uncaught error or failed check";
        let windows = true;
        let linux = false;
        let cases: [(ExitStatus, bool, Option<Crash>, String); 12] = [
            (
                ExitStatus::Exited(0),
                linux,
                None,
                String::from("Finished (exit code 0)"),
            ),
            (
                ExitStatus::Exited(0),
                windows,
                None,
                String::from("Finished (exit code 0)"),
            ),
            (
                ExitStatus::Exited(5),
                linux,
                None,
                String::from("Finished with exit code 5"),
            ),
            (
                ExitStatus::Exited(-1),
                windows,
                None,
                String::from("Finished with exit code -1"),
            ),
            (
                ExitStatus::Signaled(11),
                linux,
                Some(Crash::MemoryAccess),
                format!("{MEMORY} (SIGSEGV)."),
            ),
            (
                ExitStatus::from_windows_code(0xC000_0005),
                windows,
                Some(Crash::MemoryAccess),
                format!("{MEMORY} (exception 0xC0000005)."),
            ),
            (
                ExitStatus::from_windows_code(0xC000_00FD),
                windows,
                Some(Crash::StackOverflow),
                String::from("Crashed: stack overflow, probably infinite recursion (exception 0xC00000FD)."),
            ),
            (
                ExitStatus::Signaled(8),
                linux,
                Some(Crash::DivisionByZero),
                String::from("Crashed: integer division by zero (SIGFPE)."),
            ),
            (
                ExitStatus::from_windows_code(0xC000_0094),
                windows,
                Some(Crash::DivisionByZero),
                String::from("Crashed: integer division by zero (exception 0xC0000094)."),
            ),
            (
                ExitStatus::Signaled(6),
                linux,
                Some(Crash::Aborted),
                format!("{ABORTED} (SIGABRT)."),
            ),
            (
                ExitStatus::from_windows_code(3),
                windows,
                Some(Crash::Aborted),
                format!("{ABORTED} (exit code 3)."),
            ),
            (
                ExitStatus::from_windows_code(0xC000_0409),
                windows,
                Some(Crash::Aborted),
                format!("{ABORTED} (exception 0xC0000409)."),
            ),
        ];
        for (status, on_windows, crash, text) in cases {
            assert_eq!(status.crash_for(on_windows), crash, "{status:?}");
            assert_eq!(status.describe_for(on_windows), text, "{status:?}");
        }
    }

    /// The same Unix rows, from raw `wait` statuses as the kernel reports
    /// them (signal number in the low bits, 0x80 when a core was dumped).
    #[cfg(unix)]
    #[test]
    fn spec_exit_decoding_from_raw_unix_statuses() {
        use std::os::unix::process::ExitStatusExt as _;
        let decode = |raw| ExitStatus::from_std(std::process::ExitStatus::from_raw(raw));
        assert_eq!(decode(0x0b).crash(), Some(Crash::MemoryAccess));
        assert_eq!(decode(0x80 | 0x0b).crash(), Some(Crash::MemoryAccess));
        assert_eq!(decode(0x08).crash(), Some(Crash::DivisionByZero));
        assert_eq!(decode(0x80 | 0x06).crash(), Some(Crash::Aborted));
        assert_eq!(decode(2).crash(), Some(Crash::Interrupted));
        assert_eq!(decode(3 << 8), ExitStatus::Exited(3));
        assert_eq!(
            decode(3 << 8).crash(),
            None,
            "exit code 3 is only special on Windows"
        );
    }

    #[test]
    fn negative_exit_codes_are_not_exceptions() {
        assert_eq!(ExitStatus::from_windows_code(u32::MAX), ExitStatus::Exited(-1));
        assert_eq!(ExitStatus::from_windows_code(0), ExitStatus::Exited(0));
        assert_eq!(
            ExitStatus::from_windows_code(0xE06D_7363),
            ExitStatus::Exited(0xE06D_7363_u32.cast_signed())
        );
        assert_eq!(
            ExitStatus::from_windows_code(0x8000_0003),
            ExitStatus::Exited(0x8000_0003_u32.cast_signed())
        );
        assert_eq!(
            ExitStatus::from_windows_code(0xDFFF_FFFF),
            ExitStatus::Exception(0xDFFF_FFFF)
        );
    }

    #[cfg(unix)]
    #[test]
    fn std_statuses_decode() {
        use std::os::unix::process::ExitStatusExt as _;
        assert_eq!(
            ExitStatus::from_std(std::process::ExitStatus::from_raw(0)),
            ExitStatus::Exited(0)
        );
        assert_eq!(
            ExitStatus::from_std(std::process::ExitStatus::from_raw(7 << 8)),
            ExitStatus::Exited(7)
        );
        assert_eq!(
            ExitStatus::from_std(std::process::ExitStatus::from_raw(11)),
            ExitStatus::Signaled(11)
        );
        // Signal 11 with the core-dump bit.
        assert_eq!(
            ExitStatus::from_std(std::process::ExitStatus::from_raw(0x80 | 0x0b)),
            ExitStatus::Signaled(11)
        );
    }
}
