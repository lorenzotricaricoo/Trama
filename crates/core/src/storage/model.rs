//! Records stored in a library. See `docs/architecture/data-model.md`.

use std::path::PathBuf;

use uuid::Uuid;

use super::error::StorageError;

/// UTC milliseconds since the Unix epoch.
pub type Timestamp = i64;

/// Identifier of a stored record: a UUIDv7, stored as text.
pub type Id = Uuid;

/// Generates a stored enum with its text form, matching the `CHECK` constraints of the schema.
macro_rules! text_enum {
    ($(#[$meta:meta])* $name:ident { $($(#[$vmeta:meta])* $variant:ident = $text:literal),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum $name {
            $($(#[$vmeta])* $variant),+
        }

        impl $name {
            /// The text stored in the database.
            pub const fn as_str(self) -> &'static str {
                match self {
                    $(Self::$variant => $text),+
                }
            }
        }

        impl std::str::FromStr for $name {
            type Err = StorageError;

            fn from_str(text: &str) -> Result<Self, Self::Err> {
                match text {
                    $($text => Ok(Self::$variant),)+
                    other => Err(StorageError::Corrupt(format!(
                        "unknown {} {other:?}",
                        stringify!($name)
                    ))),
                }
            }
        }
    };
}

text_enum! {
    /// What a source is.
    SourceKind {
        Meeting = "meeting",
        Note = "note",
    }
}

text_enum! {
    /// Who gave a source its title.
    TitleOrigin {
        /// Generated from the summary; replaced when a newer summary arrives.
        Auto = "auto",
        /// Written by the user; never overwritten.
        User = "user",
    }
}

text_enum! {
    /// How a version was produced.
    VersionOrigin {
        Transcription = "transcription",
        Editor = "editor",
    }
}

text_enum! {
    /// Where a meeting is in its life.
    MeetingStatus {
        Recording = "recording",
        /// Transcription and summary are running.
        Processing = "processing",
        Ready = "ready",
        Failed = "failed",
    }
}

text_enum! {
    /// Who a track records.
    Role {
        /// The user's microphone.
        Me = "me",
        /// The other participants (system audio).
        Others = "others",
    }
}

/// A meeting or a note.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Source {
    pub id: Id,
    pub kind: SourceKind,
    pub title: String,
    pub title_origin: TitleOrigin,
    /// Meeting language (BCP 47); `None` for notes.
    pub language: Option<String>,
    pub occurred_at: Timestamp,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
    /// Set while the source is in the trash.
    pub deleted_at: Option<Timestamp>,
}

/// One state of a source's content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Version {
    pub id: Id,
    pub source_id: Id,
    /// 1, 2, … per source; the highest is the current version.
    pub number: u32,
    pub origin: VersionOrigin,
    /// Transcription engine and model, for transcriptions.
    pub engine: Option<String>,
    /// Note text (Markdown), for notes.
    pub body: Option<String>,
    pub created_at: Timestamp,
    pub edited_at: Timestamp,
    /// Once set, the version never changes again.
    pub sealed_at: Option<Timestamp>,
}

impl Version {
    pub fn is_sealed(&self) -> bool {
        self.sealed_at.is_some()
    }
}

/// The recording side of a meeting source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Meeting {
    pub source_id: Id,
    pub status: MeetingStatus,
    pub duration_ms: Option<i64>,
    pub audio_deleted_at: Option<Timestamp>,
    /// Why the meeting failed, as a code the interface translates (see [`meeting_error`],
    /// and [`crate::transcription::error_code`] for a failed transcription).
    pub error_code: Option<String>,
}

/// Codes stored in [`Meeting::error_code`].
pub mod meeting_error {
    /// The app stopped while the meeting was recording; the audio may be incomplete.
    pub const RECORDING_INTERRUPTED: &str = "recording_interrupted";
}

/// One audio track of a meeting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Track {
    pub id: Id,
    pub meeting_id: Id,
    pub role: Role,
    /// Relative to the library root.
    pub audio_path: PathBuf,
    /// Relative to the library root.
    pub timing_path: Option<PathBuf>,
    pub device: Option<String>,
}

/// A track to add to a meeting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewTrack {
    pub role: Role,
    /// Relative to the library root, without `..`.
    pub audio_path: PathBuf,
    pub timing_path: Option<PathBuf>,
    pub device: Option<String>,
}

/// A stretch of speech in a transcription.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segment {
    /// Stable once written; what citations point to.
    pub id: i64,
    pub version_id: Id,
    pub seq: u32,
    pub role: Role,
    pub start_ms: i64,
    pub end_ms: i64,
    pub text: String,
}

/// A segment to add to a new transcription, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewSegment {
    pub role: Role,
    pub start_ms: i64,
    pub end_ms: i64,
    pub text: String,
}

/// A note typed while recording, pinned to a moment of the meeting. Editable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordingNote {
    pub id: Id,
    pub meeting_id: Id,
    pub at_ms: i64,
    pub text: String,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

/// One key point of a summary, pinned to a moment of the meeting when it has one.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct KeyPoint {
    pub text: String,
    /// Position in the recording (start of the segment where the point is discussed);
    /// `None` for notes, or when the model gave no usable time.
    #[serde(default)]
    pub at_ms: Option<i64>,
}

impl KeyPoint {
    /// A key point without a time.
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            at_ms: None,
        }
    }

    /// A key point at `at_ms` from the start of the recording.
    pub fn at(text: impl Into<String>, at_ms: i64) -> Self {
        Self {
            text: text.into(),
            at_ms: Some(at_ms),
        }
    }
}

/// Title, summary and key points generated for a version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Summary {
    pub id: Id,
    pub version_id: Id,
    pub title: String,
    /// Markdown.
    pub body: String,
    pub key_points: Vec<KeyPoint>,
    pub language: String,
    pub model: String,
    pub prompt_version: String,
    pub created_at: Timestamp,
}

/// A summary to store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewSummary {
    pub title: String,
    pub body: String,
    pub key_points: Vec<KeyPoint>,
    pub language: String,
    pub model: String,
    pub prompt_version: String,
}

/// What a search hit points to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HitTarget {
    /// A transcript segment of the current version.
    Segment { version_id: Id, segment_id: i64 },
    /// The body of a note's current version.
    Note { version_id: Id },
    /// A note typed while recording.
    RecordingNote { note_id: Id },
}

/// One search result.
#[derive(Debug, Clone, PartialEq)]
pub struct SearchHit {
    pub source_id: Id,
    pub target: HitTarget,
    /// Excerpt with matches between [`HIGHLIGHT_START`] and [`HIGHLIGHT_END`].
    pub excerpt: String,
    /// Lower is better.
    pub rank: f64,
}

/// Marks the start of a match in [`SearchHit::excerpt`] (U+0002, never in user text).
pub const HIGHLIGHT_START: char = '\u{2}';
/// Marks the end of a match in [`SearchHit::excerpt`] (U+0003).
pub const HIGHLIGHT_END: char = '\u{3}';

/// What [`crate::storage::Library::purge_trash`] removed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Purged {
    /// Ids of the sources removed for good.
    pub sources: Vec<Id>,
    /// Audio files and folders of the removed meetings, relative to the library root,
    /// for the caller to delete from disk.
    pub audio_paths: Vec<PathBuf>,
}
