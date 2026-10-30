#!/usr/bin/env node
// `pnpm doctor` (M3-6, docs/plans/m3-contracts/w5-performance.md §7): checks this computer
// against what BUILDING.md asks for, each pinned version read from where the repository pins it,
// and says for each line OK, a warning or a problem, with the command that fixes it. It exits 1
// when there is a problem and 0 otherwise.
//
// It only reads files of the checkout and runs version commands (never through a shell); it
// changes nothing and sends nothing anywhere. Runs on Linux and Windows.

import { spawnSync } from "node:child_process";
import { existsSync, readFileSync, statSync } from "node:fs";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

/** The repository root (this file is in tools/). */
export const REPO_ROOT = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  "..",
);

/** Where BUILDING.md's steps are, for the messages. */
const BUILDING = "BUILDING.md";

/** A check's outcome: `ok`, `warning` (works, but not as BUILDING.md has it), `problem`. */
/** @typedef {{ name: string, level: 'ok' | 'warning' | 'problem', detail: string, fix?: string }} Finding */

/** Runs a command without a shell; its trimmed standard output, or `null` when it cannot run. */
export function run(command, args, options = {}) {
  const result = spawnSync(command, args, {
    encoding: "utf8",
    timeout: 30_000,
    windowsHide: true,
    ...options,
  });
  if (result.error !== undefined || result.status !== 0) {
    return null;
  }
  return `${result.stdout ?? ""}${result.stderr ?? ""}`.trim();
}

/** The first version (`major`, `major.minor` or `major.minor.patch`) in a text, as numbers, or `null`. */
export function parseVersion(text) {
  const match = /(\d+)(?:\.(\d+))?(?:\.(\d+))?/.exec(text ?? "");
  return match === null
    ? null
    : [Number(match[1]), Number(match[2] ?? 0), Number(match[3] ?? 0)];
}

/** Compares two versions as numbers: negative, zero or positive. */
export function compareVersions(a, b) {
  for (let i = 0; i < 3; i += 1) {
    const difference = (a[i] ?? 0) - (b[i] ?? 0);
    if (difference !== 0) {
      return difference;
    }
  }
  return 0;
}

const show = (version) => version.join(".");

/** The pins, read from the repository's sources of truth. */
export function readPins(root = REPO_ROOT) {
  const read = (file) => readFileSync(path.join(root, file), "utf8");
  const packageJson = JSON.parse(read("package.json"));
  const toolchain = read("rust-toolchain.toml");
  const lock = read("Cargo.lock");
  const desktop = read(".github/workflows/desktop.yml");
  const probe = read("crates/b2c-toolchain/src/probe.rs");
  const wasmBindgen =
    /\[\[package\]\]\nname = "wasm-bindgen"\nversion = "([^"]+)"/.exec(lock);
  return {
    node: /^>=\s*(\S+)$/.exec(packageJson.engines?.node ?? "")?.[1] ?? null,
    pnpm: /^pnpm@(\S+)$/.exec(packageJson.packageManager ?? "")?.[1] ?? null,
    rust: /^channel\s*=\s*"([^"]+)"/m.exec(toolchain)?.[1] ?? null,
    targets:
      /^targets\s*=\s*\[([^\]]*)\]/m
        .exec(toolchain)?.[1]
        ?.split(",")
        .map((target) => target.trim().replace(/^"|"$/g, ""))
        .filter((target) => target !== "") ?? [],
    wasmBindgen: wasmBindgen?.[1] ?? null,
    tauriDriver:
      /TAURI_DRIVER_VERSION:\s*"?([\d.]+)"?/.exec(desktop)?.[1] ?? null,
    gccMajor: Number(
      /pub const MIN_GCC_MAJOR: u32 = (\d+);/.exec(probe)?.[1] ?? "NaN",
    ),
  };
}

