//! Recording a meeting: Tauri commands to start and stop the capture and to pin notes to
//! the current minute, the floating recording panel, and the "meeting ready"
//! notification once transcription and summary are done.
//!
//! - Start: creates the meeting, opens the two tracks (asking for the microphone the first
//!   time) and opens the panel. If the capture cannot start, the empty meeting is removed
//!   and the error code goes back to the interface.
//! - While recording: levels go to the panel (`recording-level`), warnings and lost devices
//!   to every window (`recording-notice`).
//! - Stop: finishes the files, marks the meeting as processing and queues its
//!   transcription; the summary follows (see `trama_core::summary`). When the summary is
//!   done, or the transcription fails for good, a system notification says so.
//! - Quitting the app while recording stops and keeps the recording.
//!
//! The panel is hidden from screen sharing (`NSWindow.sharingType = none`). Apps that
//! capture with ScreenCaptureKit on macOS 15 and later ignore that setting, so the panel
//! can also shrink to a small pill.

use std::collections::HashSet;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use serde::Serialize;
use tauri::{
    AppHandle, Emitter, LogicalSize, Manager, Runtime, State, WebviewUrl, WebviewWindowBuilder,
};
use tauri_plugin_notification::NotificationExt;
use trama_core::capture::{
    CaptureBackend, CaptureError, CaptureEvent, CaptureObserver, Level, Recording, RecordingOptions,
};
use trama_core::jobs::lock;
use trama_core::storage::{Id, Job, JobKind, JobState, MeetingStatus, RecordingNote, Timestamp};

use crate::jobs::Core;

/// Label of the recording panel; the frontend picks the panel view from it.
pub const RECORDING_WINDOW: &str = "recording";

/// Sent to every window when a recording starts or stops.
pub const RECORDING_STATE_EVENT: &str = "recording-state";

/// Sent to the panel about 20 times a second per track.
pub const RECORDING_LEVEL_EVENT: &str = "recording-level";

/// Sent to every window for a warning or a device change during the recording.
pub const RECORDING_NOTICE_EVENT: &str = "recording-notice";

/// Error codes of this module, on top of the capture codes
/// (`trama_core::capture::error_code`). The interface translates them.
pub mod error_code {
    /// A recording is already running or starting.
    pub const ALREADY_RECORDING: &str = "already_recording";
    /// No recording is running.
    pub const NOT_RECORDING: &str = "not_recording";
    /// The library could not be read or written.
    pub const STORAGE_FAILED: &str = "storage_failed";
    /// Recording is not available on this system.
    #[cfg(not(target_os = "macos"))]
    pub const UNSUPPORTED: &str = "recording_unsupported";
    /// A note without text.
    pub const EMPTY_NOTE: &str = "empty_note";
}

/// Size of the panel, in logical pixels.
const PANEL_SIZE: (f64, f64) = (360.0, 380.0);

/// Size of the panel reduced to a pill.
const PILL_SIZE: (f64, f64) = (220.0, 52.0);

/// Distance of the panel from the top right corner of the screen.
const PANEL_MARGIN: f64 = 16.0;

/// Longest note, in characters.
const MAX_NOTE_CHARS: usize = 2000;

/// Texts of the shell shown outside the webviews, taken from the catalogs at start.
#[derive(Debug, Clone, Default)]
pub struct Texts {
    pub panel_title: String,
    pub ready_title: String,
    pub ready_body: String,
    pub failed_title: String,
    pub failed_body: String,
}

/// Where the recording stands.
enum Phase {
    Idle,
    /// Opening the streams, possibly waiting for the microphone prompt.
    Starting,
    Recording(Box<Active>),
    /// Finishing the files.
    Stopping,
}

struct Active {
    recording: Recording,
    backend: Arc<dyn CaptureBackend>,
    meeting_id: Id,
    started_at: Timestamp,
}

impl Active {
    /// Time since the recording started, on the capture clock, so notes line up with the
    /// audio.
    fn elapsed_ms(&self) -> u64 {
        self.backend
            .now_ns()
            .saturating_sub(self.recording.start_ns())
            / 1_000_000
    }

    fn status(&self) -> RecordingStatus {
        RecordingStatus {
            meeting_id: self.meeting_id.to_string(),
            started_at: self.started_at,
            elapsed_ms: self.elapsed_ms(),
        }
    }
}

