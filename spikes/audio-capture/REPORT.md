## Spike #11 report: meeting audio capture on macOS

**Short answer: yes, with one condition.** A Rust core records the meeting on two tracks:

- the microphone, with `cpal`;
- the system audio, with a Core Audio process tap called from Rust through
  `objc2-core-audio`, with no Swift or Objective-C.

Both tracks are written progressively to disk. Build, crash recovery and clock drift
pass on this machine.

The condition is permissions. System audio contains sound only after the user grants
"System Audio Recording" to a **signed .app**. That grant survives rebuilds only if the
app is signed with a real identity, not ad-hoc. The permission prompts and the real
60-minute call still need a test by the maintainer (last section).

Tested on macOS 27.2.1 (Apple Silicon), Rust 1.96.1, with the built-in MacBook mic and
speakers. Code is in `spikes/audio-capture/` on branch `spike/11-audio-capture`
(throwaway).

### Outcomes

| Measurable outcome | Status | Evidence |
|---|---|---|
| Build: mic via `cpal`, system audio via process tap, from Rust | **Met** | `cargo build --release`. The ScreenCaptureKit fallback builds with `--features sck`. |
| Capture paths without user interaction | **Partial (TCC)** | **Mic:** works with real signal (the parent process already had mic access). **Process tap:** the tap, the aggregate device and the IOProc are created and run at full rate, but the audio is **all zeros** until System Audio Recording is granted, with no error. **ScreenCaptureKit:** fails with an explicit TCC error. |
| Process killed mid-recording → audio recoverable | **Met** | `kill -9` after ~15.5 s. Both WAVs open as-is: 14.5 s, up to the last 1 s checkpoint. After `trama-rec repair`: 15.30 s mic, 15.17 s system. Loss is at most ~0.1 s. |
| Drift between tracks < 100 ms over 60 min | **Met on one shared clock; separate clocks pending real call** | 10-min run, built-in mic + tap on built-in speakers: both tracks −3.6 ppm against the host clock, relative drift 0.0 ppm (0.0 ms extrapolated to 60 min). Anchor jitter ≤ 0.04 ms, 0 dropped frames, 0 gaps. A second 10-min run (tap-only aggregate + a virtual mic): both exactly host-clocked. **Not measured:** devices on separate hardware clocks (USB or Bluetooth headset). Typical ±20–50 ppm means 72–180 ms raw drift at 60 min; the sidecar mapping below removes it (residual < 0.1 ms). |
| 60-min recording with Teams/Meet/Zoom, both tracks complete | **Pending maintainer test** | Needs the TCC grant and a real call. |
| Permissions requested once across rebuilds | **Met** | Signed with an "Apple Development" identity. After granting Microphone and System Audio Recording once, a rebuilt binary (different cdhash, same designated requirement) recorded both tracks with sound and **no prompt** (maintainer test, 2026-11-27). Ad-hoc signing pins the cdhash, so any code change makes TCC ask again. |
| Windows notes for the stack ADR | **Done** | Below. |

Maintainer tests on 2026-11-27: after the grant, a clean 60 s run had both tracks with
sound, 0 dropped frames and 0.0 ppm relative drift (0.03 ms misalignment at the end).

### Chosen approach

- **Mic:** `cpal` 0.18 default input (or `--mic-device NAME`), f32 at the device rate.
  Callback timestamps are mach host time.
- **System audio:**
  1. `CATapDescription`: a stereo global tap that excludes our own process, or specific PIDs.
  2. `AudioHardwareCreateProcessTap`.
  3. A **private** aggregate device that holds the tap (drift compensation on), with the
     default output device as clock.
  4. `AudioDeviceCreateIOProcID`.

  The tap and the private device belong to the process, so a crash leaves nothing
  behind. A tap-only aggregate with no output sub-device (`--tap-only`) also works and
  is clocked on host time; it is a candidate for surviving headphone switches.
- **Real-time safety:** audio callbacks only copy into lock-free ring buffers (`rtrb`);
  one writer thread per track does the disk I/O.
- **Progressive writing:** 32-bit float WAV; the header is patched and `fsync`ed every
  second; after a crash the file is valid up to the last checkpoint, and `repair`
  recovers the tail from the file length.
- **Timing sidecar** (`<track>.timing.jsonl`): one anchor per second of audio
  (host time, frame, sample time, dropped, written) plus a line for each discontinuity;
  `session.json` holds the start time, devices and formats.
- **Realignment:** both tracks are mapped to one clock (mach host time) by linear
  interpolation between anchors. This absorbs the different start times (the tap starts
  ~110 ms after the mic), clock drift and gaps, without resampling.
- **Fallback:** ScreenCaptureKit (audio-only stream, `excludesCurrentProcessAudio`). It
  needs the broader Screen & System Audio Recording permission, which macOS
  periodically asks the user to confirm again, so it is used only where the tap is not
  available.

### Risks

1. **Silent TCC denial for the tap.** Without permission the tap delivers zeros and
   reports no error, and there is no public API to read the status. The app needs an
   onboarding check (play a sound, verify the level).
