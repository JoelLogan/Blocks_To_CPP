#!/usr/bin/env bash
# Weekly dependency health report (docs/spec/08-security.md §8.9 and §8.13,
# docs/spec/09-quality-and-delivery.md §9.3: weekly.yml).
#
# Writes a Markdown report with four sections:
#
#   1. RustSec advisories for every crate in the graph (cargo-deny). Unlike
#      the gating check in ci.yml, unmaintained and unsound crates are listed
#      at any depth, not only when a workspace crate depends on them directly.
#   2. Outdated npm packages in every pnpm workspace package (pnpm outdated -r).
#   3. Crates present in more than one version (cargo-deny's bans check, as
#      deny.toml configures it).
#   4. Tracked OSV-Scanner exceptions (osv-scanner.toml) that have expired or
#      expire within the warning window (14 days by default).
#
# It is a report, not a gate: findings never make it fail. It exits with 1
# when a section could not be produced (a tool is missing or failed), so that
# a broken report is not mistaken for a clean one, and with 2 on a usage
# error. Run it from anywhere in the repository; it needs bash, python3 (3.11
# or later, for tomllib), cargo-deny and pnpm (with `pnpm install` done, so
# that the installed versions are known).
#
# Usage: tools/dependency-health.sh [--output FILE] [--today YYYY-MM-DD]
#                                   [--expiry-days N]
#        tools/dependency-health.sh --self-test
#
#   --output FILE      append the report to FILE (default: $GITHUB_STEP_SUMMARY
#                      when it is set, otherwise standard output)
#   --today DATE       the date expiry is counted from (default: today, UTC)
#   --expiry-days N    the warning window for exceptions, 1 to 365 (default 14)
#   --self-test        check the report itself, with stand-ins for cargo-deny
#                      and pnpm that report findings or fail

set -euo pipefail

usage() {
    sed -n '/^# Usage:/,/^$/s/^# \{0,1\}//p' "${BASH_SOURCE[0]}" >&2
    exit 2
}

# Runs this script with stand-ins for cargo-deny and pnpm first on PATH and
# checks the outcome: findings in every section give a complete report and
# exit status 0; a tool that fails gives a section marked "Not produced" and
# exit status 1. The stand-ins report the way the real tools do: cargo-deny
# writes JSON lines to standard error and exits with one bit per check that
# found errors (advisories 1, bans 2), and `pnpm outdated` exits with 1 when
# something is outdated.
self_test() {
    self_test_dir="$(mktemp -d)"
    trap 'rm -rf "$self_test_dir"' EXIT
    local bin="$self_test_dir/bin" failures=0 report
    mkdir "$bin"
    cat > "$bin/cargo-deny" << 'STUB'
#!/bin/sh
case "$*" in
*"check advisories")
    echo '{"type":"diagnostic","fields":{"code":"unmaintained","message":"m","advisory":{"id":"RUSTSEC-2099-0001","package":"old","title":"old is unmaintained","informational":"unmaintained"},"graphs":[{"Krate":{"name":"old","version":"1.0.0"}}]}}' >&2
    echo '{"type":"summary","fields":{"advisories":{"errors":1,"warnings":0,"notes":0,"helps":0}}}' >&2
    exit 1
    ;;
*"check bans")
    if [ -n "${STUB_BANS_BROKEN:-}" ]; then
        echo "error: the configuration could not be read" >&2
        exit 2
    fi
    echo '{"type":"diagnostic","fields":{"code":"duplicate","message":"found 2 duplicate entries for crate a","graphs":[{"Krate":{"name":"a","version":"1.0.0"}},{"Krate":{"name":"a","version":"2.0.0"}}]}}' >&2
    echo '{"type":"diagnostic","fields":{"code":"banned","message":"crate b = 1.0.0 is explicitly banned"}}' >&2
    echo '{"type":"summary","fields":{"bans":{"errors":1,"warnings":1,"notes":0,"helps":0}}}' >&2
    exit 2
    ;;
esac
echo "unexpected arguments: $*" >&2
exit 101
STUB
    cat > "$bin/pnpm" << 'STUB'