/// The recording and the meetings waiting for their notification.
pub struct Recorder {
    phase: Mutex<Phase>,
    /// Meetings stopped while the app was open whose notification is still due.
    awaiting: Mutex<HashSet<Id>>,
    /// In the interface language, which can change while the app runs.
    texts: Mutex<Texts>,
}

impl Default for Recorder {
    fn default() -> Self {
        Self {
            phase: Mutex::new(Phase::Idle),
            awaiting: Mutex::new(HashSet::new()),
            texts: Mutex::new(Texts::default()),
        }
    }
}

impl Recorder {
    /// Sets the texts in the interface language.
    pub fn set_texts(&self, texts: Texts) {
        *self.texts.lock().unwrap_or_else(PoisonError::into_inner) = texts;
    }

    fn texts(&self) -> Texts {
        self.texts
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    fn phase(&self) -> MutexGuard<'_, Phase> {
        self.phase.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn awaiting(&self) -> MutexGuard<'_, HashSet<Id>> {
        self.awaiting.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// A running recording as the frontend sees it.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RecordingStatus {
    pub meeting_id: String,
    pub started_at: Timestamp,
    /// Time recorded so far, for the timer.
    pub elapsed_ms: u64,
}

/// Payload of [`RECORDING_STATE_EVENT`].
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RecordingStatePayload {
    /// The running recording; `None` once it stopped.
    pub recording: Option<RecordingStatus>,
    /// The meeting that was just stopped.
    pub stopped_meeting_id: Option<String>,
}

/// Payload of [`RECORDING_LEVEL_EVENT`].
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LevelPayload {
    /// `microphone` (the user) or `system` (the others).
    pub source: &'static str,
    pub peak_db: f32,
    pub rms_db: f32,
}

/// Payload of [`RECORDING_NOTICE_EVENT`].
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NoticePayload {
    pub source: &'static str,
    /// `started` (first audio), `device_changed`, `interrupted` or `problem`.
    pub kind: &'static str,
    /// Capture error code for `interrupted` and `problem`.
    pub code: Option<&'static str>,
}

/// A note taken while recording.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RecordingNotePayload {
    pub id: String,
    pub meeting_id: String,
    pub at_ms: i64,
    pub text: String,
}

impl From<&RecordingNote> for RecordingNotePayload {
    fn from(note: &RecordingNote) -> Self {
        Self {
            id: note.id.to_string(),
            meeting_id: note.meeting_id.to_string(),
            at_ms: note.at_ms,
            text: note.text.clone(),
        }
    }
}

impl NoticePayload {
    fn from_event(event: &CaptureEvent) -> Option<Self> {
        let (source, kind, code) = match event {
            CaptureEvent::DeviceOpened { .. } => return None,
            CaptureEvent::TrackStarted { source, .. } => (source, "started", None),
            CaptureEvent::DeviceChanged { source, .. } => (source, "device_changed", None),
            CaptureEvent::TrackInterrupted { source, code, .. } => {
                (source, "interrupted", Some(*code))
            }
            CaptureEvent::TrackProblem { source, code, .. } => (source, "problem", Some(*code)),
        };
        Some(Self {
            source: source.as_str(),
            kind,
            code,
        })
    }
}

/// Forwards levels and events to the windows. Called from the capture threads: emitting
/// only queues a message for the webview.
struct Forwarder<R: Runtime> {
    app: AppHandle<R>,
}

impl<R: Runtime> CaptureObserver for Forwarder<R> {
    fn level(&self, level: Level) {
        let payload = LevelPayload {
            source: level.source.as_str(),
            peak_db: level.peak_dbfs(),
            rms_db: level.rms_dbfs(),
        };
        // The panel may be closing; a lost level is harmless.
        let _ = self
            .app
            .emit_to(RECORDING_WINDOW, RECORDING_LEVEL_EVENT, payload);
    }

    fn event(&self, event: CaptureEvent) {
        if let CaptureEvent::TrackInterrupted { detail, .. }
        | CaptureEvent::TrackProblem { detail, .. } = &event
        {
            eprintln!("trama: recording: {event:?} ({detail})");
        }
        if let Some(payload) = NoticePayload::from_event(&event)
            && let Err(err) = self.app.emit(RECORDING_NOTICE_EVENT, payload)
        {
            eprintln!("trama: cannot send a recording notice: {err}");
        }
    }
}

fn storage_error(err: impl std::fmt::Display) -> String {
    eprintln!("trama: recording: storage error: {err}");
    error_code::STORAGE_FAILED.to_owned()
}

fn capture_error(err: &CaptureError) -> String {
    eprintln!("trama: recording: {err}");
    err.code.to_owned()
}

