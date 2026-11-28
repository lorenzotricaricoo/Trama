//! What Trama is doing in the background: the recording and the meetings being transcribed
//! or summarized. The menu bar item shows it, quitting asks for confirmation while it
//! lasts, and the Mac does not go to sleep while recording or processing (`caffeinate`
//! holds an idle-sleep assertion for as long as it runs).

use std::collections::BTreeMap;
use std::process::{Child, Command};
use std::sync::{Mutex, MutexGuard, PoisonError};

use tauri::{AppHandle, Manager, Runtime};
use trama_core::storage::{Id, Job, JobKind, JobState};

/// A meeting being transcribed or summarized.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Processing {
    pub kind: JobKind,
    pub target_id: Id,
    pub running: bool,
    /// Waiting to try again after a failure: it is not about to run.
    pub retrying: bool,
    /// From 0 to 1, while running, when known.
    pub progress: Option<f32>,
}

/// The recording and the processing jobs, by job id (oldest first).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct State {
    pub recording: bool,
    pub jobs: BTreeMap<Id, Processing>,
}

/// What keeps quitting from being silent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Busy {
    Recording,
    Processing,
}

/// What a change did to the state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change {
    None,
    /// Only the progress of a running job moved.
    Progress,
    /// Something to rebuild the menu for.
    Structure,
}

impl State {
    /// Takes in a job change.
    pub fn apply_job(&mut self, job: &Job, progress: Option<f32>) -> Change {
        if !matches!(job.kind, JobKind::Transcribe | JobKind::Summarize) {
            return Change::None;
        }
        let active = matches!(job.state, JobState::Queued | JobState::Running);
        if !active {
            return if self.jobs.remove(&job.id).is_some() {
                Change::Structure
            } else {
                Change::None
            };
        }
        let running = job.state == JobState::Running;
        let next = Processing {
            kind: job.kind,
            target_id: job.target_id,
            running,
            retrying: job.state == JobState::Queued && job.attempts > 0,
            progress: if running { progress } else { None },
        };
        match self.jobs.insert(job.id, next) {
            Some(before)
                if Processing {
                    progress: next.progress,
                    ..before
                } == next =>
            {
                if before.progress == next.progress {
                    Change::None
                } else {
                    Change::Progress
                }
            }
            _ => Change::Structure,
        }
    }

    /// Whether the Mac must stay awake: recording, or a meeting being processed now.
    pub fn keeps_awake(&self) -> bool {
        self.recording || self.jobs.values().any(|job| job.running)
    }

    /// What quitting would interrupt: the recording first, then a meeting being processed
    /// or about to be. A job waiting to retry resumes at the next start without loss.
    pub fn busy(&self) -> Option<Busy> {
        if self.recording {
            Some(Busy::Recording)
        } else if self.jobs.values().any(|job| !job.retrying) {
            Some(Busy::Processing)
        } else {
            None
        }
    }
}

/// A progress from 0 to 1 in whole percent, never 100 before the end.
pub fn percent(progress: f32) -> u32 {
    // Clamped to 0..=99 first, so the cast cannot truncate.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let value = (progress.clamp(0.0, 0.99) * 100.0).floor() as u32;
    value
}

/// The state and the process that keeps the Mac awake.
#[derive(Default)]
pub struct Activity {
    state: Mutex<State>,
    awake: Mutex<Option<Child>>,
}

impl Activity {
    pub fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Starts or stops `caffeinate`, which prevents idle sleep while it runs and ends with
    /// Trama (`-w`), even after a crash.
    fn keep_awake(&self, on: bool) {
        let mut awake = self.awake.lock().unwrap_or_else(PoisonError::into_inner);
        match (on, awake.as_mut()) {
            (true, None) => {
                match Command::new("/usr/bin/caffeinate")
                    .args(["-i", "-w", &std::process::id().to_string()])
                    .spawn()
                {
                    Ok(child) => *awake = Some(child),
                    Err(err) => eprintln!("trama: cannot keep the Mac awake: {err}"),
                }
            }
            (false, Some(child)) => {
                if let Err(err) = child.kill() {
                    eprintln!("trama: cannot let the Mac sleep again: {err}");
                }
                let _ = child.wait();
                *awake = None;
            }
            _ => {}
        }
    }
}