#!/bin/sh
echo '{"left-pad":{"current":"1.0.0","wanted":"1.0.0","latest":"1.3.0","isDeprecated":false,"dependencyType":"devDependencies","dependentPackages":[{"name":"@blocks2cpp/desktop"}]}}'
exit 1
STUB
    chmod +x "$bin/cargo-deny" "$bin/pnpm"

    # Writes one report to $1 (with the extra variables $2...) and prints the
    # script's exit status.
    report_status() {
        local file="$1" status=0
        shift
        env "$@" PATH="$bin:$PATH" GITHUB_ACTIONS= "${BASH_SOURCE[0]}" --output "$file" \
            --today 2026-01-05 > "$self_test_dir/log" 2>&1 || status=$?
        echo "$status"
    }
    expect() {
        local what="$1"
        shift
        if ! "$@"; then
            echo "FAILED: $what" >&2
            failures=$((failures + 1))
        fi
    }

    report="$self_test_dir/findings.md"
    expect "findings exit with status 0" test "$(report_status "$report")" = 0
    expect "the advisory is listed" grep -q "RUSTSEC-2099-0001" "$report"
    expect "the outdated package is listed" grep -q "left-pad" "$report"
    expect "the duplicate is counted" grep -q '^\*\*1\*\* crate with more than one version' "$report"
    expect "the banned crate is listed" grep -q "crate b = 1.0.0 is explicitly banned" "$report"
    expect "every section is produced" test "$(grep -c "Not produced" "$report")" = 0

    report="$self_test_dir/broken.md"
    expect "a failed tool exits with status 1" test "$(report_status "$report" STUB_BANS_BROKEN=1)" = 1
    expect "the failed section is marked" grep -qF "**Not produced:** \`cargo deny check bans\`" "$report"
    expect "the tool's error is shown" grep -q "the configuration could not be read" "$report"
    expect "the other sections are produced" grep -q "RUSTSEC-2099-0001" "$report"

    if [ "$failures" -ne 0 ]; then
        echo "dependency-health self-test: $failures check(s) failed" >&2
        exit 1
    fi
    echo "dependency-health self-test passed."
    exit 0
}

if [ "$#" -eq 1 ] && [ "$1" = --self-test ]; then
    self_test
fi
output="${GITHUB_STEP_SUMMARY:-}"
today="$(date -u +%Y-%m-%d)"
expiry_days=14
while [ "$#" -gt 0 ]; do
    case "$1" in
        --output)
            if [ "$#" -lt 2 ] || [ -z "$2" ]; then usage; fi
            output="$2"
            shift 2
            ;;
        --today)
            [ "$#" -ge 2 ] || usage
            today="$2"
            shift 2
            ;;
        --expiry-days)
            [ "$#" -ge 2 ] || usage
            expiry_days="$2"
            shift 2
            ;;
        -h | --help) usage ;;
        *)
            echo "dependency-health: unknown argument: $1" >&2
            usage
            ;;
    esac
done
if ! [[ "$today" =~ ^[0-9]{4}-[0-9]{2}-[0-9]{2}$ ]] ||
    [ "$(date -u -d "$today" +%Y-%m-%d 2> /dev/null)" != "$today" ]; then
    echo "dependency-health: --today must be YYYY-MM-DD, not '$today'" >&2
    exit 2
fi
if ! [[ "$expiry_days" =~ ^[0-9]{1,3}$ ]] || [ "$expiry_days" -lt 1 ] || [ "$expiry_days" -gt 365 ]; then
    echo "dependency-health: --expiry-days must be a whole number from 1 to 365, not '$expiry_days'" >&2
    exit 2
fi
if ! command -v python3 > /dev/null 2>&1; then
    echo "dependency-health: python3 is required" >&2
    exit 2
fi

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

# Runs a tool with its standard output and error in $work/<name>.out and
# .err and its exit status in $work/<name>.status. A missing tool is status
# 127. Never fails: the renderer decides what each status means.
run_tool() {
    local name="$1"
    shift
    local status=0
    if command -v "$1" > /dev/null 2>&1; then
        "$@" > "$work/$name.out" 2> "$work/$name.err" < /dev/null || status=$?
    else
        : > "$work/$name.out"
        echo "$1 is not installed" > "$work/$name.err"
        status=127
    fi
    echo "$status" > "$work/$name.status"
}

