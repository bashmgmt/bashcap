#!/usr/bin/env bash
# Build the debug binary and run it over the demo fixture, then show what it
# captured. Plain bash so `make bashcap-demo` works without frontmatter.
set -uo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$HERE/.." && pwd)"
TARGET="$(cd "$ROOT" && cargo metadata --format-version 1 --no-deps 2>/dev/null \
    | grep -o '"target_directory":"[^"]*"' | head -1 | cut -d'"' -f4)"
WORK="$TARGET/bashcap-demo"
BIN="$TARGET/debug/bashcap"
FIXTURE="${1:-$ROOT/__fixtures/bashcap_demo/demo.bash}"

hr() { printf '\n\033[1;34m── %s\033[0m\n' "$1"; }

mkdir -p "$WORK"
hr "build"
( cd "$ROOT" && cargo build -p bashcap --bin bashcap ) || exit 1

hr "the words (what a client script vendors, and how it guards it)"
ASSET="$ROOT/assets/bashcap.bash"
sed 's/^/   /' "$ASSET"

VENDORED="$(dirname "$FIXTURE")/bashcap.bash"
if [[ -f $VENDORED ]] && ! diff -q "$ASSET" "$VENDORED" >/dev/null; then
    printf '   %s has drifted from %s\n' "$VENDORED" "$ASSET"
    exit 1
fi

hr "the same fixture without the tool — the guard installs an empty hook"
bash "$FIXTURE" >/dev/null
printf '   exited %s\n' "$?"

hr "run"
"$BIN" run --into "$WORK/capture.jsonl" -- bash "$FIXTURE"
printf '   the wrapped script exited %s\n' "$?"

hr "captured snapshots"
"$BIN" show "$WORK/capture.jsonl" | sed 's/^/   /'

hr "artifacts"
printf '   %s\n   %s --help\n\n' "$WORK/capture.jsonl" "$BIN"
