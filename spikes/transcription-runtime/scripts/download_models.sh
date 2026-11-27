#!/usr/bin/env bash
# Downloads the spike models from the official sherpa-onnx GitHub release "asr-models"
# into ~/Library/Caches/trama-spikes/ (outside the repo) and checks their SHA-256.
set -euo pipefail
C="$HOME/Library/Caches/trama-spikes"
B="https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models"
mkdir -p "$C/downloads" "$C/models"
cd "$C/downloads"
while read -r sha f; do
  [ -f "$f" ] || curl -sSL --fail -o "$f" "$B/$f"
  echo "$sha  $f" | shasum -a 256 -c -
done <<'EOF'
d216b71d7f628b3f2f2a20ee7ead39ff0b075984959eb4bf6ddf8dc8da737c09 sherpa-onnx-nemo-parakeet-tdt-0.6b-ultra-int8.tar.bz2
1495e91b59d7fc53ddca91f3431758ec99f8fa253fdb0e1e851be0f72b14b0f9 sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8.tar.bz2
9e2449e1087496d8d4caba907f23e0bd3f78d91fa552479bb9c23ac09cbb1fd6 silero_vad.onnx
EOF
cd "$C/models"
for f in "$C"/downloads/*.tar.bz2; do
  d="$(basename "$f" .tar.bz2)"
  [ -d "$d" ] || tar -xjf "$f"
done
cp -n "$C/downloads/silero_vad.onnx" . 2>/dev/null || true
ls "$C/models"