# 1. Advisories at any depth: a copy of deny.toml with the unmaintained and
# unsound scopes widened to "all" (the renderer says so if that failed and
# the repository's scopes were used instead).
python3 - deny.toml "$work/deny-advisories.toml" > "$work/widen.status" 2>&1 <<'PY' || true
import re
import sys
import tomllib

source, target = sys.argv[1], sys.argv[2]
with open(source, encoding="utf-8") as f:
    text = f.read()
original = tomllib.loads(text)
lines = text.splitlines()
out, section = [], None
for line in lines:
    header = re.match(r"^\s*\[\[?\s*([^\]\s]+)\s*\]\]?\s*(#.*)?$", line)
    if header:
        section = header.group(1)
        out.append(line)
        if section == "advisories":
            out.append('unmaintained = "all"')
            out.append('unsound = "all"')
        continue
    if section == "advisories" and re.match(r"^\s*(unmaintained|unsound)\s*=", line):
        continue
    out.append(line)
widened = tomllib.loads("\n".join(out) + "\n")
expected = dict(original)
expected["advisories"] = dict(original.get("advisories", {}), unmaintained="all", unsound="all")
if widened != expected:
    sys.exit("could not widen the advisory scopes in deny.toml")
with open(target, "w", encoding="utf-8") as f:
    f.write("\n".join(out) + "\n")
print("widened")
PY
advisory_config=deny.toml
if [ "$(cat "$work/widen.status")" = widened ]; then
    advisory_config="$work/deny-advisories.toml"
fi
run_tool advisories cargo-deny --format json --color never --config "$advisory_config" check advisories

# 2. Outdated npm packages. pnpm exits with 1 when something is outdated, so
# success is judged by the JSON it prints.
npm_config_update_notifier=false run_tool outdated pnpm outdated --recursive --format json

# 3. Duplicate crate versions, as the gating bans check sees them.
run_tool bans cargo-deny --format json --color never --config deny.toml check bans

report="$work/report.md"
render_status=0
python3 - "$work" "$today" "$expiry_days" "$advisory_config" > "$report" <<'PY' || render_status=$?
import datetime
import html
import json
import os
import sys
import tomllib

work, today_text, window_text, advisory_config = sys.argv[1:5]
today = datetime.date.fromisoformat(today_text)
window = int(window_text)
github = os.environ.get("GITHUB_ACTIONS") == "true"
annotations = []

# Explicit bounds: the job summary is limited to 1 MiB per step.
MAX_ROWS = 100
MAX_CELL = 160
MAX_LOG_LINES = 40
MAX_LOG_LINE = 300

failed = []


def cell(value, limit=MAX_CELL):
    """Untrusted text (advisory titles, package names) as one table cell."""
    text = " ".join(str(value).split())
    if len(text) > limit:
        text = text[: limit - 1] + "…"
    return html.escape(text, quote=False).replace("|", "\\|").replace("`", "'") or " "


def code(value):
    return "<code>" + cell(value) + "</code>"


def read(name, suffix):
    with open(os.path.join(work, f"{name}.{suffix}"), encoding="utf-8", errors="replace") as f:
        return f.read()


def status(name):
    return int(read(name, "status").strip())


def log_excerpt(text):
    lines = [line[:MAX_LOG_LINE] for line in text.strip().splitlines()[-MAX_LOG_LINES:]]
    body = html.escape("\n".join(lines) or "(no output)", quote=False)
    return f"<pre>{body}</pre>"


def tool_failed(section, name, what):
    failed.append(section)
    print(f"**Not produced:** {what} failed (exit status {status(name)}). Its last output:\n")
    print(log_excerpt(read(name, "err") + "\n" + read(name, "out")))
    print()


def table(header, rows):
    print("| " + " | ".join(header) + " |")
    print("|" + "---|" * len(header))
    for row in rows[:MAX_ROWS]:
        print("| " + " | ".join(row) + " |")
    if len(rows) > MAX_ROWS:
        print(f"\n… and {len(rows) - MAX_ROWS} more (run the script locally for the full list).")
    print()