/// Follows a change: the sleep assertion, then the menu bar item.
fn changed<R: Runtime>(app: &AppHandle<R>, change: Change) {
    let activity = app.state::<Activity>();
    let awake = activity.state().keeps_awake();
    activity.keep_awake(awake);
    match change {
        Change::None => {}
        Change::Progress => crate::tray::progress(app),
        Change::Structure => crate::tray::refresh(app),
    }
}

/// Takes in a job event.
pub fn on_job<R: Runtime>(app: &AppHandle<R>, job: &Job, progress: Option<f32>) {
    let Some(activity) = app.try_state::<Activity>() else {
        return;
    };
    let change = activity.state().apply_job(job, progress);
    if change != Change::None {
        changed(app, change);
    }
}

/// Takes in the jobs already queued at start.
pub fn seed<R: Runtime>(app: &AppHandle<R>, jobs: &[Job]) {
    let activity = app.state::<Activity>();
    let mut changed_any = false;
    for job in jobs {
        changed_any |= activity.state().apply_job(job, None) != Change::None;
    }
    if changed_any {
        changed(app, Change::Structure);
    }
}

/// Takes in the start or the end of a recording.
pub fn set_recording<R: Runtime>(app: &AppHandle<R>, recording: bool) {
    let Some(activity) = app.try_state::<Activity>() else {
        return;
    };
    let before = std::mem::replace(&mut activity.state().recording, recording);
    if before != recording {
        changed(app, Change::Structure);
    }
}

/// Lets the Mac sleep again when Trama quits.
pub fn stop<R: Runtime>(app: &AppHandle<R>) {
    if let Some(activity) = app.try_state::<Activity>() {
        activity.keep_awake(false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn job(kind: JobKind, state: JobState, attempts: u32) -> Job {
        Job {
            id: Id::now_v7(),
            kind,
            target_id: Id::now_v7(),
            state,
            attempts,
            max_attempts: 5,
            run_after: 0,
            error_code: None,
            error_detail: None,
            created_at: 0,
            updated_at: 0,
            finished_at: None,
        }
    }

    #[test]
    fn processing_follows_the_jobs() {
        let mut state = State::default();
        let mut transcribe = job(JobKind::Transcribe, JobState::Queued, 0);
        assert_eq!(state.apply_job(&transcribe, None), Change::Structure);
        assert_eq!(state.busy(), Some(Busy::Processing));
        // Queued, not running yet: the Mac may still sleep.
        assert!(!state.keeps_awake());

        transcribe.state = JobState::Running;
        transcribe.attempts = 1;
        assert_eq!(state.apply_job(&transcribe, None), Change::Structure);
        assert!(state.keeps_awake());
        assert_eq!(state.apply_job(&transcribe, Some(0.42)), Change::Progress);
        assert_eq!(state.apply_job(&transcribe, Some(0.42)), Change::None);

        transcribe.state = JobState::Done;
        assert_eq!(state.apply_job(&transcribe, None), Change::Structure);
        assert_eq!(state, State::default());
        assert_eq!(state.busy(), None);
        // Gone already: nothing changes.
        assert_eq!(state.apply_job(&transcribe, None), Change::None);
    }

    #[test]
    fn other_jobs_are_not_processing() {
        let mut state = State::default();
        let delete = job(JobKind::DeleteAudio, JobState::Running, 1);
        assert_eq!(state.apply_job(&delete, None), Change::None);
        assert!(state.jobs.is_empty());
    }

    #[test]
    fn a_job_waiting_to_retry_does_not_ask_before_quitting() {
        let mut state = State::default();
        let summary = job(JobKind::Summarize, JobState::Queued, 2);
        state.apply_job(&summary, None);
        assert_eq!(state.busy(), None);
        assert!(!state.keeps_awake());
    }

    #[test]
    fn the_recording_comes_first() {
        let mut state = State {
            recording: true,
            ..State::default()
        };
        state.apply_job(&job(JobKind::Transcribe, JobState::Running, 1), None);
        assert_eq!(state.busy(), Some(Busy::Recording));
        assert!(state.keeps_awake());
    }

    #[test]
    fn percent_is_whole_and_never_100_early() {
        assert_eq!(percent(0.0), 0);
        assert_eq!(percent(0.625), 62);
        assert_eq!(percent(1.0), 99);
        assert_eq!(percent(-1.0), 0);
        assert_eq!(percent(f32::NAN), 0);
    }
}
