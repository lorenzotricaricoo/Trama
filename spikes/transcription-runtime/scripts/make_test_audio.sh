#!/usr/bin/env bash
# Generates the spike's synthetic test audio (macOS only: uses `say`).
# Output goes OUTSIDE the repo, in ~/Library/Caches/trama-spikes/audio/<set>/,
# one X.wav + X.txt (reference) per file. Nothing here is committed.
set -euo pipefail

HERE="$(cd "$(dirname "$0")/.." && pwd)"
BIN="$HERE/target/release/trama-stt"
OUT="${TRAMA_SPIKE_AUDIO:-$HOME/Library/Caches/trama-spikes/audio}"
PARTS="$OUT/_parts"
[ -x "$BIN" ] || (cd "$HERE" && cargo build --release)
rm -rf "$OUT"
mkdir -p "$PARTS"

# 1. Speech sets from data/synthetic.tsv (set, file_id, lang, text), two voice profiles:
#    natural   -> sets as named (mixed, mono_it, mono_en): Alice (it), Samantha/Daniel (en)
#    eloquence -> mixed_eloquence: robotic Eddy (it) / Flo UK (en), a harder, less
#                 realistic stress test.
voice_for() { # profile lang file_index
  case "$1:$2" in
    natural:it) echo "Alice" ;;
    natural:en) if (( $3 % 2 )); then echo "Samantha"; else echo "Daniel"; fi ;;
    eloquence:it) echo "Eddy (Italiano (Italia))" ;;
    eloquence:en) echo "Flo (Inglese (UK))" ;;
  esac
}
n=0; fidx=0; prev=""
while IFS=$'\t' read -r set id lang text; do
  [[ -z "$set" || "$set" == \#* ]] && continue
  n=$((n + 1))
  [[ "$id" != "$prev" ]] && fidx=$((fidx + 1)) && prev="$id"
  for profile in natural eloquence; do
    if [[ "$profile" == eloquence ]]; then
      [[ "$set" == mixed ]] || continue
      pset="mixed_eloquence"; pid="${id}_eloq"
    else
      pset="$set"; pid="$id"
    fi
    part="$PARTS/$(printf '%s__%03d' "$pid" "$n").wav"
    say -v "$(voice_for "$profile" "$lang" "$fidx")" --file-format=WAVE \
      --data-format=LEI16@16000 -o "$part" "$text"
    echo "$pset" > "$PARTS/$pid.set"
    printf '%s ' "$text" >> "$PARTS/$pid.ref"
  done
done < "$HERE/data/synthetic.tsv"

for ref in "$PARTS"/*.ref; do
  id="$(basename "$ref" .ref)"
  set="$(cat "$PARTS/$id.set")"
  mkdir -p "$OUT/$set"
  "$BIN" concat --gap-ms 700 -o "$OUT/$set/$id.wav" "$PARTS/$id"__*.wav 2>/dev/null
  sed 's/ *$//' "$ref" > "$OUT/$set/$id.txt"
done

# 2. Silence / noise only (empty reference => every output word is invented).
S="$OUT/silence"; mkdir -p "$S"
"$BIN" synth --kind silence --seconds 300 -o "$S/silence_digital_5min.wav"
"$BIN" synth --kind white --dbfs -60 --seconds 300 -o "$S/white_-60dBFS_5min.wav"
"$BIN" synth --kind white --dbfs -40 --seconds 300 -o "$S/white_-40dBFS_5min.wav"
"$BIN" synth --kind room --dbfs -50 --seconds 300 -o "$S/room_-50dBFS_5min.wav"
"$BIN" synth --kind room --dbfs -35 --seconds 300 -o "$S/room_-35dBFS_5min.wav"
"$BIN" synth --kind white --dbfs -25 --seconds 300 -o "$S/white_-25dBFS_5min.wav"
"$BIN" synth --kind room --dbfs -20 --seconds 300 -o "$S/room_-20dBFS_5min.wav"
"$BIN" synth --kind clicks --dbfs -30 --seconds 300 -o "$S/clicks_-30dBFS_5min.wav"
for f in "$S"/*.wav; do : > "${f%.wav}.txt"; done

# 3. Speech with long pauses (60 s of room tone / silence between turns).
P="$OUT/pauses"; mkdir -p "$P"
"$BIN" synth --kind room --dbfs -45 --seconds 60 --seed 7 -o "$PARTS/room60.wav"
"$BIN" synth --kind silence --seconds 60 -o "$PARTS/sil60.wav"
"$BIN" concat --gap-ms 0 -o "$P/pauses_mixed.wav" \
  "$OUT/mono_it/it01.wav" "$PARTS/room60.wav" "$OUT/mono_en/en01.wav" "$PARTS/sil60.wav" \
  "$OUT/mixed/mix03_codeswitch.wav" "$PARTS/room60.wav" "$OUT/mixed/mix07_alternating.wav" 2>/dev/null
cat "$OUT/mono_it/it01.txt" "$OUT/mono_en/en01.txt" "$OUT/mixed/mix03_codeswitch.txt" \
  "$OUT/mixed/mix07_alternating.txt" | tr '\n' ' ' > "$P/pauses_mixed.txt"

# 4. Long file (>= 10 min) for speed: all speech files, repeated, 1.5 s gaps.
L="$OUT/long"; mkdir -p "$L"
files=()
for _ in 1 2 3 4; do
  for f in "$OUT"/mixed/*.wav "$OUT"/mono_it/*.wav "$OUT"/mono_en/*.wav; do files+=("$f"); done
done
"$BIN" concat --gap-ms 1500 -o "$L/long_mixed.wav" "${files[@]}"
for f in "${files[@]}"; do cat "${f%.wav}.txt"; printf ' '; done > "$L/long_mixed.txt"

echo "Test audio in $OUT:"; ls "$OUT"