def deny_lines(name, bit):
    """cargo-deny's JSON lines, or None when it did not finish the check.

    cargo-deny's exit status has one bit per check that found errors
    (advisories 1, bans 2, licenses 4, sources 8), so `bit` is the status of a
    finished run of this check with findings; any other non-zero status is a
    failure of the tool.
    """
    records, summary = [], None
    for line in read(name, "err").splitlines():
        line = line.strip()
        if not line.startswith("{"):
            continue
        try:
            record = json.loads(line)
        except json.JSONDecodeError:
            continue
        if record.get("type") == "summary":
            summary = record
        elif record.get("type") == "diagnostic":
            records.append(record.get("fields", {}))
    if summary is None or status(name) not in (0, bit):
        return None
    return records


def annotate(level, message):
    """A workflow-command annotation, printed to the job log by the caller."""
    if github:
        message = message.replace("%", "%25").replace("\r", "%0D").replace("\n", "%0A")
        annotations.append(f"::{level}::{message}\n")


# OSV-Scanner exceptions are read first: the advisory table refers to them.
exceptions, osv_error = [], None
try:
    with open("osv-scanner.toml", "rb") as f:
        osv = tomllib.load(f)
    for entry in osv.get("IgnoredVulns", []):
        until = entry.get("ignoreUntil")
        if isinstance(until, datetime.datetime):
            until = until.date()
        elif not isinstance(until, datetime.date):
            until = None
        exceptions.append(
            {"id": str(entry.get("id", "?")), "until": until, "reason": str(entry.get("reason", ""))}
        )
except FileNotFoundError:
    pass
except (OSError, tomllib.TOMLDecodeError) as error:
    osv_error = str(error)
tracked = {entry["id"]: entry for entry in exceptions}

print("## Dependency health")
print()
print(f"Weekly report for {today.isoformat()} (docs/spec/08-security.md §8.9). Findings here do not")
print("fail the job; the gating checks are cargo-deny, `pnpm audit` and OSV-Scanner in ci.yml and")
print("osv-scanner.yml.")
print()

# 1. Advisories.
print("### RustSec advisories (all crates, including unmaintained)")
print()
if advisory_config == "deny.toml":
    print("**Note:** deny.toml could not be widened, so unmaintained and unsound crates are listed")
    print("only where deny.toml's own scopes include them.")
    print()
records = deny_lines("advisories", 1)
if records is None:
    tool_failed("advisories", "advisories", "`cargo deny check advisories`")
else:
    rows = []
    for fields in records:
        advisory = fields.get("advisory") or {}
        crates = sorted(
            {f"{g['Krate']['name']} {g['Krate']['version']}" for g in fields.get("graphs", []) if "Krate" in g}
        )
        ident = advisory.get("id") or fields.get("code", "?")
        exception = tracked.get(ident)
        note = "—"
        if exception:
            note = "osv-scanner.toml" + (f" until {exception['until']}" if exception["until"] else "")
        rows.append(
            [
                code(ident),
                cell(", ".join(crates) or advisory.get("package", "?")),
                cell(advisory.get("informational") or fields.get("code", "?")),
                cell(advisory.get("title") or fields.get("message", "")),
                cell(note),
            ]
        )
    if rows:
        print(f"{len(rows)} advisor{'y' if len(rows) == 1 else 'ies'}:")
        print()
        table(["Advisory", "Crate", "Kind", "Title", "Tracked exception"], rows)
    else:
        print("No advisories.")
        print()

# 2. Outdated npm packages.
print("### Outdated npm packages (`pnpm outdated -r`)")
print()
outdated = None
if status("outdated") in (0, 1):
    try:
        outdated = json.loads(read("outdated", "out") or "{}")
    except json.JSONDecodeError:
        outdated = None
if not isinstance(outdated, dict):
    tool_failed("outdated", "outdated", "`pnpm outdated -r`")
