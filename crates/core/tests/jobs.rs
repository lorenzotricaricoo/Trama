//! Job queue and runner tests, headless: a real library on disk and the real worker thread.

mod common;

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use common::{DAY, ManualClock, library};
use tempfile::TempDir;
use trama_core::jobs::*;
use trama_core::storage::*;

const PATIENCE: Duration = Duration::from_secs(10);

fn meeting(library: &mut Library) -> Id {
    library.create_meeting("it", ManualClock::START).unwrap().id
}

/// Events from a runner, and a way to wait for one.
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

    /// Waits for an event matching `want`, returning every event seen up to it.
    #[track_caller]
    fn wait_for(&self, want: impl Fn(&JobEvent) -> bool) -> Vec<JobEvent> {
        let deadline = Instant::now() + PATIENCE;
        let mut seen = Vec::new();
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let Ok(event) = self.0.recv_timeout(left) else {
                panic!("event not received; seen: {seen:#?}");
            };
            let done = want(&event);
            seen.push(event);
            if done {
                return seen;
            }
        }
    }
}

fn state_is(id: Id, state: JobState) -> impl Fn(&JobEvent) -> bool {
    move |event| event.job.id == id && event.job.state == state && event.progress.is_none()
}

/// Simulates a transcription: ten steps, reporting progress, stopping when asked. Steps
/// after `pause_at` wait until `gate` opens, so a test can stop the core mid-way.
struct Steps {
    gate: Arc<AtomicBool>,
    pause_at: u32,
    runs: Arc<AtomicU32>,
}

impl JobHandler for Steps {
    fn kind(&self) -> JobKind {
        JobKind::Transcribe
    }

