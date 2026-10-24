"""Asks a process's X11 windows to close, as a window manager's close button does.

Usage: python3 -I close-window.py <pid>

The end-to-end flow "closing the app kills the program tree" (docs/spec/07-toolchain-build-run.md
section 7.6.2) needs the window closed the way a person closes it. WebDriver cannot do that for a
Tauri app (its "close window" destroys only the web view), and the tests' virtual display has no
window manager, so this sends what one would send: the ICCCM WM_DELETE_WINDOW client message, to
every top-level window whose _NET_WM_PID is <pid> and that takes part in WM_DELETE_WINDOW. GTK
turns it into the window's delete event, which the app handles as a close request.

It uses only the X11 client library (libX11), which GTK already needs, through ctypes, so nothing
has to be installed. It reads DISPLAY (and XAUTHORITY) from the environment, as xvfb-run sets them.

Exit status: 0 when at least one window was asked to close, 2 when the process has no such
window, 1 on an error (no display, a bad argument).
"""

import ctypes
import ctypes.util
import sys

# X11 protocol constants (X.h, Xatom.h).
CLIENT_MESSAGE = 33
NO_EVENT_MASK = 0
XA_CARDINAL = 6
SUCCESS = 0
CURRENT_TIME = 0

# The most windows the walk looks at, and how deep: a top-level window is a child of the root
# window, or a grandchild when a window manager reparents it.
MAX_WINDOWS = 20000
MAX_DEPTH = 3

c_window = ctypes.c_ulong
c_atom = ctypes.c_ulong


class XClientMessageEvent(ctypes.Structure):
    """The XClientMessageEvent structure of Xlib.h."""

    _fields_ = [
        ("type", ctypes.c_int),
        ("serial", ctypes.c_ulong),
        ("send_event", ctypes.c_int),
        ("display", ctypes.c_void_p),
        ("window", c_window),
        ("message_type", c_atom),
        ("format", ctypes.c_int),
        ("data", ctypes.c_long * 5),
    ]


class XEvent(ctypes.Union):
    """The XEvent union of Xlib.h: 24 longs, of which only the client message is used here."""

    _fields_ = [("xclient", XClientMessageEvent), ("pad", ctypes.c_long * 24)]


# Errors such as BadWindow (a window that went away during the walk) are expected; Xlib's default
# handler would end the process on them.
ERROR_HANDLER_TYPE = ctypes.CFUNCTYPE(ctypes.c_int, ctypes.c_void_p, ctypes.c_void_p)


def ignore_error(_display, _event):
    return 0


IGNORE_ERRORS = ERROR_HANDLER_TYPE(ignore_error)


def load_xlib():
    """Loads libX11 and declares the functions used, so ctypes passes 64-bit values correctly."""
    xlib = ctypes.CDLL(ctypes.util.find_library("X11") or "libX11.so.6")
    p_window = ctypes.POINTER(c_window)
    xlib.XOpenDisplay.argtypes = [ctypes.c_char_p]
    xlib.XOpenDisplay.restype = ctypes.c_void_p
    xlib.XCloseDisplay.argtypes = [ctypes.c_void_p]
    xlib.XDefaultRootWindow.argtypes = [ctypes.c_void_p]
    xlib.XDefaultRootWindow.restype = c_window
    xlib.XInternAtom.argtypes = [ctypes.c_void_p, ctypes.c_char_p, ctypes.c_int]
    xlib.XInternAtom.restype = c_atom
    xlib.XQueryTree.argtypes = [
        ctypes.c_void_p,
        c_window,
        p_window,
        p_window,
        ctypes.POINTER(p_window),
        ctypes.POINTER(ctypes.c_uint),
    ]
    xlib.XQueryTree.restype = ctypes.c_int
    xlib.XGetWindowProperty.argtypes = [
        ctypes.c_void_p,
        c_window,
        c_atom,
        ctypes.c_long,
        ctypes.c_long,
        ctypes.c_int,
        c_atom,
        ctypes.POINTER(c_atom),
        ctypes.POINTER(ctypes.c_int),
        ctypes.POINTER(ctypes.c_ulong),
        ctypes.POINTER(ctypes.c_ulong),
        ctypes.POINTER(ctypes.POINTER(ctypes.c_ubyte)),
    ]
    xlib.XGetWindowProperty.restype = ctypes.c_int
    xlib.XGetWMProtocols.argtypes = [
        ctypes.c_void_p,
        c_window,
        ctypes.POINTER(ctypes.POINTER(c_atom)),
        ctypes.POINTER(ctypes.c_int),
    ]
    xlib.XGetWMProtocols.restype = ctypes.c_int
    xlib.XSendEvent.argtypes = [
        ctypes.c_void_p,
        c_window,
        ctypes.c_int,
        ctypes.c_long,
        ctypes.POINTER(XEvent),
    ]
    xlib.XSendEvent.restype = ctypes.c_int
    xlib.XSync.argtypes = [ctypes.c_void_p, ctypes.c_int]
    xlib.XFree.argtypes = [ctypes.c_void_p]
    xlib.XSetErrorHandler.argtypes = [ERROR_HANDLER_TYPE]
    xlib.XSetErrorHandler.restype = ctypes.c_void_p
    return xlib


