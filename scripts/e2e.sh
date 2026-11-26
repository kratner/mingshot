#!/bin/sh
# End-to-end M0 check (scripts/e2e.swift): starts the last release build of MingShot, drives it with ⌃⇧2 and real
# mouse drags, compares the clipboard with screencapture, then quits it. Don't touch the mouse while it runs.
set -eu
cd "$(dirname "$0")/.."
APP=src-tauri/target/release/bundle/macos/MingShot.app/Contents/MacOS/mingshot
OUT=selftest-out/e2e
mkdir -p "$OUT"
MINGSHOT_E2E_SELECT=140,180,301,187 "$APP" & PID=$!
trap 'kill $PID 2>/dev/null || true' EXIT
sleep 2.5
swift scripts/e2e.swift "$OUT" "$@"