#[cfg(target_os = "macos")]
fn backend() -> Result<Arc<dyn CaptureBackend>, String> {
    Ok(Arc::new(trama_core::capture::macos::MacBackend::new()))
}

#[cfg(not(target_os = "macos"))]
fn backend() -> Result<Arc<dyn CaptureBackend>, String> {
    Err(error_code::UNSUPPORTED.to_owned())
}

/// Opens the panel at the top right of the main screen, or shows it again.
fn open_panel<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    if let Some(window) = app.get_webview_window(RECORDING_WINDOW) {
        window.show()?;
        return window.set_focus();
    }
    let title = app.state::<Recorder>().texts().panel_title;
    let mut builder =
        WebviewWindowBuilder::new(app, RECORDING_WINDOW, WebviewUrl::App("index.html".into()))
            .title(&title)
            .inner_size(PANEL_SIZE.0, PANEL_SIZE.1)
            .resizable(false)
            .maximizable(false)
            .minimizable(false)
            .decorations(false)
            .transparent(true)
            .shadow(true)
            .always_on_top(true)
            .visible_on_all_workspaces(true)
            .skip_taskbar(true)
            // Hidden from screen sharing and screenshots where the system honors it.
            .content_protected(true);
    if let Some(monitor) = app.primary_monitor()? {
        let scale = monitor.scale_factor();
        let area = monitor.work_area();
        let right = f64::from(area.position.x) / scale + f64::from(area.size.width) / scale;
        let top = f64::from(area.position.y) / scale;
        builder = builder.position(right - PANEL_SIZE.0 - PANEL_MARGIN, top + PANEL_MARGIN);
    }
    #[cfg(target_os = "macos")]
    {
        use tauri::window::{Effect, EffectState, EffectsBuilder};
        builder = builder.effects(
            EffectsBuilder::new()
                .effect(Effect::HudWindow)
                .state(EffectState::Active)
                .radius(16.0)
                .build(),
        );
    }
    builder.build()?;
    Ok(())
}

/// Closes the panel without asking (it only hides when the user closes it).
fn close_panel<R: Runtime>(app: &AppHandle<R>) {
    if let Some(window) = app.get_webview_window(RECORDING_WINDOW)
        && let Err(err) = window.destroy()
    {
        eprintln!("trama: cannot close the recording panel: {err}");
    }
}

fn emit_state<R: Runtime>(app: &AppHandle<R>, payload: RecordingStatePayload) {
    if let Err(err) = app.emit(RECORDING_STATE_EVENT, payload) {
        eprintln!("trama: cannot send the recording state: {err}");
    }
}

/// Starts recording a new meeting and opens the panel. Returns once both tracks are
/// recording; the first time it waits for the user to answer the microphone prompt.
#[tauri::command]
pub async fn start_recording<R: Runtime>(app: AppHandle<R>) -> Result<RecordingStatus, String> {
    {
        let recorder = app.state::<Recorder>();
        let mut phase = recorder.phase();
        if !matches!(*phase, Phase::Idle) {
            return Err(error_code::ALREADY_RECORDING.to_owned());
        }
        *phase = Phase::Starting;
    }
    let result = start(&app).await;
    let recorder = app.state::<Recorder>();
    match result {
        Ok(active) => {
            let status = active.status();
            *recorder.phase() = Phase::Recording(Box::new(active));
            if let Err(err) = open_panel(&app) {
                // The recording goes on; the main window still shows it and can stop it.
                eprintln!("trama: cannot open the recording panel: {err}");
            }
            emit_state(
                &app,
                RecordingStatePayload {
                    recording: Some(status.clone()),
                    stopped_meeting_id: None,
                },
            );
            Ok(status)
        }
        Err(code) => {
            *recorder.phase() = Phase::Idle;
            Err(code)
        }
    }
}

