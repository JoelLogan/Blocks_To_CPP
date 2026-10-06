# Getting started

This page takes you from the first start of Blocks2Cpp to a number-guessing
game you built from blocks and played in the app's console. It describes the
desktop editor as milestone M2 delivers it: what is not in this version is
listed [at the end](#what-is-not-here-yet).

## What you need

- **Windows** 10 (version 1809 or later) or 11, or **Linux** on a 64-bit PC
  (Ubuntu 22.04, Fedora 39, Debian 12 or newer).
- **g++ 11 or newer**, the free C++ compiler of GCC. If you do not have it
  yet, Blocks2Cpp shows you how to install it (see
  [Setting up a compiler](#setting-up-a-compiler)). Blocks2Cpp never downloads
  a compiler for you.
- **Blocks2Cpp itself.** There are no installers yet; they come with the 1.0
  release work. Until then the app is built from its source code, as
  [the desktop app's README](../../apps/desktop/README.md#develop) describes.

## The first start

The app opens on the **Start** page:

- **New project** offers two templates: _Empty project_ (a _when program
  starts_ block, ready for your own program) and _Hello World_ (a small
  program that prints a greeting).
- **Open a project** has an _Open…_ button, which shows your system's file
  dialog.
- **Recent projects** lists up to ten projects you opened or saved, newest
  first. The button next to each one removes it from the list.

While the start page is shown, Blocks2Cpp looks for g++ on your computer in
the background. When it finds one, the status bar at the bottom of the window
names it (for example _g++ 13.3.0_) and nothing else happens. When it finds
none, the **Set up a C++ compiler** page opens by itself.

## Setting up a compiler

The setup page explains what a compiler is and shows the steps for your
system. Each command has a button that copies it.

On **Windows** there are two ways:

1. **MSYS2** (recommended): install MSYS2 from its website (_Open msys2.org_
   opens it in your web browser), open the **MSYS2 UCRT64** shell from the
   Start menu, and run:

   ```sh
   pacman -S mingw-w64-ucrt-x86_64-gcc
   ```

2. **WinLibs**: in a Command Prompt or PowerShell window, run:

   ```sh
   winget install BrechtSanders.WinLibs.POSIX.UCRT
   ```

On **Linux** the page shows the command for your distribution, read from
`/etc/os-release` (all three when it cannot tell):

```sh
sudo apt install g++        # Debian, Ubuntu and their relatives
sudo dnf install gcc-c++    # Fedora, RHEL, CentOS and their relatives
sudo pacman -S gcc          # Arch Linux and its relatives
```

Then come back and press **I installed it → Rescan**. If your g++ is in a
folder Blocks2Cpp does not search, press **Choose g++ manually…** and pick the
program yourself (on Windows, a file named `g++.exe`).

Once a compiler works, the same page lists every compiler found, with its
version, target, location, the C++ standards it supports and its health
checks, and **Select as default** chooses the one to build with. You can come
back to this page at any time by clicking the compiler's name in the status
bar.

## A tour of the window

Open a project (for example _New project → Empty project_) to see the editor:

- **The toolbar** at the top: the **≡** main menu (_New project…_, _Open…_,
  _Save_, _Save as…_, _Close project_), the project's name with a **•** while
  there are unsaved changes, the **Debug / Release** choice, **► Run**,
  **■ Stop**, **Build** and **Settings**.
- **The toolbox** on the left: the categories Program, Variables, Math,
  Logic, Text, Control, Loops, Input / Output and Functions. Click a category
  to scroll the list of blocks beside it to that category, then drag a block
  onto the canvas.
- **The canvas** in the middle, where you snap blocks together. Drag empty
  canvas to move around; the buttons at the bottom right zoom.
- **The C++ panel** on the right shows the C++ your blocks become, updated as
  you edit. Click a line of code to select the block it came from; select a
  block to highlight its code.
- **The bottom panel** has three tabs: **Problems** (what is wrong, and
  where), **Console** (where your program runs) and **Build output** (what
  the compiler said).
- **The status bar** says whether the project is saved, shows the build
  configuration and the C++ standard, and names the compiler.

## Build a guessing game

The game: the computer picks a random whole number from 1 to 100, you type
guesses, and it answers _Too low!_, _Too high!_ or _Correct!_ until you find
it. When you are done, your blocks read:

```text
when program starts
│ create int variable secret = random integer from 1 to 100
│ create int variable guess = 0
│ print "Guess a number from 1 to 100!"
│ repeat until (guess = secret)
│ │ ask "Your guess: " and save answer in guess (keep asking until valid)
│ │ if (guess < secret) then
│ │ │ print "Too low!"
│ │ else if (guess > secret)
│ │ │ print "Too high!"
│ │ else
│ │ │ print "Correct!"
```

1. **Start a project.** On the start page, choose _New project → Empty
   project_. The canvas shows a _when program starts_ block: everything
   attached below it runs when the program starts.
2. **Make the secret number.** From **Variables**, drag _create int variable
   value = 0_ under _when program starts_. Click the name _value_ and type
   `secret`. From **Math**, drag _random integer from 1 to 6_ onto the `0`, so
   it replaces it, then click the `6` and type `100`.
3. **Make the guess.** Drag another _create int variable_ block below the
   first one and name it `guess`. Leave its value at `0`.
4. **Say hello.** From **Input / Output**, drag a _print_ block below the
   variables. Click its text and type `Guess a number from 1 to 100!`.
5. **Repeat until the guess is right.** From **Loops**, drag _repeat until_
   below the print. From **Math**, drag the comparison block (`0 < 0`) into
   the loop's condition and choose `=` in its middle. Then click the loop to
   select it, open **Variables** (it now lists the variables you can use
   there) and drag `guess` into the left side of the comparison and `secret`
   into the right side.
6. **Ask for a guess.** From **Input / Output**, drag _ask "Your answer: "
   and save answer in …_ into the loop. Change the question to _Your guess:_
   (with a space after the colon) and choose `guess` in the variable menu. Until you do, the block shows an
   error badge and Problems explains it: that is how Blocks2Cpp shows a
   mistake on the block that has it.
7. **Answer the guess.** From **Control**, drag _if … else_ below the ask,
   inside the loop. Press the **⊕** after _else if_ to add an _else if_ part.
   Put a comparison `guess < secret` into the first condition and
   `guess > secret` into the second, and a _print_ into each of the three
   parts: `Too low!`, `Too high!` and `Correct!`.

Watch the C++ panel while you build: every block you add appears there as
C++, and clicking a line selects its block. The connection checker helps
too: a block that does not fit a slot (text where a number must go, for
example) does not snap into it.

## Run it

Press **► Run** (or **F5**). Blocks2Cpp builds the program first (the
**Build output** tab shows how it goes), then runs it in the **Console**:

```text
Guess a number from 1 to 100!
Your guess: 50
Too high!
Your guess: 25
Too low!
…
Correct!
```

Click into the console and type each guess followed by **Enter**. When the
program ends, the console's header says **Finished (exit code 0)**.

- **■ Stop** (or **Shift+F5**) stops a running program; the header then says
  _Stopped_.
- **⟲ Run again** runs it once more; **Clear** empties the console.
- **Build** (or **Ctrl+B**) only builds. When nothing changed since the
  last build, the Build output just says _Up to date_.

**When Run is held back.** While the project has errors, ► Run is disabled
and its hint says how many (_2 errors – click to see the first_); clicking it
shows the first one in Problems. Each problem has a _Learn more_ link to the
diagnostics reference. Settings can make Run open Problems instead (see
[Settings](#settings)); either way, a project with errors is never built.

## Save it

Press **Ctrl+S** (or **≡ → Save**). A new project has no file yet, so your
system's save dialog asks where to put it; projects are saved as `.b2c`
files. The **•** after the name disappears, and the status bar says _Saved_.
Closing the window, opening another project or quitting with unsaved
changes asks whether to save them first.

**If the app crashes**, your work is not lost: while a project has unsaved
changes, Blocks2Cpp keeps a copy every 30 seconds and whenever the window
loses focus. The next time it starts, the start page offers to **Restore** or
**Discard** that copy.

**If the file changes outside the app** (another program edits or deletes
it), a dialog asks whether to **Reload** it, **Keep mine (save as…)** to save
your version elsewhere, or decide **Not now**. Blocks2Cpp never overwrites a
file that changed on disk.

## Projects from somewhere else: Restricted Mode

A project you did not create on this computer (downloaded, copied from a
friend, or one of the examples in the repository's `examples/` folder) opens
in **Restricted Mode**. A banner under the toolbar says why. You can look at
and edit its blocks and read its C++, but **Build and Run stay off**, because
building and running a program lets it do anything you can do on your
computer.

When you know where the project comes from, press **Trust…** in the banner.
Your system's dialog lists what in the project is worth a second look and
offers three choices:

- **Trust this project**: build and run this project.
- **Trust everything in this folder**: build and run every project in this
  folder and the folders below it.
- **Stay in Restricted Mode**: keep editing and reading the C++ without
  building or running it. Escape and closing the dialog do the same.

On Windows, a file downloaded from the Internet gets a stronger warning.
Projects you create in the app are trusted from the start. If a trusted
project's Raw C++ code, libraries or defines change outside the app, it opens
in Restricted Mode again. To take trust back, open **Settings**: its _This
project_ section says why the open project is trusted and offers **Revoke
trust**.

## Settings

**Settings** in the toolbar opens a page whose changes are saved at once and
apply to every project on this computer:

- **Code style**: indent the C++ by 2 or 4 spaces. The C++ panel and your
  builds use the same code.
- **Run on errors**: _Disable Run_ (the default) or _Show problems_.
- **Console**: how many lines the console keeps (1,000 to 100,000).
- **Toolchain**: a link to the compiler page.
- **Build cache**: **Clear build cache** asks first, then deletes the
  programs Blocks2Cpp built and kept, except those in use, and says how much
  space it freed.

_Back to the editor_ returns to your project.

## Keyboard

| Keys       | What they do                                 |
| ---------- | -------------------------------------------- |
| F5         | Run (building first when needed)             |
| Shift+F5   | Stop the running program or build            |
| Ctrl+B     | Build                                        |
| Ctrl+S     | Save                                         |
| Ctrl+C / X | Copy or cut the selected blocks              |
| Ctrl+V     | Paste                                        |
| Ctrl+D     | Duplicate the selected block                 |
| Ctrl+Z / Y | Undo or redo                                 |
| Ctrl+Tab   | Leave the console while a program is running |

The canvas works with the keyboard alone, too: Tab moves through the
toolbar, the toolbox, its blocks and the canvas; on the canvas the arrow keys
move from block to block, **Enter** edits a field, **M** moves the block (the
arrow keys choose where, Enter puts it there), **T** opens the toolbox to add
a block, and **Ctrl+Enter** opens the block's menu.

## What is not here yet

This first version of the editor has the blocks of
[the block reference](../reference/blocks/README.md) and nothing that a later
milestone adds: you cannot type expressions into slots or use Quick Insert,
export a C++ project, use collections, structs, files, classes, libraries or
Raw C++ blocks, set a project's options or a program's arguments, or debug.
The app has no buttons or menus for those yet;
[04 §4.13](../spec/04-user-interface.md#413-what-the-m2-editor-includes) lists
what comes in which milestone.

## Where to go next

- [The block reference](../reference/blocks/README.md): every block, its C++
  and its options.
- [The diagnostics reference](../reference/diagnostics/README.md): every
  message Blocks2Cpp can show, with examples and fixes.
- [The command-line tool](../reference/cli.md): build and run projects
  without the editor.
- [The example projects](../../examples/README.md), to open (and trust) in
  the editor.
