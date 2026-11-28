#!/usr/bin/env bash
# Builds Trama.app and the .dmg, signed with a stable identity, and checks the result.
#
#   scripts/bundle-macos.sh            release build
#   scripts/bundle-macos.sh --debug    debug build (faster, same signature), named
#                                      "Trama Dev" with the DEV icon (tauri.debug.conf.json)
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
APP_NAME=Trama
TAURI_ARGS=()
if [[ "${1:-}" == "--debug" ]]; then
  PROFILE=debug
  APP_NAME="Trama Dev"
  TAURI_ARGS+=(--debug --config src-tauri/tauri.debug.conf.json)
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
# The same version everywhere: the app, the Rust crates and the frontend package.
version_of() { sed -n "s/$2/\\1/p" "$1" | head -n 1; }
APP_VERSION="$(version_of src-tauri/tauri.conf.json '^  "version": "\(.*\)",$')"
NPM_VERSION="$(version_of package.json '^  "version": "\(.*\)",$')"
CARGO_VERSION="$(version_of "$REPO_ROOT/Cargo.toml" '^version = "\(.*\)"$')"
if [[ -z "$APP_VERSION" || "$APP_VERSION" != "$NPM_VERSION" || "$APP_VERSION" != "$CARGO_VERSION" ]]; then
  echo "Versions differ: tauri.conf.json '$APP_VERSION', package.json '$NPM_VERSION'," >&2
  echo "Cargo.toml '$CARGO_VERSION'." >&2
  exit 1
fi

if [[ "$IDENTITY" == "-" ]]; then
  echo "Warning: ad-hoc signature; permissions will not survive the next build." >&2
fi

APPLE_SIGNING_IDENTITY="$IDENTITY" npx tauri build ${TAURI_ARGS[@]+"${TAURI_ARGS[@]}"} --bundles app,dmg

BUNDLE="$REPO_ROOT/target/$PROFILE/bundle"
APP="$BUNDLE/macos/$APP_NAME.app"
DMG="$(ls -t "$BUNDLE"/dmg/*.dmg | head -n 1)"

codesign --verify --strict --deep "$APP"
echo
echo "Version:     $APP_VERSION"
echo "App:         $APP"
echo "DMG:         $DMG"
echo "Signed by:   $(codesign -dvv "$APP" 2>&1 | sed -n 's/^Authority=//p' | head -n 1)"
echo "Designated:  $(codesign -d -r- "$APP" 2>&1 | sed -n 's/^designated => //p')"
echo "SHA-256:     $(shasum -a 256 "$DMG" | cut -d ' ' -f 1)"
