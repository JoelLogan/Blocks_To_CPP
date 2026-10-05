//! How a run session's program ended, decoded for the `exit` run event
//! (`docs/spec/07-toolchain-build-run.md` §7.6.4, `docs/spec/02-architecture.md`
//! §2.5.3).
//!
//! * A program the user stopped is `{ type: "stopped" }` with the message
//!   *Stopped*, whatever signal ended it.
//! * Otherwise the status is the exit code, signal or `NTSTATUS`, `crash` is
//!   the closed kind from [`b2c_process::Crash`], and the message is
//!   [`ProgramExit::describe`]'s.
//! * When a sanitizer report was found in the output and the program did not
//!   finish successfully, the message summarises the report instead, for
//!   example *Crashed: heap-buffer-overflow (AddressSanitizer)*: a sanitizer
//!   ends the program with exit code 1, which alone would say nothing.

use b2c_ipc::dto::{
    Crash as IpcCrash, ExitStatus as IpcStatus, SanitizerKind, SanitizerReport, SanitizerTool,
};
use b2c_process::{Crash, ExitStatus, PtyExit};
use b2c_toolchain::sanitizer;

use super::ProgramExit;

/// The decoded fields of a [`b2c_ipc::dto::RunEvent::Exit`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExitReport {
    /// How it ended.
    pub(crate) status: IpcStatus,
    /// Why it crashed, when it did.
    pub(crate) crash: Option<IpcCrash>,
    /// The first sanitizer report in its output.
    pub(crate) sanitizer: Option<SanitizerReport>,
    /// The friendly summary.
    pub(crate) message: String,
}

/// Decodes how a session's program ended, with the first sanitizer report
/// found in its output.
pub(crate) fn decode(exit: &PtyExit, report: Option<&sanitizer::SanitizerReport>) -> ExitReport {
    let program = ProgramExit::from_pty(exit);
    let sanitizer = report.and_then(ipc_report);
    if program == ProgramExit::Stopped {
        return ExitReport {
            status: IpcStatus::Stopped,
            crash: None,
            sanitizer,
            message: program.describe(),
        };
    }
    let message = match report {
        Some(report) if sanitizer.is_some() && !exit.status.success() => report.summary(),
        _ => program.describe(),
    };
    ExitReport {
        status: ipc_status(exit.status),
        crash: exit.status.crash().map(ipc_crash),
        sanitizer,
        message,
    }
}

/// The IPC form of an exit status.
fn ipc_status(status: ExitStatus) -> IpcStatus {
    match status {
        ExitStatus::Exited(code) => IpcStatus::Exited { code },
        ExitStatus::Signaled(signal) => IpcStatus::Signaled { signal },
        ExitStatus::Exception(ntstatus) => IpcStatus::Exception { ntstatus },
    }
}

/// The IPC form of a crash kind (the same fourteen kinds).
pub(crate) fn ipc_crash(crash: Crash) -> IpcCrash {
    match crash {
        Crash::MemoryAccess => IpcCrash::MemoryAccess,
        Crash::StackOverflow => IpcCrash::StackOverflow,
        Crash::DivisionByZero => IpcCrash::DivisionByZero,
        Crash::Aborted => IpcCrash::Aborted,
        Crash::Trap => IpcCrash::Trap,
        Crash::Interrupted => IpcCrash::Interrupted,
        Crash::Terminated => IpcCrash::Terminated,
        Crash::Killed => IpcCrash::Killed,
        Crash::OutOfMemory => IpcCrash::OutOfMemory,
        Crash::HeapCorruption => IpcCrash::HeapCorruption,
        Crash::MissingDll => IpcCrash::MissingDll,
        Crash::BrokenPipe => IpcCrash::BrokenPipe,
        Crash::ResourceLimit => IpcCrash::ResourceLimit,
        Crash::Other => IpcCrash::Other,
    }
}

