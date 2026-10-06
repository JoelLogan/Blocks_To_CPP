# Finding and using the compiler (`B2C-T1xxx`)

These messages are about the C++ compiler on this computer, not about your
blocks, so they point at the whole project
([spec chapter 7](../../spec/07-toolchain-build-run.md)). Blocks2Cpp needs
**GCC's g++, version 11 or newer** (13 or newer is recommended). On Windows,
install [MSYS2](https://www.msys2.org) and run
`pacman -S mingw-w64-ucrt-x86_64-gcc` in its UCRT64 terminal, or install
WinLibs. On Linux, install `g++` with your package manager.

**Severity:** errors stop the build; warnings and notes do not.

| Code | Severity | Meaning |
|------|----------|---------|
| [B2C-T1001](#b2c-t1001-no-usable-compiler) | error | No usable g++ was found |
| [B2C-T1002](#b2c-t1002-the-chosen-compiler-cannot-be-used) | error | A g++ path given explicitly cannot be used |
| [B2C-T1003](#b2c-t1003-the-compiler-did-not-run) | error | The compiler could not be started or did not answer |
| [B2C-T1004](#b2c-t1004-gcc-is-too-old) | error | GCC is older than version 11 |
| [B2C-T1005](#b2c-t1005-clang-is-not-supported-yet) | error | The compiler is Clang |
| [B2C-T1006](#b2c-t1006-the-installation-is-broken) | error | The installation is broken or incomplete |
| [B2C-T1007](#b2c-t1007-cygwin-compiler) | warning | A Cygwin/MSYS compiler: programs need `cygwin1.dll` |
| [B2C-T1008](#b2c-t1008-old-mingw) | warning | The outdated mingw.org MinGW |
| [B2C-T1009](#b2c-t1009-the-compiler-changed) | note | The compiler changed since it was checked, so it was checked again |
| [B2C-T1010](#b2c-t1010-c-standard-not-supported) | error | The project's C++ standard is not supported by this compiler |
| [B2C-T1011](#b2c-t1011-a-sanitizer-was-left-out) | note | A sanitizer was left out |
| [B2C-T1012](#b2c-t1012-a-hardening-option-was-left-out) | note | A hardening option was left out |
| [B2C-T1013](#b2c-t1013-linked-dynamically) | note | Static linking did not work, so the program is linked dynamically |
| [B2C-T1014](#b2c-t1014-not-gcc) | error | The program does not look like GCC's g++ |
| [B2C-T1015](#b2c-t1015-an-extra-flag-was-refused) | error | A machine-local extra flag was refused |
| [B2C-T1016](#b2c-t1016-an-environment-variable-was-not-passed-on) | warning | A pass-through environment variable was refused |
| [B2C-T1017](#b2c-t1017-a-pkg-config-flag-was-ignored) | warning | A flag from `pkg-config` was ignored |
| [B2C-T1018](#b2c-t1018-invalid-library-settings) | error | A library profile is invalid |
| [B2C-T1019](#b2c-t1019-a-define-cannot-be-used) | error | A project define cannot be passed to the compiler |
| [B2C-T1020](#b2c-t1020-compiler-on-a-network-folder) | warning | The compiler is on a network folder |
| [B2C-T1021](#b2c-t1021-no-leak-detection) | note | Leak detection is not available where programs run |
| [B2C-T1022](#b2c-t1022-the-selected-compiler-cannot-be-used) | warning | The selected compiler is missing or unusable, so another one was used |

## B2C-T1001: no usable compiler

No g++ 11 or newer was found in the folders on `PATH` or in the usual
install locations (on Windows: MSYS2, WinLibs, Scoop, Chocolatey, WinGet,
TDM-GCC and Strawberry Perl folders; on Linux: `/usr/bin`, `/usr/local/bin`
and the GCC toolsets in `/opt/rh`). The project folder and the current folder
are never searched, so a project cannot bring its own "compiler".

**How to fix:** install g++ as described at the top of this page, then try
again. `b2c toolchains` lists what was found and why each one was rejected.

## B2C-T1002: the chosen compiler cannot be used

A compiler chosen by path (for example `b2c build --toolchain <path>`) is not
an absolute path to an executable file named `g++` (or `g++.exe` on
Windows). Batch files (`.bat`, `.cmd`) are never accepted. In the app,
*Choose g++ manually…* accepts on Windows only a file named exactly
`g++.exe`.

The same code refuses a compiler, chosen or found, that lies inside the open
project's folder: a project must never bring its own compiler, so such a
program is never run. In the app, *Choose g++ manually…* also refuses a file
inside an open project's folder or the build cache before running it.

*Example:* "The compiler C:\tools\g++.cmd cannot be used: only .exe programs
can be used (never .bat or .cmd scripts)."

**How to fix:** choose the `g++` (or `g++.exe`) program itself, by its full
path, from a folder outside your projects.

## B2C-T1003: the compiler did not run

Asking the compiler for its version failed: it could not be started, it
crashed, or it did not answer within 10 seconds.

*Example:* "The compiler /usr/bin/g++ could not be used: it did not answer
within 10000 ms."

**How to fix:** check that the compiler works on its own (`g++ --version` in
a terminal). Security software sometimes blocks new programs; allow it there.

## B2C-T1004: GCC is too old

Blocks2Cpp needs GCC 11 or newer for the C++20 features its generated code
and checks rely on.

**How to fix:** install a newer g++ (13 or newer is recommended).

## B2C-T1005: Clang is not supported yet

The program called `g++` is actually Clang (as on macOS). Clang support is
planned but not available yet.

**How to fix:** install GCC's g++ and choose it.

## B2C-T1006: the installation is broken

The compiler answered, but cannot do its job: its C++ compiler program
(`cc1plus`) is missing, it could not build and run a tiny test program, or it
cannot compile C++20.

*Example:* "The installation of /usr/bin/g++ is incomplete: its C++ compiler
program (cc1plus) is missing. Reinstall the compiler (for example the g++
package)."

**How to fix:** reinstall the compiler. On Linux, make sure the `g++` package
(not only `gcc`) is installed.

## B2C-T1007: Cygwin compiler

The compiler targets Cygwin or MSYS, so the programs it builds only run where
`cygwin1.dll` is available.

**How to fix:** use MSYS2's **UCRT64** g++ (`mingw-w64-ucrt-x86_64-gcc`),
which builds normal Windows programs.

## B2C-T1008: old MinGW

The compiler is the old MinGW from mingw.org, which is outdated and lacks
many modern C++ features.

**How to fix:** install MSYS2 (UCRT64) or WinLibs.

## B2C-T1009: the compiler changed

The compiler file changed since it was last checked (it was probably
updated), so its capabilities were checked again. Nothing to do.

## B2C-T1010: C++ standard not supported

The project uses a C++ standard (for example C++26) that this compiler does
not support.

*Example:* "This project uses C++26, which /usr/bin/g++ does not support.
Choose an older C++ standard in the project settings or install a newer
g++."

**How to fix:** choose an older standard in the project settings, or install
a newer g++.

## B2C-T1011: a sanitizer was left out

A debug build normally checks memory accesses and undefined behaviour with
sanitizers. This compiler cannot provide one of them, so the build goes ahead
without it.

*Example:* "AddressSanitizer is not available for g++ on Windows, so debug
builds do not check memory accesses."

## B2C-T1012: a hardening option was left out

Some protections against exploits (spec §7.4.3) are not supported by this
compiler or linker and were left out. The build goes ahead.

## B2C-T1013: linked dynamically

On Windows, programs are linked statically by default so they run anywhere.
That did not work with this compiler, so the program is linked dynamically and
needs the compiler's DLLs (on `PATH`) to run.

**How to fix:** usually nothing; to run the program elsewhere, copy the DLLs
next to it or use a compiler that can link statically (MSYS2 UCRT64 can).

## B2C-T1014: not GCC

The program's version output does not look like GCC's g++.

**How to fix:** choose GCC's g++ program.

## B2C-T1015: an extra flag was refused

A machine-local extra compiler or linker flag (an advanced setting) is on the
denylist because it could change which programs the compiler runs, where it
writes files, or what it reads (spec §7.4.5).

*Example:* "The extra compiler flag `-fplugin=/tmp/x.so` is not allowed:
it could make the compiler run other programs or load plugins. Remove it from the machine settings."

**How to fix:** remove the flag from the machine settings.

## B2C-T1016: an environment variable was not passed on

A variable in the machine settings' pass-through list was not passed to the
compiler, because it could change how the compiler finds its programs and
headers (for example `GCC_EXEC_PREFIX` or `LD_PRELOAD`).

**How to fix:** remove it from the pass-through list.

## B2C-T1017: a pkg-config flag was ignored

Only `-I`, `-isystem`, `-L`, `-l`, `-D` and `-pthread` flags from
`pkg-config` are used (with absolute folders); anything else is ignored.

*Example:* "The flag `-Wl,-rpath,/opt/x` from pkg-config was ignored, because
only -I, -isystem, -L, -l, -D and -pthread are allowed."

**How to fix:** usually nothing. If the library does not work without the
flag, set it up as a library profile instead.

## B2C-T1018: invalid library settings

A library profile has a folder that is not an absolute path, or a library name
that is not a plain name.

*Example:* "The library folder libs/SFML/include is not an absolute path;
choose it again in the library settings."

**How to fix:** choose the folders again in the library settings.

## B2C-T1019: a define cannot be used

A project define has a name that is not a valid identifier or a value that
cannot be written safely on the command line.

**How to fix:** rename the define, or simplify its value, in the project's
build settings.

## B2C-T1020: compiler on a network folder

The compiler is on a network (UNC) path. Builds are slower, and anyone who can
change that folder can change what runs on this computer.

**How to fix:** install the compiler on a local disk.

## B2C-T1021: no leak detection

AddressSanitizer works, but its leak detection does not (this happens in some
containers and under debuggers), so debug builds run without leak checks.
Memory errors are still found.

## B2C-T1022: the selected compiler cannot be used

The compiler selected as the default in the app's toolchain list (*Select as
default*) cannot be used for this build, so the build used the first usable
compiler in discovery order instead, the same one it would use with nothing
selected ([spec §7.2](../../spec/07-toolchain-build-run.md#72-discovery)).
The selected compiler is checked again before every build, and this happens
when:

* it is no longer on this computer (it was uninstalled or moved);
* it changed and now fails its checks, for example after an update to a
  version that is too old or broken;
* it lies inside the open project's folder, where compilers are never run
  (see [B2C-T1002](#b2c-t1002-the-chosen-compiler-cannot-be-used)).

The build never switches compilers silently: this warning names the
compiler that was used. When no other usable compiler exists either, it
comes with [B2C-T1001](#b2c-t1001-no-usable-compiler).

*Example:* "The selected compiler is no longer available on this computer, so
/usr/bin/g++ was used instead. Select another compiler in the toolchain list
to stop this warning."

**How to fix:** open the toolchain list, then reinstall the compiler you
selected and choose *Rescan*, or select another compiler as the default.
