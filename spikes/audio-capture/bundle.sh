#!/usr/bin/env bash
# Build build/TramaRec.app (Info.plist with usage strings + entitlements) and sign it.
#   SIGN_IDENTITY=-        ad-hoc signature (designated requirement = cdhash, changes every build)
#   SIGN_IDENTITY=<sha1>   a codesigning identity (default: first "Apple Development" identity)
#   SPIKE_BUILD_ID=<x>     embedded in the binary, to force a different binary between builds
set -euo pipefail
cd "$(dirname "$0")"
cargo build --release --quiet
APP=build/TramaRec.app
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS"
cp target/release/trama-rec "$APP/Contents/MacOS/trama-rec"
cp packaging/Info.plist "$APP/Contents/Info.plist"
ID="${SIGN_IDENTITY:-$(security find-identity -v -p codesigning | awk '/Apple Development/ {print $2; exit}')}"
codesign --force --options runtime --timestamp=none \
  --entitlements packaging/entitlements.plist --sign "$ID" "$APP"
codesign --verify --strict "$APP"
echo "cdhash:     $(codesign -dvvv "$APP" 2>&1 | sed -n 's/^CDHash=//p')"
echo "designated: $(codesign -d -r- "$APP" 2>&1 | sed -n 's/^#* *designated => //p')"