/// The IPC form of a sanitizer report. The kind is validated again: it came
/// from the program's output.
fn ipc_report(report: &sanitizer::SanitizerReport) -> Option<SanitizerReport> {
    Some(SanitizerReport {
        tool: match report.tool {
            sanitizer::Tool::Address => SanitizerTool::Address,
            sanitizer::Tool::Undefined => SanitizerTool::Undefined,
        },
        kind: SanitizerKind::new(&report.kind)?,
    })
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn pty_exit(status: ExitStatus) -> PtyExit {
        PtyExit {
            status,
            stopped: false,
            duration: Duration::from_millis(5),
            timed_out: false,
            too_many_processes: false,
        }
    }

    fn report(tool: sanitizer::Tool, kind: &str) -> sanitizer::SanitizerReport {
        sanitizer::SanitizerReport {
            tool,
            kind: kind.to_owned(),
        }
    }

    #[test]
    fn normal_exits_and_crashes() {
        let finished = decode(&pty_exit(ExitStatus::Exited(0)), None);
        assert_eq!(finished.status, IpcStatus::Exited { code: 0 });
        assert_eq!(finished.crash, None);
        assert_eq!(finished.message, "Finished (exit code 0)");

        let failed = decode(&pty_exit(ExitStatus::Exited(3)), None);
        assert_eq!(failed.status, IpcStatus::Exited { code: 3 });
        assert_eq!(failed.message, ExitStatus::Exited(3).describe());

        let segv = decode(&pty_exit(ExitStatus::Signaled(11)), None);
        assert_eq!(segv.status, IpcStatus::Signaled { signal: 11 });
        assert_eq!(segv.crash, Some(IpcCrash::MemoryAccess));
        assert!(
            segv.message
                .starts_with("Crashed: the program tried to use memory")
        );

        let fpe = decode(&pty_exit(ExitStatus::Signaled(8)), None);
        assert_eq!(fpe.crash, Some(IpcCrash::DivisionByZero));
        assert_eq!(fpe.message, "Crashed: integer division by zero (SIGFPE).");

        let abort = decode(&pty_exit(ExitStatus::Signaled(6)), None);
        assert_eq!(abort.crash, Some(IpcCrash::Aborted));

        let overflow = decode(&pty_exit(ExitStatus::Exception(0xC000_00FD)), None);
        assert_eq!(
            overflow.status,
            IpcStatus::Exception {
                ntstatus: 0xC000_00FD
            }
        );
        assert_eq!(overflow.crash, Some(IpcCrash::StackOverflow));
        assert!(overflow.message.starts_with("Crashed: stack overflow"));
    }

    #[test]
    fn a_stopped_program_is_stopped_whatever_ended_it() {
        for status in [
            ExitStatus::Signaled(15),
            ExitStatus::Signaled(9),
            ExitStatus::Exception(1),
            ExitStatus::Exited(1),
        ] {
            let exit = PtyExit {
                stopped: true,
                ..pty_exit(status)
            };
            let decoded = decode(&exit, Some(&report(sanitizer::Tool::Address, "segv")));
            assert_eq!(decoded.status, IpcStatus::Stopped);
            assert_eq!(decoded.crash, None);
            assert_eq!(decoded.message, "Stopped");
            // The report is still passed on: it was in the output.
            assert_eq!(
                decoded.sanitizer.map(|r| r.kind.to_string()).as_deref(),
                Some("segv")
            );
        }
    }

    #[test]
    fn sanitizer_reports_summarise_failed_runs() {
        let asan = decode(
            &pty_exit(ExitStatus::Exited(1)),
            Some(&report(sanitizer::Tool::Address, "heap-buffer-overflow")),
        );
        assert_eq!(asan.status, IpcStatus::Exited { code: 1 });
        assert_eq!(asan.crash, None);
        assert_eq!(asan.message, "Crashed: heap-buffer-overflow (AddressSanitizer)");
        let sanitizer = asan.sanitizer.unwrap();
        assert_eq!(sanitizer.tool, SanitizerTool::Address);
        assert_eq!(sanitizer.kind.as_str(), "heap-buffer-overflow");

        let ubsan = decode(
            &pty_exit(ExitStatus::Signaled(6)),
            Some(&report(sanitizer::Tool::Undefined, "division-by-zero")),
        );
        assert_eq!(
            ubsan.message,
            "Crashed: division-by-zero (UndefinedBehaviorSanitizer)"
        );
        assert_eq!(ubsan.crash, Some(IpcCrash::Aborted));

        // A report in the output of a program that then finished successfully
        // does not turn the exit into a crash.
        let fine = decode(
            &pty_exit(ExitStatus::Exited(0)),
            Some(&report(sanitizer::Tool::Undefined, "shift-base")),
        );
        assert_eq!(fine.message, "Finished (exit code 0)");
        assert!(fine.sanitizer.is_some());

        // A malformed kind (impossible from the detector) is dropped rather
        // than sent.
        let malformed = decode(
            &pty_exit(ExitStatus::Exited(1)),
            Some(&report(sanitizer::Tool::Address, "Bad Kind")),
        );
        assert_eq!(malformed.sanitizer, None);
        assert_eq!(malformed.message, "Finished with exit code 1");
    }

    #[test]
    fn every_crash_kind_has_its_ipc_twin() {
        let kinds = [
            Crash::MemoryAccess,
            Crash::StackOverflow,
            Crash::DivisionByZero,
            Crash::Aborted,
            Crash::Trap,
            Crash::Interrupted,
            Crash::Terminated,
            Crash::Killed,
            Crash::OutOfMemory,
            Crash::HeapCorruption,
            Crash::MissingDll,
            Crash::BrokenPipe,
            Crash::ResourceLimit,
            Crash::Other,
        ];
        let converted: Vec<IpcCrash> = kinds.into_iter().map(ipc_crash).collect();
        assert_eq!(converted, IpcCrash::ALL);
    }
}