else:
    rows = []
    for name in sorted(outdated):
        info = outdated[name] if isinstance(outdated[name], dict) else {}
        users = sorted(
            {str(p.get("name", "?")) for p in info.get("dependentPackages", []) if isinstance(p, dict)}
        )
        rows.append(
            [
                code(name),
                cell(info.get("current", "not installed")),
                cell(info.get("wanted", "?")),
                cell(info.get("latest", "?")),
                cell("yes" if info.get("isDeprecated") else "no"),
                cell(info.get("dependencyType", "?")),
                cell(", ".join(users) or "?"),
            ]
        )
    if rows:
        print(f"{len(rows)} package{'' if len(rows) == 1 else 's'} behind (a version published less than")
        print("7 days ago cannot be installed yet: pnpm-workspace.yaml `minimumReleaseAge`):")
        print()
        table(["Package", "Current", "Wanted", "Latest", "Deprecated", "Type", "Used by"], rows)
    else:
        print("Every npm dependency is up to date.")
        print()

# 3. Duplicate crate versions.
print("### Crates in more than one version")
print()
records = deny_lines("bans", 2)
if records is None:
    tool_failed("bans", "bans", "`cargo deny check bans`")
else:
    duplicates, other = [], []
    for fields in records:
        if fields.get("code") == "duplicate":
            versions = sorted(
                {g["Krate"]["version"] for g in fields.get("graphs", []) if "Krate" in g},
            )
            names = {g["Krate"]["name"] for g in fields.get("graphs", []) if "Krate" in g}
            duplicates.append([code(", ".join(sorted(names)) or "?"), cell(", ".join(versions))])
        else:
            other.append([code(fields.get("code", "?")), cell(fields.get("message", ""))])
    duplicates.sort(key=lambda row: row[0])
    print(f"**{len(duplicates)}** crate{'' if len(duplicates) == 1 else 's'} with more than one version in Cargo.lock.")
    print()
    if duplicates:
        print("<details><summary>Crates and versions</summary>")
        print()
        table(["Crate", "Versions"], duplicates)
        print("</details>")
        print()
    if other:
        print("Other findings of the bans check:")
        print()
        table(["Code", "Message"], other)

# 4. Expiring OSV-Scanner exceptions.
print(f"### OSV-Scanner exceptions due within {window} days")
print()
if osv_error is not None:
    failed.append("osv-scanner.toml")
    print(f"**Not produced:** osv-scanner.toml could not be read: {cell(osv_error)}")
    print()
elif not exceptions:
    print("osv-scanner.toml has no exceptions.")
    print()
else:
    rows, due = [], 0
    for entry in sorted(exceptions, key=lambda e: (e["until"] is not None, e["until"] or today, e["id"])):
        if entry["until"] is None:
            state, due = "**no expiry date** (every exception must expire)", due + 1
            annotate("warning", f"osv-scanner.toml: {entry['id']} has no ignoreUntil date")
        else:
            days = (entry["until"] - today).days
            if days < 0:
                state, due = f"**expired** {-days} day{'s' if days != -1 else ''} ago", due + 1
                annotate("warning", f"osv-scanner.toml: the exception for {entry['id']} expired on {entry['until']}")
            elif days <= window:
                state, due = f"**expires in {days} day{'s' if days != 1 else ''}**", due + 1
                annotate("warning", f"osv-scanner.toml: the exception for {entry['id']} expires on {entry['until']}")
            else:
                state = f"in {days} days"
        rows.append([code(entry["id"]), cell(entry["until"] or "—"), state, cell(entry["reason"])])
    if due:
        print(f"**{due}** of {len(exceptions)} exception{'s' if len(exceptions) != 1 else ''} need re-checking")
        print("(renew with a new reason, or remove when a fix is available):")
    else:
        print(f"None of the {len(exceptions)} exception{'s' if len(exceptions) != 1 else ''} expire within {window} days.")
    print()
    table(["Advisory", "Ignored until", "Status", "Reason"], rows)

with open(os.path.join(work, "annotations"), "w", encoding="utf-8") as f:
    f.writelines(annotations)
if failed:
    print(f"**Incomplete report:** {', '.join(failed)} could not be produced.")
    sys.exit(1)
PY

if [ -n "$output" ]; then
    cat "$report" >> "$output"
    echo "dependency-health: report appended to $output"
else
    cat "$report"
fi
# Annotations for the workflow run (only written under GitHub Actions).
if [ -s "$work/annotations" ]; then
    cat "$work/annotations"
fi
if [ "$render_status" -ne 0 ]; then
    echo "dependency-health: the report is incomplete; see the sections marked 'Not produced'" >&2
    exit 1
fi
