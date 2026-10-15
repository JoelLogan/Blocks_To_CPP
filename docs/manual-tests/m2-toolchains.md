# Manual test: M2 toolchain matrix

Blocks2Cpp builds programs with the g++ already on the user's computer, and
there are many ways to have one
([07 §7.1](../spec/07-toolchain-build-run.md#71-supported-toolchains)). CI
builds and runs the example projects with GCC 11, 13 and 15 on Linux (in
containers) and with MSYS2 UCRT64 on Windows
([09 §9.2](../spec/09-quality-and-delivery.md#92-testing-strategy)). It cannot
show that the app finds, checks and uses the compilers people actually
install: a distribution's own g++ on a desktop, WinLibs, TDM-GCC, Scoop,
Strawberry Perl's GCC. This protocol checks each of them once per milestone,
in a release build, from discovery to a program that runs.

For M2 one cell is required: the owner's Debian computer with Debian's own
g++ (owner decision D16 of the
[polish plan](../plans/m2-polish-plan.md#w7--m2-close-out)). The other cells
are run if time allows; a cell that is not run is recorded as deferred to M3
([10 §10.1](../spec/10-roadmap.md#101-milestones)).

A cell takes about 30 minutes, plus installing the compiler and building the
app.

## The matrix

| Cell | System                        | Compiler                                                                                   | Install                                                                                               | M2                         |
| ---- | ----------------------------- | ------------------------------------------------------------------------------------------ | ----------------------------------------------------------------------------------------------------- | -------------------------- |
| L1   | Debian 12 or 13 (the owner's) | Debian's g++ (GCC 12 on Debian 12, GCC 14 on Debian 13)                                    | `sudo apt install g++`                                                                                | Required                   |
| L2   | Ubuntu 22.04 or later         | Ubuntu's g++                                                                               | `sudo apt install g++`                                                                                | If time, or deferred to M3 |
| L3   | Debian or Ubuntu              | A second, side-by-side `g++-N` next to `g++`                                               | `sudo apt install g++-14` (a version the release has)                                                 | If time, or deferred to M3 |
| L4   | Fedora                        | Fedora's g++                                                                               | `sudo dnf install gcc-c++`                                                                            | If time, or deferred to M3 |
| L5   | Arch Linux                    | Arch's g++                                                                                 | `sudo pacman -S gcc`                                                                                  | If time, or deferred to M3 |
| L6   | RHEL, Rocky or AlmaLinux      | `gcc-toolset-N` (`/opt/rh/gcc-toolset-N/root/usr/bin`)                                     | `sudo dnf install gcc-toolset-14`                                                                     | If time, or deferred to M3 |
| L7   | Any Linux                     | Homebrew on Linux (`/home/linuxbrew/.linuxbrew/bin`)                                       | `brew install gcc`                                                                                    | If time, or deferred to M3 |
| W1   | Windows 10 or 11              | MSYS2 UCRT64 (what CI uses, and what the setup page recommends)                            | [`BUILDING.md`](../../BUILDING.md#windows), step 2                                                    | If time, or deferred to M3 |
| W2   | Windows 10 or 11              | MSYS2 MINGW64 (the older MSVCRT runtime)                                                   | `pacman -S mingw-w64-x86_64-gcc` in the _MSYS2 MINGW64_ shell                                         | If time, or deferred to M3 |
| W3   | Windows 10 or 11              | WinLibs, through winget                                                                    | `winget install BrechtSanders.WinLibs.POSIX.UCRT`                                                     | If time, or deferred to M3 |
| W4   | Windows 10 or 11              | TDM-GCC 10.3, its newest release (`C:\TDM-GCC-64\bin`): expected to be flagged **too old** | The installer from the TDM-GCC site                                                                   | If time, or deferred to M3 |
| W5   | Windows 10 or 11              | Scoop's `gcc` or `mingw`                                                                   | `scoop install gcc`                                                                                   | If time, or deferred to M3 |
| W6   | Windows 10 or 11              | Chocolatey's `mingw`                                                                       | `choco install mingw` (administrator)                                                                 | If time, or deferred to M3 |
| W7   | Windows 10 or 11              | Strawberry Perl's bundled GCC (`C:\Strawberry\c\bin`)                                      | `winget install StrawberryPerl.StrawberryPerl`                                                        | If time, or deferred to M3 |
| W8   | Windows 10 or 11              | Cygwin's g++: expected to be found and **warned against**                                  | The Cygwin installer, package `gcc-g++`; then `C:\cygwin64\bin` on the user path (below)              | If time, or deferred to M3 |
| W9   | Windows 10 or 11              | Legacy mingw.org in `C:\MinGW\bin`: expected to be flagged outdated                        | An existing old install only; do not install it for this test                                         | If time, or deferred to M3 |
| X1   | Linux or Windows              | Clang installed as `g++`: expected to be labelled _not supported yet_                      | Linux: a `g++` symbolic link to `clang++` in a folder of its own, put on `PATH` before the app starts | If time, or deferred to M3 |

Cells W4, W8, W9 and X1 check that the app says what is wrong with a
compiler it should not be used with: they pass when the toolchain page shows
the expected warning or error
([toolchain diagnostics](../reference/diagnostics/toolchain.md)). Cell W4
cannot build: TDM-GCC's newest release is GCC 10.3, and Blocks2Cpp needs
GCC 11 or newer. For cell W8, put Cygwin's folder on the user path as
[`BUILDING.md`](../../BUILDING.md#windows) step 2 does for MSYS2's: the
Cygwin installer does not, and the app does not look there by itself.

## Before you start

1. Build the release build of the commit under test with
   [`BUILDING.md`](../../BUILDING.md) (never the development server, which
   differs in speed and looks). Build the command-line tool too
   ([The command-line tool](../../BUILDING.md#the-command-line-tool)). Write
   `git rev-parse --short HEAD` into the [record](#record).
2. Install the cell's compiler as the matrix says. On Windows, open a new
   PowerShell window afterwards, so that its folder is on the path. Start
   the app only after that: it reads the path once, when it starts.
3. Write down what the compiler says about itself, in a new terminal:

   ```sh
   g++ --version
   g++ -dumpfullversion
   g++ -dumpmachine
   command -v g++                # Linux; on Windows: where.exe g++
   ```

4. Start from a fresh profile: no settings, recent projects, trust records,
   recovery snapshots or remembered toolchains of earlier runs. Quit the app,
   then move its folders aside
   ([02 §2.7](../spec/02-architecture.md#27-persistence-locations)):
   - Linux: `~/.config/blocks2cpp/`, `~/.cache/blocks2cpp/` and
     `~/.local/state/blocks2cpp/` (or their `$XDG_…_HOME` equivalents);
   - Windows: `%APPDATA%\Blocks2Cpp\` and `%LOCALAPPDATA%\Blocks2Cpp\`.

   Put them back when the cell is done.

## Steps

Do these in order for each cell. Write each step's result into the cell's
row of the [results](#results): **pass**, or **fail** with what you saw.

1. **Discovery.** Start the app. Within a few seconds the status bar names the
   compiler. Click it: the toolchain page lists the compiler with its version,
   target, flavour (for example _MSYS2 UCRT64_), location and capabilities
   (the C++ standards, `std::format`, sanitizers, SARIF diagnostics), with no
   problems, and marks it as selected. Write down the location and the
   capabilities shown. The version and target must match step 3 of
   [Before you start](#before-you-start).
   - When several compilers are found (cell L3, or a computer with more than
     one), each is listed, and _Select as default_ changes the one the status
     bar names.
   - Cells W8 and W9: the compiler is listed with the warning `B2C-T1007`
     (a Cygwin compiler, whose programs need `cygwin1.dll`) or `B2C-T1008`
     (the old mingw.org MinGW). Do the other steps with it only when it is
     selected, and note what happens.
   - Cells W4 and X1: the compiler is listed as not usable, with the error
     `B2C-T1004` (_too old_: Blocks2Cpp needs GCC 11 or newer) or
     `B2C-T1005` (_Clang is not supported yet_). Skip steps 2 to 5 when no
     other compiler is found.
2. **Hello World.** On the start page choose the _Hello World_ template, and
   press **F5**. The console shows `Hello, world!` and
   the header says _Finished (exit code 0)_. The Build output tab shows no
   error or warning.
3. **The guessing game.** Open `examples/guessing_game.b2c` from the
   repository with _Open…_. It opens in Restricted Mode: choose _Trust…_ in the
   banner, then _Trust this project_. Press **F5**, answer the questions
   until the program says `Correct!`, and check that the header says
   _Finished (exit code 0)_. Then press **⟲ Run again** and **■ Stop** while
   the program waits for a guess: the header says _Stopped_.
4. **An exit code.** Open and trust `examples/exit_code.b2c`, and press
   **F5**. The console shows `Checking...` and `Stopping early`, and the
   header says _Finished with exit code 3_.
5. **Release.** Switch the toolbar's _Debug_ to _Release_, and run the
   guessing game again. It is built again (the Build output shows a new
   build) and runs as in step 3.
6. **Rescan.** With the app running, make the compiler unavailable as below,
   then choose _Rescan_ on the toolchain page: the compiler is no longer
   listed (with no other compiler, the setup page opens instead, with the
   install command for the system). Put it back and choose _I installed it →
   Rescan_ or _Rescan_: the compiler is listed and usable again.

   Changing the path does not make a compiler unavailable: the app reads the
   path once, when it starts, and also looks in the well-known folders of
   [07 §7.2](../spec/07-toolchain-build-run.md#72-discovery) whatever the
   path says (`/usr/bin`, `/usr/local/bin`, MSYS2, Scoop, Chocolatey,
   WinGet's WinLibs, TDM-GCC, Strawberry Perl), and on Linux under the names
   `g++` and `g++-16` down to `g++-11`. Instead:
   - Debian and Ubuntu (L1, L2): `sudo apt remove g++-N`, with N the major
     version from step 3 of [Before you start](#before-you-start) (`g++-12`
     on Debian 12, `g++-14` on Debian 13); apt also removes `g++` and
     `build-essential`, which depend on it. Put them back with
     `sudo apt install g++ build-essential`.
   - L3: `sudo apt remove g++-M`, the side-by-side version you installed:
     only that compiler disappears, and `g++` stays listed. Put it back with
     `sudo apt install g++-M`.
   - Fedora and Arch Linux (L4, L5): `sudo mv /usr/bin/g++ /usr/bin/g++.off`,
     and back afterwards.
   - L6: rename `/opt/rh/gcc-toolset-N` (with `sudo mv`), and back
     afterwards. L7: `brew unlink gcc`, then `brew link gcc`.
   - Windows: rename the compiler's install folder, and back afterwards:
     `C:\msys64`, `C:\TDM-GCC-64`, `C:\Strawberry`, `C:\cygwin64`, Scoop's
     `%USERPROFILE%\scoop\apps\gcc`, Chocolatey's `mingw` folder, or the
     WinLibs folder under `%LOCALAPPDATA%\Microsoft\WinGet\Packages`. Close
     every program that runs from it first.
   - X1: delete the `g++` link, and make it again afterwards.

7. **The command-line tool.** In a terminal in the repository folder:

   ```sh
   ./target/release/b2c toolchains
   ./target/release/b2c run examples/hello_world.b2c
   ```

   The first lists the same compiler with the same version and target, and
   the second prints `Hello, world!`. On Windows, write
   `.\target\release\b2c.exe` and `examples\hello_world.b2c`.

Only for cell L1, also check the setup page's Debian command. When Debian's
g++ is the only compiler, step 6 does it: after `sudo apt remove g++-N` and
_Rescan_, the setup page shows `sudo apt install g++` with a _Copy_ button,
and after installing, _I installed it → Rescan_ finds the compiler without
restarting the app. Otherwise, if g++ was not installed before this pass,
start the app once **before** installing it: the setup page opens by itself
and shows the same command.

## Recording results

- Record locations without your user name: write `<user>` instead (for
  example `C:\Users\<user>\scoop\apps\gcc\current\bin`).
- A failure is a defect of the app unless the compiler is broken outside it
  too (for example `g++` fails on a hello-world program in a terminal). File
  an issue for each failure, quoting the cell and the step, with the output
  of step 3 of [Before you start](#before-you-start) and the toolchain page's
  problems.
- To defer a cell to M3, write _Deferred to M3_ and the date into its row,
  instead of results. M2 needs cell L1; every other cell is either run or
  deferred.

## Record

| Item                                   | Value |
| -------------------------------------- | ----- |
| Commit (`git rev-parse --short HEAD`)  |       |
| Date                                   |       |
| Tester (a role, such as _the owner_)   |       |
| System for L1 (Debian release, kernel) |       |
| Desktop session (Wayland or X11)       |       |

### Results

Steps: 1 discovery, 2 Hello World, 3 guessing game, 4 exit code, 5 Release,
6 rescan, 7 command-line tool.

| Cell | Compiler (version, target) | Location | Capabilities shown | 1   | 2   | 3   | 4   | 5   | 6   | 7   | Notes |
| ---- | -------------------------- | -------- | ------------------ | --- | --- | --- | --- | --- | --- | --- | ----- |
| L1   |                            |          |                    |     |     |     |     |     |     |     |       |
| L2   |                            |          |                    |     |     |     |     |     |     |     |       |
| L3   |                            |          |                    |     |     |     |     |     |     |     |       |
| L4   |                            |          |                    |     |     |     |     |     |     |     |       |
| L5   |                            |          |                    |     |     |     |     |     |     |     |       |
| L6   |                            |          |                    |     |     |     |     |     |     |     |       |
| L7   |                            |          |                    |     |     |     |     |     |     |     |       |
| W1   |                            |          |                    |     |     |     |     |     |     |     |       |
| W2   |                            |          |                    |     |     |     |     |     |     |     |       |
| W3   |                            |          |                    |     |     |     |     |     |     |     |       |
| W4   |                            |          |                    |     |     |     |     |     |     |     |       |
| W5   |                            |          |                    |     |     |     |     |     |     |     |       |
| W6   |                            |          |                    |     |     |     |     |     |     |     |       |
| W7   |                            |          |                    |     |     |     |     |     |     |     |       |
| W8   |                            |          |                    |     |     |     |     |     |     |     |       |
| W9   |                            |          |                    |     |     |     |     |     |     |     |       |
| X1   |                            |          |                    |     |     |     |     |     |     |     |       |

### Sign-off

| Date | Tester    | Build (commit) | Cells run | Cells deferred to M3 | Issues filed |
| ---- | --------- | -------------- | --------- | -------------------- | ------------ |
|      | The owner |                |           |                      |              |
