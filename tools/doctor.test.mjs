// Tests of tools/doctor.mjs (`pnpm doctor`), with `pnpm test:tools`: the pins read from the
// repository, the version checks, and a whole report on a made-up computer. The script names the
// files with a pattern Node expands itself: Node 22 takes `node --test tools/` for a module.

import assert from "node:assert/strict";
import { test } from "node:test";

import {
  check,
  compareVersions,
  exitCode,
  parseVersion,
  readPins,
  REPO_ROOT,
  report,
  versionFinding,
} from "./doctor.mjs";

test("reads every pin from the repository", () => {
  const pins = readPins(REPO_ROOT);
  for (const [name, value] of Object.entries(pins)) {
    assert.ok(value !== null && !Number.isNaN(value), `${name} is not pinned`);
  }
  assert.match(pins.rust, /^\d+\.\d+\.\d+$/);
  assert.ok(pins.targets.includes("wasm32-unknown-unknown"));
  assert.ok(pins.gccMajor >= 11);
});

test("parses and compares versions as numbers", () => {
  assert.deepEqual(
    parseVersion("rustc 1.97.0 (2d8144b78 2026-07-07)"),
    [1, 97, 0],
  );
  assert.deepEqual(parseVersion("wasm-opt version 108"), [108, 0, 0]);
  assert.deepEqual(parseVersion("wasm-bindgen 0.2.129"), [0, 2, 129]);
  assert.equal(parseVersion("no digits"), null);
  assert.ok(compareVersions([22, 14, 0], [22, 9, 0]) > 0);
  assert.equal(compareVersions([1, 2, 0], [1, 2]), 0);
});

test("a version is at least or exactly the pin", () => {
  assert.equal(
    versionFinding("x", "22.14.0", "22.14", { minimum: true }).level,
    "ok",
  );
  assert.equal(
    versionFinding("x", "22.9.0", "22.14", { minimum: true }).level,
    "problem",
  );
  assert.equal(versionFinding("x", "1.96.0", "1.97.0", {}).level, "problem");
  assert.equal(
    versionFinding("x", "1.98.0", "1.97.0", { mismatch: "warning" }).level,
    "warning",
  );
  assert.equal(
    versionFinding("x", null, "1", { missing: "warning" }).level,
    "warning",
  );
  assert.equal(versionFinding("x", "garbled", "1", {}).level, "warning");
});

test("reports a computer with everything as BUILDING.md has it", async () => {
  const pins = readPins(REPO_ROOT);
  const answers = {
    "pnpm --version": pins.pnpm,
    "rustc --version": `rustc ${pins.rust} (abc 2026-01-01)`,
    "rustup target list --installed": `x86_64-unknown-linux-gnu\n${pins.targets.join("\n")}`,
    "wasm-bindgen --version": `wasm-bindgen ${pins.wasmBindgen}`,
    "wasm-opt --version": "wasm-opt version 133",
    "pkg-config --modversion webkit2gtk-4.1": "2.48.0",
    "g++ -dumpfullversion": "13.2.0",
    "cargo install --list": `tauri-driver v${pins.tauriDriver}:\n    tauri-driver`,
  };
  const exec = (command, args) => answers[[command, ...args].join(" ")] ?? null;
  const findings = await check({
    exec,
    platform: "linux",
    nodeVersion: "22.14.0",
    userAgent: "",
  });
  const levels = Object.fromEntries(
    findings.map((finding) => [finding.name, finding.level]),
  );
  for (const name of [
    "Node.js",
    "pnpm",
    "Rust",
    "wasm-bindgen CLI",
    "wasm-opt (binaryen)",
    "WebKitGTK 4.1",
    "g++",
    "tauri-driver",
  ]) {
    assert.equal(levels[name], "ok", name);
  }
  assert.match(report(findings), /BUILDING\.md has the steps\.$/);
});

test("never lets rustup install the pinned toolchain while it looks", async () => {
  const asked = new Map();
  const exec = (command, args, options) => {
    asked.set([command, ...args].join(" "), options);
    return null;
  };
  await check({
    exec,
    platform: "linux",
    nodeVersion: "22.14.0",
    userAgent: "",
  });
  for (const command of [
    "rustc --version",
    "rustup target list --installed",
    "cargo install --list",
  ]) {
    assert.ok(asked.has(command), command);
    assert.equal(asked.get(command)?.env?.RUSTUP_AUTO_INSTALL, "0", command);
    assert.equal(asked.get(command)?.cwd, REPO_ROOT, command);
  }
});

test("reports problems with their fixes, and what is optional as warnings", async () => {
  const pins = readPins(REPO_ROOT);
  const answers = {
    "rustc --version": "rustc 1.80.0 (abc 2024-01-01)",
    "rustup target list --installed": "x86_64-unknown-linux-gnu",
    "g++ -dumpfullversion": "9.4.0",
  };
  // Windows: every command is asked for as an .exe.
  const exec = (command, args) =>
    command.endsWith(".exe")
      ? (answers[[command.slice(0, -4), ...args].join(" ")] ?? null)
      : null;
  const findings = await check({
    exec,
    platform: "win32",
    nodeVersion: "20.0.0",
    userAgent: `pnpm/${pins.pnpm} npm/? node/v20.0.0 linux x64`,
  });
  const by = Object.fromEntries(
    findings.map((finding) => [finding.name, finding]),
  );
  assert.equal(by["Node.js"].level, "problem");
  assert.equal(by["pnpm"].level, "ok");
  assert.equal(by["Rust"].level, "problem");
  assert.equal(by["Rust target wasm32-unknown-unknown"].level, "problem");
  assert.equal(by["wasm-bindgen CLI"].level, "problem");
  assert.match(
    by["wasm-bindgen CLI"].fix,
    /cargo install wasm-bindgen-cli --version/,
  );
  assert.equal(by["wasm-opt (binaryen)"].level, "warning");
  assert.equal(by["g++"].level, "warning");
  assert.equal(by["tauri-driver"].level, "ok");
  // No WebKitGTK on Windows.
  assert.equal(by["WebKitGTK 4.1"], undefined);
  const text = report(findings);
  assert.match(text, /^PROBLEM {2}Rust: 1\.80\.0/m);
  assert.match(text, /problems? to fix first/);
  assert.equal(exitCode(findings), 1);
  // Warnings alone do not fail it.
  assert.equal(
    exitCode(findings.filter((finding) => finding.level !== "problem")),
    0,
  );
  assert.equal(exitCode([]), 0);
});

test("on Windows, run on its own, asks to be run as pnpm doctor for pnpm", async () => {
  const findings = await check({
    exec: () => null,
    platform: "win32",
    nodeVersion: "22.14.0",
    userAgent: "",
  });
  const pnpm = findings.find((finding) => finding.name === "pnpm");
  assert.equal(pnpm?.level, "warning");
  assert.match(pnpm?.fix ?? "", /pnpm doctor/);
});
