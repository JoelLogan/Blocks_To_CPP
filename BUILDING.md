# Building Blocks2Cpp

Blocks2Cpp has no installers yet (they come with milestone M6), so the app is
built from its source code. This page is the one place with the steps: what
to install, how to build the desktop app and the command-line tool, and how
to start them. Follow the section for your system from top to bottom; each
command runs from where the previous one left off.

- [What you need](#what-you-need)
- [Linux: Debian and Ubuntu](#linux-debian-and-ubuntu) (also
  [Fedora](#fedora-untested) and [Arch Linux](#arch-linux-untested), untested)
- [Windows](#windows)
- [Run Blocks2Cpp](#run-blocks2cpp)
- [Develop](#develop)
- [The command-line tool](#the-command-line-tool)
- [After you pull changes](#after-you-pull-changes)
- [Troubleshooting](#troubleshooting)

## What you need

| Tool                       | Version                                                 | Used for                                                       |
| -------------------------- | ------------------------------------------------------- | -------------------------------------------------------------- |
| Git and curl               | any                                                     | getting the source and the installers below                    |
| Rust, through rustup       | 1.97.0, from `rust-toolchain.toml`, installed by rustup | the app's backend, the command-line tool and the compiler core |
| C and C++ build tools      | Linux: GCC; Windows: the Microsoft C++ Build Tools      | the native parts of Rust crates                                |
| Webview                    | Linux: WebKitGTK 4.1; Windows: WebView2                 | the app's window (Windows 10 and 11 already have WebView2)     |
| Node.js                    | 22, from 22.14 on                                       | building the editor                                            |
| pnpm                       | 11.27.1, from `package.json`, through Corepack          | the editor's packages                                          |
| wasm-bindgen CLI           | 0.2.129, the `wasm-bindgen` version in `Cargo.lock`     | the editor's compiler core (WebAssembly)                       |
| binaryen's `wasm-opt`      | 105 or later (optional)                                 | making the WebAssembly core smaller                            |
| tauri-driver (E2E tests)   | 2.1.0, `TAURI_DRIVER_VERSION` in the CI workflows       | the end-to-end tests (contributors only)                       |
| g++ (only to run programs) | 11 or later                                             | building the programs you make; not needed to build the app    |

The pinned versions come from the repository (`rust-toolchain.toml`,
`package.json`, `Cargo.lock`, the CI workflows), and the commands below
install exactly those. rustup and Corepack use the pinned Rust and pnpm by
themselves, and the WebAssembly core's build stops, naming the
`cargo install` command to run, when the wasm-bindgen CLI has another
version. Nothing checks the others: pnpm only warns about an older Node.js,
and tauri-driver must be installed again by hand, with the command under
[End-to-end tests](#end-to-end-tests), when the workflows'
`TAURI_DRIVER_VERSION` changes. binaryen and g++ are minimums; the core has
been built and tested with Ubuntu 22.04's binaryen and with CI's release.

Rust's build folder, `target/`, takes about 2 GB for the app. The checks in
[`CONTRIBUTING.md`](CONTRIBUTING.md#checks) build many more configurations:
keep about 20 GB free for them.

## Linux: Debian and Ubuntu

For Debian 12 and 13 and Ubuntu 22.04 and later, on a 64-bit PC. The
packages and build commands are the ones CI uses on Ubuntu 24.04
(`.github/workflows/desktop.yml`).

1. Install the system packages: the C and C++ compilers (including g++),
   WebKitGTK and GTK with their development files, binaryen, Git and curl.

   ```sh
   sudo apt update
   sudo apt install build-essential pkg-config libwebkit2gtk-4.1-dev \
     libjavascriptcoregtk-4.1-dev libsoup-3.0-dev libgtk-3-dev librsvg2-dev \
     binaryen git curl ca-certificates
   ```

2. Install rustup, Rust's installer. The repository names its own Rust
   version, so rustup installs no default one here.

   ```sh
   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain none
   . "$HOME/.cargo/env"
   ```

3. Install Node.js 22 into `~/.local/node` (no `sudo`; the distributions'
   own Node.js packages are too old), check the download, and turn on
   Corepack, which fetches the pnpm version the repository names. If you
   already have Node.js 22.14 or later, run only `corepack enable`. On an ARM
   computer, write `linux-arm64` instead of `linux-x64`.

   ```sh
   mkdir -p ~/.local/node && cd ~/.local/node
   curl -fsSLO https://nodejs.org/dist/latest-v22.x/SHASUMS256.txt
   archive=$(grep -o 'node-v22\.[0-9.]*-linux-x64\.tar\.gz' SHASUMS256.txt)
   curl -fsSLO "https://nodejs.org/dist/latest-v22.x/$archive"
   grep " $archive\$" SHASUMS256.txt | sha256sum --check   # must print: OK
   tar -xzf "$archive" --strip-components=1 && rm "$archive" SHASUMS256.txt
   cd -
   echo 'export PATH="$HOME/.local/node/bin:$PATH"' >> ~/.profile
   export PATH="$HOME/.local/node/bin:$PATH"
   corepack enable
   ```

4. Get the source, and install the Rust toolchain, the wasm-bindgen CLI and
   the editor's packages. `cargo install` compiles wasm-bindgen and takes a
   few minutes. The first time pnpm runs, Corepack may ask to download pnpm:
   answer `Y`.

   ```sh
   git clone https://github.com/JoelLogan/Blocks_To_CPP.git
   cd Blocks_To_CPP
   rustup toolchain install
   cargo install wasm-bindgen-cli --version 0.2.129 --locked
   pnpm install --frozen-lockfile
   ```

5. Build the WebAssembly core (the compiler the editor runs for its live C++
   view), then the app: `pnpm app:build` does both, in that order. The first
   build of the app compiles several hundred Rust crates and takes a while.

   ```sh
   pnpm app:build
   ```

6. Start it:

   ```sh
   ./target/release/blocks2cpp-desktop
   ```

Then read [Run Blocks2Cpp](#run-blocks2cpp).

### Fedora (untested)

Nobody has followed these steps yet. Step 1 becomes:

```sh
sudo dnf install gcc gcc-c++ pkgconf-pkg-config webkit2gtk4.1-devel \
  javascriptcoregtk4.1-devel libsoup3-devel gtk3-devel librsvg2-devel \
  binaryen git curl
```

Steps 2 to 6 are the same as for Debian and Ubuntu.

### Arch Linux (untested)

Nobody has followed these steps yet. Step 1 becomes:

```sh
sudo pacman -S --needed base-devel webkit2gtk-4.1 libsoup3 gtk3 librsvg \
  binaryen git curl
```

Steps 2 to 6 are the same as for Debian and Ubuntu.

## Windows

For Windows 10 (version 1809 or later) and 11; CI builds the app the same
way on Windows Server 2025. Run the commands in PowerShell. After the
`winget install` commands, open a new PowerShell window so that the new
programs are on the path.

1. Install Git, the Microsoft C++ Build Tools (the C++ workload, which Rust
   needs), rustup, Node.js 22 and MSYS2 (for g++):

   ```powershell
   winget install --exact --id Git.Git
   winget install --exact --id Microsoft.VisualStudio.2022.BuildTools --override "--wait --passive --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"
   winget install --exact --id Rustlang.Rustup
   winget install --exact --id OpenJS.NodeJS.22
   winget install --exact --id MSYS2.MSYS2
   ```

2. Install g++ in MSYS2 and put it on your path. Blocks2Cpp needs it to build
   your programs, not to build the app itself.

   ```powershell
   C:\msys64\usr\bin\bash.exe -lc "pacman -S --needed --noconfirm mingw-w64-ucrt-x86_64-gcc"
   [Environment]::SetEnvironmentVariable('Path', [Environment]::GetEnvironmentVariable('Path', 'User') + ';C:\msys64\ucrt64\bin', 'User')
   ```

3. Turn on Corepack, which fetches the pnpm version the repository names.
   Node.js is installed for all users, so this one command needs a PowerShell
   window opened with **Run as administrator**:

   ```powershell
   corepack enable
   ```

4. Open a new (normal) PowerShell window. The `pnpm` command Corepack added
   is a PowerShell script, and Windows runs no scripts until you allow them,
   so first allow scripts for your account (this needs no administrator
   rights):

   ```powershell
   Set-ExecutionPolicy -Scope CurrentUser RemoteSigned
   ```

   Then get the source into a short folder such as `C:\src` (long paths can
   break the build), and install the Rust toolchain, the wasm-bindgen CLI and
   the editor's packages:

   ```powershell
   mkdir C:\src -Force; cd C:\src
   git clone https://github.com/JoelLogan/Blocks_To_CPP.git
   cd Blocks_To_CPP
   rustup toolchain install
   cargo install wasm-bindgen-cli --version 0.2.129 --locked
   pnpm install --frozen-lockfile
   ```

5. Build the WebAssembly core and then the app (`pnpm app:build` does both).
   `B2C_SKIP_WASM_OPT` skips binaryen's `wasm-opt`, which Windows does not
   have; the core is then larger but works the same (see
   [Troubleshooting](#the-webassembly-core-does-not-build) to use
   `wasm-opt`).

   ```powershell
   $env:B2C_SKIP_WASM_OPT = '1'
   pnpm app:build
   ```

6. Start it:

   ```powershell
   .\target\release\blocks2cpp-desktop.exe
   ```

## Run Blocks2Cpp

The release build from the steps above is the app as it ships: the editor is
embedded in `target/release/blocks2cpp-desktop` (`.exe` on Windows), its
security settings apply, and it runs at full speed. Use it whenever you use
the app or judge how it looks and how fast it is. You can copy the
executable elsewhere, for example to `~/.local/bin/`; build it again after
you pull changes ([below](#after-you-pull-changes)).

The app finds g++ on your path when it starts and names it in the status
bar. Without one, it shows a setup page that explains how to install it.
[Getting started](docs/user-guide/getting-started.md) walks you from the first
start to a number-guessing game built from blocks.

`pnpm desktop:build` builds the same executable plus every installer type
configured in `apps/desktop/src-tauri/tauri.conf.json` (MSI and NSIS on
Windows; AppImage, `.deb` and `.rpm` on Linux). Nobody tests those installers
yet; real installers come with milestone M6.

## Develop

To change the app, run it from the development server, which reloads the
editor as you edit. It needs steps 1 to 4 and the WebAssembly core:

```sh
pnpm wasm:build
pnpm desktop:dev
```

On Windows, set `$env:B2C_SKIP_WASM_OPT = '1'` first in each new PowerShell
window, as in step 5: Windows has no `wasm-opt`, and without the variable
`pnpm wasm:build` stops and leaves no core at all.

The development server warns when the WebAssembly core is older than its
source, and the window does not start when the core's block catalog differs
from the backend's; run `pnpm wasm:build` again in either case.

**Do not judge speed or looks from it.** It runs a debug build of the
backend, React's development mode (which renders everything twice to find
bugs) and the editor from the Vite dev server without the app's Content
Security Policy, so it is slower and can look and behave differently from the
release build. For a quicker build that still behaves like the release, use
`pnpm --filter @blocks2cpp/desktop tauri build --debug --no-bundle`, which
writes `target/debug/blocks2cpp-desktop`.

The checks are listed in [CONTRIBUTING.md](CONTRIBUTING.md#checks); the code
layout, the tests and how the app is built inside are described in the
[desktop app's README](apps/desktop/README.md) and the
[developer guide](docs/developer-guide/README.md).

### End-to-end tests

The end-to-end tests drive the app through WebDriver. Besides steps 1 to 4
they need tauri-driver at the version the CI workflows pin and, on Linux,
WebKitGTK's WebDriver and a virtual display:

```sh
cargo install tauri-driver --version 2.1.0 --locked
sudo apt install webkit2gtk-driver xvfb
```

How to build the app under test and run the tests is in the
[end-to-end tests' README](apps/desktop/e2e/README.md#running-them-locally-linux).

## The command-line tool

The command-line tool `b2c` checks, translates, builds and runs projects
without the app. It needs Git, Rust and g++, but not Node.js, the webview
libraries or the WebAssembly tools: on Debian and Ubuntu, step 1 can be
`sudo apt install build-essential git curl`, then follow step 2 and the
`git clone` and `cd` of step 4. From the repository folder:

```sh
cargo build --release -p b2c-cli
./target/release/b2c run examples/hello_world.b2c        # build and run
./target/release/b2c run examples/guessing_game.b2c      # an interactive program
./target/release/b2c generate examples/fizzbuzz.b2c --out target/fizzbuzz   # see the C++
./target/release/b2c check examples/primes.b2c           # check without building
./target/release/b2c toolchains                          # the compilers found
```

On Windows, write `.\target\release\b2c.exe` and `examples\…`.
[`examples/`](examples/README.md) has 15 projects to try, and
[`docs/reference/cli.md`](docs/reference/cli.md) describes every command.

## After you pull changes

Run the install and build steps again; each one finishes quickly when its
part has not changed:

```sh
git pull
rustup toolchain install
pnpm install --frozen-lockfile
pnpm app:build
```

`pnpm app:build` rebuilds the WebAssembly core before the app: the app
embeds the core that is built at that moment, and an old core would show C++
from the old compiler. The app's build stops, naming `pnpm wasm:build`, when
the core is older than its source. If the repository moved to a new
wasm-bindgen version, the core's build stops and names the `cargo install`
command to run. On Windows, set `$env:B2C_SKIP_WASM_OPT = '1'` first, as in
step 5.

## Troubleshooting

`pnpm doctor`, run from the repository root, checks this computer against
this page: each tool's version against the one the repository pins, the
system libraries, the editor's packages and whether the WebAssembly core is
built from the current source. It names the command that fixes each
problem, and changes nothing itself.

### The WebAssembly core does not build

- **"The WebAssembly core of the editor has not been built"**: the app or
  the development server was started before the core was built, or a failed
  `pnpm wasm:build` removed it. Run `pnpm wasm:build` (on Windows with
  `$env:B2C_SKIP_WASM_OPT = '1'` set, as in step 5), then build again.
- **"The WebAssembly core in packages/b2c-core-wasm/pkg/ is out of date"**:
  the compiler's source changed since the core was built (after a pull, for
  example), so the app would show C++ from the old compiler. Run
  `pnpm wasm:build`, or `pnpm app:build` for the core and the app (on
  Windows with `$env:B2C_SKIP_WASM_OPT = '1'` set, as in step 5).
- **wasm-bindgen is missing or has another version**: the build names the
  exact `cargo install wasm-bindgen-cli --version … --locked` command; the
  version must equal the one in `Cargo.lock`. cargo puts the program in
  `~/.cargo/bin` (`%USERPROFILE%\.cargo\bin` on Windows), which must be on
  your path. `WASM_BINDGEN=/path/to/wasm-bindgen` selects another copy.
- **wasm-opt is missing or too old**: install binaryen (step 1 on Linux), or
  set `WASM_OPT=/path/to/wasm-opt` to use one from a
  [binaryen release](https://github.com/WebAssembly/binaryen/releases) (also
  on Windows). Or build without it: set `B2C_SKIP_WASM_OPT=1` for the build
  (`$env:B2C_SKIP_WASM_OPT = '1'` in PowerShell). The core is then larger but
  behaves the same.
- **Corepack fails to verify pnpm's signature**: Node.js is older than 22.14;
  install a current Node.js 22 (step 3).

### The window is blank or flickers (Linux)

Some graphics drivers, virtual machines and virtual displays cannot run
WebKitGTK's GPU renderer. Start the app with it turned off:

```sh
WEBKIT_DISABLE_DMABUF_RENDERER=1 ./target/release/blocks2cpp-desktop
```

If parts of the window are drawn wrongly or scrolling jumps, also try
`WEBKIT_DISABLE_COMPOSITING_MODE=1`, which turns off WebKitGTK's accelerated
compositing (slower, but drawn in software). To keep a setting, put the line
into a small script that starts the app.

### Wayland

On a Wayland desktop (the default for GNOME on Debian and Ubuntu) the app
runs as a native Wayland window. `echo $XDG_SESSION_TYPE` prints
`wayland` or `x11`.

- The window opens where the desktop places it: Wayland does not let apps
  choose their position, so it is not always centred.
- A blank window, or the app ending with _"Error 71 (Protocol error)
  dispatching to Wayland display"_ (seen most often with NVIDIA's driver):
  start it with `WEBKIT_DISABLE_DMABUF_RENDERER=1` as above.
- If a problem appears only on Wayland, compare with X11 by starting the app
  through XWayland: `GDK_BACKEND=x11 ./target/release/blocks2cpp-desktop`.
  Text may look less sharp there at a fractional display scale (125%, 150%).
- When you report a display problem, say whether it happens on Wayland, X11
  or both, and give your display scale, desktop, graphics card and driver,
  and the WebKitGTK version (`dpkg -s libwebkit2gtk-4.1-0 | grep Version`).

### Windows

- **A path is too long** (for example during `pnpm install` or the Rust
  build): clone into a short folder such as `C:\src`.
- **`cargo` or `pnpm` is not found**: open a new PowerShell window after the
  `winget install` and `corepack enable` steps.
- **_"pnpm.ps1 cannot be loaded because running scripts is disabled on this
  system"_**: PowerShell blocks the `pnpm` script that Corepack added. Run
  `Set-ExecutionPolicy -Scope CurrentUser RemoteSigned` as in step 4, or type
  `pnpm.cmd` instead of `pnpm`.
- **The linker `link.exe` is not found**: the C++ Build Tools are missing
  their C++ workload; run step 1's second command again.
