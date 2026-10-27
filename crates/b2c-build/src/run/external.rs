//! Running a program in a terminal window of its own
//! (`docs/spec/07-toolchain-build-run.md` §7.6.2; the owner's M3-D5).
//!
//! * **Windows:** the program gets a console window of its own
//!   ([`b2c_process::spawn_in_new_console`]), in its Job Object like any
//!   run. The IDE init unit waits for a key before that window closes, when
//!   [`EXTERNAL_TERMINAL_VAR`] is set (`crate::ide`).
//! * **Linux:** a terminal program from a fixed list, found only in
//!   `/usr/bin` and `/bin` (never through `PATH`), runs `/bin/sh` with a
//!   constant script ([`SCRIPT`]). The working folder, the status file, the
//!   program and its arguments are the script's positional parameters, so no
//!   argument is ever read by a shell. The script runs the program, writes
//!   its exit status into the status file (in a new owner-only folder) and
//!   waits for Enter. The terminal runs in the run's containment (its
//!   process group or cgroup scope), so Stop closes its window; a terminal
//!   that hands its windows to a server process (`gnome-terminal`, used
//!   last and with `--wait`) keeps the program outside it.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// The variable that tells the IDE init unit to wait for a key before the
/// program's own console window closes (Windows). The unit removes it, so
/// the program's children never see it.
pub const EXTERNAL_TERMINAL_VAR: &str = "B2C_EXTERNAL_TERMINAL";

/// The script the terminal runs: `cd` to `$1`, run `$3…` (the program and its
/// arguments), write the exit status into the file `$2`, say how it ended and
/// wait for Enter. Constant: nothing from the project or the user is part of
/// it.
pub(super) const SCRIPT: &str = r#"cd -- "$1" || exit 126
status_file=$2
shift 2
"$@"
status=$?
printf '%s\n' "$status" > "$status_file"
printf '\n[The program ended with exit status %s. Press Enter to close this window.]' "$status"
read -r _
"#;

/// The `$0` of [`SCRIPT`], shown by the shell in its messages.
const SCRIPT_NAME: &str = "blocks2cpp-run";

/// The folders a terminal program is looked for in.
const TERMINAL_DIRS: [&str; 2] = ["/usr/bin", "/bin"];

/// The terminals tried, in order, with the arguments that come before the
/// command they run. `x-terminal-emulator` is Debian's choice of the
/// user's terminal; it is used only when it leads to a terminal known to
/// keep its window in its own process ([`LINKABLE_TERMINALS`]), as others
/// (with a server process) come later with the flags that make them wait.
const TERMINALS: [(&str, &[&str]); 8] = [
    ("x-terminal-emulator", &["-e"]),
    ("konsole", &["--nofork", "-e"]),
    ("xfce4-terminal", &["--disable-server", "-x"]),
    ("kitty", &[]),
    ("alacritty", &["-e"]),
    ("foot", &[]),
    ("xterm", &["-e"]),
    ("gnome-terminal", &["--wait", "--"]),
];

/// The terminals `x-terminal-emulator` may stand for: ones whose window
/// belongs to the process started, with no flag needed. Others (`konsole`,
/// `gnome-terminal`, `tilix`, `terminator`, …) may hand the window to a
/// running instance, so the program would run outside the run's containment.
const LINKABLE_TERMINALS: [&str; 7] = ["xterm", "uxterm", "lxterm", "kitty", "alacritty", "foot", "urxvt"];

/// How a program is put in a window of its own on this system.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalTerminal {
    kind: Kind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Kind {
    /// Windows: a console window of the program's own.
    NewConsole,
    /// A terminal program and the arguments before the command it runs.
    Emulator {
        program: PathBuf,
        before: &'static [&'static str],
    },
}

impl ExternalTerminal {
    /// The terminal of this system: on Windows a new console window; on
    /// Linux the first terminal program found (see the module
    /// documentation), or `None` when there is none.
    pub fn find() -> Option<Self> {
        if cfg!(windows) {
            return Some(Self {
                kind: Kind::NewConsole,
            });
        }
        Self::find_in(&TERMINAL_DIRS.map(Path::new))
    }

