//! Audio retention with a manual clock: which audio the setting deletes, when, and what
//! a change of setting would delete. The deletions run on the real job runner.

mod common;

use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use common::{DAY, ManualClock, library};
use tempfile::TempDir;
use trama_core::jobs::*;
use trama_core::storage::*;

const PATIENCE: Duration = Duration::from_secs(10);

/// A recorded meeting with two tracks and a timing file on disk, not transcribed yet.
fn recorded(library: &mut Library) -> Id {
    let id = library.create_meeting("it", ManualClock::START).unwrap().id;
    let dir = Library::meeting_audio_dir(id);
    std::fs::create_dir_all(library.root().join(&dir)).unwrap();
    for role in [Role::Me, Role::Others] {
        let audio_path = dir.join(format!("{}.wav", role.as_str()));
        let timing_path = dir.join(format!("{}.timing.json", role.as_str()));
        std::fs::write(library.root().join(&audio_path), b"RIFF").unwrap();
        std::fs::write(library.root().join(&timing_path), b"{}").unwrap();
        library
            .add_track(
                id,
                &NewTrack {
                    role,
                    audio_path,
                    timing_path: Some(timing_path),
                    device: None,
                },
            )
            .unwrap();
    }
    library
        .set_meeting_status(id, MeetingStatus::Processing, Some(1_000))
        .unwrap();
    id
}

/// Stores a successful transcription now, as the transcription job does.
fn transcribe(library: &mut Library, id: Id) {
    library
        .complete_transcription(
            id,
            "engine",
            &[NewSegment {
                role: Role::Me,
                start_ms: 0,
                end_ms: 500,
                text: "resta".into(),
            }],
        )
        .unwrap();
}

fn transcribed(library: &mut Library) -> Id {
    let id = recorded(library);
    transcribe(library, id);
    id
}

fn set_retention(library: &mut Library, retention: AudioRetention) {
    let mut settings = library.library_settings().unwrap();
    settings.audio_retention = retention;
    library.set_library_settings(&settings).unwrap();
}

fn audio_dir(root: &Path, id: Id) -> PathBuf {
    root.join(Library::meeting_audio_dir(id))
}

fn impact(delete_now: u32, delete_later: u32) -> RetentionImpact {
    RetentionImpact {
        delete_now,
        delete_later,
    }
}

/// Puts the audio of `id` on hold, as the migration does for audio recorded before
/// retention existed: reopens the library around a plain connection.
fn put_on_hold(dir: &TempDir, clock: &ManualClock, library: Library, id: Id) -> Library {
    drop(library);
    let conn = rusqlite::Connection::open(dir.path().join(DATABASE_FILE)).unwrap();
    conn.execute(
        "UPDATE meeting SET audio_on_hold = 1 WHERE source_id = ?1",
        [id.to_string()],
    )
    .unwrap();
    drop(conn);
    Library::open_with_clock(dir.path(), Box::new(clock.clone())).unwrap()
}

struct Events(Receiver<JobEvent>);

impl Events {
    fn sink() -> (Arc<dyn EventSink>, Self) {
        let (tx, rx) = channel();
        let tx = Mutex::new(tx);
        let sink = move |event: JobEvent| {
            let _ = tx.lock().unwrap().send(event);
        };
        (Arc::new(sink), Self(rx))
    }

    /// Waits until the audio of `id` was deleted by a job.
    #[track_caller]
    fn wait_for_deletion(&self, id: Id) -> Job {
        let deadline = Instant::now() + PATIENCE;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let event = self
                .0
                .recv_timeout(left)
                .expect("the audio was not deleted");
            if event.job.kind == JobKind::DeleteAudio
                && event.job.target_id == id
                && event.job.state == JobState::Done
            {
                return event.job;
            }
        }
    }

    /// True when no event arrives for a moment.
    fn quiet(&self) -> bool {
        self.0.recv_timeout(Duration::from_millis(300)).is_err()
    }
}

fn start(library: Library) -> (SharedLibrary, Runner, Events) {
    let library = Arc::new(Mutex::new(library));
    let (sink, events) = Events::sink();
    let runner = Runner::start(Arc::clone(&library), vec![Arc::new(DeleteAudio)], sink);
    (library, runner, events)
}

