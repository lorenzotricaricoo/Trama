//! Meetings: tracks, transcriptions and notes typed while recording.

use rusqlite::{OptionalExtension, Row, params};

use super::sources::{insert_version, live_source, load_source, load_version, touch_source};
use super::{
    Id, Library, Meeting, MeetingStatus, NewSegment, NewTrack, RecordingNote, Result, Segment,
    Source, SourceKind, StorageError, Timestamp, Track, Version, VersionOrigin, enum_column,
    id_column, library_relative, new_id,
};

fn meeting_row(row: &Row<'_>) -> rusqlite::Result<Meeting> {
    Ok(Meeting {
        source_id: id_column(row, 0)?,
        status: enum_column(row, 1)?,
        duration_ms: row.get(2)?,
        audio_deleted_at: row.get(3)?,
        error_code: row.get(4)?,
    })
}

fn track_row(row: &Row<'_>) -> rusqlite::Result<Track> {
    Ok(Track {
        id: id_column(row, 0)?,
        meeting_id: id_column(row, 1)?,
        role: enum_column(row, 2)?,
        audio_path: row.get::<_, String>(3)?.into(),
        timing_path: row.get::<_, Option<String>>(4)?.map(Into::into),
        device: row.get(5)?,
    })
}

fn segment_row(row: &Row<'_>) -> rusqlite::Result<Segment> {
    Ok(Segment {
        id: row.get(0)?,
        version_id: id_column(row, 1)?,
        seq: row.get(2)?,
        role: enum_column(row, 3)?,
        start_ms: row.get(4)?,
        end_ms: row.get(5)?,
        text: row.get(6)?,
    })
}

fn recording_note_row(row: &Row<'_>) -> rusqlite::Result<RecordingNote> {
    Ok(RecordingNote {
        id: id_column(row, 0)?,
        meeting_id: id_column(row, 1)?,
        at_ms: row.get(2)?,
        text: row.get(3)?,
        created_at: row.get(4)?,
        updated_at: row.get(5)?,
    })
}

const RECORDING_NOTE_COLUMNS: &str = "id, meeting_id, at_ms, text, created_at, updated_at";

impl Library {
    /// Creates a meeting that starts recording at `started_at`, in `language` (BCP 47).
    pub fn create_meeting(&mut self, language: &str, started_at: Timestamp) -> Result<Source> {
        let now = self.now();
        let id = new_id();
        let tx = self.conn.transaction()?;
        tx.execute(
            "INSERT INTO source (id, kind, language, occurred_at, created_at, updated_at)
             VALUES (?1, 'meeting', ?2, ?3, ?4, ?4)",
            params![id.to_string(), language, started_at, now],
        )?;
        tx.execute(
            "INSERT INTO meeting (source_id, status) VALUES (?1, 'recording')",
            [id.to_string()],
        )?;
        tx.commit()?;
        load_source(&self.conn, id)
    }

    /// The recording side of a meeting.
    pub fn meeting(&self, id: Id) -> Result<Meeting> {
        self.conn
            .query_row(
                "SELECT source_id, status, duration_ms, audio_deleted_at, error_code FROM meeting WHERE source_id = ?1",
                [id.to_string()],
                meeting_row,
            )
            .optional()?
            .ok_or_else(|| StorageError::not_found("meeting", id))
    }

    /// Updates a meeting's status, and its duration when known.
    pub fn set_meeting_status(
        &mut self,
        id: Id,
        status: MeetingStatus,
        duration_ms: Option<i64>,
    ) -> Result<Meeting> {
        let now = self.now();
        let tx = self.conn.transaction()?;
        live_source(&tx, id, SourceKind::Meeting)?;
        tx.execute(
            "UPDATE meeting SET status = ?2, duration_ms = COALESCE(?3, duration_ms), error_code = NULL
             WHERE source_id = ?1",
            params![id.to_string(), status.as_str(), duration_ms],
        )?;
        touch_source(&tx, id, now)?;
        tx.commit()?;
        self.meeting(id)
    }