async fn start<R: Runtime>(app: &AppHandle<R>) -> Result<Active, String> {
    let backend = backend()?;
    // Without a library (it could not be opened at start) there is nowhere to record.
    let core = app
        .try_state::<Core>()
        .ok_or_else(|| error_code::STORAGE_FAILED.to_owned())?;
    let (meeting_id, started_at, root) = {
        let mut library = lock(&core.library);
        let language = library.meeting_language().map_err(storage_error)?;
        let started_at = library.now();
        let meeting = library
            .create_meeting(&language, started_at)
            .map_err(storage_error)?;
        (meeting.id, started_at, library.root().to_path_buf())
    };
    let observer = Arc::new(Forwarder { app: app.clone() });
    let options = RecordingOptions::new(root, meeting_id);
    let opened = {
        let backend = Arc::clone(&backend);
        tauri::async_runtime::spawn_blocking(move || Recording::start(backend, options, observer))
            .await
    };
    let recording = match opened {
        Ok(Ok(recording)) => recording,
        failed => {
            // Nothing was recorded: leave no empty meeting behind.
            if let Err(err) = lock(&core.library).discard_meeting(meeting_id) {
                eprintln!("trama: cannot remove the meeting that did not record: {err}");
            }
            return Err(match failed {
                Ok(Err(err)) => capture_error(&err),
                _ => trama_core::capture::error_code::INTERNAL.to_owned(),
            });
        }
    };
    if let Err(err) = recording.register_tracks(&mut lock(&core.library)) {
        // Without its tracks the meeting cannot be transcribed: stop at once.
        let code = storage_error(err);
        let _ = recording.stop();
        if let Err(err) = lock(&core.library).set_meeting_failed(meeting_id, &code) {
            eprintln!("trama: cannot mark the meeting as failed: {err}");
        }
        return Err(code);
    }
    Ok(Active {
        recording,
        backend,
        meeting_id,
        started_at,
    })
}

/// Takes the running recording, leaving the recorder stopping.
fn take_active(recorder: &Recorder) -> Result<Box<Active>, String> {
    let mut phase = recorder.phase();
    match std::mem::replace(&mut *phase, Phase::Stopping) {
        Phase::Recording(active) => Ok(active),
        other => {
            *phase = other;
            Err(error_code::NOT_RECORDING.to_owned())
        }
    }
}

/// Stores the outcome of a stopped recording and queues its transcription.
fn finish(core: &Core, meeting_id: Id, stopped: Result<u64, CaptureError>) -> Result<(), String> {
    match stopped {
        Ok(duration_ms) => {
            let duration = i64::try_from(duration_ms).unwrap_or(i64::MAX);
            lock(&core.library)
                .set_meeting_status(meeting_id, MeetingStatus::Processing, Some(duration))
                .map_err(storage_error)?;
            core.runner
                .enqueue(JobKind::Transcribe, meeting_id)
                .map_err(storage_error)?;
            Ok(())
        }
        Err(err) => {
            let code = capture_error(&err);
            lock(&core.library)
                .set_meeting_failed(meeting_id, &code)
                .map_err(storage_error)?;
            Err(code)
        }
    }
}

/// Stops the recording, closes the panel and queues the transcription. Returns the id of
/// the meeting, to open its page.
#[tauri::command]
pub async fn stop_recording<R: Runtime>(app: AppHandle<R>) -> Result<String, String> {
    let active = take_active(&app.state::<Recorder>())?;
    let meeting_id = active.meeting_id;
    let stopped = tauri::async_runtime::spawn_blocking(move || {
        active.recording.stop().map(|summary| summary.duration_ms)
    })
    .await
    .unwrap_or_else(|_| {
        Err(CaptureError::new(
            trama_core::capture::error_code::INTERNAL,
            "the stop panicked",
        ))
    });
    let result = finish(&app.state::<Core>(), meeting_id, stopped);
    let recorder = app.state::<Recorder>();
    *recorder.phase() = Phase::Idle;
    if result.is_ok() {
        recorder.awaiting().insert(meeting_id);
    }
    close_panel(&app);
    emit_state(
        &app,
        RecordingStatePayload {
            recording: None,
            stopped_meeting_id: Some(meeting_id.to_string()),
        },
    );
    result.map(|()| meeting_id.to_string())
}

/// Stops and keeps a running recording when the app quits. Its transcription runs at the
/// next start.
pub fn stop_on_exit<R: Runtime>(app: &AppHandle<R>) {
    let (Some(recorder), Some(core)) = (app.try_state::<Recorder>(), app.try_state::<Core>())
    else {
        return;
    };
    let Ok(active) = take_active(&recorder) else {
        return;
    };
    let meeting_id = active.meeting_id;
    let stopped = active.recording.stop().map(|summary| summary.duration_ms);
    if let Err(code) = finish(&core, meeting_id, stopped) {
        eprintln!("trama: the recording stopped by quitting failed: {code}");
    }
    *recorder.phase() = Phase::Idle;
}

