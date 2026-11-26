#!/bin/sh
# End-to-end checks (scripts/e2e.swift): for each mode (region, window, display) starts the last release build of
# MingShot with that mode fixed, drives it with the real ⌃⇧2, compares the clipboard with screencapture, then quits
# it. TAKES OVER THE SCREEN for about half a minute (overlays, pointer moves): don't touch the mouse or keyboard.
#
#   sh scripts/e2e.sh                 all three modes
#   sh scripts/e2e.sh window          one mode
set -eu
cd "$(dirname "$0")/.."
APP=src-tauri/target/release/bundle/macos/MingShot.app/Contents/MacOS/mingshot
OUT=selftest-out/e2e
mkdir -p "$OUT"
status=0
for mode in ${1:-region window display}; do
  if [ "$mode" = region ]; then SELECT=140,180,301,187; else SELECT=pointer; fi
  MINGSHOT_E2E_MODE=$mode MINGSHOT_E2E_SELECT=$SELECT "$APP" & PID=$!
  sleep 4  # first launch after a build loads the overlays slowly; the test must not beat them
  swift scripts/e2e.swift "$OUT" "$mode" || status=1
  kill $PID 2>/dev/null || true
  wait $PID 2>/dev/null || true
done
exit $status
