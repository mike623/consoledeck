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
codesign --force --sign - "$APP"  # ad-hoc: unsigned for distribution, fine on this Mac
echo "Built $APP"
if [[ "${1:-}" == "install" ]]; then
  pkill -x ConsoleDeck || true
  rm -rf ~/Applications/ConsoleDeck.app
  mkdir -p ~/Applications
  cp -R "$APP" ~/Applications/
  open ~/Applications/ConsoleDeck.app
  echo "Installed ~/Applications/ConsoleDeck.app"
fi