def children_of(xlib, display, window):
    """The child windows of a window (none when it is gone)."""
    root = c_window()
    parent = c_window()
    children = ctypes.POINTER(c_window)()
    count = ctypes.c_uint()
    if not xlib.XQueryTree(
        display,
        window,
        ctypes.byref(root),
        ctypes.byref(parent),
        ctypes.byref(children),
        ctypes.byref(count),
    ):
        return []
    try:
        return [children[index] for index in range(count.value)]
    finally:
        if children:
            xlib.XFree(children)


def pid_of(xlib, display, window, net_wm_pid):
    """The window's _NET_WM_PID, or None."""
    actual_type = c_atom()
    actual_format = ctypes.c_int()
    items = ctypes.c_ulong()
    after = ctypes.c_ulong()
    data = ctypes.POINTER(ctypes.c_ubyte)()
    status = xlib.XGetWindowProperty(
        display,
        window,
        net_wm_pid,
        0,
        1,
        0,
        XA_CARDINAL,
        ctypes.byref(actual_type),
        ctypes.byref(actual_format),
        ctypes.byref(items),
        ctypes.byref(after),
        ctypes.byref(data),
    )
    try:
        if status != SUCCESS or actual_format.value != 32 or items.value != 1 or not data:
            return None
        # Xlib returns 32-bit properties as an array of C longs.
        return ctypes.cast(data, ctypes.POINTER(ctypes.c_ulong))[0]
    finally:
        if data:
            xlib.XFree(data)


def takes_delete(xlib, display, window, wm_delete_window):
    """Whether the window's WM_PROTOCOLS has WM_DELETE_WINDOW."""
    protocols = ctypes.POINTER(c_atom)()
    count = ctypes.c_int()
    if not xlib.XGetWMProtocols(display, window, ctypes.byref(protocols), ctypes.byref(count)):
        return False
    try:
        return any(protocols[index] == wm_delete_window for index in range(count.value))
    finally:
        if protocols:
            xlib.XFree(protocols)


def windows_of(xlib, display, pid, atoms):
    """The windows to close: those of `pid` that take WM_DELETE_WINDOW, at most MAX_DEPTH deep."""
    found = []
    pending = [(xlib.XDefaultRootWindow(display), 0)]
    seen = 0
    while pending and seen < MAX_WINDOWS:
        window, depth = pending.pop()
        seen += 1
        if depth > 0 and pid_of(xlib, display, window, atoms["_NET_WM_PID"]) == pid:
            if takes_delete(xlib, display, window, atoms["WM_DELETE_WINDOW"]):
                found.append(window)
                continue
        if depth < MAX_DEPTH:
            pending.extend((child, depth + 1) for child in children_of(xlib, display, window))
    return found


def ask_to_close(xlib, display, window, atoms):
    """Sends WM_DELETE_WINDOW to a window."""
    event = XEvent()
    event.xclient.type = CLIENT_MESSAGE
    event.xclient.window = window
    event.xclient.message_type = atoms["WM_PROTOCOLS"]
    event.xclient.format = 32
    event.xclient.data[0] = atoms["WM_DELETE_WINDOW"]
    event.xclient.data[1] = CURRENT_TIME
    return xlib.XSendEvent(display, window, 0, NO_EVENT_MASK, ctypes.byref(event)) != 0


def main(argv):
    if len(argv) != 2 or not argv[1].isdigit() or int(argv[1]) <= 0:
        sys.stderr.write("usage: close-window.py <pid>\n")
        return 1
    pid = int(argv[1])
    try:
        xlib = load_xlib()
    except OSError as error:
        sys.stderr.write(f"close-window.py: libX11 cannot be loaded ({error})\n")
        return 1
    xlib.XSetErrorHandler(IGNORE_ERRORS)
    display = xlib.XOpenDisplay(None)
    if not display:
        sys.stderr.write("close-window.py: cannot open the X display (is DISPLAY set?)\n")
        return 1
    try:
        atoms = {
            name: xlib.XInternAtom(display, name.encode("ascii"), 0)
            for name in ("_NET_WM_PID", "WM_PROTOCOLS", "WM_DELETE_WINDOW")
        }
        windows = windows_of(xlib, display, pid, atoms)
        asked = sum(1 for window in windows if ask_to_close(xlib, display, window, atoms))
        xlib.XSync(display, 0)
    finally:
        xlib.XCloseDisplay(display)
    if asked == 0:
        sys.stderr.write(f"close-window.py: process {pid} has no window to close\n")
        return 2
    sys.stdout.write(f"asked {asked} window(s) of process {pid} to close\n")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