    /// [`ExternalTerminal::find`] for Linux, looking in `dirs`.
    #[cfg_attr(windows, allow(dead_code))]
    fn find_in(dirs: &[&Path]) -> Option<Self> {
        for (name, before) in TERMINALS {
            for dir in dirs {
                let program = dir.join(name);
                if !is_executable_file(&program) {
                    continue;
                }
                if name == "x-terminal-emulator" && !stands_for_a_linkable_terminal(&program) {
                    continue;
                }
                return Some(Self {
                    kind: Kind::Emulator { program, before },
                });
            }
        }
        None
    }

    /// A terminal program at `program` taking the command after `-e`, for
    /// tests that play the terminal with a script of their own.
    #[doc(hidden)]
    pub fn emulator_for_tests(program: PathBuf) -> Self {
        Self {
            kind: Kind::Emulator {
                program,
                before: &["-e"],
            },
        }
    }

    /// Whether the program runs in a console window of its own
    /// ([`b2c_process::spawn_in_new_console`]) rather than through a
    /// terminal program.
    pub fn is_new_console(&self) -> bool {
        self.kind == Kind::NewConsole
    }

    /// The program and arguments that run `executable` with `args` in
    /// `working_dir`, writing its exit status into `status_file`: the
    /// executable itself on Windows, the terminal with a fixed `/bin/sh` script on
    /// Linux (see the module documentation).
    pub fn command_line(
        &self,
        executable: &Path,
        args: &[OsString],
        working_dir: &Path,
        status_file: &Path,
    ) -> (PathBuf, Vec<OsString>) {
        match &self.kind {
            Kind::NewConsole => (executable.to_path_buf(), args.to_vec()),
            Kind::Emulator { program, before } => {
                let mut words: Vec<OsString> = before.iter().map(OsString::from).collect();
                words.extend(["/bin/sh", "-c", SCRIPT, SCRIPT_NAME].iter().map(OsString::from));
                words.push(working_dir.as_os_str().to_owned());
                words.push(status_file.as_os_str().to_owned());
                words.push(executable.as_os_str().to_owned());
                words.extend(args.iter().cloned());
                (program.clone(), words)
            }
        }
    }
}

/// Whether `path` is a regular file (after links) that may be run.
fn is_executable_file(path: &Path) -> bool {
    let Ok(metadata) = std::fs::metadata(path) else {
        return false;
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        metadata.is_file() && metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        metadata.is_file()
    }
}

/// Whether the link `x-terminal-emulator` leads to one of
/// [`LINKABLE_TERMINALS`] (Debian's alternatives link to the program, or to
/// a wrapper such as `gnome-terminal.wrapper`, which is not one).
fn stands_for_a_linkable_terminal(program: &Path) -> bool {
    std::fs::canonicalize(program).is_ok_and(|target| {
        target
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| LINKABLE_TERMINALS.contains(&name))
    })
}

