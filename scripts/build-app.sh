#!/bin/sh
# Builds MingShot.app (release) and, on a Mac with the "MingDesk Dev" certificate (~/Projects/mingdesk/scripts/dev-cert.sh; one self-signed cert serves both apps), signs it
# so the Screen Recording permission survives rebuilds.
#
#   npm run app            build
#   npm run app -- --open  build, quit any running copy, and open the new one
set -eu
cd "$(dirname "$0")/.."

NAME="MingDesk Dev"
if [ "$(uname)" = Darwin ] && security find-certificate -c "$NAME" >/dev/null 2>&1; then
  export APPLE_SIGNING_IDENTITY="$NAME"
  echo "Signing with \"$NAME\"."
else
  echo "No \"$NAME\" certificate: building unsigned (run sh ~/Projects/mingdesk/scripts/dev-cert.sh once to fix)."
fi

npx tauri build --bundles app

APP="src-tauri/target/release/bundle/macos/MingShot.app"
if [ "$(uname)" = Darwin ]; then
  codesign -dvv "$APP" 2>&1 | grep -E '^(Identifier|Authority|Signature)=' || true
  if [ "${1:-}" = "--open" ]; then
    osascript -e 'tell application "MingShot" to quit' >/dev/null 2>&1 || true
    sleep 1
    open "$APP"
  fi
fi
echo "Built $APP"
