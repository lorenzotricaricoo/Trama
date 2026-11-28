//! Capture on real macOS devices. Ignored by default: they need audio hardware and
//! permissions that CI does not have. Run them by hand with
//! `cargo test -p trama-core --test capture_macos -- --ignored`, and see
//! `docs/development/audio-capture.md` for the full manual checklist.

#![cfg(target_os = "macos")]

use std::sync::Arc;
use std::time::Duration;

use trama_core::capture::macos::MacBackend;
use trama_core::capture::*;

#[test]
#[ignore = "needs real audio devices"]
fn lists_the_default_devices() {
    let backend = MacBackend::new();
    let output = backend.default_device(Source::System).unwrap();
    assert!(output.is_some_and(|d| !d.id.is_empty()));
    assert!(!backend.input_devices().unwrap().is_empty());
}

#[test]
#[ignore = "needs an output device; plays an inaudible probe"]
fn probes_the_system_audio_permission() {
    let state = MacBackend::new().probe_system_audio_permission();
    assert!(
        matches!(state, PermissionState::Granted | PermissionState::Denied),
        "{state:?}"
    );
}

#[test]
#[ignore = "needs an output device and System Audio Recording for the test process"]
fn records_system_audio_through_the_process_tap() {
    let dir = tempfile::tempdir().unwrap();
    let mut options = RecordingOptions::new(dir.path(), uuid::Uuid::now_v7());
    options.microphone = false;
    let recording =
        Recording::start(Arc::new(MacBackend::new()), options, Arc::new(NoObserver)).unwrap();
    std::thread::sleep(Duration::from_secs(3));
    let summary = recording.stop().unwrap();
    let track = &summary.tracks[0];
    assert_eq!(track.segments, 1);
    assert!(
        track.duration_ms.abs_diff(summary.duration_ms) < 100,
        "{summary:?}"
    );
    let timeline = Timeline::load(&dir.path().join(&track.timing_path)).unwrap();
    // On one host clock the tap drifts by a few ppm at most.
    if let Some(ppm) = timeline.drift_ppm() {
        assert!(ppm.abs() < 100.0, "{ppm} ppm");
    }
}