    fn run(&self, _job: &Job, ctx: &JobContext) -> Result<(), JobError> {
        self.runs.fetch_add(1, Ordering::SeqCst);
        for step in 1..=10 {
            while step > self.pause_at && !self.gate.load(Ordering::SeqCst) {
                if ctx.should_stop() {
                    return Err(JobError::stopped());
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            ctx.progress(step as f32 / 10.0);
        }
        Ok(())
    }
}

/// Fails with the given error the first `failures` times, then succeeds.
struct Flaky {
    failures: u32,
    error: JobError,
    runs: Arc<AtomicU32>,
}

impl JobHandler for Flaky {
    fn kind(&self) -> JobKind {
        JobKind::Summarize
    }

    fn run(&self, _job: &Job, _ctx: &JobContext) -> Result<(), JobError> {
        if self.runs.fetch_add(1, Ordering::SeqCst) < self.failures {
            Err(self.error.clone())
        } else {
            Ok(())
        }
    }
}

struct Panics;

impl JobHandler for Panics {
    fn kind(&self) -> JobKind {
        JobKind::Summarize
    }

    fn run(&self, _job: &Job, _ctx: &JobContext) -> Result<(), JobError> {
        panic!("boom")
    }
}

// --- The queue in the library ---

#[test]
fn enqueueing_the_same_job_twice_returns_the_first() {
    let (_dir, _, mut library) = library();
    let target = meeting(&mut library);
    let first = library.enqueue_job(JobKind::Transcribe, target).unwrap();
    let again = library.enqueue_job(JobKind::Transcribe, target).unwrap();
    assert_eq!(first.id, again.id);
    assert_eq!(first.state, JobState::Queued);
    assert_eq!(first.max_attempts, JOB_MAX_ATTEMPTS);
    let other = library.enqueue_job(JobKind::Summarize, target).unwrap();
    assert_ne!(other.id, first.id);
    assert_eq!(library.active_jobs().unwrap().len(), 2);

    let note = library.create_note("N", "").unwrap();
    assert!(matches!(
        library.enqueue_job(JobKind::Transcribe, note.id),
        Err(StorageError::Invalid(_))
    ));
}

#[test]
fn claims_due_jobs_in_order_and_only_known_kinds() {
    let (_dir, clock, mut library) = library();
    let a = meeting(&mut library);
    let b = meeting(&mut library);
    let first = library.enqueue_job(JobKind::Transcribe, a).unwrap();
    clock.advance(1);
    let second = library.enqueue_job(JobKind::Transcribe, b).unwrap();
    library.enqueue_job(JobKind::Summarize, a).unwrap();

    assert_eq!(library.claim_job(&[]).unwrap(), None);
    let claimed = library.claim_job(&[JobKind::Transcribe]).unwrap().unwrap();
    assert_eq!(claimed.id, first.id);
    assert_eq!(claimed.state, JobState::Running);
    assert_eq!(claimed.attempts, 1);
    assert_eq!(
        library
            .claim_job(&[JobKind::Transcribe])
            .unwrap()
            .unwrap()
            .id,
        second.id
    );
    assert_eq!(library.claim_job(&[JobKind::Transcribe]).unwrap(), None);
}

#[test]
fn retryable_failures_back_off_then_give_up() {
    let (_dir, clock, mut library) = library();
    let target = meeting(&mut library);
    let job = library.enqueue_job(JobKind::Transcribe, target).unwrap();
    let kinds = [JobKind::Transcribe];

    for attempt in 1..JOB_MAX_ATTEMPTS {
        let claimed = library.claim_job(&kinds).unwrap().unwrap();
        assert_eq!(claimed.attempts, attempt);
        let failed = library.fail_job(job.id, "busy", "detail", true).unwrap();
        assert_eq!(failed.state, JobState::Queued);
        assert_eq!(failed.error_code.as_deref(), Some("busy"));
        assert_eq!(failed.run_after, clock.now() + retry_delay(attempt));
        // Not due yet.
        assert_eq!(library.claim_job(&kinds).unwrap(), None);
        assert_eq!(
            library.next_job_due(&kinds).unwrap(),
            Some(failed.run_after)
        );
        clock.advance(retry_delay(attempt));
    }
    library.claim_job(&kinds).unwrap().unwrap();
    let failed = library.fail_job(job.id, "busy", "detail", true).unwrap();
    assert_eq!(failed.state, JobState::Failed);
    assert_eq!(failed.finished_at, Some(clock.now()));
    assert!(library.active_jobs().unwrap().is_empty());

    // The user retries it: all attempts again.
    let retried = library.retry_job(job.id).unwrap();
    assert_eq!((retried.state, retried.attempts), (JobState::Queued, 0));
}

#[test]
fn permanent_failures_and_cancellation_stop_at_once() {
    let (_dir, _, mut library) = library();
    let target = meeting(&mut library);
    let job = library.enqueue_job(JobKind::Transcribe, target).unwrap();
    library.claim_job(&[JobKind::Transcribe]).unwrap();
    let failed = library
        .fail_job(job.id, "model_missing", "", false)
        .unwrap();
    assert_eq!(failed.state, JobState::Failed);
    assert_eq!(failed.attempts, 1);

    let other = library.enqueue_job(JobKind::Summarize, target).unwrap();
    let cancelled = library.cancel_job(other.id).unwrap();
    assert_eq!(cancelled.state, JobState::Cancelled);
    assert!(matches!(
        library.cancel_job(other.id),
        Err(StorageError::Invalid(_))
    ));

    // A retry is refused while the same job is queued again.
    library.enqueue_job(JobKind::Summarize, target).unwrap();
    assert!(matches!(
        library.retry_job(other.id),
        Err(StorageError::Invalid(_))
    ));
}

#[test]
fn released_jobs_keep_their_attempts() {
    let (_dir, _, mut library) = library();
    let target = meeting(&mut library);
    let job = library.enqueue_job(JobKind::Transcribe, target).unwrap();
    library.claim_job(&[JobKind::Transcribe]).unwrap();
    let released = library.release_job(job.id).unwrap();
    assert_eq!((released.state, released.attempts), (JobState::Queued, 0));
}

#[test]
fn jobs_wait_while_their_source_is_in_the_trash_and_go_with_it() {
    let (_dir, clock, mut library) = library();
    let target = meeting(&mut library);
    library.enqueue_job(JobKind::Transcribe, target).unwrap();
    library.trash(target).unwrap();
    assert_eq!(library.claim_job(&[JobKind::Transcribe]).unwrap(), None);
    assert_eq!(library.next_job_due(&[JobKind::Transcribe]).unwrap(), None);
    assert!(matches!(
        library.enqueue_job(JobKind::Summarize, target),
        Err(StorageError::NotFound { .. })
    ));

    library.restore(target).unwrap();
    assert!(library.claim_job(&[JobKind::Transcribe]).unwrap().is_some());

    library.trash(target).unwrap();
    clock.advance(31 * DAY);
    library.purge_trash(clock.now()).unwrap();
    assert!(library.active_jobs().unwrap().is_empty());
}

// --- Recovery after a crash ---

#[test]
fn a_crash_leaves_nothing_stuck() {
    let dir = TempDir::new().unwrap();
    let clock = ManualClock::new();
    let (interrupted, recording, last_try) = {
        let mut library = Library::open_with_clock(dir.path(), Box::new(clock.clone())).unwrap();
        assert!(library.recovery().is_empty());
        let target = meeting(&mut library);
        library
            .set_meeting_status(target, MeetingStatus::Processing, None)
            .unwrap();
        let interrupted = library.enqueue_job(JobKind::Transcribe, target).unwrap();
        library.claim_job(&[JobKind::Transcribe]).unwrap();

        let last_try = library.enqueue_job(JobKind::Summarize, target).unwrap();
        for attempt in 1..JOB_MAX_ATTEMPTS {
            library.claim_job(&[JobKind::Summarize]).unwrap().unwrap();
            library.fail_job(last_try.id, "busy", "", true).unwrap();
            clock.advance(retry_delay(attempt));
        }
        library.claim_job(&[JobKind::Summarize]).unwrap().unwrap();

        let recording = meeting(&mut library);
        // The process dies here: two jobs running, a meeting recording.
        (interrupted.id, recording, last_try.id)
    };

    let library = Library::open(dir.path()).unwrap();
    let recovery = library.recovery();
    assert_eq!(recovery.requeued_jobs, [interrupted]);
    assert_eq!(recovery.failed_jobs, [last_try]);
    assert!(recovery.interrupted_recordings.contains(&recording));

    assert_eq!(library.job(interrupted).unwrap().state, JobState::Queued);
    let gave_up = library.job(last_try).unwrap();
    assert_eq!(gave_up.state, JobState::Failed);
    assert_eq!(gave_up.error_code.as_deref(), Some(JOB_ERROR_INTERRUPTED));
    let meeting = library.meeting(recording).unwrap();
    assert_eq!(meeting.status, MeetingStatus::Failed);
    assert_eq!(
        meeting.error_code.as_deref(),
        Some(meeting_error::RECORDING_INTERRUPTED)
    );
    drop(library);

    assert!(Library::open(dir.path()).unwrap().recovery().is_empty());
}

// --- The runner ---

#[test]
fn jobs_survive_stopping_the_core_mid_way() {
    let dir = TempDir::new().unwrap();
    let runs = Arc::new(AtomicU32::new(0));
    let (targets, jobs) = {
        let library = Arc::new(Mutex::new(Library::open(dir.path()).unwrap()));
        let targets: Vec<Id> = (0..3).map(|_| meeting(&mut lock(&library))).collect();
        let (sink, events) = Events::sink();
        let runner = Runner::start(
            Arc::clone(&library),
            vec![Arc::new(Steps {
                gate: Arc::new(AtomicBool::new(false)),
                pause_at: 5,
                runs: Arc::clone(&runs),
            })],
            sink,
        );
        let jobs: Vec<Job> = targets
            .iter()
            .map(|t| runner.enqueue(JobKind::Transcribe, *t).unwrap())
            .collect();

        // The first job reaches 50%, then waits on the gate: stop the core there.
        let seen = events.wait_for(|e| e.job.id == jobs[0].id && e.progress == Some(0.5));
        let progress: Vec<f32> = seen
            .iter()
            .filter(|e| e.job.id == jobs[0].id)
            .filter_map(|e| e.progress)
            .collect();
        assert_eq!(progress, [0.0, 0.1, 0.2, 0.3, 0.4, 0.5]);
        runner.stop();
        events.wait_for(state_is(jobs[0].id, JobState::Queued));
        drop(runner);
        let library = lock(&library);
        let stopped = library.job(jobs[0].id).unwrap();
        assert_eq!((stopped.state, stopped.attempts), (JobState::Queued, 0));
        (targets, jobs)
    };
    assert_eq!(runs.load(Ordering::SeqCst), 1);

    // Restart: everything completes.
    let library = Arc::new(Mutex::new(Library::open(dir.path()).unwrap()));
    let (sink, events) = Events::sink();
    let runner = Runner::start(
        Arc::clone(&library),
        vec![Arc::new(Steps {
            gate: Arc::new(AtomicBool::new(true)),
            pause_at: 0,
            runs: Arc::clone(&runs),
        })],
        sink,
    );
    let seen = events.wait_for(|e| e.job.id == jobs[2].id && e.job.state == JobState::Done);
    let finished: Vec<Id> = seen
        .iter()
        .filter(|e| e.job.state == JobState::Done)
        .map(|e| e.job.id)
        .collect();
    // The interrupted job keeps its place at the head of the queue.
    assert_eq!(finished, jobs.iter().map(|j| j.id).collect::<Vec<_>>());
    drop(runner);
    let library = lock(&library);
    for (job, target) in jobs.iter().zip(&targets) {
        let done = library.job(job.id).unwrap();
        assert_eq!(
            (done.state, done.attempts, done.target_id),
            (JobState::Done, 1, *target)
        );
    }
    assert_eq!(runs.load(Ordering::SeqCst), 4);
}

#[test]
fn runner_retries_after_the_backoff() {
    let (_dir, clock, mut library) = library();
    let target = meeting(&mut library);
    let library = Arc::new(Mutex::new(library));
    let runs = Arc::new(AtomicU32::new(0));
    let (sink, events) = Events::sink();
    let runner = Runner::start(
        Arc::clone(&library),
        vec![Arc::new(Flaky {
            failures: 1,
            error: JobError::retryable("provider_unavailable", "503"),
            runs: Arc::clone(&runs),
        })],
        sink,
    );
    let job = runner.enqueue(JobKind::Summarize, target).unwrap();
    let seen = events.wait_for(|e| e.job.id == job.id && e.job.error_code.is_some());
    let retry = seen.last().unwrap().job.clone();
    assert_eq!(retry.state, JobState::Queued);
    assert_eq!(retry.error_code.as_deref(), Some("provider_unavailable"));
    assert_eq!(retry.run_after, clock.now() + retry_delay(1));

    clock.advance(retry_delay(1));
    runner.notify();
    events.wait_for(state_is(job.id, JobState::Done));
    assert_eq!(runs.load(Ordering::SeqCst), 2);
    let done = lock(&library).job(job.id).unwrap();
    assert_eq!(done.error_code, None);
}

#[test]
fn errors_are_never_silent() {
    let (_dir, _, mut library) = library();
    let a = meeting(&mut library);
    let library = Arc::new(Mutex::new(library));

    let (sink, events) = Events::sink();
    let runner = Runner::start(
        Arc::clone(&library),
        vec![Arc::new(Flaky {
            failures: u32::MAX,
            error: JobError::permanent("unsupported_language", "xx"),
            runs: Arc::new(AtomicU32::new(0)),
        })],
        sink,
    );
    let job = runner.enqueue(JobKind::Summarize, a).unwrap();
    let failed = events.wait_for(state_is(job.id, JobState::Failed));
    let failed = &failed.last().unwrap().job;
    assert_eq!(failed.error_code.as_deref(), Some("unsupported_language"));
    assert_eq!(failed.error_detail.as_deref(), Some("xx"));
    drop(runner);

    let retried = lock(&library).retry_job(job.id).unwrap();
    let (sink, events) = Events::sink();
    let runner = Runner::start(Arc::clone(&library), vec![Arc::new(Panics)], sink);
    runner.notify();
    let failed = events.wait_for(state_is(retried.id, JobState::Failed));
    let failed = &failed.last().unwrap().job;
    assert_eq!(failed.error_code.as_deref(), Some(error_code::PANICKED));
    assert_eq!(failed.error_detail.as_deref(), Some("boom"));
}

#[test]
fn cancelling_stops_a_running_job() {
    let (_dir, _, mut library) = library();
    let target = meeting(&mut library);
    let library = Arc::new(Mutex::new(library));
    let (sink, events) = Events::sink();
    let runner = Runner::start(
        Arc::clone(&library),
        vec![Arc::new(Steps {
            gate: Arc::new(AtomicBool::new(false)),
            pause_at: 2,
            runs: Arc::new(AtomicU32::new(0)),
        })],
        sink,
    );
    let job = runner.enqueue(JobKind::Transcribe, target).unwrap();
    events.wait_for(|e| e.job.id == job.id && e.progress == Some(0.2));
    assert_eq!(runner.cancel(job.id).unwrap().state, JobState::Cancelled);
    events.wait_for(state_is(job.id, JobState::Cancelled));
    drop(runner);
    assert_eq!(
        lock(&library).job(job.id).unwrap().state,
        JobState::Cancelled
    );
}

#[test]
fn delete_audio_removes_files_and_keeps_the_transcript() {
    let (dir, _, mut library) = library();
    let target = meeting(&mut library);
    let audio_dir = Library::meeting_audio_dir(target);
    std::fs::create_dir_all(dir.path().join(&audio_dir)).unwrap();
    for role in [Role::Me, Role::Others] {
        let audio_path = audio_dir.join(format!("{}.wav", role.as_str()));
        std::fs::write(dir.path().join(&audio_path), b"RIFF").unwrap();
        library
            .add_track(
                target,
                &NewTrack {
                    role,
                    audio_path,
                    timing_path: None,
                    device: None,
                },
            )
            .unwrap();
    }
    let version = library
        .add_transcription(
            target,
            "engine",
            &[NewSegment {
                role: Role::Me,
                start_ms: 0,
                end_ms: 1,
                text: "resta".into(),
            }],
        )
        .unwrap();

    let library = Arc::new(Mutex::new(library));
    let (sink, events) = Events::sink();
    let runner = Runner::start(Arc::clone(&library), vec![Arc::new(DeleteAudio)], sink);
    let job = runner.enqueue(JobKind::DeleteAudio, target).unwrap();
    events.wait_for(state_is(job.id, JobState::Done));
    drop(runner);

    assert!(!dir.path().join(&audio_dir).exists());
    let library = lock(&library);
    assert!(library.meeting(target).unwrap().audio_deleted_at.is_some());
    assert_eq!(library.segments(version.id).unwrap()[0].text, "resta");
}