2. **Device changes during a call** (headphones connecting or disconnecting): not
   tested. The app must rebuild the tap when the default output changes (or use the
   tap-only aggregate) and reopen the mic on a stream error. Restarts appear in the
   sidecar and remain realignable.
3. **Bluetooth headsets.** Using the headset mic switches the output to HFP
   (16/24 kHz) and puts the mic on a separate clock: drift is higher than measured here.
   The sidecar mapping handles it; to be validated in the 60-minute test.
4. **Signing.** A grant tied to an "Apple Development" certificate survives rebuilds,
   not a change of certificate or team. Distribution needs Developer ID plus
   notarization. The binary must run as the app (launched by LaunchServices): a CLI
   started from a terminal inherits the terminal's permissions.
5. **Mic level.** Float input does not clip: the built-in mic produced 1,788 samples
   above 1.0 in 10 minutes. Clamp before converting to int16 for transcription.
6. **The system track starts with the first sound.** With the grant in place, the
   tap's first frame arrived only when the Mac first played audio (2.5–8 s after the
   mic in the maintainer's tests); after that it runs continuously, silences included.
   The sidecar realigns the tracks; the app should pad the start of the system track
   (or treat the offset explicitly) so both files share the same timeline.
7. **API ergonomics.** `objc2-core-audio` exposes the whole process-tap API, all
   `unsafe`, so it needs a ~250-line wrapper.

### Windows notes (for the stack ADR)

- **WASAPI loopback of the output device:** already supported by `cpal` (an input
  stream on an output device). Works on all Windows 10/11 versions with no prompt.
  Loopback delivers no packets while nothing is playing, so gaps are filled from the
  QPC timestamps; the sidecar design fits as-is.
- **Process loopback** (`AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK`): the Windows
  equivalent of the process tap, independent of the output device. Documented for
  Windows 10 build 20348+ / Windows 11 (19041 reportedly works, to be verified). Not in
  `cpal`; available in the `wasapi` crate.
- **Mic:** `cpal` WASAPI, governed by the "Let desktop apps access your microphone"
  setting; no per-app prompt for unpackaged apps.
- **Conclusion:** only the capture backend is per-OS (~300 lines each). Ring buffers,
  writer, sidecar and realignment are shared, consistent with ADR 0005.

### To finish the spike (maintainer)

Run all commands from `spikes/audio-capture/`.

**1. Build and sign the app, grant permissions (once)**

```sh
./bundle.sh        # signs with the first "Apple Development" identity
mkdir -p ~/trama-spike
open -W build/TramaRec.app --stdout ~/trama-spike/log.txt --stderr ~/trama-spike/log.txt \
  --args record --out ~/trama-spike/perm --seconds 20
```

While it records, play something (for example `say "permission test"`) and allow
**Microphone** and **System Audio Recording** for TramaRec. If no prompt appears, enable
TramaRec in System Settings → Privacy & Security → Screen & System Audio Recording →
"System Audio Recording Only". Run it again, then:

```sh
./target/release/trama-rec analyze ~/trama-spike/perm    # both tracks: peak > 0, no "SILENT"
```

**2. Rebuild-and-check-permissions test**

```sh
SPIKE_BUILD_ID=rebuild2 ./bundle.sh                    # different binary, same identity
build/TramaRec.app/Contents/MacOS/trama-rec version    # prints "build rebuild2"
open -W build/TramaRec.app --args record --out ~/trama-spike/rebuild --seconds 20
./target/release/trama-rec analyze ~/trama-spike/rebuild
```

Expected: **no prompt**, and both tracks have sound. Counter-test, which should lose the
grant: `SIGN_IDENTITY=- ./bundle.sh` and record again. Clean up afterwards:

```sh
tccutil reset Microphone dev.trama.spike.rec
tccutil reset AudioCapture dev.trama.spike.rec
```

**3. 60-minute real call (Teams, Meet or Zoom, headphones on)**

```sh
./bundle.sh
open build/TramaRec.app --args record --out ~/trama-spike/call60 --seconds 3660
# headset mic: add --mic-device "<part of its name>"  (list: ./target/release/trama-rec devices)
```

Then `./target/release/trama-rec analyze ~/trama-spike/call60`. Pass criteria: both WAVs
about 3660 s; `dropped` 0; relative drift at 60 min under 100 ms (or corrected by the
sidecar: note the ppm); your voice only in `mic.wav`, the others only in `system.wav`.

**4. Optional alignment check (speakers, no headphones, ~2 min)**

```sh
./target/release/trama-rec gen-tone --out ~/trama-spike/clicks.wav --seconds 120
open build/TramaRec.app --args record --out ~/trama-spike/align --seconds 125 &
sleep 2; afplay ~/trama-spike/clicks.wav
./target/release/trama-rec align-check ~/trama-spike/align
```

The offset between the clicks in `system` and `mic` should stay constant within a few ms.

Recordings stay under `~/trama-spike`, never in the repository.
