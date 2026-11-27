#!/usr/bin/env bash
# Runs the eval harness on every synthetic set produced by make_test_audio.sh.
# Usage: scripts/run_synthetic_evals.sh <model-dir> [extra trama-stt flags...]
set -euo pipefail
HERE="$(cd "$(dirname "$0")/.." && pwd)"
C="$HOME/Library/Caches/trama-spikes"
MODEL="${1:?model dir}"; shift
TAG="$(basename "$MODEL")${*:+_$(echo "$*" | tr -c 'A-Za-z0-9\n' '_')}"
for set in mono_it mono_en mixed mixed_eloquence pauses silence long; do
  [ -d "$C/audio/$set" ] || continue
  echo "== $set =="
  "$HERE/target/release/trama-stt" eval --model "$MODEL" --vad "$C/models/silero_vad.onnx" "$@" \
    --out "$C/eval-out/$TAG/$set" "$C/audio/$set" 2>/dev/null
done