/// How a program in a terminal program ended, from the status [`SCRIPT`]
/// wrote: its exit code, or, as the shell writes it, 128 + the number of the
/// signal that ended it. `None` when the file is missing or not a number
/// (the window was closed before the program ended, or a terminal handed it
/// to a server process).
pub(super) fn read_status(status_file: &Path) -> Option<b2c_process::ExitStatus> {
    let text = std::fs::read_to_string(status_file).ok()?;
    let code: i32 = text.trim().parse().ok()?;
    Some(match code {
        129..=192 => b2c_process::ExitStatus::Signaled(code - 128),
        _ => b2c_process::ExitStatus::Exited(code),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    fn tool(path: &Path) {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, "").unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn terminals_are_tried_in_order_and_links_to_others_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("bin");
        assert_eq!(ExternalTerminal::find_in(&[&bin]), None);
        tool(&bin.join("gnome-terminal"));
        tool(&bin.join("xterm"));
        let found = ExternalTerminal::find_in(&[&bin]).unwrap();
        assert_eq!(
            found.kind,
            Kind::Emulator {
                program: bin.join("xterm"),
                before: &["-e"]
            }
        );
        // x-terminal-emulator that is gnome-terminal's wrapper, or tilix, is
        // skipped.
        tool(&bin.join("gnome-terminal.wrapper"));
        std::os::unix::fs::symlink(
            bin.join("gnome-terminal.wrapper"),
            bin.join("x-terminal-emulator"),
        )
        .unwrap();
        assert!(matches!(
            ExternalTerminal::find_in(&[&bin]).unwrap().kind,
            Kind::Emulator { program, .. } if program == bin.join("xterm")
        ));
        std::fs::remove_file(bin.join("x-terminal-emulator")).unwrap();
        tool(&bin.join("tilix"));
        std::os::unix::fs::symlink(bin.join("tilix"), bin.join("x-terminal-emulator")).unwrap();
        assert!(matches!(
            ExternalTerminal::find_in(&[&bin]).unwrap().kind,
            Kind::Emulator { program, .. } if program == bin.join("xterm")
        ));
        std::fs::remove_file(bin.join("x-terminal-emulator")).unwrap();
        std::os::unix::fs::symlink(bin.join("xterm"), bin.join("x-terminal-emulator")).unwrap();
        assert!(matches!(
            ExternalTerminal::find_in(&[&bin]).unwrap().kind,
            Kind::Emulator { program, .. } if program == bin.join("x-terminal-emulator")
        ));
        // A file that may not be run does not count.
        let other = dir.path().join("other");
        std::fs::create_dir_all(&other).unwrap();
        std::fs::write(other.join("xterm"), "").unwrap();
        assert_eq!(ExternalTerminal::find_in(&[&other]), None);
    }

    #[test]
    fn the_command_passes_everything_as_arguments() {
        let terminal = ExternalTerminal {
            kind: Kind::Emulator {
                program: PathBuf::from("/usr/bin/xterm"),
                before: &["-e"],
            },
        };
        let args = [OsString::from("$(rm -rf ~)"), OsString::from("a b")];
        let (program, words) = terminal.command_line(
            Path::new("/p/out/game"),
            &args,
            Path::new("/home/ada/game folder"),
            Path::new("/tmp/b2c-run/status"),
        );
        assert_eq!(program, PathBuf::from("/usr/bin/xterm"));
        let words: Vec<&str> = words.iter().map(|word| word.to_str().unwrap()).collect();
        assert_eq!(
            words,
            [
                "-e",
                "/bin/sh",
                "-c",
                SCRIPT,
                "blocks2cpp-run",
                "/home/ada/game folder",
                "/tmp/b2c-run/status",
                "/p/out/game",
                "$(rm -rf ~)",
                "a b"
            ]
        );
        let console = ExternalTerminal {
            kind: Kind::NewConsole,
        };
        assert!(console.is_new_console());
        assert_eq!(
            console.command_line(Path::new("/p/game"), &args, Path::new("/w"), Path::new("/s")),
            (PathBuf::from("/p/game"), args.to_vec())
        );
    }

    #[test]
    fn statuses_are_read_as_the_shell_writes_them() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("status");
        assert_eq!(read_status(&file), None);
        for (text, status) in [
            ("0\n", b2c_process::ExitStatus::Exited(0)),
            ("3\n", b2c_process::ExitStatus::Exited(3)),
            ("139\n", b2c_process::ExitStatus::Signaled(11)),
            ("255\n", b2c_process::ExitStatus::Exited(255)),
        ] {
            std::fs::write(&file, text).unwrap();
            assert_eq!(read_status(&file), Some(status), "{text:?}");
        }
        std::fs::write(&file, "soon").unwrap();
        assert_eq!(read_status(&file), None);
    }

    #[cfg(unix)]
    #[test]
    fn the_script_runs_the_program_in_its_folder_and_records_its_status() {
        let dir = tempfile::tempdir().unwrap();
        let work = dir.path().join("work dir");
        std::fs::create_dir(&work).unwrap();
        let status = dir.path().join("status");
        let mut command = b2c_process::Command::new("/bin/sh", dir.path()).unwrap();
        command
            .args(["-c", SCRIPT, SCRIPT_NAME])
            .arg(&work)
            .arg(&status)
            .args([
                "/bin/sh",
                "-c",
                "pwd > where.txt; printf %s \"$1\" > arg.txt; exit 7",
                "x",
                "$HOME;'",
            ])
            .env("PATH", "/usr/bin:/bin")
            .stdin(b2c_process::Stdin::Bytes(b"\n".to_vec()));
        let captured = b2c_process::run_captured(&command).unwrap();
        assert!(captured.status.success(), "{captured:?}");
        assert_eq!(read_status(&status), Some(b2c_process::ExitStatus::Exited(7)));
        assert_eq!(std::fs::read_to_string(work.join("arg.txt")).unwrap(), "$HOME;'");
        assert_eq!(
            std::fs::read_to_string(work.join("where.txt"))
                .unwrap()
                .trim_end(),
            work.to_str().unwrap()
        );
        assert!(String::from_utf8_lossy(&captured.stdout).contains("exit status 7"));
    }
}
