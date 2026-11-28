//! Bridge between the core's meetings and the frontend: the meeting page reads a meeting
//! with its transcript here. Its summary comes from `summary.rs`, its notes from
//! `recording.rs`, and transcription progress from the `job` event.

use serde::Serialize;
use tauri::State;
use trama_core::jobs::lock;
use trama_core::storage::{Id, JobKind, Library, Meeting, Segment, Source, Timestamp, Track};

use crate::jobs::{Core, JobPayload};

/// A stretch of the transcript.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SegmentPayload {
    pub id: i64,
    /// `me` or `others`.
    pub role: &'static str,
    pub start_ms: i64,
    pub end_ms: i64,
    pub text: String,
}

impl From<&Segment> for SegmentPayload {
    fn from(segment: &Segment) -> Self {
        Self {
            id: segment.id,
            role: segment.role.as_str(),
            start_ms: segment.start_ms,
            end_ms: segment.end_ms,
            text: segment.text.clone(),
        }
    }
}

/// An audio track of a meeting, for the player.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AudioTrackPayload {
    /// `me` or `others`.
    pub role: &'static str,
    /// Absolute path of the WAV file, played through the asset protocol.
    pub path: String,
}

/// A meeting as its page shows it.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MeetingPayload {
    pub id: String,
    /// Empty until the summary or the user gives one.
    pub title: String,
    pub occurred_at: Timestamp,
    /// Language of the meeting (BCP 47), when known.
    pub language: Option<String>,
    /// `recording`, `processing`, `ready` or `failed`.
    pub status: &'static str,
    pub duration_ms: Option<i64>,
    /// Why the meeting failed, as a code the interface translates.
    pub error_code: Option<String>,
    /// The current transcript; empty until the transcription is done.
    pub segments: Vec<SegmentPayload>,
    /// The tracks whose audio is still on disk; empty once the audio is deleted.
    pub audio: Vec<AudioTrackPayload>,
    /// The audio was deleted (audio retention).
    pub audio_deleted: bool,
}

impl MeetingPayload {
    fn new(
        source: &Source,
        meeting: &Meeting,
        segments: &[Segment],
        audio: Vec<AudioTrackPayload>,
    ) -> Self {
        Self {
            id: source.id.to_string(),
            title: source.title.clone(),
            occurred_at: source.occurred_at,
            language: source.language.clone(),
            status: meeting.status.as_str(),
            duration_ms: meeting.duration_ms,
            error_code: meeting.error_code.clone(),
            segments: segments.iter().map(Into::into).collect(),
            audio,
            audio_deleted: meeting.audio_deleted_at.is_some(),
        }
    }
}

/// The tracks a player can open: not while recording (the files are still growing), not
/// once the audio is deleted, and only files still on disk.
fn playable(library: &Library, meeting: &Meeting, tracks: &[Track]) -> Vec<AudioTrackPayload> {
    if meeting.audio_deleted_at.is_some()
        || meeting.status == trama_core::storage::MeetingStatus::Recording
    {
        return Vec::new();
    }
    tracks
        .iter()
        .filter_map(|track| {
            let path = library.root().join(&track.audio_path);
            path.is_file().then(|| AudioTrackPayload {
                role: track.role.as_str(),
                path: path.to_string_lossy().into_owned(),
            })
        })
        .collect()
}

fn read(library: &Library, id: Id) -> trama_core::storage::Result<MeetingPayload> {
    let source = library.source(id)?;
    let meeting = library.meeting(id)?;
    let segments = match library.current_version(id)? {
        Some(version) => library.segments(version.id)?,
        None => Vec::new(),
    };
    let audio = playable(library, &meeting, &library.tracks(id)?);
    Ok(MeetingPayload::new(&source, &meeting, &segments, audio))
}

/// A meeting with its current transcript.
#[tauri::command]
pub fn get_meeting(core: State<'_, Core>, id: String) -> Result<MeetingPayload, String> {
    let id = Id::parse_str(&id).map_err(|_| format!("invalid meeting id {id:?}"))?;
    read(&lock(&core.library), id).map_err(|err| err.to_string())
}

/// Transcribes a meeting again from its audio, after a failure without a job to retry
/// (the app stopped while recording, for example). Returns the queued job.
#[tauri::command]
pub fn retranscribe(core: State<'_, Core>, id: String) -> Result<JobPayload, String> {
    let id = Id::parse_str(&id).map_err(|_| format!("invalid meeting id {id:?}"))?;
    let job = core
        .runner
        .enqueue(JobKind::Transcribe, id)
        .map_err(|err| err.to_string())?;
    Ok(JobPayload::new(&job, None))
}

#[cfg(test)]
mod tests {
    use super::*;
    use trama_core::storage::{MeetingStatus, NewSegment, Role};

    #[test]
    fn a_meeting_reads_with_its_transcript() {
        let dir = tempfile::tempdir().unwrap();
        let mut library = Library::open(dir.path()).unwrap();
        let source = library.create_meeting("it", 1_000).unwrap();
        let payload = read(&library, source.id).unwrap();
        assert_eq!(payload.status, "recording");
        assert!(payload.segments.is_empty());

        library
            .set_meeting_status(source.id, MeetingStatus::Processing, Some(90_000))
            .unwrap();
        library
            .complete_transcription(
                source.id,
                "test-engine",
                &[NewSegment {
                    role: Role::Others,
                    start_ms: 0,
                    end_ms: 1_500,
                    text: "Buongiorno a tutti".into(),
                }],
            )
            .unwrap();
        let payload = read(&library, source.id).unwrap();
        assert_eq!(payload.duration_ms, Some(90_000));
        assert_eq!(payload.segments.len(), 1);
        let json = serde_json::to_value(&payload).unwrap();
        assert_eq!(json["occurredAt"], 1_000);
        assert_eq!(json["segments"][0]["role"], "others");
        assert_eq!(json["segments"][0]["endMs"], 1_500);
        // No track was recorded: nothing to play.
        assert_eq!(json["audio"], serde_json::json!([]));
        assert_eq!(json["audioDeleted"], false);
    }

    #[test]
    fn only_audio_still_on_disk_is_playable() {
        let dir = tempfile::tempdir().unwrap();
        let mut library = Library::open(dir.path()).unwrap();
        let source = library.create_meeting("it", 1_000).unwrap();
        let audio_dir = Library::meeting_audio_dir(source.id);
        for role in [Role::Me, Role::Others] {
            library
                .add_track(
                    source.id,
                    &trama_core::storage::NewTrack {
                        role,
                        audio_path: audio_dir.join(format!("{}.wav", role.as_str())),
                        timing_path: None,
                        device: None,
                    },
                )
                .unwrap();
        }
        std::fs::create_dir_all(dir.path().join(&audio_dir)).unwrap();
        std::fs::write(dir.path().join(audio_dir.join("me.wav")), b"RIFF").unwrap();
        // Still recording: the files are growing.
        assert!(read(&library, source.id).unwrap().audio.is_empty());

        library
            .set_meeting_status(source.id, MeetingStatus::Processing, Some(1_000))
            .unwrap();
        let audio = read(&library, source.id).unwrap().audio;
        assert_eq!(audio.len(), 1);
        assert_eq!(audio[0].role, "me");
        assert!(audio[0].path.ends_with("me.wav"));

        library.mark_audio_deleted(source.id).unwrap();
        let payload = read(&library, source.id).unwrap();
        assert!(payload.audio.is_empty());
        assert!(payload.audio_deleted);
    }
}