/// The running recording, if any: for a window that opens while recording.
#[tauri::command]
pub fn recording_status(recorder: State<'_, Recorder>) -> Option<RecordingStatus> {
    match &*recorder.phase() {
        Phase::Recording(active) => Some(active.status()),
        _ => None,
    }
}

/// Shows the panel again after it was closed.
#[tauri::command]
pub fn show_recording_panel<R: Runtime>(
    app: AppHandle<R>,
    recorder: State<'_, Recorder>,
) -> Result<(), String> {
    if !matches!(*recorder.phase(), Phase::Recording(_)) {
        return Err(error_code::NOT_RECORDING.to_owned());
    }
    open_panel(&app).map_err(|err| err.to_string())
}

/// Shrinks the panel to a pill, or brings it back to its full size. The top right corner
/// stays where it is.
#[tauri::command]
pub fn set_recording_panel_compact<R: Runtime>(
    app: AppHandle<R>,
    compact: bool,
) -> Result<(), String> {
    let Some(window) = app.get_webview_window(RECORDING_WINDOW) else {
        return Ok(());
    };
    let (width, height) = if compact { PILL_SIZE } else { PANEL_SIZE };
    let resize = || -> tauri::Result<()> {
        let scale = window.scale_factor()?;
        let position = window.outer_position()?.to_logical::<f64>(scale);
        let size = window.outer_size()?.to_logical::<f64>(scale);
        window.set_size(LogicalSize::new(width, height))?;
        window.set_position(tauri::LogicalPosition::new(
            position.x + size.width - width,
            position.y,
        ))
    };
    resize().map_err(|err| err.to_string())
}

fn parse_id(id: &str) -> Result<Id, String> {
    Id::parse_str(id).map_err(|_| format!("invalid id {id:?}"))
}

/// A note as typed: trimmed and at most [`MAX_NOTE_CHARS`] long.
fn clean_note(text: &str) -> Result<String, String> {
    let text = text.trim();
    if text.is_empty() {
        return Err(error_code::EMPTY_NOTE.to_owned());
    }
    Ok(text.chars().take(MAX_NOTE_CHARS).collect())
}

/// Adds a note pinned to the current moment of the recording.
#[tauri::command]
pub fn add_recording_note(
    core: State<'_, Core>,
    recorder: State<'_, Recorder>,
    text: String,
) -> Result<RecordingNotePayload, String> {
    let text = clean_note(&text)?;
    let (meeting_id, at_ms) = match &*recorder.phase() {
        Phase::Recording(active) => (active.meeting_id, active.elapsed_ms()),
        _ => return Err(error_code::NOT_RECORDING.to_owned()),
    };
    let at_ms = i64::try_from(at_ms).unwrap_or(i64::MAX);
    let note = lock(&core.library)
        .add_recording_note(meeting_id, at_ms, &text)
        .map_err(storage_error)?;
    Ok((&note).into())
}

/// Changes the text of a note; its moment stays.
#[tauri::command]
pub fn update_recording_note(
    core: State<'_, Core>,
    id: String,
    text: String,
) -> Result<RecordingNotePayload, String> {
    let text = clean_note(&text)?;
    let note = lock(&core.library)
        .update_recording_note(parse_id(&id)?, &text)
        .map_err(storage_error)?;
    Ok((&note).into())
}

/// Deletes a note.
#[tauri::command]
pub fn delete_recording_note(core: State<'_, Core>, id: String) -> Result<(), String> {
    lock(&core.library)
        .delete_recording_note(parse_id(&id)?)
        .map_err(storage_error)
}

/// The notes of a meeting, in order of time.
#[tauri::command]
pub fn recording_notes(
    core: State<'_, Core>,
    meeting_id: String,
) -> Result<Vec<RecordingNotePayload>, String> {
    let notes = lock(&core.library)
        .recording_notes(parse_id(&meeting_id)?)
        .map_err(storage_error)?;
    Ok(notes.iter().map(Into::into).collect())
}

/// What a job change means for a meeting stopped in this session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Outcome {
    /// The summary is done or failed for good: the meeting is ready.
    Ready,
    /// The transcription failed for good.
    Failed,
    /// Cancelled by the user: nothing to tell.
    Dropped,
}

fn outcome(job: &Job) -> Option<Outcome> {
    match (job.kind, job.state) {
        (JobKind::Summarize, JobState::Done | JobState::Failed) => Some(Outcome::Ready),
        (JobKind::Transcribe, JobState::Failed) => Some(Outcome::Failed),
        (JobKind::Transcribe | JobKind::Summarize, JobState::Cancelled) => Some(Outcome::Dropped),
        _ => None,
    }
}

