# Audio capture: manual checks

The capture core ([design](../architecture/audio-capture.md)) is covered by headless tests
with fake devices (`cargo test -p trama-core --test capture`). What needs real devices,
real permissions and a real call is checked by hand with the `capture` example, before a
release and after changes to `crates/core/src/capture/macos/`.

## The `capture` example

```sh
cargo run -p trama-core --example capture -- devices
cargo run -p trama-core --example capture -- permissions
cargo run -p trama-core --example capture -- record LIBRARY --seconds 60
cargo run -p trama-core --example capture -- analyze LIBRARY/audio/<meeting id> [--clicks]
cargo run -p trama-core --example capture -- repair LIBRARY <meeting id>
cargo run -p trama-core --example capture -- clicks clicks.wav --seconds 120
```

`record` creates a library at `LIBRARY` if needed, records a new meeting, registers its
tracks and prints levels and events; `analyze` prints, for each track, its length, level,
first audio, clock drift, streams and gaps, and the drift between the tracks.

Use a library outside the repository, for example `~/trama-capture-checks`, and delete it
afterwards: recordings never go into the repository or an issue.

**Permissions belong to the process.** Run from a terminal, the example uses the
terminal's microphone and System Audio Recording permissions (System Settings → Privacy &
Security). The checks marked **app** need the signed `Trama.app`
([macOS signing](macos-signing.md)) and wait for the recording window
([#31](https://github.com/lorenzotricaricoo/Trama/issues/31)).

## Checklist

Write down the macOS version, the Mac and the devices used.

### Permissions

- [ ] `permissions` reports the microphone as `Granted` or `NotDetermined`, never an error.
- [ ] With System Audio Recording **not** granted, `permissions` reports system audio as
      `Denied`, and a 15 s `record` reports `system_audio_silent` after about 10 s.
- [ ] With System Audio Recording granted, `permissions` reports `Granted` and you hear
      nothing while it probes.
- [ ] With the microphone denied in System Settings, `record` fails at once with
      `microphone_denied` and leaves no files.
- [ ] **app**: the first recording shows the microphone prompt once; a rebuilt app keeps
      both permissions (no new prompt).

### Two tracks

- [ ] `record --seconds 30` while speaking and playing a video: `analyze` shows both
      tracks about 30 s long, with sound, `dropped` 0; your voice only in `me.wav`, the video
      only in `others.wav` (listen to both).
- [ ] Start a recording with nothing playing and play something after 5 s: `others`
      starts with about 5 s of silence (`silence (start)`), and `TrackStarted` reports the
      offset.

### Alignment (speakers, no headphones)

- [ ] `clicks clicks.wav --seconds 120`, then `record --seconds 125` and, 2 s in,
      `afplay clicks.wav`. `analyze --clicks` matches the clicks in both tracks with a
      spread under 5 ms (the constant offset is the acoustic path and the input latency).

### Device changes

- [ ] Connect wired or Bluetooth headphones during a recording: `DeviceChanged` for
      `system` (and for `microphone` if its default changes), the recording goes on, and
      `analyze` shows a second stream with a short `silence (gap)`.
- [ ] Select a Bluetooth headset microphone during a recording (16 kHz HFP): the `me`
      track keeps its format and stays intelligible after the switch.
- [ ] Disconnect the headset during a recording: the stream is reopened on the next
      default device.

### Crash

- [ ] Press Ctrl-C during `record`: the files stay readable. `analyze` may ask for
      `repair`; after it, the tracks end within 0.1 s of the moment you pressed Ctrl-C.

### The 60-minute call (issue #25, "done when")

- [ ] Record a real Teams, Meet or Zoom call of 60 minutes **with a headset on its own
      clock** (USB or Bluetooth), with `record --seconds 3660` (or **app** once #31 lands).
- [ ] `analyze`: both tracks about 3660 s; no `frames_dropped`; note each track's drift
      (ppm) and the relative drift.
- [ ] Realigned within 100 ms from start to end: a sound both tracks hear (a click, a
      clap with speakers on, or someone's voice leaking into the microphone) is at the same
      recording time in both, at the start and at the end. `analyze --clicks` does it for
      clicks played at the start and at the end.
