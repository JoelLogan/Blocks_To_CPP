# Manual test: M2 performance profile

This is work item W2-2 of the
[M2 polish plan](../plans/m2-polish-plan.md#w2--performance-guard). In the
first M2 trial the owner saw a window and a toolbox that did not fit the
screen (U6) and lag when clicking and editing (U11). That trial probably ran
the development server (`pnpm desktop:dev`): a debug backend, React's
development mode and no Content Security Policy, so its speed says little
about the app. This pass measures the same things again in a release build on
the owner's computer (Debian, Wayland). It also records what the automated
benchmarks cannot know: the webview's version, the graphics card and driver,
the compositing mode, Wayland or X11, and the display scale.

Two open questions are answered on the way:

- Clicks that change nothing kept the page busy for 130–210 ms in the
  investigation, for an unknown reason. Section 6 profiles them.
- Tauri's window fitting (`preventOverflow`, planned in W3-3) and its
  centring need a *primary monitor* from GTK. GTK 3's Wayland backend may
  report none (a hypothesis). Section 2 checks it.

The results inform the performance budgets. In M2 the absolute numbers are
reported, not gated, and only regressions block a merge; the interaction
goal N12 of [01 §1.4](../spec/01-overview.md#14-goals) is the M5 target
([09 §9.2](../spec/09-quality-and-delivery.md#92-testing-strategy)). Nothing
here blocks W4 or W5. Write the results into the [record](#record) at the
end.

It takes about an hour, plus the build.

## How a release build is profiled

A release build has no developer tools of its own: the app is built without
Tauri's `devtools` feature, so there is no *Inspect Element*
([08 §8.8](../spec/08-security.md#88-webview-and-ipc-hardening)). WebKitGTK
has a remote Web Inspector instead, which works for any WebKitGTK program,
release builds included:

- The app is started with `WEBKIT_INSPECTOR_SERVER=127.0.0.1:9333`, which
  makes WebKitGTK listen on that address.
- A second WebKitGTK window opens `inspector://127.0.0.1:9333`, lists the
  app's page, and opens the Web Inspector for it (Console, Timelines).

Nothing in the app changes, so you measure the binary you use. (Checked with
WebKitGTK 2.52.6: a web view with developer tools turned off, as in a release
build, is listed, and the inspector runs code in it.)

**Keep it safe.** While the app runs with that variable, any program on the
computer that connects to the port controls the app's page, and through it
can do what the page can: change and save projects, and build and run
trusted ones.

- Use `127.0.0.1` only, never another address.
- Use it only for this session, and quit the app when you are done. Never put
  the variable into a launcher, a script or your shell's profile.
- Use `WEBKIT_INSPECTOR_SERVER`, not `WEBKIT_INSPECTOR_HTTP_SERVER`. The HTTP
  variant also serves the inspector to web browsers, and it accepts a
  connection from any web page, so a page open in your browser could take
  over the app.

## Before you start

1. Build the release build of the commit under test, following
   [`BUILDING.md`](../../BUILDING.md#linux-debian-and-ubuntu) (or, for a
   copy you built before, its
   [After you pull changes](../../BUILDING.md#after-you-pull-changes)).
   Write `git rev-parse --short HEAD` into the record. All commands below run
   in the repository folder.
2. Install the tools this protocol uses. `gir1.2-webkit2-4.1` lets a small
   Python program open WebKitGTK windows with the library and GTK version the
   app uses; `mesa-utils` has `glxinfo`.

   ```sh
   sudo apt install python3-gi gir1.2-webkit2-4.1 mesa-utils pciutils
   ```

3. Close programs that keep the computer busy (a build, a video), and plug a
   laptop in.
4. Write into the record how you started the app in the first trial, if you
   remember (`pnpm desktop:dev`, a debug build or a release build).

**The helper.** Save this small program. It prints what GTK reports about
your monitors (the same call Tauri's window fitting makes), then opens the
address you give it in a window:

```sh
cat > ~/b2c-viewer.py <<'EOF'
import sys

import gi

gi.require_version("Gdk", "3.0")
gi.require_version("Gtk", "3.0")
gi.require_version("WebKit2", "4.1")
from gi.repository import Gdk, Gtk, WebKit2

display = Gdk.Display.get_default()
print(f"GDK display: {display.get_name()} ({type(display).__name__})")
print("Primary monitor:", "none" if display.get_primary_monitor() is None else "yes")
for index in range(display.get_n_monitors()):
    monitor = display.get_monitor(index)
    area, work = monitor.get_geometry(), monitor.get_workarea()
    primary = " (primary)" if monitor.is_primary() else ""
    print(f"Monitor {index}{primary}: {area.width}x{area.height} at {area.x},{area.y}, "
          f"work area {work.width}x{work.height}, scale {monitor.get_scale_factor()}")

window = Gtk.Window(title=sys.argv[1])
window.set_default_size(1100, 800)
window.connect("destroy", Gtk.main_quit)
view = WebKit2.WebView()
view.load_uri(sys.argv[1])
window.add(view)
window.show_all()
Gtk.main()
EOF
```

**A fresh profile.** Section 3 starts the app as on a new computer. Quit the
app if it runs, and move its settings and state aside (section 7 brings back
your trust records, recent projects and settings):

```sh
for dir in "${XDG_CONFIG_HOME:-$HOME/.config}/blocks2cpp" "${XDG_STATE_HOME:-$HOME/.local/state}/blocks2cpp"; do
  if [ -e "$dir" ]; then mv "$dir" "$dir.before-profile"; fi
done
```

## 1. The system

Run these in a terminal of the desktop session you use the app in (not over
SSH), and copy the output into the record:

```sh
cat /etc/debian_version; uname -r
echo "session: $XDG_SESSION_TYPE, desktop: $XDG_CURRENT_DESKTOP"
gnome-shell --version 2>/dev/null; plasmashell --version 2>/dev/null
dpkg-query -W 'libwebkit2gtk-4.1-0*' 'libgtk-3-0*'
lspci -nnk | grep -A3 -E 'VGA|3D|Display'
glxinfo -B | grep -E 'renderer|version string'
lscpu | grep 'Model name'; free -h | grep Mem
env | grep '^WEBKIT_'
```

- `lspci` names the graphics card and its kernel driver (*Kernel driver in
  use*); `glxinfo` names the OpenGL driver and its version. With NVIDIA's own
  driver, also run
  `nvidia-smi --query-gpu=name,driver_version --format=csv`.
- The last line lists the `WEBKIT_…` variables set in your session (often
  none). Also write down any you add when you start the app, such as
  `WEBKIT_DISABLE_DMABUF_RENDERER=1`
  ([`BUILDING.md`](../../BUILDING.md#the-window-is-blank-or-flickers-linux)).

The display scale:

- GNOME: *Settings → Displays → Scale* (for example 100%, 125%, 200%). Also
  run `gsettings get org.gnome.desktop.interface text-scaling-factor` (the
  *Large Text* setting, which enlarges all text) and
  `gsettings get org.gnome.mutter experimental-features` (it lists
  `scale-monitor-framebuffer` when fractional scales such as 125% are on).
- KDE Plasma: `kscreen-doctor -o` prints each output's *Scale*.

Then let WebKitGTK describe itself. Start the helper with the same `WEBKIT_…`
variables you start the app with:

```sh
python3 ~/b2c-viewer.py webkit://gpu
```

The window shows WebKitGTK's own report: its version, the display type
(Wayland or X11), screen geometry, work area and device scale, and, under
*Hardware Acceleration Information*, the compositing policy, the renderer and
whether the DMA-BUF renderer is used. Press *Print in stdout* at the top: the
report appears in the terminal, and you copy it from there into the record
(*Copy to clipboard* works as well). The report names no user or computer.
Keep the terminal's first lines too: the next section uses them.

## 2. Does GTK report a primary monitor?

The helper's first lines in the terminal look like this:

```text
GDK display: wayland-0 (GdkWaylandDisplay)
Primary monitor: none
Monitor 0: 1920x1080 at 0,0, work area 1920x1080, scale 1
```

Tauri centres the window (`"center": true` in `tauri.conf.json`) and, once
W3-3 lands, fits it to the screen (`preventOverflow`) only on the monitor
GTK calls primary: tao asks `gdk_display_get_primary_monitor` for it
(tao 0.37.1 `src/platform_impl/linux/event_loop.rs:88-93`, used by
tauri-runtime-wry 2.12.1 `src/lib.rs:4448-4453`). GTK 3.24's Wayland backend
does not implement that call, so *Primary monitor: none* is expected under
Wayland; then Tauri neither centres nor fits the window.

1. Write the three kinds of line into the record (Wayland column).
2. Close the helper's window and compare with X11 (through XWayland):

   ```sh
   GDK_BACKEND=x11 python3 ~/b2c-viewer.py webkit://gpu
   ```

   Write the lines into the X11 column, then close the window.

## 3. Start the app with the inspector

This is the app's first start with the fresh profile: until section 4 is
done, do not move or resize its window.

1. Start the release build, with any `WEBKIT_…` variable you normally use in
   front. `GDK_BACKEND=wayland` makes sure it runs as a Wayland window: it is
   the default in a Wayland session, and with the variable the app stops
   instead of falling back to X11. Note roughly how long it takes until the
   start page shows.

   ```sh
   WEBKIT_INSPECTOR_SERVER=127.0.0.1:9333 GDK_BACKEND=wayland ./target/release/blocks2cpp-desktop
   ```

2. In a second terminal, open the inspector's list:

   ```sh
   python3 ~/b2c-viewer.py inspector://127.0.0.1:9333
   ```

   It lists one target, *Blocks2Cpp* at `tauri://localhost/`. Press
   *Inspect*: a *Web Inspector* window opens. If the list is empty, the app
   was started without the variable, or another program uses port 9333 (use
   another port in both commands).
3. Place the Web Inspector beside the app, without moving the app. Open the
   inspector's *Console* tab, paste these two helpers and press Enter. They
   stay defined until the app quits.

```js
window.b2cLayout = () => {
  const box = (selector) => {
    const element = document.querySelector(selector);
    if (element === null) return null;
    const r = element.getBoundingClientRect();
    return [r.left, r.top, r.width, r.height].map(Math.round);
  };
  const canvas = box('[data-testid="workspace"] svg.blocklySvg');
  const flyout = box('[data-testid="workspace"] svg.blocklyToolboxFlyout');
  return JSON.stringify({
    window: [outerWidth, outerHeight],
    viewport: [innerWidth, innerHeight],
    devicePixelRatio,
    screen: [screen.width, screen.height, screen.availWidth, screen.availHeight],
    toolbox: box('.blocklyToolbox'),
    flyout,
    canvas,
    canvasRightOfFlyout: canvas && flyout ? canvas[0] + canvas[2] - (flyout[0] + flyout[2]) : null,
    rightDock: box('.right-dock'),
    bottomDock: box('.bottom-dock'),
  });
};
window.b2cWatch = (seconds = 10) => {
  const stalls = [];
  const channel = new MessageChannel();
  const end = performance.now() + seconds * 1000;
  let last = performance.now();
  channel.port1.onmessage = () => {
    const now = performance.now();
    if (now - last >= 50) {
      stalls.push({ atMs: Math.round(last), lengthMs: Math.round(now - last) });
    }
    last = now;
    if (now < end) {
      channel.port2.postMessage(null);
      return;
    }
    channel.port1.close();
    const longest = Math.max(0, ...stalls.map((stall) => stall.lengthMs));
    console.log(`b2cWatch: ${stalls.length} stalls of 50 ms or more, the longest ${longest} ms`);
    console.table(stalls);
    window.b2cStalls = stalls;
  };
  channel.port2.postMessage(null);
  return `Watching the main thread for ${seconds} s: click in the app now.`;
};
```

- `b2cLayout()` returns the sizes and positions of the window and the
  editor's parts, in CSS pixels (boxes are `[left, top, width, height]`).
- `b2cWatch(10)` watches the page for 10 seconds and then lists every stall
  of 50 ms or more: a time in which the page could do nothing else, such as
  answer a click or draw a frame. It works as the benchmarks' heartbeat does
  (a message the page posts to itself again and again), so it keeps one
  processor core busy while it watches.
- Neither changes the app. On the start page `b2cLayout()` has no editor to
  measure, so its boxes are `null`. If they are still `null` with a project
  open, or the helpers are not defined, the console evaluates in another
  frame: choose the page's main frame as the console's execution context
  (the menu next to the prompt) and paste them again.

If the app does not start or hangs with the variable set, try once more with
`GIO_USE_NETWORK_MONITOR=base` in front of the command (a hang inside GLib's
network monitor was seen in a container without a desktop session).

## 4. U6: the layout at the first start

1. Look at the app's window and note in the record:
   - whether all four edges and the title bar are on the screen;
   - whether the window opened in the middle of the screen;
   - how long the start took (step 1 of section 3, by eye; N2's target is
     2 s).
2. Open `examples/guessing_game.b2c` with *Open…* (it opens in Restricted
   Mode; trusting it is not needed here). Without touching the canvas:
   - Take a screenshot (Print Screen) and keep it with your notes.
   - Run `b2cLayout()` in the console and copy its line into the record.
   - Note whether the toolbox's block area (the flyout) covers part of the
     program, whether any block is hidden under it, and whether the C++
     panel and the bottom panel are both shown and big enough to use.
3. Maximise the window and run `b2cLayout()` again. Copy the line into the
   record.

## 5. U11: lag in the release build

Use the guessing game from section 4, with the window maximised and the
inspector's console beside it. The program has fewer than 200 blocks, so
N12's bound for small programs applies: no main-thread task over 50 ms. In
M2 this is reported, not required. Click blocks on their words or their
edge, never on a field or a drop-down, so that nothing opens.

For each interaction in the table:

1. Run `b2cWatch(10)` in the console.
2. Click the app's title bar once, which focuses it without changing
   anything.
3. Do the interaction three times, about two seconds apart.
4. When the ten seconds are over, the console prints a table of stalls. Note
   the three stalls that followed your three interactions (rows about two
   seconds apart), for example `142 / 151 / 138`, with `–` where an
   interaction caused none. Note extra rows too.
5. Note how it felt: *instant*, *a slight delay* or *clearly slow*.

| ID | Interaction | How |
| --- | --- | --- |
| I0 | Nothing (baseline) | Leave the app alone for the ten seconds. |
| I1 | Select across scopes | Click *create int variable secret* (nothing is declared before it) and the *ask* block inside *repeat while* (both variables are visible there), alternately. Each click changes the variables the Variables category lists. |
| I2 | Click the selected block | Click the block that is already selected. This changes nothing. |
| I3 | Click the empty canvas | Click the empty canvas once before you start the watcher (this deselects). Then click the empty canvas three times. This changes nothing. |
| I4 | Select within one scope | Click the *print* block after the two *create variable* blocks and the *repeat while* block, alternately (the same variables are visible at both). |
| I5 | Choose a category | Click *Variables*, then another category, then *Variables* in the toolbox. |
| I6 | Open a drop-down | Open the variable menu of the *ask* block (where it says `guess`) and close it with Escape. |
| I7 | Edit a number | Click the `100` of *random integer from 1 to 100*, type `50`, press Enter; then set it back to `100`, and once more to `50`. |
| I8 | Move the program | Drag *when program starts* about 100 pixels to the right and drop it. |
| I9 | What felt slow before | Whatever felt slow in the first trial. Describe what you did. |

Finally, use the app for a few minutes as you did in the first trial and note
whether it still feels slow, and where.

## 6. Profile the clicks that change nothing

The watcher says how long the page is busy; the inspector's *Timelines* tab
says what it does in that time. Record one profile of I2 (click the selected
block), and one of I3 if it stalls too.

1. In the Web Inspector, open *Timelines*. Make sure the timelines
   *JavaScript & Events* and *Layout & Rendering* are recorded (use *Edit*
   above the list of timelines; add *CPU* if it is offered). The names here
   are those of WebKit's Web Inspector and can differ slightly between
   WebKitGTK versions.
2. Start the recording (the record button at the top left of the tab), switch
   to the app, do the interaction three times, two seconds apart, switch
   back and stop the recording.
3. In the overview at the top, drag across the time ruler to select one of
   the three tall bars, from just before it to just after it. Note its
   length.
4. Select *JavaScript & Events* and switch its view from *Events* to *Call
   Trees*, then to *Top Functions* (or *Bottom Up*). Note the five functions
   with the most self time and their times, and the event that started the
   work (for example `pointerdown`, `click` or a timer).
5. Select *Layout & Rendering* and note, for the same span, the time spent
   recalculating styles, in layout, painting and compositing.
6. Export the recording (the export button in the Timelines tab) and keep the
   file with your notes. Do not commit it: it can contain your project's
   text.

**A readable build (optional).** The release build's editor code is
minified, so the app's own functions and React's show up in *Call Trees*
with short, meaningless names (Blockly's method names survive). For names
that say where the time goes, build the same commit with the editor left
unminified, start it as in section 3 and repeat steps 1–5. Use it only for
this profile: its larger code loads more slowly, so its numbers are not the
app's.

```sh
pnpm --filter @blocks2cpp/desktop tauri build --no-bundle \
  --config '{"build":{"beforeBuildCommand":"pnpm exec vite build --minify false"}}'
```

This replaces `target/release/blocks2cpp-desktop`. Build the normal release
build again afterwards (section 7).

## 7. Finish

1. Quit the app and close the helper's windows. The inspector's port closes
   with the app.
2. Bring back your own settings and state (this deletes the fresh profile
   that sections 3 to 6 used):

   ```sh
   for dir in "${XDG_CONFIG_HOME:-$HOME/.config}/blocks2cpp" "${XDG_STATE_HOME:-$HOME/.local/state}/blocks2cpp"; do
     if [ -e "$dir.before-profile" ]; then rm -rf "$dir" && mv "$dir.before-profile" "$dir"; fi
   done
   ```

3. If you made the readable build, build the normal release build again: the
   last command of
   [After you pull changes](../../BUILDING.md#after-you-pull-changes)
   rebuilds the minified editor and the app.
4. Delete the helper: `rm ~/b2c-viewer.py`.
5. Fill in the record below and commit it (only the record), or send it to
   the maintainer. Keep the screenshots and Timeline exports out of the
   repository; they can show your desktop and your project. Name nobody: the
   tester is *the owner*.

## Record

### System

| Item | Value |
| --- | --- |
| Commit (`git rev-parse --short HEAD`) | |
| Date | |
| How the first trial ran (`pnpm desktop:dev`, debug build, release build, unknown) | |
| Debian release, kernel | |
| Desktop and its version | |
| Session type (`$XDG_SESSION_TYPE`) | |
| Display scale; text scaling factor; fractional scaling on? | |
| WebKitGTK and GTK packages (`dpkg-query`) | |
| Graphics card and kernel driver (`lspci`) | |
| OpenGL renderer and version (`glxinfo`), NVIDIA driver if any | |
| Processor, memory | |
| `WEBKIT_…` variables in the session and when starting the app | |

WebKitGTK's report (`webkit://gpu`, *Print in stdout*):

```text
(paste here)
```

### Window fitting

| Check | Wayland | X11 (`GDK_BACKEND=x11`) |
| --- | --- | --- |
| GDK display (helper's first line) | | |
| Primary monitor (helper's second line) | | |
| Monitors: size, work area, scale | | |

### U6: layout at the first start

| Check | Value |
| --- | --- |
| Window entirely on the screen, title bar included? | |
| Window opened in the middle of the screen? | |
| Time to the start page, by eye | |
| `b2cLayout()` at the first start, guessing game open | |
| `b2cLayout()` with the window maximised | |
| Flyout covers part of the program? A block hidden under it? | |
| C++ panel and bottom panel shown and usable? | |
| Other observations | |

### U11: stalls

Stalls of 50 ms or more after each of three tries, in ms (`–` for none).

| ID | Stalls (three tries) | Extra rows | Felt |
| --- | --- | --- | --- |
| I0 | | | |
| I1 | | | |
| I2 | | | |
| I3 | | | |
| I4 | | | |
| I5 | | | |
| I6 | | | |
| I7 | | | |
| I8 | | | |
| I9 (describe) | | | |

Does the release build still feel slow? Where?

### Profile of the clicks that change nothing

| Build | Interaction | Length of the busy span (ms) | Styles / layout / paint / composite (ms) | Top five functions by self time (ms) | Started by |
| --- | --- | --- | --- | --- | --- |
| Release | I2 | | | | |
| Release | I3 | | | | |
| Readable (optional) | I2 | | | | |

### Sign-off

| Date | Tester | Build (commit) | Completed sections | Notes |
| --- | --- | --- | --- | --- |
| | The owner | | | |
