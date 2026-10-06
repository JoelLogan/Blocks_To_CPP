# Flow tests

The breadth end-to-end tests of M2 ([09 §9.2](../../../../../docs/spec/09-quality-and-delivery.md#92-testing-strategy)):
the app's flows beyond the exit criterion of [`../exit/`](../exit/guessing-game.e2e.ts), each driven
like a person drives it, on Linux (WebKitGTK) and Windows (WebView2). Every spec runs in well under
three minutes; they run with the rest of the suite (`pnpm --filter @blocks2cpp/desktop run e2e`, see
[`../../README.md`](../../README.md)).

| Spec                                          | What it checks                                                                                                                                                                                                                                                                                                                                                        |
| --------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| [`restricted-trust`](restricted-trust.e2e.ts) | A copied example opens in Restricted Mode: the banner, Build and Run held back, the C++ still readable, F5 does nothing. _Trust…_ answered _Stay in Restricted Mode_ keeps it restricted; _Trust this project_ lets it build and run and writes `trust.json`; _Revoke trust_ in Settings brings the banner back ([08 §8.3](../../../../../docs/spec/08-security.md)). |
| [`save-reload`](save-reload.e2e.ts)           | Ctrl+S on a new project is _Save as_; closing and opening the file again shows the same blocks; saving the unchanged project writes the same bytes (05 §5.2, F5).                                                                                                                                                                                                     |
| [`crash-recovery`](crash-recovery.e2e.ts)     | The 30 s autosave writes a snapshot with the editor's blocks; the app is killed (`SIGKILL`, `taskkill /F /T`); the next start offers the snapshot and _Restore_ brings back the same blocks, still unsaved (05 §5.10).                                                                                                                                                |
| [`stop`](stop.e2e.ts)                         | An endless loop is stopped with the console's ■ Stop and, after ⟲ Run again, with Shift+F5: _Stopped_ within 5 s, and no process of the program left (07 §7.5.4).                                                                                                                                                                                                     |
| [`exit-decoding`](exit-decoding.e2e.ts)       | `examples/exit_code.b2c` shows its exit code 3; a Release program that divides by zero shows _Crashed: integer division by zero_ with `SIGFPE` or `0xC0000094` (07 §7.6.4).                                                                                                                                                                                           |
| [`app-close`](app-close.e2e.ts)               | A program waits for input; the window is closed with its close button (saved project: it closes at once; unsaved: _Don't save_); the app exits and no process runs from the build cache (07 §7.6.2).                                                                                                                                                                  |
| [`flood`](flood.e2e.ts)                       | 10,000,000 lines: the "… N lines skipped" marker, the editor answers while the output pours in, the end of the output arrives; an endless flood is stopped within 5 s (07 §7.6.5).                                                                                                                                                                                    |
| [`toolchain-setup`](toolchain-setup.e2e.ts)   | No g++ where the app looks: the setup page opens with this platform's install command (apt on Ubuntu, from `/etc/os-release`); once g++ is back, _Rescan_ finds it, the status bar names it and a program runs (04 §4.6).                                                                                                                                             |
| [`external-change`](external-change.e2e.ts)   | The file changed by another program: _Reload_ shows its blocks; changed again with unsaved edits: _Keep mine (save as…)_ saves the editor's blocks to a new file and leaves the changed one; a deleted file offers only _Keep mine_ (05 §5.10).                                                                                                                       |
| [`clipboard`](clipboard.e2e.ts)               | Ctrl+C and Ctrl+V on a clicked block paste a copy after it with a new block ID (a declaration also gets a new symbol ID); Ctrl+X takes a block away until Ctrl+V puts it back (05 §5.12).                                                                                                                                                                             |
| [`settings`](settings.e2e.ts)                 | Indent width 2 on the Settings page reaches `settings.json` and the C++ panel at once, and still applies after a restart (05 §5.9).                                                                                                                                                                                                                                   |
| [`editor`](editor.e2e.ts)                     | The _ask_ and getter variable menus list exactly the variables in scope (no `const` for _ask_); a text block does not connect to _repeat (10) times_ where a number does; clicking `guess = b2c::ask<int>("Your guess: ");` in the C++ panel selects block `b005`; a block is dragged from every toolbox category.                                                    |
| [`build-run`](build-run.e2e.ts)               | Ctrl+B builds without running, a second Ctrl+B is _Up to date_, and F5 runs a program that reads a number typed into the console.                                                                                                                                                                                                                                     |

## How they work

- **Launching:** [`lib/launch.ts`](lib/launch.ts) starts the app as the harness's `launchApp` does,
  but with a profile (`B2C_E2E_ROOT`) that outlives one launch, so a test can restart the app,
  kill it, or close its window, and with `B2C_E2E_TOOLCHAIN_DIRS` changed for the setup page. A
  failed test keeps a screenshot, the page, the console, the project and the logs of every
  launch, as the harness does.
- **Native dialogs** are answered by the e2e build's dialog script (`open`, `saveAs`, `trust`).
- **Processes:** [`lib/processes.ts`](lib/processes.ts) reads `/proc` on Linux and
  `Win32_Process` (Windows PowerShell's `Get-CimInstance`) on Windows: the app's process, and the
  processes whose executable is in the profile's build cache, which is where every program the app
  builds runs from.
- **Closing the window:** [`lib/window.ts`](lib/window.ts) does what the close button does:
  `WM_DELETE_WINDOW` through [`lib/close-window.py`](lib/close-window.py) (Python and libX11, both
  on the Linux runners; the virtual display has no window manager) and `WM_CLOSE` through
  `Process.CloseMainWindow()` on Windows. WebDriver's own "close window" only destroys the web
  view of a Tauri app.
- **Hiding g++:** [`lib/toolchain.ts`](lib/toolchain.ts) points the app at links that do not exist
  yet and creates them (a symbolic link, or a junction on Windows) to give the compiler back
  without a restart.
- **The console** is read through the test hook's transcript (its last megabyte). While a program
  floods it, [`lib/console.ts`](lib/console.ts) checks it inside the webview and fetches only a
  summary.

The helpers' own logic has unit tests (`lib/*.test.ts`), which run with the harness's
(`--project support`). The one fixture, [`../../fixtures/flows/scopes.b2c`](../../fixtures/flows/scopes.b2c),
is a project with variables in several scopes for the variable menus; the other tests build their
programs through the test hook or start from `examples/`.

## Windows

Written for both systems, run on Windows in CI only: Windows' pseudoconsole may repaint lines with
cursor moves, so the console checks look for text rather than line breaks; a program's exit code 3
reads _Stopped itself: an uncaught error or failed check (exit code 3)._ there (07 §7.6.4); and a
process listing starts PowerShell, so process checks allow 30 s.
