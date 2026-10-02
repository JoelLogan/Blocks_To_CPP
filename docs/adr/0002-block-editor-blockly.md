# ADR-0002: Block editor: Blockly with the Zelos renderer

* Status: Accepted
* Date: 2026-10-02

## Context

The editor must look and feel like Scratch, support custom fields (type
picker, typed expression slots, symbol dropdowns), variadic mutators, custom
connection checking, keyboard navigation and accessibility, and stay
maintained for years.

## Options considered

1. **scratch-blocks** (Scratch's fork of Blockly)
   * Pros: the exact Scratch look.
   * Cons: forked from an old Blockly, maintained primarily for Scratch's own
     VM, with limited support for outside use. We would inherit its age and its
     coupling to Scratch.
2. **Blockly** with the **Zelos** renderer
   * Pros: actively maintained (moved from Google to the Raspberry Pi
     Foundation in November 2025, still Apache-2.0); Zelos provides the
     Scratch-style look; a rich plugin system (custom fields, renderers,
     connection checkers, toolbox, keyboard navigation, workspace search,
     minimap); an ongoing accessibility effort.
   * Cons: its native serialisation and variable model do not fit typed,
     scoped C++. We need our own document model (ADR-0004) and symbol fields.
3. **A custom canvas editor**
   * Pros: full control.
   * Cons: an enormous amount of effort before reaching parity on drag/drop,
     rendering, accessibility and touch.

## Decision

**Blockly (12.x) with Zelos**, extended through supported plugin APIs only
(custom fields, `ConnectionChecker`, mutators, renderer constants/theme,
toolbox). We do **not** use Blockly's code-generator framework or its
global variable model. Code generation lives in the Rust core (ADR-0003).

## Consequences

* `packages/blockly-ext` contains all Blockly customisation, isolated behind a
  small interface, so Blockly upgrades are contained.
* Block definitions are generated from our catalog. They are not hand-written
  Blockly JSON.
* Blockly upgrades are reviewed (it is a large dependency) and tested by the
  round-trip and E2E suites.
* Blockly injects its CSS at runtime, which requires `style-src
  'unsafe-inline'` in our CSP. This is tracked as a known exception
  ([08 §8.8](../spec/08-security.md#88-webview-and-ipc-hardening)).