/** A version against a pin: at least it (`minimum`), or exactly it. */
export function versionFinding(
  name,
  found,
  pin,
  { minimum = false, fix, missing = "problem", mismatch = "problem" },
) {
  if (found === null) {
    return { name, level: missing, detail: "not found", fix };
  }
  const have = parseVersion(found);
  const want = parseVersion(pin);
  if (have === null || want === null) {
    return {
      name,
      level: "warning",
      detail: `cannot tell the version from "${found}"`,
      fix,
    };
  }
  const order = compareVersions(have, want);
  if (minimum ? order >= 0 : order === 0) {
    return { name, level: "ok", detail: show(have) };
  }
  return {
    name,
    level: mismatch,
    detail: `${show(have)}, but BUILDING.md asks for ${minimum ? `${pin} or later` : pin}`,
    fix,
  };
}

/** Whether `file` is older than any file under `folders` (by modification time). */
export function olderThan(file, folders) {
  if (!existsSync(file)) {
    return true;
  }
  const since = statSync(file).mtimeMs;
  for (const folder of folders) {
    if (existsSync(folder) && statSync(folder).mtimeMs > since) {
      return true;
    }
  }
  return false;
}

/** Every check, in BUILDING.md's order. `exec` and `platform` are replaceable in tests. */
export async function check({
  root = REPO_ROOT,
  exec = run,
  platform = process.platform,
  nodeVersion = process.versions.node,
  userAgent = process.env.npm_config_user_agent ?? "",
} = {}) {
  const exe = (name) => (platform === "win32" ? `${name}.exe` : name);
  const pins = readPins(root);
  /** @type {Finding[]} */
  const findings = [];

  findings.push(
    versionFinding("Node.js", nodeVersion, pins.node ?? "0", {
      minimum: true,
      fix: `install Node.js 22 (${BUILDING}, step 3)`,
    }),
  );
  // pnpm runs this as `pnpm doctor` and says which version it is. Run on its own, the script
  // asks pnpm, except on Windows, where pnpm is a .cmd file that only a shell runs.
  const pnpm =
    /pnpm\/(\S+)/.exec(userAgent)?.[1] ??
    (platform === "win32" ? null : exec("pnpm", ["--version"]));
  findings.push(
    versionFinding("pnpm", pnpm, pins.pnpm ?? "0", {
      missing: platform === "win32" ? "warning" : "problem",
      fix:
        platform === "win32"
          ? "run this as pnpm doctor, so pnpm says its version"
          : "corepack enable (pnpm then comes in the version package.json names)",
    }),
  );

  const rustc = exec(exe("rustc"), ["--version"], { cwd: root });
  findings.push(
    versionFinding("Rust", rustc, pins.rust ?? "0", {
      fix: `rustup toolchain install (from the repository; ${BUILDING}, steps 2 and 4)`,
    }),
  );
  const installed = exec(exe("rustup"), ["target", "list", "--installed"], {
    cwd: root,
  });
  for (const target of pins.targets) {
    findings.push(
      installed !== null && installed.split(/\r?\n/).includes(target)
        ? { name: `Rust target ${target}`, level: "ok", detail: "installed" }
        : {
            name: `Rust target ${target}`,
            level: "problem",
            detail: installed === null ? "rustup not found" : "not installed",
            fix: "rustup toolchain install (from the repository)",
          },
    );
  }
  findings.push(
    versionFinding(
      "wasm-bindgen CLI",
      exec(exe("wasm-bindgen"), ["--version"]),
      pins.wasmBindgen ?? "0",
      {
        fix: `cargo install wasm-bindgen-cli --version ${pins.wasmBindgen ?? "<Cargo.lock>"} --locked`,
      },
    ),
  );
  findings.push(
    versionFinding(
      "wasm-opt (binaryen)",
      exec(exe("wasm-opt"), ["--version"]),
      "105",
      {
        minimum: true,
        missing: "warning",
        fix: `install binaryen, or build with B2C_SKIP_WASM_OPT=1 (${BUILDING})`,
      },
    ),
  );

  if (platform === "linux") {
    const webkit = exec("pkg-config", ["--modversion", "webkit2gtk-4.1"]);
    findings.push(
      webkit === null
        ? {
            name: "WebKitGTK 4.1",
            level: "problem",
            detail: "pkg-config does not find webkit2gtk-4.1",
            fix: `the apt install of ${BUILDING}, step 1`,
          }
        : { name: "WebKitGTK 4.1", level: "ok", detail: webkit },
    );
  }

  const gxx =
    exec(exe("g++"), ["-dumpfullversion"]) ??
    exec(exe("g++"), ["-dumpversion"]);
  const gxxMajor = parseVersion(gxx)?.[0] ?? null;
  findings.push(
    gxx === null
      ? {
          name: "g++",
          level: "warning",
          detail: "not found (needed only to run the programs you make)",
          fix: `install g++ ${String(pins.gccMajor)} or later`,
        }
      : gxxMajor !== null && gxxMajor >= pins.gccMajor
        ? { name: "g++", level: "ok", detail: gxx }
        : {
            name: "g++",
            level: "warning",
            detail: `${gxx}, older than GCC ${String(pins.gccMajor)}, which programs need`,
            fix: `install g++ ${String(pins.gccMajor)} or later`,
          },
  );

  // tauri-driver has no --version; Cargo lists what it installed.
  const driver =
    /^tauri-driver v(\S+):/m.exec(
      exec(exe("cargo"), ["install", "--list"]) ?? "",
    )?.[1] ?? null;
  findings.push(
    driver === null
      ? {
          name: "tauri-driver",
          level: "ok",
          detail: "not installed (only the end-to-end tests need it)",
        }
      : // Contributors only: another version is a warning, not a problem.
        versionFinding("tauri-driver", driver, pins.tauriDriver ?? "0", {
          mismatch: "warning",
          fix: `cargo install tauri-driver --version ${pins.tauriDriver ?? "<pin>"} --locked`,
        }),
  );

  const modules = path.join(root, "node_modules", ".modules.yaml");
  findings.push(
    olderThan(modules, [path.join(root, "pnpm-lock.yaml")])
      ? {
          name: "Packages (node_modules)",
          level: "problem",
          detail: existsSync(modules)
            ? "older than pnpm-lock.yaml"
            : "not installed",
          fix: "pnpm install --frozen-lockfile",
        }
      : { name: "Packages (node_modules)", level: "ok", detail: "installed" },
  );

  findings.push(await coreFinding(root));
  return findings;
}

