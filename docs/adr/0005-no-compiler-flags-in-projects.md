# ADR-0005: Project files never contain compiler flags, paths or commands

* Status: Accepted
* Date: 2026-10-02

## Context

Real projects need build customisation: C++ standard, optimisation, warnings,
defines, and third-party libraries. Letting a project file carry raw g++
flags is convenient, but many flags execute code or write files at **build**
time:

* `-fplugin=<lib>`: loads a shared library into the compiler
* `-B<dir>`, `-wrapper <cmd>`: replace or wrap the compiler's subprograms
* `-specs=<file>`: rewrites driver behaviour
* `@<file>`: reads more flags from a file
* `-o`, `-MF`, `-save-temps`, `-fdump-*`: write to arbitrary paths
* `-Wl,-plugin=…`: loads linker plugins

A shared project with any of these would compromise the user as soon as they
pressed *Build*, before any of their own code ran.

## Decision

* Project build settings are **closed enums and validated scalars only**:
  standard, optimisation, debug info, sanitizers, warning level, hardening,
  defines (validated identifier + typed value), and **library names**.
* Libraries are resolved through **machine-local library profiles** (include
  dirs, lib dirs, link names), set up by the user through native dialogs.
* Advanced users may add extra flags in **machine-local settings** only. These
  go through a denylist and need native confirmation to change.
* The backend constructs every argv itself. No shell is ever involved.

## Consequences

* Building a shared project can never run code through flags. The remaining
  build-time risk (hostile source exercising g++ bugs) is handled by the trust
  gate ([08 §8.3](../spec/08-security.md#83-workspace-trust-and-restricted-mode)).
* A project that needs a library shows a guided *"set up library X"* flow on
  a new machine, instead of failing obscurely.
* Some exotic build setups need the machine-local extra-flags escape hatch.
  That is acceptable, because it is a deliberate, local, confirmed action.
