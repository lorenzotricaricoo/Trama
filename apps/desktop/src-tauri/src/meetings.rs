//! Bridge between the core's meetings and the frontend: the meeting page reads a meeting
//! with its transcript here. Its summary comes from `summary.rs`, its notes from
//! `recording.rs`, and transcription progress from the `job` event.

use serde::Serialize;
use tauri::State;
use trama_core::jobs::lock;
use trama_core::storage::{Id, Library, Meeting, Segment, Source, Timestamp};

use crate::jobs::Core;

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

/// A meeting as its page shows it.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MeetingPayload {
    pub id: String,
    /// Empty until the summary or the user gives one.
    pub title: String,
    pub occurred_at: Timestamp,
    /// `recording`, `processing`, `ready` or `failed`.
    pub status: &'static str,
    pub duration_ms: Option<i64>,
    /// Why the meeting failed, as a code the interface translates.
    pub error_code: Option<String>,
    /// The current transcript; empty until the transcription is done.
    pub segments: Vec<SegmentPayload>,
}

impl MeetingPayload {
    fn new(source: &Source, meeting: &Meeting, segments: &[Segment]) -> Self {
        Self {
            id: source.id.to_string(),
            title: source.title.clone(),
            occurred_at: source.occurred_at,
            status: meeting.status.as_str(),
            duration_ms: meeting.duration_ms,
            error_code: meeting.error_code.clone(),
            segments: segments.iter().map(Into::into).collect(),
        }
    }
}

fn read(library: &Library, id: Id) -> trama_core::storage::Result<MeetingPayload> {
    let source = library.source(id)?;
    let meeting = library.meeting(id)?;
    let segments = match library.current_version(id)? {
        Some(version) => library.segments(version.id)?,
        None => Vec::new(),
    };
    Ok(MeetingPayload::new(&source, &meeting, &segments))
}

/// A meeting with its current transcript.
#[tauri::command]
pub fn get_meeting(core: State<'_, Core>, id: String) -> Result<MeetingPayload, String> {
    let id = Id::parse_str(&id).map_err(|_| format!("invalid meeting id {id:?}"))?;
    read(&lock(&core.library), id).map_err(|err| err.to_string())
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
    }
}