    /// Marks a meeting as failed, with a code the interface translates (see [`meeting_error`]).
    ///
    /// [`meeting_error`]: super::meeting_error
    pub fn set_meeting_failed(&mut self, id: Id, error_code: &str) -> Result<Meeting> {
        let now = self.now();
        let tx = self.conn.transaction()?;
        live_source(&tx, id, SourceKind::Meeting)?;
        tx.execute(
            "UPDATE meeting SET status = 'failed', error_code = ?2 WHERE source_id = ?1",
            params![id.to_string(), error_code],
        )?;
        touch_source(&tx, id, now)?;
        tx.commit()?;
        self.meeting(id)
    }

    /// Removes a meeting whose recording never started: still `recording`, with no tracks
    /// and no transcript. Used when the capture fails to start, so no empty meeting is left
    /// behind; any other meeting is refused (it goes to the trash instead).
    pub fn discard_meeting(&mut self, id: Id) -> Result<()> {
        let tx = self.conn.transaction()?;
        live_source(&tx, id, SourceKind::Meeting)?;
        let (status, tracks, versions): (String, i64, i64) = tx.query_row(
            "SELECT m.status,
                    (SELECT COUNT(*) FROM track t WHERE t.meeting_id = m.source_id),
                    (SELECT COUNT(*) FROM source_version v WHERE v.source_id = m.source_id)
             FROM meeting m WHERE m.source_id = ?1",
            [id.to_string()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;
        if status != MeetingStatus::Recording.as_str() || tracks > 0 || versions > 0 {
            return Err(StorageError::Invalid(
                "a meeting that started recording can't be discarded",
            ));
        }
        tx.execute("DELETE FROM source WHERE id = ?1", [id.to_string()])?;
        tx.commit()?;
        Ok(())
    }

    /// Records that the audio of a meeting was removed from disk.
    pub fn mark_audio_deleted(&mut self, id: Id) -> Result<Meeting> {
        let now = self.now();
        let changed = self.conn.execute(
            "UPDATE meeting SET audio_deleted_at = ?2 WHERE source_id = ?1 AND audio_deleted_at IS NULL",
            params![id.to_string(), now],
        )?;
        if changed == 0 {
            self.meeting(id)?;
        }
        self.meeting(id)
    }

    /// Adds an audio track to a meeting. One track per role.
    pub fn add_track(&mut self, meeting_id: Id, track: &NewTrack) -> Result<Track> {
        let audio_path = library_relative(&track.audio_path)?;
        let timing_path = track
            .timing_path
            .as_deref()
            .map(library_relative)
            .transpose()?;
        live_source(&self.conn, meeting_id, SourceKind::Meeting)?;
        let id = new_id();
        self.conn.execute(
            "INSERT INTO track (id, meeting_id, role, audio_path, timing_path, device)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                id.to_string(),
                meeting_id.to_string(),
                track.role.as_str(),
                audio_path,
                timing_path,
                track.device
            ],
        )?;
        Ok(self.conn.query_row(
            "SELECT id, meeting_id, role, audio_path, timing_path, device FROM track WHERE id = ?1",
            [id.to_string()],
            track_row,
        )?)
    }

    /// The tracks of a meeting.
    pub fn tracks(&self, meeting_id: Id) -> Result<Vec<Track>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT id, meeting_id, role, audio_path, timing_path, device FROM track
             WHERE meeting_id = ?1 ORDER BY role",
        )?;
        let rows = stmt.query_map([meeting_id.to_string()], track_row)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Stores a transcription of a meeting as a new, sealed version with its segments
    /// (in order). It becomes the current version.
    pub fn add_transcription(
        &mut self,
        meeting_id: Id,
        engine: &str,
        segments: &[NewSegment],
    ) -> Result<Version> {
        let now = self.now();
        let tx = self.conn.transaction()?;
        live_source(&tx, meeting_id, SourceKind::Meeting)?;
        let version = insert_transcription(&tx, meeting_id, engine, segments, now)?;
        touch_source(&tx, meeting_id, now)?;
        let version = load_version(&tx, version)?;
        tx.commit()?;
        Ok(version)
    }

