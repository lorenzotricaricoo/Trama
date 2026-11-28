//! Bridge between the core's background jobs and the frontend: Tauri commands that wrap
//! the core API, and the `job` event that carries every state change and progress step.

use std::sync::Arc;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, Runtime, State};
use trama_core::jobs::{JobEvent, Runner, SharedLibrary, lock};
use trama_core::storage::{Id, Job};

/// Name of the event sent to the frontend for every job change.
pub const JOB_EVENT: &str = "job";

/// The library and the job runner, owned by the app for its whole life.
pub struct Core {
    pub library: SharedLibrary,
    pub runner: Runner,
}

/// Code of a command that needs the library when none could be opened at start.
pub const NO_LIBRARY: &str = "storage";

/// The library and the runner; `storage` if no library could be opened at start, so a
/// command fails with a code instead of a panic.
pub fn core_state<R: Runtime>(app: &AppHandle<R>) -> Result<State<'_, Core>, String> {
    app.try_state::<Core>().ok_or_else(|| NO_LIBRARY.to_owned())
}

/// A job as the frontend sees it.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct JobPayload {
    pub id: String,
    pub kind: &'static str,
    pub target_id: String,
    pub state: &'static str,
    pub attempts: u32,
    pub max_attempts: u32,
    /// Error code for the interface to translate.
    pub error_code: Option<String>,
    /// Progress of a running job, from 0 to 1, when known.
    pub progress: Option<f32>,
}

impl JobPayload {
    pub fn new(job: &Job, progress: Option<f32>) -> Self {
        Self {
            id: job.id.to_string(),
            kind: job.kind.as_str(),
            target_id: job.target_id.to_string(),
            state: job.state.as_str(),
            attempts: job.attempts,
            max_attempts: job.max_attempts,
            error_code: job.error_code.clone(),
            progress,
        }
    }
}

/// Forwards job events to every window.
pub fn event_sink<R: Runtime>(app: AppHandle<R>) -> Arc<dyn trama_core::jobs::EventSink> {
    Arc::new(move |event: JobEvent| {
        if let Err(err) = app.emit(JOB_EVENT, JobPayload::new(&event.job, event.progress)) {
            eprintln!("trama: cannot send a job event to the frontend: {err}");
        }
        crate::recording::on_job(&app, &event.job);
        crate::activity::on_job(&app, &event.job, event.progress);
    })
}

fn parse_id(id: &str) -> Result<Id, String> {
    Id::parse_str(id).map_err(|_| format!("invalid job id {id:?}"))
}

/// Queued and running jobs, to show progress after the window opens.
#[tauri::command]
pub fn active_jobs(core: State<'_, Core>) -> Result<Vec<JobPayload>, String> {
    let jobs = lock(&core.library)
        .active_jobs()
        .map_err(|err| err.to_string())?;
    Ok(jobs.iter().map(|job| JobPayload::new(job, None)).collect())
}

/// Queues a failed or cancelled job again.
#[tauri::command]
pub fn retry_job(core: State<'_, Core>, id: String) -> Result<JobPayload, String> {
    let job = core
        .runner
        .retry(parse_id(&id)?)
        .map_err(|err| err.to_string())?;
    Ok(JobPayload::new(&job, None))
}

/// Cancels a queued or running job.
#[tauri::command]
pub fn cancel_job(core: State<'_, Core>, id: String) -> Result<JobPayload, String> {
    let job = core
        .runner
        .cancel(parse_id(&id)?)
        .map_err(|err| err.to_string())?;
    Ok(JobPayload::new(&job, None))
}

#[cfg(test)]
mod tests {
    use super::*;
    use trama_core::storage::{JobKind, JobState};

    #[test]
    fn payload_uses_the_frontend_names() {
        let job = Job {
            id: Id::now_v7(),
            kind: JobKind::Transcribe,
            target_id: Id::now_v7(),
            state: JobState::Running,
            attempts: 1,
            max_attempts: 5,
            run_after: 0,
            error_code: None,
            error_detail: Some("not sent".into()),
            created_at: 0,
            updated_at: 0,
            finished_at: None,
        };
        let json = serde_json::to_value(JobPayload::new(&job, Some(0.62))).unwrap();
        assert_eq!(json["kind"], "transcribe");
        assert_eq!(json["state"], "running");
        assert_eq!(json["targetId"], job.target_id.to_string());
        assert_eq!(json["maxAttempts"], 5);
        assert!((json["progress"].as_f64().unwrap() - 0.62).abs() < 1e-6);
        assert!(json.get("errorDetail").is_none());
    }
}
