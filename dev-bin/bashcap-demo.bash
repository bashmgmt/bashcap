#!/usr/bin/env bash
# Build the debug binary and run it over the demo fixture, then show what it
# captured. Plain bash so `make bashcap-demo` works without frontmatter.
set -uo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$HERE/.." && pwd)"
WORK="$ROOT/target/bashcap-demo"
BIN="$ROOT/target/debug/bashcap"
FIXTURE="${1:-$ROOT/__fixtures/bashcap_demo/demo.bash}"

hr() { printf '\n\033[1;34m── %s\033[0m\n' "$1"; }

mkdir -p "$WORK"
hr "build"
( cd "$ROOT" && cargo build --bin bashcap ) || exit 1

hr "polyfill (what a client script sources)"
"$BIN" polyfill | tee "$(dirname "$FIXTURE")/polyfill.bash" | sed 's/^/   /'

hr "run"
"$BIN" run --into "$WORK/capture.jsonl" -- "$FIXTURE"
printf '   the wrapped script exited %s\n' "$?"

hr "captured snapshots"
python3 - "$WORK/capture.jsonl" <<'PY'
import json, sys
rows = [json.loads(line) for line in open(sys.argv[1])]
print(f"   {len(rows)} snapshots from {len({r['pid'] for r in rows})} shells\n")
for i, r in enumerate(rows):
    stack = " ← ".join(f"{f['funcname']}@{f['source'].rsplit('/',1)[-1]}:{f['lineno']}"
                       for f in r["frames"]) or "(top level)"
    print(f"   [{i}] pid={r['pid']} subshell={r['state']['subshell']} shlvl={r['state']['shlvl']}")
    print(f"       {stack}")
    for note in r.get("notes", []):
        print(f"       note  {note}")
    for name, var in (r.get("vars") or {}).items():
        kind, value = next(iter(var["value"].items()))
        print(f"       var   {name} [{var['attrs'] or '--'}] {kind} = {value}")
    if r.get("rematch"):
        print(f"       regex {' | '.join(r['rematch'])}")
    print()
PY

hr "artifacts"
printf '   %s\n   %s --help\n\n' "$WORK/capture.jsonl" "$BIN"
