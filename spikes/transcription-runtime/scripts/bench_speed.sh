#!/usr/bin/env bash
# Speed/memory benchmark on the long file: threads x provider.
# Usage: scripts/bench_speed.sh <model-dir> [audio.wav]
set -uo pipefail
HERE="$(cd "$(dirname "$0")/.." && pwd)"
C="$HOME/Library/Caches/trama-spikes"
MODEL="${1:?model dir}"
AUDIO="${2:-$C/audio/long/long_mixed.wav}"
for provider in cpu coreml; do
  for threads in 2 4 8; do
    [[ "$provider" == coreml && "$threads" != 4 ]] && continue
    echo "== provider=$provider threads=$threads"
    /usr/bin/time -l "$HERE/target/release/trama-stt" transcribe --model "$MODEL" \
      --vad "$C/models/silero_vad.onnx" --provider "$provider" --threads "$threads" "$AUDIO" \
      2>&1 >/dev/null | grep -E 'real time|load|maximum resident|peak memory|rror'
  done
done
