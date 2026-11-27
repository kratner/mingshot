#!/bin/sh
# End-to-end checks (scripts/e2e.swift): for each mode (region, window, display) starts the last release build of
# MingShot with that mode fixed, drives it with the real ⌃⇧2, compares the clipboard with screencapture, then quits
# it. TAKES OVER THE SCREEN for about half a minute (overlays, pointer moves): don't touch the mouse or keyboard.
#
#   sh scripts/e2e.sh                 all runs: region (+ Repeat, Esc), window, display, save
#   sh scripts/e2e.sh window          one run
set -eu
cd "$(dirname "$0")/.."
APP=build/MingShot.app/Contents/MacOS/mingshot
OUT=selftest-out/e2e
mkdir -p "$OUT"
status=0
for run in ${1:-region window display save}; do
  case $run in
    region) mode=region; SELECT=140,180,301,187; AFTER=copy ;;
    save) mode=region; SELECT=140,180,301,187; AFTER=save ;;
    *) mode=$run; SELECT=pointer; AFTER=copy ;;
  esac
  MINGSHOT_E2E_MODE=$mode MINGSHOT_E2E_SELECT=$SELECT MINGSHOT_E2E_AFTER=$AFTER "$APP" & PID=$!
  sleep 4  # first launch after a build loads the overlays slowly; the test must not beat them
  swift scripts/e2e.swift "$OUT" "$run" || status=1
  kill $PID 2>/dev/null || true
  wait $PID 2>/dev/null || true
done
exit $status
