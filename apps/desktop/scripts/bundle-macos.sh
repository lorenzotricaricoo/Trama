#!/usr/bin/env bash
# Builds Trama.app and the .dmg, signed with a stable identity, and checks the result.
#
#   scripts/bundle-macos.sh            release build
#   scripts/bundle-macos.sh --debug    debug build (faster, same signature)
#
# The signing identity is never committed. It is read from, in order:
#   1. the TRAMA_SIGNING_IDENTITY environment variable
#   2. the first line of .signing-identity at the repository root (gitignored)
# Use the full name, for example "Apple Development: Jane Doe (ABCDE12345)"; list the
# available ones with: security find-identity -v -p codesigning
# "-" signs ad hoc: fine for a quick check, but macOS asks for permissions again after
# every build.
set -euo pipefail

cd "$(dirname "$0")/.."
REPO_ROOT="$(cd ../.. && pwd)"

PROFILE=release
TAURI_ARGS=()
if [[ "${1:-}" == "--debug" ]]; then
  PROFILE=debug
  TAURI_ARGS+=(--debug)
elif [[ -n "${1:-}" ]]; then
  echo "Unknown option: $1 (use --debug or nothing)" >&2
  exit 2
fi

IDENTITY="${TRAMA_SIGNING_IDENTITY:-}"
if [[ -z "$IDENTITY" && -f "$REPO_ROOT/.signing-identity" ]]; then
  IDENTITY="$(head -n 1 "$REPO_ROOT/.signing-identity")"
fi
if [[ -z "$IDENTITY" ]]; then
  echo "No signing identity. Set TRAMA_SIGNING_IDENTITY or write it in .signing-identity." >&2
  echo "Available identities:" >&2
  security find-identity -v -p codesigning | sed -n 's/^ *[0-9]*) [0-9A-F]* //p' >&2
  exit 1
fi
if [[ "$IDENTITY" == "-" ]]; then
  echo "Warning: ad-hoc signature; permissions will not survive the next build." >&2
fi

APPLE_SIGNING_IDENTITY="$IDENTITY" npx tauri build ${TAURI_ARGS[@]+"${TAURI_ARGS[@]}"} --bundles app,dmg

BUNDLE="$REPO_ROOT/target/$PROFILE/bundle"
APP="$BUNDLE/macos/Trama.app"
DMG="$(ls -t "$BUNDLE"/dmg/*.dmg | head -n 1)"

codesign --verify --strict --deep "$APP"
echo
echo "App:         $APP"
echo "DMG:         $DMG"
echo "Signed by:   $(codesign -dvv "$APP" 2>&1 | sed -n 's/^Authority=//p' | head -n 1)"
echo "Designated:  $(codesign -d -r- "$APP" 2>&1 | sed -n 's/^designated => //p')"