    /// Ends a meeting's transcription: stores it like [`Self::add_transcription`] and
    /// marks the meeting `ready`, clearing its error code, in one transaction. A job
    /// that fails after this, or runs again, cannot leave a version without the status
    /// or store the same transcription twice.
    ///
    /// Fails with [`StorageError::NotFound`], writing nothing, if the meeting is in the
    /// trash.
    pub fn complete_transcription(
        &mut self,
        meeting_id: Id,
        engine: &str,
        segments: &[NewSegment],
    ) -> Result<Version> {
        let now = self.now();
        let tx = self.conn.transaction()?;
        live_source(&tx, meeting_id, SourceKind::Meeting)?;
        let version = insert_transcription(&tx, meeting_id, engine, segments, now)?;
        tx.execute(
            "UPDATE meeting SET status = ?2, error_code = NULL WHERE source_id = ?1",
            params![meeting_id.to_string(), MeetingStatus::Ready.as_str()],
        )?;
        touch_source(&tx, meeting_id, now)?;
        let version = load_version(&tx, version)?;
        tx.commit()?;
        Ok(version)
    }

    /// The segments of a transcription version, in order.
    pub fn segments(&self, version_id: Id) -> Result<Vec<Segment>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT id, version_id, seq, role, start_ms, end_ms, text FROM transcript_segment
             WHERE version_id = ?1 ORDER BY seq",
        )?;
        let rows = stmt.query_map([version_id.to_string()], segment_row)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Adds a note pinned to `at_ms` from the start of the meeting.
    pub fn add_recording_note(
        &mut self,
        meeting_id: Id,
        at_ms: i64,
        text: &str,
    ) -> Result<RecordingNote> {
        let now = self.now();
        live_source(&self.conn, meeting_id, SourceKind::Meeting)?;
        let id = new_id();
        self.conn.execute(
            "INSERT INTO recording_note (id, meeting_id, at_ms, text, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
            params![id.to_string(), meeting_id.to_string(), at_ms, text, now],
        )?;
        self.recording_note(id)
    }

    /// Changes the text of a note typed while recording.
    pub fn update_recording_note(&mut self, id: Id, text: &str) -> Result<RecordingNote> {
        let now = self.now();
        let changed = self.conn.execute(
            "UPDATE recording_note SET text = ?2, updated_at = ?3 WHERE id = ?1",
            params![id.to_string(), text, now],
        )?;
        if changed == 0 {
            return Err(StorageError::not_found("recording note", id));
        }
        self.recording_note(id)
    }

    /// Deletes a note typed while recording.
    pub fn delete_recording_note(&mut self, id: Id) -> Result<()> {
        let changed = self
            .conn
            .execute("DELETE FROM recording_note WHERE id = ?1", [id.to_string()])?;
        if changed == 0 {
            return Err(StorageError::not_found("recording note", id));
        }
        Ok(())
    }

    /// The notes typed while recording a meeting, in time order.
    pub fn recording_notes(&self, meeting_id: Id) -> Result<Vec<RecordingNote>> {
        let mut stmt = self.conn.prepare_cached(&format!(
            "SELECT {RECORDING_NOTE_COLUMNS} FROM recording_note
             WHERE meeting_id = ?1 ORDER BY at_ms, id"
        ))?;
        let rows = stmt.query_map([meeting_id.to_string()], recording_note_row)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    fn recording_note(&self, id: Id) -> Result<RecordingNote> {
        self.conn
            .query_row(
                &format!("SELECT {RECORDING_NOTE_COLUMNS} FROM recording_note WHERE id = ?1"),
                [id.to_string()],
                recording_note_row,
            )
            .optional()?
            .ok_or_else(|| StorageError::not_found("recording note", id))
    }
}

/// Inserts a sealed transcription version with its segments; returns its id.
fn insert_transcription(
    conn: &rusqlite::Connection,
    meeting_id: Id,
    engine: &str,
    segments: &[NewSegment],
    now: Timestamp,
) -> Result<Id> {
    let version = insert_version(
        conn,
        meeting_id,
        VersionOrigin::Transcription,
        Some(engine),
        None,
        now,
    )?;
    let mut insert = conn.prepare_cached(
        "INSERT INTO transcript_segment (version_id, seq, role, start_ms, end_ms, text)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
    )?;
    for (seq, segment) in segments.iter().enumerate() {
        insert.execute(params![
            version.id.to_string(),
            seq as i64,
            segment.role.as_str(),
            segment.start_ms,
            segment.end_ms,
            segment.text
        ])?;
    }
    conn.execute(
        "UPDATE source_version SET sealed_at = ?2 WHERE id = ?1",
        params![version.id.to_string(), now],
    )?;
    Ok(version.id)
}
