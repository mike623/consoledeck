#!/bin/bash
# Build ConsoleDeck.app into mac/build/. Pass "install" to copy it to ~/Applications and launch it.
set -euo pipefail
cd "$(dirname "$0")"
swift build -c release
APP=build/ConsoleDeck.app
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS"
cp .build/release/ConsoleDeck "$APP/Contents/MacOS/"
cp Info.plist "$APP/Contents/"
# A stable identity keeps the Accessibility permission across rebuilds; ad-hoc signing
# changes every build, so macOS would forget it each time.
IDENTITY="${CODESIGN_IDENTITY:-$(security find-identity -v -p codesigning | awk -F'"' '/Apple Development/ {print $2; exit}')}"
if [[ -z "$IDENTITY" ]]; then
  echo "No Apple Development identity: signing ad-hoc (Accessibility permission resets on each build)"
  IDENTITY=-
fi
codesign --force --sign "$IDENTITY" "$APP"
echo "Built $APP"
if [[ "${1:-}" == "install" ]]; then
  pkill -x ConsoleDeck || true
  rm -rf ~/Applications/ConsoleDeck.app
  mkdir -p ~/Applications
  cp -R "$APP" ~/Applications/
  open ~/Applications/ConsoleDeck.app
  echo "Installed ~/Applications/ConsoleDeck.app"
fi
