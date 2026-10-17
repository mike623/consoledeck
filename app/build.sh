#!/bin/bash
# Build ConsoleDeck into src-tauri/target/release/bundle. On a Mac, pass "install" to copy it
# to ~/Applications (replacing the old Swift app, same bundle ID) and launch it.
set -euo pipefail
cd "$(dirname "$0")/src-tauri"
if [[ "$(uname)" == Darwin ]]; then
  # A stable identity keeps the Accessibility permission across rebuilds; ad-hoc signing
  # changes every build, so macOS would forget it each time.
  APPLE_SIGNING_IDENTITY="${APPLE_SIGNING_IDENTITY:-$(security find-identity -v -p codesigning | awk -F'"' '/Apple Development/ {print $2; exit}')}"
  if [[ -z "$APPLE_SIGNING_IDENTITY" ]]; then
    echo "No Apple Development identity: signing ad-hoc (Accessibility permission resets on each build)"
    APPLE_SIGNING_IDENTITY=-
  fi
  export APPLE_SIGNING_IDENTITY
  BUNDLES=app
else
  BUNDLES=nsis
fi
# Updater bundles are signed with the key from `tauri signer generate -w ~/.tauri/consoledeck.key`
# (CI has it as a secret). Without it, build without them.
KEY=~/.tauri/consoledeck.key
if [[ -z "${TAURI_SIGNING_PRIVATE_KEY:-}" && -f $KEY ]]; then
  export TAURI_SIGNING_PRIVATE_KEY="$(cat $KEY)" TAURI_SIGNING_PRIVATE_KEY_PASSWORD="${TAURI_SIGNING_PRIVATE_KEY_PASSWORD:-}"
fi
NO_UPDATER=()
[[ -z "${TAURI_SIGNING_PRIVATE_KEY:-}" ]] && NO_UPDATER=(--config '{"bundle":{"createUpdaterArtifacts":false}}')
npx -y @tauri-apps/cli@2 build "${NO_UPDATER[@]+"${NO_UPDATER[@]}"}" --bundles "$BUNDLES"
if [[ "${1:-}" == "install" && "$(uname)" == Darwin ]]; then
  pkill -x consoledeck || true
  pkill -x ConsoleDeck || true
  rm -rf ~/Applications/ConsoleDeck.app
  mkdir -p ~/Applications
  cp -R target/release/bundle/macos/ConsoleDeck.app ~/Applications/
  open ~/Applications/ConsoleDeck.app
  echo "Installed ~/Applications/ConsoleDeck.app"
fi
