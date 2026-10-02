# Security Policy

## Reporting a vulnerability

**Please do not open public issues for security problems.**

Report vulnerabilities privately through GitHub's
[private vulnerability reporting](https://github.com/JoelLogan/Blocks_To_CPP/security/advisories/new)
for this repository. Please include:

* a description of the issue and its impact
* steps to reproduce, or a proof of concept (e.g. a crafted `.b2c` project file)
* affected version(s) and platform (Windows/Linux, g++ version)

What to expect:

| Step | Target |
|------|--------|
| Acknowledgement | within 3 business days |
| Initial assessment and severity | within 10 business days |
| Fix for critical/high issues | as fast as possible, normally within 30 days |
| Disclosure | coordinated with the reporter, via a GitHub Security Advisory (CVE where applicable) |

## Supported versions

Before 1.0, only the latest release receives security fixes. A support window
for stable releases will be defined at 1.0.

## Scope

Examples of what we consider vulnerabilities:

* Opening, previewing or editing a project file causes code execution,
  file writes outside expected locations, or a crash/hang of the app
* Block content (strings, comments, identifiers, numbers, expressions) that
  produces C++ different from what the blocks show (code injection or hidden
  code)
* Building or running a project while it is in **Restricted Mode**
* Bypassing the trust dialog, or changing trust or compiler settings from the
  webview without native confirmation
* Path traversal, symlink attacks, or overwriting files through export, saving
  or the build cache
* Weaknesses in the update or release-signing process

**Not** vulnerabilities, by design (see
[the threat model](docs/spec/08-security.md#81-scope-and-assumptions)):

* A program you **chose to trust and run** doing harmful things. Programs run
  with your privileges, as in any IDE.
* Bugs in g++ itself, the operating system or the webview engine (please
  report those upstream; we still welcome a heads-up)
* Attacks that require malware already running as your user

## Security design

How the project is protected is documented in detail in
[`docs/spec/08-security.md`](docs/spec/08-security.md), covering workspace
trust, injection-proof code generation, compiler invocation safety,
filesystem and process hardening, webview/IPC hardening, supply-chain
controls, and our continuous scanning process.
