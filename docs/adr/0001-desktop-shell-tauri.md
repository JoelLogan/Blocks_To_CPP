# ADR-0001: Desktop shell: Tauri 2

* Status: Accepted
* Date: 2026-10-02

## Context

We need a desktop application for Windows (required) and Linux (strongly
desired) that hosts a web-based block editor (see ADR-0002). The backend must
spawn compilers and programs, manage pseudo-terminals, and handle files from
untrusted sources. Security, installer size and long-term maintainability
matter more than squeezing out the last bit of rendering consistency.

## Options considered

1. **Electron**
   * Pros: bundles Chromium, so rendering is identical everywhere; huge
     ecosystem.
   * Cons: 80–150 MB installers; the backend is Node.js (no memory-safety
     advantage, a large npm surface in the privileged process); Electron
     security depends on getting many settings right (`contextIsolation`,
     `sandbox`, `nodeIntegration`, …).
2. **Tauri 2**
   * Pros: uses the OS webview (small installers), a Rust backend (memory
     safety for process/file/PTY code), and a capability-based permission
     model for IPC with an optional isolation pattern.
   * Cons: two rendering engines (WebView2 on Windows, WebKitGTK on Linux)
     need testing on both; WebKitGTK quality depends on the distribution.
3. **Native toolkit (Qt / C++ or Rust GUI) with a custom block canvas**
   * Pros: no webview.
   * Cons: we would have to re-implement a Blockly-quality block editor
     (rendering, drag/drop, accessibility, keyboard navigation), which is many
     person-years of work.

## Decision

**Tauri 2.** It gives the strongest security posture for the privileged side
(Rust, capabilities, no Node in the backend) and the smallest footprint, and
it supports both target platforms. We accept the cost of testing two webview
engines.

## Consequences

* The E2E suite and a visual check of the canvas run on both Windows and
  Linux in CI.
* Frontend code avoids engine-specific features and is tested on WebKit early.
* All privileged logic lives in Rust crates that can be tested and fuzzed
  without the UI.
* macOS remains possible later (Tauri supports it), but would need a Clang
  toolchain driver.