#[test]
fn never_deletes_the_audio_once_the_transcription_succeeds() {
    let (dir, _clock, mut library) = library();
    let id = recorded(&mut library);
    // Not transcribed yet: the audio stays, even with "never".
    assert!(library.audio_due_for_deletion().unwrap().is_empty());
    assert!(!library.is_audio_due(id).unwrap());

    transcribe(&mut library, id);
    assert_eq!(library.audio_due_for_deletion().unwrap(), [id]);

    // The runner finds it on its first sweep, at start.
    let (library, runner, events) = start(library);
    events.wait_for_deletion(id);
    drop(runner);

    assert!(!audio_dir(dir.path(), id).exists());
    let library = lock(&library);
    assert!(library.meeting(id).unwrap().audio_deleted_at.is_some());
    let version = library.current_version(id).unwrap().unwrap();
    assert_eq!(library.segments(version.id).unwrap()[0].text, "resta");
    assert!(library.audio_due_for_deletion().unwrap().is_empty());
}

#[test]
fn failed_or_unfinished_transcriptions_keep_the_audio() {
    let (_dir, _clock, mut library) = library();
    let processing = recorded(&mut library);
    let failed = recorded(&mut library);
    library.set_meeting_failed(failed, "model_missing").unwrap();
    // Transcribed once, then transcribed again: the audio waits for the new run.
    let again = transcribed(&mut library);
    library.enqueue_job(JobKind::Transcribe, again).unwrap();

    assert!(library.audio_due_for_deletion().unwrap().is_empty());
    for id in [processing, failed, again] {
        assert!(!library.is_audio_due(id).unwrap());
    }
    assert!(
        library
            .audio_retention_impact(AudioRetention::Never)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn days_are_counted_from_the_transcription() {
    let (dir, clock, mut library) = library();
    set_retention(&mut library, AudioRetention::Days7);
    let id = recorded(&mut library);
    // Transcribed a day after the recording.
    clock.advance(DAY);
    transcribe(&mut library, id);

    clock.advance(7 * DAY - 1);
    assert!(library.audio_due_for_deletion().unwrap().is_empty());

    let (library, runner, events) = start(library);
    assert!(events.quiet());
    assert!(audio_dir(dir.path(), id).exists());

    // A week after the transcription, the runner's next sweep deletes it.
    clock.advance(SWEEP_INTERVAL_MS);
    runner.notify();
    events.wait_for_deletion(id);
    drop(runner);
    assert!(!audio_dir(dir.path(), id).exists());
    assert!(
        lock(&library)
            .meeting(id)
            .unwrap()
            .audio_deleted_at
            .is_some()
    );
}

#[test]
fn thirty_days_and_always() {
    let (_dir, clock, mut library) = library();
    set_retention(&mut library, AudioRetention::Days30);
    let id = transcribed(&mut library);
    clock.advance(30 * DAY - 1);
    assert!(!library.is_audio_due(id).unwrap());
    clock.advance(1);
    assert!(library.is_audio_due(id).unwrap());

    set_retention(&mut library, AudioRetention::Always);
    clock.advance(10 * 365 * DAY);
    assert!(!library.is_audio_due(id).unwrap());
    assert!(library.audio_due_for_deletion().unwrap().is_empty());
}

#[test]
fn a_new_setting_applies_to_existing_meetings_from_their_transcription() {
    let (_dir, clock, mut library) = library();
    set_retention(&mut library, AudioRetention::Always);
    let old = transcribed(&mut library);
    clock.advance(20 * DAY);
    let recent = transcribed(&mut library);
    clock.advance(5 * DAY);

    // `old` was transcribed 25 days ago, `recent` 5 days ago.
    assert_eq!(
        library
            .audio_retention_impact(AudioRetention::Never)
            .unwrap(),
        impact(2, 0)
    );
    assert_eq!(
        library
            .audio_retention_impact(AudioRetention::Days7)
            .unwrap(),
        impact(1, 1)
    );
    assert_eq!(
        library
            .audio_retention_impact(AudioRetention::Days30)
            .unwrap(),
        impact(0, 2)
    );
    assert!(
        library
            .audio_retention_impact(AudioRetention::Always)
            .unwrap()
            .is_empty()
    );

    set_retention(&mut library, AudioRetention::Days7);
    assert_eq!(library.audio_due_for_deletion().unwrap(), [old]);
    clock.advance(2 * DAY);
    assert_eq!(library.audio_due_for_deletion().unwrap(), [old, recent]);

    // From 7 to 30 days nothing more is deleted; back to "never", what 7 days kept until
    // now goes at once.
    clock.advance(-2 * DAY);
    assert!(
        library
            .audio_retention_impact(AudioRetention::Days30)
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        library
            .audio_retention_impact(AudioRetention::Never)
            .unwrap(),
        impact(1, 0)
    );
}

#[test]
fn audio_on_hold_waits_for_the_user_to_confirm() {
    let (dir, clock, mut library) = library();
    let id = transcribed(&mut library);
    let mut library = put_on_hold(&dir, &clock, library, id);
    let fresh = recorded(&mut library);

    // "Never" is the setting, but the audio recorded before is kept.
    assert!(library.audio_due_for_deletion().unwrap().is_empty());
    assert!(!library.is_audio_due(id).unwrap());
    assert_eq!(
        library.held_audio().unwrap(),
        HeldAudio {
            count: 1,
            impact: impact(1, 0)
        }
    );
    // Choosing any setting that deletes audio counts it as well.
    assert_eq!(
        library
            .audio_retention_impact(AudioRetention::Days30)
            .unwrap(),
        impact(0, 1)
    );

    // A deletion job queued anyway (by hand, by an older build) keeps the audio.
    let (shared, runner, events) = start(library);
    let job = runner.enqueue(JobKind::DeleteAudio, id).unwrap();
    let done = loop {
        let event = events.0.recv_timeout(PATIENCE).unwrap();
        if event.job.id == job.id && !event.job.is_active() && event.progress.is_none() {
            break event.job;
        }
    };
    assert_eq!(done.state, JobState::Done);
    assert!(audio_dir(dir.path(), id).exists());

    // Once confirmed, the setting applies.
    assert_eq!(lock(&shared).release_audio_holds().unwrap(), 1);
    assert_eq!(lock(&shared).held_audio().unwrap(), HeldAudio::default());
    runner.request_sweep();
    events.wait_for_deletion(id);
    drop(runner);
    assert!(!audio_dir(dir.path(), id).exists());
    // Recorded after the update: never on hold.
    assert!(audio_dir(dir.path(), fresh).exists());
    assert!(
        lock(&shared)
            .meeting(fresh)
            .unwrap()
            .audio_deleted_at
            .is_none()
    );
}

#[test]
fn a_job_no_longer_due_keeps_the_audio() {
    let (dir, _clock, mut library) = library();
    set_retention(&mut library, AudioRetention::Always);
    let id = transcribed(&mut library);
    let (library, runner, events) = start(library);
    let job = runner.enqueue(JobKind::DeleteAudio, id).unwrap();
    let done = loop {
        let event = events.0.recv_timeout(PATIENCE).unwrap();
        if event.job.id == job.id && !event.job.is_active() && event.progress.is_none() {
            break event.job;
        }
    };
    drop(runner);
    assert_eq!(done.state, JobState::Done);
    assert!(audio_dir(dir.path(), id).exists());
    assert!(
        lock(&library)
            .meeting(id)
            .unwrap()
            .audio_deleted_at
            .is_none()
    );
}

#[test]
fn a_failed_deletion_is_not_queued_again_on_its_own() {
    let (_dir, _clock, mut library) = library();
    let id = transcribed(&mut library);
    let job = library.enqueue_job(JobKind::DeleteAudio, id).unwrap();
    library.claim_job(&[JobKind::DeleteAudio]).unwrap();
    library
        .fail_job(job.id, "audio_delete_failed", "permission denied", false)
        .unwrap();
    assert!(library.audio_due_for_deletion().unwrap().is_empty());
    // The user can still retry it, and the job deletes the audio.
    assert!(library.is_audio_due(id).unwrap());
}

#[test]
fn trashed_meetings_wait_until_restored() {
    let (_dir, _clock, mut library) = library();
    let id = transcribed(&mut library);
    library.trash(id).unwrap();
    assert!(library.audio_due_for_deletion().unwrap().is_empty());
    library.restore(id).unwrap();
    assert_eq!(library.audio_due_for_deletion().unwrap(), [id]);
}