/** Whether the editor's WebAssembly core is built from the source as it is now. */
async function coreFinding(root) {
  const name = "WebAssembly core";
  try {
    const info = await import(
      pathToFileURL(
        path.join(root, "packages/b2c-core-wasm/scripts/build-info.mjs"),
      ).href
    );
    const result = info.checkBuild(root);
    if (result.state === "fresh") {
      return { name, level: "ok", detail: "built from the current source" };
    }
    const detail =
      result.state === "stale"
        ? `built from older source (${result.changed.join(", ")} changed)`
        : result.state === "notBuilt"
          ? "not built"
          : "built by an older build script";
    return { name, level: "warning", detail, fix: info.REBUILD_COMMAND };
  } catch (error) {
    return {
      name,
      level: "warning",
      detail: `cannot tell: ${error instanceof Error ? error.message : String(error)}`,
    };
  }
}

/** The report's lines. */
export function report(findings) {
  const label = { ok: "OK     ", warning: "WARNING", problem: "PROBLEM" };
  const lines = findings.map(
    (finding) =>
      `${label[finding.level]}  ${finding.name}: ${finding.detail}` +
      (finding.level === "ok" || finding.fix === undefined
        ? ""
        : `\n         Fix: ${finding.fix}`),
  );
  const problems = findings.filter(
    (finding) => finding.level === "problem",
  ).length;
  const warnings = findings.filter(
    (finding) => finding.level === "warning",
  ).length;
  lines.push(
    "",
    problems === 0
      ? `Ready to build${warnings === 0 ? "" : ` (${String(warnings)} warning${warnings === 1 ? "" : "s"})`}. ${BUILDING} has the steps.`
      : `${String(problems)} problem${problems === 1 ? "" : "s"} to fix first; ${BUILDING} has the steps.`,
  );
  return lines.join("\n");
}

if (import.meta.url === pathToFileURL(process.argv[1] ?? "").href) {
  const findings = await check();
  process.stdout.write(`${report(findings)}\n`);
  process.exitCode = findings.some((finding) => finding.level === "problem")
    ? 1
    : 0;
}