/// Called for every job change: notifies when a meeting stopped in this session is ready
/// or its transcription failed.
pub fn on_job<R: Runtime>(app: &AppHandle<R>, job: &Job) {
    let Some(outcome) = outcome(job) else {
        return;
    };
    let Some(recorder) = app.try_state::<Recorder>() else {
        return;
    };
    if !recorder.awaiting().remove(&job.target_id) || outcome == Outcome::Dropped {
        return;
    }
    let meeting_id = job.target_id;
    let app = app.clone();
    // Job events may arrive while the runner holds the library: read it from another
    // thread.
    std::thread::spawn(move || {
        let texts = app.state::<Recorder>().texts();
        let (title, body) = match outcome {
            Outcome::Ready => {
                let name = app.try_state::<Core>().and_then(|core| {
                    lock(&core.library)
                        .source(meeting_id)
                        .ok()
                        .map(|source| source.title)
                        .filter(|title| !title.is_empty())
                });
                (
                    texts.ready_title.clone(),
                    name.unwrap_or_else(|| texts.ready_body.clone()),
                )
            }
            _ => (texts.failed_title.clone(), texts.failed_body.clone()),
        };
        if let Err(err) = app.notification().builder().title(title).body(body).show() {
            eprintln!("trama: cannot show the meeting notification: {err}");
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use trama_core::capture::Source;

    #[test]
    fn payloads_use_the_frontend_names() {
        let status = RecordingStatus {
            meeting_id: "m".into(),
            started_at: 5,
            elapsed_ms: 61_000,
        };
        let json = serde_json::to_value(RecordingStatePayload {
            recording: Some(status),
            stopped_meeting_id: None,
        })
        .unwrap();
        assert_eq!(json["recording"]["meetingId"], "m");
        assert_eq!(json["recording"]["elapsedMs"], 61_000);
        assert!(json["stoppedMeetingId"].is_null());

        let json = serde_json::to_value(LevelPayload {
            source: Source::System.as_str(),
            peak_db: -6.0,
            rms_db: -18.0,
        })
        .unwrap();
        assert_eq!(json["source"], "system");
        assert_eq!(json["rmsDb"], -18.0);
    }

    #[test]
    fn notices_carry_the_capture_codes() {
        let notice = NoticePayload::from_event(&CaptureEvent::TrackProblem {
            source: Source::Microphone,
            code: trama_core::capture::error_code::MICROPHONE_SILENT,
            detail: "no signal".into(),
        })
        .unwrap();
        assert_eq!(notice.source, "microphone");
        assert_eq!(notice.kind, "problem");
        assert_eq!(notice.code, Some("microphone_silent"));
        let started = NoticePayload::from_event(&CaptureEvent::TrackStarted {
            source: Source::System,
            offset_ms: 4_000,
        })
        .unwrap();
        assert_eq!(started.kind, "started");
        assert_eq!(started.code, None);
    }

    #[test]
    fn notes_are_trimmed_and_never_empty() {
        assert_eq!(clean_note("  ok \n").unwrap(), "ok");
        assert_eq!(clean_note(" \n ").unwrap_err(), error_code::EMPTY_NOTE);
        assert_eq!(
            clean_note(&"a".repeat(MAX_NOTE_CHARS + 5))
                .unwrap()
                .chars()
                .count(),
            MAX_NOTE_CHARS
        );
    }

    fn job(kind: JobKind, state: JobState) -> Job {
        Job {
            id: Id::now_v7(),
            kind,
            target_id: Id::now_v7(),
            state,
            attempts: 1,
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
    fn the_meeting_is_ready_when_the_summary_ends() {
        use JobKind::{Summarize, Transcribe};
        use JobState::{Cancelled, Done, Failed, Queued, Running};
        assert_eq!(outcome(&job(Summarize, Done)), Some(Outcome::Ready));
        // A failed summary leaves the transcript: the meeting is ready all the same.
        assert_eq!(outcome(&job(Summarize, Failed)), Some(Outcome::Ready));
        assert_eq!(outcome(&job(Transcribe, Failed)), Some(Outcome::Failed));
        assert_eq!(outcome(&job(Transcribe, Cancelled)), Some(Outcome::Dropped));
        // The transcription done only queues the summary.
        assert_eq!(outcome(&job(Transcribe, Done)), None);
        assert_eq!(outcome(&job(Transcribe, Queued)), None);
        assert_eq!(outcome(&job(Summarize, Running)), None);
    }
}
